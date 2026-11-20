//! The Software tab's Builds half (operator, 2026-10-15: "take in
//! build.ndexr.io page but instead should be interactive on a combined (curves
//! + cost, swappable diagram), and the cumulatives we should also be able to
//! click at what was built in that moment and interactively explore the
//! curves and their data, also need to show what is 'to build' and 'building'").
//!
//!   Curves   build.ndexr.io's packages-built and cost windows as one chart:
//!            built · spend · $ per package · packages an hour, one line per
//!            machine, any of them over any other (⇄ swaps). Hover reads every
//!            machine at that moment; a click pins it and lists what finished
//!            then and what was still compiling - the long step that flattens a
//!            curve. Drag across selects a stretch of time, drag down a
//!            stretch of the measure, aslant both (brush.rs); Shift+drag
//!            pans, Ctrl+wheel zooms, double-click fits.
//!   Queue    build.ndexr.io's "to build" page: each machine's running step
//!            against its estimate, and what is left, each bar split into the
//!            entry's own time and its dependencies'.
//!
//! The history is the catalog (every step, deduplicated - a run carries the
//! steps it inherited); prices and the queue come from `/builds.json` (host.rs
//! builds_live), asked every 30 s while a Builds view shows.

use std::collections::HashMap;
use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, FontId, Rect, RichText, Sense, Shape, Stroke};
use serde::Deserialize;

use super::branches::{self, Dim, Dims};
use super::brush;
use super::{fmt_date, fmt_dur, now_secs, run_began, thousands, tick_step, Catalog, SoftwarePane};
use crate::backend::Backend;
use crate::theme;

// ── /builds.json ────────────────────────────────────────────────────────────

#[derive(Deserialize, Default)]
#[serde(default)]
pub struct Live {
    generated: i64,
    eta: Option<Eta>,
    eta_at: Option<i64>,
    prices: Option<HashMap<String, f64>>,
    /// Each machine's package lane, from its log (host.rs build_lanes).
    lanes: Vec<Lane>,
}

/// What a build machine's log says it is installing, and how far through.
#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct Lane {
    key: String,
    /// When the log last grew (unix seconds).
    mtime: i64,
    state: Option<String>,
    /// host.rs box_power: on · off · unconfirmed - the one answer every view gives.
    power: Option<String>,
    now: Option<String>,
    kind: Option<String>,
    n: u64,
    of: u64,
}

impl Live {
    /// On now, by the same answer the Machines panel and the atlas give:
    /// the box's power where its lane says it (four-box), else its rollup.
    fn machine_live(&self, m: &Machine) -> bool {
        match self.lane(&m.id).and_then(|l| l.power.as_deref()) {
            Some(p) => p != "off",
            None => m.live(),
        }
    }

    /// The lane of eta machine `four-box/arm`: the log `arm`.
    fn lane(&self, id: &str) -> Option<&Lane> {
        let key = id.rsplit('/').next()?;
        self.lanes.iter().find(|l| l.key == key && id.starts_with("four-box"))
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Eta {
    machines: Vec<Machine>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Machine {
    id: String,
    arch: String,
    kind: String,
    name: String,
    role: String,
    plan: String,
    active: bool,
    counts: Counts,
    pending: Vec<Pending>,
    running: Option<Running>,
    finish_at: Option<i64>,
    generated: i64,
    remaining_s: Option<f64>,
    /// Seconds since the machine last sent its state.
    mirror_age_s: Option<i64>,
}

/// The machines' names, told apart: two machines called "containers" (one
/// per arch) read as "containers, containers" (operator's screenshot,
/// 2026-10-22), so a shared name carries its arch, and a name that still
/// repeats is counted once ("containers ×2").
fn off_names(ms: &[&Machine]) -> Vec<String> {
    let base = |m: &Machine| if m.name.is_empty() { m.id.rsplit('/').next().unwrap_or(&m.id).to_owned() } else { m.name.clone() };
    let mut out: Vec<(String, usize)> = Vec::new();
    for m in ms {
        let n = base(m);
        let shared = ms.iter().filter(|o| base(o) == n).count() > 1;
        let n = if shared && !m.arch.is_empty() { format!("{n} ({})", m.arch) } else { n };
        match out.iter_mut().find(|(o, _)| *o == n) {
            Some((_, c)) => *c += 1,
            None => out.push((n, 1)),
        }
    }
    out.into_iter().map(|(n, c)| if c > 1 { format!("{n} ×{c}") } else { n }).collect()
}

/// Heard from within this long, a machine is on.
const LIVE_S: i64 = 15 * 60;

impl Machine {
    /// On now: building, or reporting in. A machine switched off keeps its
    /// last report (a failure, a half-done queue) forever; shown as if live,
    /// that read as an error (operator, 2026-10-18: "a build machine right
    /// now is off and shows error in the visual, consider only showing live
    /// ones").
    fn live(&self) -> bool {
        self.active || self.mirror_age_s.is_some_and(|a| a < LIVE_S)
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Counts {
    done: i64,
    total: i64,
    failed: i64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Pending {
    entry: String,
    status: String,
    basis: String,
    est_s: Option<f64>,
    own_s: Option<f64>,
    cum_s: Option<f64>,
    deps_n: Option<f64>,
    deps_s: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Running {
    name: String,
    version: String,
    est_s: Option<f64>,
    elapsed_s: Option<f64>,
    target: Option<String>,
    target_est_s: Option<f64>,
    target_remaining_s: Option<f64>,
}

pub type LiveSlot = Arc<std::sync::Mutex<Option<Result<Live, String>>>>;
pub type BoxesSlot = Arc<std::sync::Mutex<Option<Result<Vec<crate::api::BoxView>, String>>>>;

// ── the curves, worked out once per catalog ────────────────────────────────

/// One line's history - a machine's, or a paradigm's, manifest's, language's
/// (Dim): every step it finished (deduplicated), and for a machine the time it
/// was building and what that cost.
struct BoxCurve {
    name: String,
    /// Its value of the dimension the lines are split by: a click on its
    /// name focuses it (branches::State::branch).
    code: u16,
    color: Color32,
    /// The machine, when the lines are machines.
    machine: Option<usize>,
    /// (start, end, step) of each step, by end.
    steps: Vec<(i64, i64, u32)>,
    /// Ends of the steps that built, sorted (the cumulative curve), and the
    /// build time up to each.
    ends: Vec<i64>,
    secs: Vec<i64>,
    failed: usize,
    /// Building time as non-overlapping (from, to, $ per second), and the
    /// spend up to the end of each.
    spans: Vec<(i64, i64, f64)>,
    cum: Vec<f64>,
    /// The latest run's instance type and its $/h.
    itype: String,
    per_h: Option<f64>,
    /// When the machine first and last did anything (a step, a paid run): its
    /// line is drawn only between them (operator, 2026-10-15: "i dont like
    /// lines extending where they dont exist").
    first: i64,
    last: i64,
    /// When it was on and working: its steps and paid runs, merged across
    /// short pauses. Off between them, the line is not drawn at all - no long
    /// flat run while a box sat off, only the climbs where it built
    /// (operator, 2026-10-18: "if the box is off, I'd actually rather us not
    /// display any work there at all").
    active: Vec<(i64, i64)>,
}

/// A pause shorter than this is still the machine at work (between packages,
/// a slow download); longer, it was off.
const ON_GAP: i64 = 20 * 60;

/// Intervals merged where they overlap or sit closer than `gap`.
fn merged(mut v: Vec<(i64, i64)>, gap: i64) -> Vec<(i64, i64)> {
    v.sort_unstable();
    let mut out: Vec<(i64, i64)> = Vec::with_capacity(v.len());
    for (a, b) in v {
        match out.last_mut() {
            Some(l) if a <= l.1 + gap => l.1 = l.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

/// The x axis. On the calendar every second counts; in building time only the
/// stretches when some machine was building do (each line's `active`, merged),
/// so a day the boxes sat off takes no width and the distance across is hours
/// of compiling. Values are always read at real moments: `fwd` places a moment
/// on the axis, `back` finds the moment at an axis position.
struct Axis {
    /// (from, to, axis position of `from`), in order, when building time.
    segs: Vec<(f64, f64, f64)>,
}

impl Axis {
    fn new(cv: &Curves, calendar: bool) -> Axis {
        if calendar {
            return Axis { segs: Vec::new() };
        }
        let mut at = 0.0;
        let segs = merged(cv.boxes.iter().flat_map(|c| c.active.iter().copied()).collect(), 0)
            .into_iter()
            .map(|(a, b)| {
                let s = (a as f64, b as f64, at);
                at += (b - a) as f64;
                s
            })
            .collect();
        Axis { segs }
    }

    fn compressed(&self) -> bool {
        !self.segs.is_empty()
    }

    /// A moment's place on the axis (in a cut gap: where the gap was cut).
    fn fwd(&self, t: f64) -> f64 {
        let Some(last) = self.segs.last() else { return t };
        let i = self.segs.partition_point(|s| s.1 < t);
        match self.segs.get(i) {
            Some(s) if t >= s.0 => s.2 + (t - s.0),
            Some(s) => s.2,
            None => last.2 + (last.1 - last.0) + (t - last.1).min(0.0),
        }
    }

    /// The moment at an axis place; past the last stretch, real time runs on.
    fn back(&self, c: f64) -> f64 {
        let Some(last) = self.segs.last() else { return c };
        let i = self.segs.partition_point(|s| s.2 + (s.1 - s.0) < c);
        match self.segs.get(i) {
            Some(s) => s.0 + (c - s.2).max(0.0),
            None => last.1 + (c - last.2 - (last.1 - last.0)),
        }
    }

    /// Where a gap was cut: the axis place and the moment building resumed.
    fn cuts(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.segs.iter().skip(1).map(|s| (s.2, s.0))
    }
}

impl BoxCurve {
    /// It was working at some moment in [a, b].
    fn on(&self, a: f64, b: f64) -> bool {
        let i = self.active.partition_point(|s| (s.1 as f64) < a);
        self.active.get(i).is_some_and(|s| (s.0 as f64) <= b)
    }

    fn built(&self, x: f64) -> f64 {
        self.ends.partition_point(|&e| (e as f64) <= x) as f64
    }

    /// Hours of building finished by `x`.
    fn busy(&self, x: f64) -> f64 {
        match self.ends.partition_point(|&e| (e as f64) <= x) {
            0 => 0.0,
            n => self.secs[n - 1] as f64 / 3600.0,
        }
    }

    fn spend(&self, x: f64) -> f64 {
        let i = self.spans.partition_point(|s| (s.1 as f64) <= x);
        let before = if i == 0 { 0.0 } else { self.cum[i - 1] };
        match self.spans.get(i) {
            Some(&(a, _, p)) if (a as f64) < x => before + (x - a as f64) * p,
            _ => before,
        }
    }

    fn hours(&self) -> f64 {
        self.spans.iter().map(|s| (s.1 - s.0) as f64).sum::<f64>() / 3600.0
    }
}

pub struct Curves {
    key: (String, bool, String),
    boxes: Vec<BoxCurve>,
    lo: i64,
    hi: i64,
    priced: bool,
}

/// The lines: one per value of `split`, only the steps under `branch`. Spend
/// is a machine's (its building time × its price), so it is worked out only
/// when the lines are machines and the focus does not cut a machine's builds
/// apart (a paradigm's share of an hour is not on any bill).
fn curves(cat: &Catalog, dims: &Dims, prices: Option<&HashMap<String, f64>>, split: Dim, branch: &[(Dim, u16)]) -> Curves {
    let by_machine = split == Dim::Machine && branch.iter().all(|(d, _)| matches!(d, Dim::Machine | Dim::Arch));
    let nb = dims.values(split).max(1);
    let mut steps: Vec<Vec<(i64, i64, u32)>> = vec![Vec::new(); nb];
    let mut ends: Vec<Vec<i64>> = vec![Vec::new(); nb];
    let mut failed = vec![0usize; nb];
    let mut run_last: HashMap<usize, i64> = HashMap::new();
    let (mut lo, mut hi) = (i64::MAX, i64::MIN);
    for k in 0..cat.st_i.len().min(cat.st_t.len()).min(cat.st_d.len()).min(cat.st_s.len()) {
        let e = cat.st_t[k] + cat.st_d[k];
        let r = cat.st_r[k] as usize;
        let last = run_last.entry(r).or_insert(e);
        *last = (*last).max(e);
    }
    // Dims::unique: a run lists the steps it inherited from the runs before
    // it, once each.
    for &k in &dims.unique {
        let k = k as usize;
        if !dims.under(branch, k) {
            continue;
        }
        let (t, e) = (cat.st_t[k], cat.st_t[k] + cat.st_d[k]);
        let b = (dims.code(split, k) as usize).min(nb - 1);
        match cat.st_s[k] {
            b'd' => {
                ends[b].push(e);
                steps[b].push((t, e, k as u32));
                lo = lo.min(t);
                hi = hi.max(e);
            }
            b'f' => failed[b] += 1,
            _ => {}
        }
    }
    // Building time: each run from its id's time to its last step, priced at
    // its instance type; where runs on one box overlap, the dearer counts once.
    let mut runs_of: Vec<Vec<(i64, i64, f64, &str)>> = vec![Vec::new(); nb];
    for (r, run) in cat.runs.iter().enumerate().filter(|_| by_machine) {
        if run.bx >= nb || !branch.iter().all(|&(d, c)| match d {
            Dim::Machine => run.bx == c as usize,
            _ => dims.name(Dim::Arch, c) == super::short_arch(&cat.box_arch(run.bx)),
        }) {
            continue;
        }
        let Some(&end) = run_last.get(&r) else { continue };
        let began = run_began(&run.id).map(|t| t - cat.t0).unwrap_or(run.start - cat.t0);
        if end <= began {
            continue;
        }
        let price = prices.and_then(|p| p.get(&run.itype)).copied().unwrap_or(0.0) / 3600.0;
        runs_of[run.bx.min(nb - 1)].push((began, end, price, run.itype.as_str()));
    }
    let boxes = (0..nb)
        .map(|b| {
            let rs = &runs_of[b];
            let mut cuts: Vec<i64> = rs.iter().flat_map(|r| [r.0, r.1]).collect();
            cuts.sort_unstable();
            cuts.dedup();
            let mut spans: Vec<(i64, i64, f64)> = Vec::new();
            for w in cuts.windows(2) {
                let mid = (w[0] + w[1]) / 2;
                let covering: Vec<f64> = rs.iter().filter(|r| r.0 <= mid && mid < r.1).map(|r| r.2).collect();
                if covering.is_empty() {
                    continue;
                }
                let p = covering.iter().copied().fold(0.0, f64::max);
                match spans.last_mut() {
                    Some(l) if l.1 == w[0] && l.2 == p => l.1 = w[1],
                    _ => spans.push((w[0], w[1], p)),
                }
            }
            let mut cum = Vec::with_capacity(spans.len());
            let mut acc = 0.0;
            for s in &spans {
                acc += (s.1 - s.0) as f64 * s.2;
                cum.push(acc);
            }
            let latest = rs.iter().max_by_key(|r| r.0);
            let itype = latest.map_or(String::new(), |r| r.3.to_owned());
            let per_h = latest.and_then(|r| prices.and_then(|p| p.get(r.3)).copied());
            let mut st = std::mem::take(&mut steps[b]);
            st.sort_by_key(|s| s.1);
            let mut en = std::mem::take(&mut ends[b]);
            en.sort_unstable();
            let mut acc = 0;
            let secs = st.iter().map(|s| {
                acc += s.1 - s.0;
                acc
            }).collect();
            let first = st.iter().map(|s| s.0).chain(spans.iter().map(|s| s.0)).min().unwrap_or(0);
            let last = en.last().copied().into_iter().chain(spans.iter().map(|s| s.1)).max().unwrap_or(first);
            let name = dims.name(split, b as u16).to_owned();
            let active = merged(st.iter().map(|s| (s.0, s.1)).chain(spans.iter().map(|s| (s.0, s.1))).collect(), ON_GAP);
            BoxCurve { name, code: b as u16, color: dims.color(split, b as u16), machine: by_machine.then_some(b), steps: st, ends: en, secs, failed: failed[b], spans, cum, itype, per_h, first, last, active }
        })
        .filter(|c| !c.ends.is_empty())
        .collect();
    let priced = by_machine && prices.is_some_and(|p| !p.is_empty());
    let sig = format!("{split:?}{branch:?}");
    Curves { key: (cat.hash.clone(), priced, sig), boxes, lo: if lo == i64::MAX { 0 } else { lo }, hi: hi.max(lo.saturating_add(60)), priced }
}

// ── what a curve can show ──────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Measure {
    Built,
    Hours,
    Spend,
    PerPackage,
    Rate,
}

impl Measure {
    const ALL: [(Measure, &'static str, &'static str); 5] = [
        (Measure::Built, "built", "packages built so far, each line counting from its first"),
        (Measure::Hours, "build time", "hours of building finished so far: where a paradigm or language spends the machines' time"),
        (Measure::Spend, "spend", "what the machine has cost so far: its building time × its instance's $/h"),
        (Measure::PerPackage, "$ / package", "spend ÷ built: where it levels off is the price of a package on that machine (from the 50th)"),
        (Measure::Rate, "per hour", "packages finished in the hour before - steep stretches are many small packages, troughs one long compile"),
    ];

    fn label(self) -> &'static str {
        Measure::ALL.iter().find(|m| m.0 == self).map_or("", |m| m.1)
    }

    fn needs_prices(self) -> bool {
        matches!(self, Measure::Spend | Measure::PerPackage)
    }

    fn value(self, c: &BoxCurve, x: f64) -> Option<f64> {
        if x < c.first as f64 || x > c.last as f64 {
            return None;
        }
        match self {
            Measure::Built => Some(c.built(x)),
            Measure::Hours => Some(c.busy(x)),
            Measure::Spend => Some(c.spend(x)),
            Measure::PerPackage => {
                let n = c.built(x);
                (n >= 50.0).then(|| c.spend(x) / n)
            }
            Measure::Rate => Some(c.built(x) - c.built(x - 3600.0)),
        }
    }

    fn fmt(self, v: f64) -> String {
        match self {
            Measure::Built => thousands(v.round().max(0.0) as usize),
            Measure::Hours if v >= 10.0 => format!("{v:.0} h"),
            Measure::Hours => format!("{v:.1} h"),
            Measure::Spend if v >= 100.0 => format!("${v:.0}"),
            Measure::Spend => format!("${v:.2}"),
            Measure::PerPackage if v >= 1.0 => format!("${v:.2}"),
            Measure::PerPackage => format!("{:.1}¢", v * 100.0),
            Measure::Rate => format!("{v:.0}/h"),
        }
    }
}

/// A round number at or above `v` for an axis top.
fn nice_top(v: f64) -> f64 {
    if v <= 0.0 {
        return 1.0;
    }
    let p = 10f64.powf(v.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * p).find(|t| *t >= v).unwrap_or(10.0 * p)
}

/// The software a queue entry names: `HISAT2-2.2.1-gompi-2024a.eb` → hisat2,
/// `statsmodels` → statsmodels, `apptainer r4.6…` → apptainer.
fn entry_name(entry: &str) -> String {
    let s = entry.trim().trim_end_matches(".eb");
    let s = s.split_whitespace().next().unwrap_or(s);
    let b = s.as_bytes();
    let cut = (1..b.len()).find(|&i| b[i - 1] == b'-' && b[i].is_ascii_digit()).map_or(s.len(), |i| i - 1);
    s[..cut].to_lowercase()
}

pub(super) fn find_item(cat: &Catalog, name: &str) -> Option<usize> {
    let name = entry_name(name);
    if name.is_empty() {
        return None;
    }
    cat.lname.iter().position(|n| *n == name).or_else(|| cat.lname.iter().position(|n| n.starts_with(&name)))
}

// ── the views ──────────────────────────────────────────────────────────────

impl SoftwarePane {
    /// The machines' queues and the prices: asked when a Builds view shows,
    /// again every 30 s while it does.
    /// The Builds mode's top (operator, 2026-10-19: "a high performance
    /// window showing us precisely where builds are across the ndexr
    /// ecosystem"): a card per build machine - on or off, the step it is
    /// compiling and how far through by its estimate, the package lane, done
    /// and failed of its queue, when it should finish, and what is next. Asked
    /// every 10 s while shown; the clocks tick between answers.
    pub fn where_builds_are(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend) {
        let now = ctx.input(|i| i.time);
        if now - self.live_asked > 10.0 {
            self.live_asked = now - 30.0;
        }
        self.live_tick(backend, ctx);
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        let live = self.live.lock().unwrap();
        let Some(Ok(live)) = live.as_ref() else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("asking the build machines…").size(13.0).color(theme::text()));
            });
            return;
        };
        let since = (now_secs() - live.generated).max(0) as f64;
        let mut machines: Vec<&Machine> = live.eta.as_ref().map(|e| e.machines.iter().collect()).unwrap_or_default();
        // Compiling first, then on, then off; a machine silent for over a day
        // is one name on a line, not a card.
        machines.sort_by_key(|m| (!live.machine_live(m), m.running.is_none() || !live.machine_live(m), m.id.clone()));
        let gone: Vec<&Machine> = machines.iter().copied().filter(|m| !live.machine_live(m) && m.mirror_age_s.is_some_and(|a| a > 86_400)).collect();
        machines.retain(|m| !gone.iter().any(|g| std::ptr::eq(*g, *m)));
        let on = machines.iter().filter(|m| live.machine_live(m)).count();
        let building = machines.iter().filter(|m| live.machine_live(m) && m.running.is_some()).count();
        let total = machines.len();
        // On, with nothing compiling and nothing queued, is one name on a
        // line too: a card that can never change (operator, 2026-10-29: "this
        // seems to always be here and never goes anywhere" - this host's
        // desktop/pkgs, kept fresh by the package promote timer, suite all
        // done). It gets its card back the moment it has work.
        let idle: Vec<&Machine> = machines.iter().copied().filter(|m| live.machine_live(m) && m.running.is_none() && m.pending.is_empty()).collect();
        machines.retain(|m| !idle.iter().any(|i| std::ptr::eq(*i, *m)));
        // Wrapped: on a phone the line is wider than the screen, and one
        // over-wide line widened every card under it past the edge.
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("⚙").size(14.0).color(theme::primary())).on_hover_text("where builds are");
            ui.label(RichText::new(format!("{building} compiling · {on}/{total} on · {} ago", fmt_dur(since as i64))).size(12.5).color(theme::text()))
                .on_hover_text(format!("{building} compiling, {on} of {total} machines on, as of {} ago", fmt_dur(since as i64)));
        });
        if total == 0 && gone.is_empty() {
            ui.label(RichText::new("No build machine has reported.").size(13.0).color(theme::text()));
            return;
        }
        let gap = 8.0;
        let per_row = ((ui.available_width() + gap) / 300.0).floor().clamp(1.0, machines.len().max(1) as f32) as usize;
        let w = (ui.available_width() - gap * (per_row as f32 - 1.0)) / per_row as f32;
        let bar = |ui: &mut egui::Ui, f: f32, c: Color32| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 5.0), Sense::hover());
            ui.painter().rect_filled(r, 2, theme::surface_hi());
            ui.painter().rect_filled(Rect::from_min_size(r.min, egui::vec2(r.width() * f.clamp(0.0, 1.0), r.height())), 2, c);
        };
        for row in machines.chunks(per_row) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for m in row {
                    let up = live.machine_live(m);
                    let lane = live.lane(&m.id);
                    egui::Frame::new()
                        .fill(theme::surface().gamma_multiply(if up { 1.0 } else { 0.6 }))
                        .stroke(Stroke::new(1.0, if m.running.is_some() && up { theme::amber().gamma_multiply(0.7) } else { theme::pane_brd() }))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            // A card's lines stack; the row it sits in lays out sideways.
                            ui.spacing_mut().item_spacing.y = 2.0;
                            ui.set_width(w - 18.0);
                            ui.vertical(|ui| {
                            ui.horizontal(|ui| {
                                let (dot, c) = if !up { ("○", theme::hush()) } else if m.running.is_some() { ("●", theme::amber()) } else { ("●", theme::ok()) };
                                ui.label(RichText::new(dot).color(c));
                                let name = if m.name.is_empty() { m.id.rsplit('/').next().unwrap_or(&m.id) } else { &m.name };
                                ui.label(RichText::new(name).size(14.0).strong().color(theme::text_strong()));
                                ui.label(RichText::new(&m.arch).size(12.5).color(theme::text()));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let state = if !up { "off" } else if m.running.is_some() { "compiling" } else if m.pending.is_empty() { "idle" } else { "waiting" };
                                    ui.label(RichText::new(state).size(12.5).color(c));
                                });
                            });
                            if !up {
                                ui.label(RichText::new(format!("⟲ {} ago", fmt_dur(m.mirror_age_s.unwrap_or(0) + since as i64))).size(12.5).color(theme::text())).on_hover_text("last heard from");
                                return;
                            }
                            // A package from the set's lane, not an EasyBuild entry: no
                            // "for <easyconfig>" under it, and the suite's ETA is not its
                            // (operator, 2026-10-20: "this is confusing" - linearmodels
                            // shown "for HISAT2-2.2.1-gompi-2024a.eb, ~3m to go").
                            let pkg_now = m.running.as_ref().is_some_and(|r| lane.and_then(|l| l.now.as_deref()) == Some(r.name.as_str()));
                            if let Some(r) = &m.running {
                                let elapsed = r.elapsed_s.unwrap_or(0.0) + since;
                                // A set builds under its own name: "r4.6_py3.13-release", once.
                                let what = if r.version.is_empty() || r.name.contains(&r.version) { r.name.clone() } else { format!("{} {}", r.name, r.version) };
                                ui.add(egui::Label::new(RichText::new(what).monospace().size(12.5).color(theme::amber())).truncate());
                                match r.est_s.filter(|e| *e > 0.0) {
                                    Some(est) => {
                                        bar(ui, (elapsed / est) as f32, theme::amber());
                                        let left = est - elapsed;
                                        let (words, hover) = if left > 0.0 {
                                            (format!("⏱ {} / ~{}", fmt_dur(elapsed as i64), fmt_dur(est as i64)), format!("{} of an estimated ~{} · ~{} left", fmt_dur(elapsed as i64), fmt_dur(est as i64), fmt_dur(left as i64)))
                                        } else {
                                            (format!("⏱ {} / ~{} ⚠", fmt_dur(elapsed as i64), fmt_dur(est as i64)), format!("{} - past its ~{} estimate", fmt_dur(elapsed as i64), fmt_dur(est as i64)))
                                        };
                                        ui.label(RichText::new(words).size(12.5).color(if left > 0.0 { theme::text() } else { theme::amber() })).on_hover_text(hover);
                                    }
                                    None => {
                                        ui.label(RichText::new(format!("⏱ {}", fmt_dur(elapsed as i64))).size(12.5).color(theme::text())).on_hover_text("so far - no estimate yet");
                                    }
                                }
                                if let Some(t) = r.target.as_deref().filter(|t| !t.is_empty() && !pkg_now) {
                                    // Past its estimate, no "~0s to it": only the target.
                                    let left = r.target_remaining_s.map(|s| s - since).filter(|s| *s >= 1.0).map(|s| format!(" · ~{} to it", fmt_dur(s as i64))).unwrap_or_default();
                                    ui.add(egui::Label::new(RichText::new(format!("◎ {}{left}", t.trim_end_matches(".eb"))).size(12.5).color(theme::text())).truncate()).on_hover_text(format!("built for {t}"));
                                }
                            }
                            if let Some(l) = lane.filter(|l| l.of > 0) {
                                let now = l.now.as_deref().unwrap_or("");
                                ui.add(egui::Label::new(RichText::new(format!("◆ {}/{}{}", l.n, l.of, if now.is_empty() { String::new() } else { format!(" · {now}") })).size(12.5).color(theme::text())).truncate())
                                    .on_hover_text(format!("{} packages · {} of {}", l.kind.as_deref().unwrap_or(""), l.n, l.of));
                                bar(ui, l.n as f32 / l.of as f32, theme::primary());
                            }
                            let c = &m.counts;
                            if c.total > 0 {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(format!("🛠 {}/{}", c.done, c.total)).size(12.5).color(theme::text())).on_hover_text(format!("EasyBuild suite: {} of {} done", c.done, c.total));
                                    if c.failed > 0 {
                                        ui.label(RichText::new(format!("✕ {}", c.failed)).size(12.5).color(theme::alt())).on_hover_text(format!("{} failed", c.failed));
                                    }
                                    if let Some(rem) = m.remaining_s.filter(|r| *r > 0.0 && !pkg_now) {
                                        ui.label(RichText::new(format!("⏱ ~{}", fmt_dur((rem - since).max(0.0) as i64))).size(12.5).color(theme::text())).on_hover_text("left in the suite");
                                    }
                                });
                                bar(ui, c.done as f32 / c.total as f32, theme::ok());
                            }
                            let next: Vec<&str> = m.pending.iter().take(3).map(|p| p.entry.trim_end_matches(".eb")).collect();
                            if !next.is_empty() {
                                ui.add(egui::Label::new(RichText::new(format!("⏭ {}{}", next.join(", "), if m.pending.len() > 3 { format!(" +{}", m.pending.len() - 3) } else { String::new() })).size(12.5).color(theme::text())).truncate())
                                    .on_hover_text(format!("next in the suite: {}", m.pending.iter().map(|p| p.entry.trim_end_matches(".eb")).collect::<Vec<_>>().join(", ")));
                            }
                            });
                        });
                }
            });
            ui.add_space(gap);
        }
        // The quiet machines on one line: an icon for each kind of quiet.
        if !idle.is_empty() || !gone.is_empty() {
            ui.horizontal_wrapped(|ui| {
                if !idle.is_empty() {
                    ui.label(RichText::new(format!("○ {}", off_names(&idle).join(", "))).size(12.5).color(theme::text())).on_hover_text("on, nothing to build");
                }
                if !gone.is_empty() {
                    ui.label(RichText::new(format!("○ {}", off_names(&gone).join(", "))).size(12.5).color(theme::hush())).on_hover_text("off for over a day");
                }
            });
        }
    }

    pub(super) fn live_tick(&mut self, backend: &Backend, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if let Some((server, verb)) = self.box_act.take() {
            *self.box_job.lock().unwrap() = Some(crate::api::JobView { what: format!("{server}: {verb}"), running: true, ..Default::default() });
            self.box_job_server = Some(server.clone());
            self.box_job_since = now;
            self.box_job_poll = now;
            let job = self.box_job.clone();
            backend.query(crate::api::Query::BoxAct { server, verb }, ctx, move |r: Result<crate::api::JobView, String>| {
                *job.lock().unwrap() = Some(r.unwrap_or_else(|e| crate::api::JobView { what: "box".into(), ok: Some(false), lines: vec![e], ..Default::default() }));
            });
            // Its state shows sooner than the next half minute.
            self.live_asked = now - 25.0;
        }
        // While it runs: its job every 2 s, the machines every 10 s.
        if let Some(server) = self.box_job_server.clone() {
            let running = self.box_job.lock().unwrap().as_ref().is_none_or(|j| j.running);
            if running {
                if now - self.box_job_poll > 2.0 {
                    self.box_job_poll = now;
                    let job = self.box_job.clone();
                    backend.query(crate::api::Query::BoxJob { server }, ctx, move |r: Result<crate::api::JobView, String>| {
                        if let Ok(v) = r {
                            if !v.what.is_empty() || !v.lines.is_empty() {
                                *job.lock().unwrap() = Some(v);
                            }
                        }
                    });
                    if now - self.live_asked > 10.0 {
                        self.live_asked = now - 30.0;
                    }
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(1000));
            }
        }
        if now - self.live_asked < 30.0 {
            return;
        }
        self.live_asked = now;
        // The machines, for an admin (a refusal leaves the Queue as it was).
        if !self.public {
            let slot = self.boxes.clone();
            backend.query(crate::api::Query::Boxes, ctx, move |r: Result<Vec<crate::api::BoxView>, String>| {
                *slot.lock().unwrap() = Some(r);
            });
        }
        let slot = self.live.clone();
        backend.builds(ctx, move |r| {
            let parsed = r.and_then(|b| serde_json::from_slice::<Live>(&b).map_err(|e| format!("builds.json did not read: {e}")));
            let mut s = slot.lock().unwrap();
            // Keep the last good answer through a failed refresh.
            if parsed.is_ok() || !matches!(*s, Some(Ok(_))) {
                *s = Some(parsed);
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_secs(30));
    }

    fn prices(&self) -> Option<HashMap<String, f64>> {
        match &*self.live.lock().unwrap() {
            Some(Ok(l)) => l.prices.clone(),
            _ => None,
        }
    }

    fn curves_for(&mut self, cat: &Catalog) -> Arc<Curves> {
        let prices = self.prices();
        let dims = self.br.dims_for(cat);
        let by_machine = self.br.split == Dim::Machine && self.br.branch.iter().all(|(d, _)| matches!(d, Dim::Machine | Dim::Arch));
        let key = (cat.hash.clone(), by_machine && prices.as_ref().is_some_and(|p| !p.is_empty()), self.br.sig());
        match &self.curves {
            Some(c) if c.key == key => c.clone(),
            _ => {
                let c = Arc::new(curves(cat, &dims, prices.as_ref(), self.br.split, &self.br.branch));
                self.curves = Some(c.clone());
                c
            }
        }
    }

    // ── Curves ────────────────────────────────────────────────────────────

    pub(super) fn curves_view(&mut self, ui: &mut egui::Ui, cat: &Catalog) {
        let dims = self.br.dims_for(cat);
        branches::branch_strip(ui, &dims, &mut self.br.branch);
        theme::strip(ui, 22.0, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.label(RichText::new("a line per").size(12.0).color(theme::text()));
            for (d, name, tip) in Dim::ALL {
                let on = self.br.split == d;
                let t = RichText::new(name).size(11.5).color(if on { theme::text_strong() } else { theme::muted() });
                if ui.add(egui::Button::new(t).fill(if on { theme::surface_hi() } else { Color32::TRANSPARENT }).corner_radius(4)).on_hover_text(tip).clicked() {
                    self.br.split = d;
                }
            }
        });
        let cv = self.curves_for(cat);
        // Spend is a machine's: with other lines it falls back to the count.
        if !cv.priced && self.measure.needs_prices() {
            self.measure = Measure::Built;
        }
        if !cv.priced && self.overlay.is_some_and(|o| o.needs_prices()) {
            self.overlay = None;
        }
        if cv.boxes.is_empty() {
            ui.add_space(14.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("No machine has finished a timed build yet.").color(theme::hush())));
            return;
        }
        // To now: the gap after the last finish says the machines are idle.
        let now = (now_secs() - cat.t0) as f64;
        let full = (cv.lo as f64, (cv.hi as f64).max(now));
        let ax = Axis::new(&cv, self.calendar);
        let (x0, x1) = self.cwin.unwrap_or(full);
        let mut fit = false;
        let mut window: Option<(f64, f64)> = None;
        theme::strip(ui, 22.0, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = 3.0;
            for (m, name, tip) in Measure::ALL {
                let usable = cv.priced || !m.needs_prices();
                let on = self.measure == m;
                let t = RichText::new(name).size(11.5).color(if on { theme::text_strong() } else if usable { theme::muted() } else { theme::hush() });
                let r = ui.add_enabled(usable, egui::Button::new(t).fill(if on { theme::surface_hi() } else { Color32::TRANSPARENT }).corner_radius(4));
                if r.on_hover_text(tip).on_disabled_hover_text("a machine's cost: with a line per machine (and a focus no narrower than a machine), once the instance prices are in").clicked() {
                    if self.overlay == Some(m) {
                        self.overlay = Some(self.measure);
                    }
                    self.measure = m;
                }
            }
            theme::tool_sep(ui);
            ui.label(RichText::new("over it").size(12.0).color(theme::text()));
            let shown = self.overlay.map_or("nothing", |m| m.label());
            egui::ComboBox::from_id_salt("bc-over").width(96.0).selected_text(RichText::new(shown).size(11.5)).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.overlay, None, "nothing");
                for (m, name, _) in Measure::ALL {
                    if m != self.measure && (cv.priced || !m.needs_prices()) {
                        ui.selectable_value(&mut self.overlay, Some(m), name);
                    }
                }
            });
            if let Some(o) = self.overlay {
                if theme::tool(ui, "⇄", "swap: put the dashed one in front").clicked() {
                    self.overlay = Some(self.measure);
                    self.measure = o;
                }
            }
            theme::tool_sep(ui);
            fit = theme::tool(ui, "Fit", "everything built").clicked();
            for (name, secs) in [("24 h", 86400.0), ("6 h", 6.0 * 3600.0), ("1 h", 3600.0)] {
                if theme::tool(ui, name, &format!("the last {name} of building")).clicked() {
                    window = Some((ax.back(ax.fwd(full.1) - secs), full.1));
                }
            }
            // Moving through time without Shift+drag (a phone has none,
            // operator 2026-10-18: "there's no scroll left or right"): a half
            // window back or on, and the same width ending now.
            theme::tool_sep(ui);
            let (c0, c1, cf) = (ax.fwd(x0), ax.fwd(x1), ax.fwd(full.1));
            let w = (c1 - c0).max(60.0);
            if theme::tool(ui, "◀", "earlier: half a window back").clicked() {
                window = Some((ax.back(c0 - w / 2.0), ax.back(c1 - w / 2.0)));
            }
            if theme::tool(ui, "▶", "later: half a window on").clicked() {
                let to = (c1 + w / 2.0).min(cf);
                window = Some((ax.back(to - w), ax.back(to)));
            }
            if theme::tool(ui, "Now", "this width, ending now").clicked() {
                window = Some((ax.back(cf - w), full.1));
            }
            theme::tool_sep(ui);
            // Across building time (the default) or across the calendar.
            for (cal, name, tip) in [
                (false, "building time", "only the hours some machine was building: the days the boxes sat off are cut out"),
                (true, "calendar", "every hour of the calendar, idle days included"),
            ] {
                let on = self.calendar == cal;
                let t = RichText::new(name).size(11.5).color(if on { theme::text_strong() } else { theme::muted() });
                if ui.add(egui::Button::new(t).fill(if on { theme::surface_hi() } else { Color32::TRANSPARENT }).corner_radius(4)).on_hover_text(tip).clicked() && !on {
                    self.calendar = cal;
                }
            }
        });
        if fit {
            self.cwin = None;
        }
        if let Some(w) = window {
            self.cwin = Some(w);
        }
        // A new stretch of time fits its own heights again.
        if fit || window.is_some() {
            self.cywin = None;
        }
        let (x0, x1) = if fit { full } else { window.unwrap_or((x0, x1)) };

        // The chart takes the top, the moment (or the machines) the rest.
        let avail = ui.available_rect_before_wrap();
        let chart_h = (avail.height() * if self.pinned.is_some() { 0.55 } else { 0.62 }).max(160.0);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(avail.width(), chart_h), Sense::click_and_drag());
        let plot = Rect::from_min_max(rect.min + egui::vec2(52.0, 10.0), rect.max - egui::vec2(if self.overlay.is_some() { 52.0 } else { 14.0 }, 22.0));
        // Across the axis (building time or the calendar), values at real moments.
        let (c0, c1) = (ax.fwd(x0), ax.fwd(x1).max(ax.fwd(x0) + 1.0));
        let to_x = |t: f64| plot.left() + ((ax.fwd(t) - c0) / (c1 - c0)) as f32 * plot.width();
        let from_x = |px: f32| ax.back(c0 + ((px - plot.left()) / plot.width().max(1.0)) as f64 * (c1 - c0));
        let cols = (plot.width() / 2.0).max(10.0) as usize;
        // A column where the line's machine was off is a gap, not a flat run.
        let half = (c1 - c0) / cols.max(1) as f64 / 2.0;
        let sample = |m: Measure, c: &BoxCurve| -> Vec<Option<f64>> {
            (0..=cols)
                .map(|i| {
                    let xc = c0 + (c1 - c0) * i as f64 / cols as f64;
                    let x = ax.back(xc);
                    if c.on(ax.back(xc - half), ax.back(xc + half)) { m.value(c, x) } else { None }
                })
                .collect()
        };
        let series: Vec<Vec<Option<f64>>> = cv.boxes.iter().map(|c| sample(self.measure, c)).collect();
        let over: Option<Vec<Vec<Option<f64>>>> = self.overlay.map(|m| cv.boxes.iter().map(|c| sample(m, c)).collect());
        let top = |s: &Vec<Vec<Option<f64>>>| nice_top(s.iter().flatten().flatten().copied().fold(0.0, f64::max));
        let ytop = top(&series);
        let otop = over.as_ref().map(top);
        // The vertical window a drag picked, while the same measure shows.
        let (y0, y1) = match self.cywin {
            Some((m, a, b)) if m == self.measure && b > a => (a, b),
            _ => (0.0, ytop),
        };

        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, theme::bg());
        // Grid: time across, four steps up, each side its own measure.
        let step = tick_step((c1 - c0) / (plot.width() / 110.0).max(1.0) as f64);
        let cx = |c: f64| plot.left() + ((c - c0) / (c1 - c0)) as f32 * plot.width();
        if ax.compressed() {
            // Hours of building along the bottom; where a gap was cut, a
            // dashed line and the day building took up again along the top.
            let mut c = (c0 / step).ceil() * step;
            while c <= c1 {
                let x = cx(c);
                p.vline(x, plot.y_range(), Stroke::new(1.0, theme::pane_brd().gamma_multiply(0.5)));
                let h = c / 3600.0;
                let label = if step >= 3600.0 { format!("{h:.0} h") } else { format!("{h:.1} h") };
                p.text(egui::pos2(x + 2.0, plot.bottom() + 4.0), Align2::LEFT_TOP, label, FontId::proportional(10.0), theme::hush());
                c += step;
            }
            let mut last_label = f32::NEG_INFINITY;
            for (c, resumed) in ax.cuts().filter(|(c, _)| *c >= c0 && *c <= c1) {
                let x = cx(c);
                p.extend(Shape::dashed_line(&[egui::pos2(x, plot.top()), egui::pos2(x, plot.bottom())], Stroke::new(1.0, theme::muted().gamma_multiply(0.5)), 3.0, 4.0));
                if x - last_label > 64.0 {
                    p.text(egui::pos2(x + 2.0, plot.top() + 14.0), Align2::LEFT_TOP, super::fmt_day(resumed as i64 + cat.t0), FontId::proportional(10.0), theme::muted());
                    last_label = x;
                }
            }
        } else {
            let mut t = ((x0 + cat.t0 as f64) / step).ceil() * step - cat.t0 as f64;
            while t <= x1 {
                let x = to_x(t);
                p.vline(x, plot.y_range(), Stroke::new(1.0, theme::pane_brd().gamma_multiply(0.5)));
                let abs = t as i64 + cat.t0;
                p.text(egui::pos2(x + 2.0, plot.bottom() + 4.0), Align2::LEFT_TOP, if step >= 86400.0 { super::fmt_day(abs) } else { fmt_date(abs) }, FontId::proportional(10.0), theme::hush());
                t += step;
            }
        }
        for q in 0..=4 {
            let y = plot.bottom() - plot.height() * q as f32 / 4.0;
            p.hline(plot.x_range(), y, Stroke::new(1.0, theme::pane_brd().gamma_multiply(if q == 0 { 1.0 } else { 0.4 })));
            p.text(egui::pos2(plot.left() - 6.0, y), Align2::RIGHT_CENTER, self.measure.fmt(y0 + (y1 - y0) * q as f64 / 4.0), FontId::proportional(10.0), theme::muted());
            if let (Some(o), Some(ot)) = (self.overlay, otop) {
                p.text(egui::pos2(plot.right() + 6.0, y), Align2::LEFT_CENTER, o.fmt(ot * q as f64 / 4.0), FontId::proportional(10.0), theme::hush());
            }
        }
        let to_y = |v: f64, lo: f64, hi: f64| plot.bottom() - ((v - lo) / (hi - lo).max(1e-9)) as f32 * plot.height();
        let from_y = |py: f32| y0 + ((plot.bottom() - py) / plot.height().max(1.0)) as f64 * (y1 - y0);
        let line = |vals: &[Option<f64>], lo: f64, hi: f64| -> Vec<Vec<egui::Pos2>> {
            let mut parts = vec![Vec::new()];
            for (i, v) in vals.iter().enumerate() {
                match v {
                    Some(v) => parts.last_mut().unwrap().push(egui::pos2(plot.left() + plot.width() * i as f32 / cols as f32, to_y(*v, lo, hi))),
                    None if !parts.last().unwrap().is_empty() => parts.push(Vec::new()),
                    None => {}
                }
            }
            parts
        };
        let pc = ui.painter_at(plot.expand(2.0));
        if let (Some(ov), Some(ot)) = (&over, otop) {
            for (c, vals) in cv.boxes.iter().zip(ov) {
                for pts in line(vals, 0.0, ot) {
                    if pts.len() > 1 {
                        pc.extend(Shape::dashed_line(&pts, Stroke::new(1.2, c.color.gamma_multiply(0.7)), 5.0, 4.0));
                    }
                }
            }
        }
        for (c, vals) in cv.boxes.iter().zip(&series) {
            for pts in line(vals, y0, y1) {
                if pts.len() > 1 {
                    pc.add(Shape::line(pts, Stroke::new(2.0, c.color)));
                }
            }
        }

        // A drag selects (brush.rs): across for a stretch of time, down for a
        // stretch of the measure, aslant for both; Shift+drag pans.
        let w = (x1 - x0).max(1.0);
        let mut brushing = false;
        let mut pan = if resp.hovered() { ui.input(|i| i.smooth_scroll_delta.x) } else { 0.0 };
        match brush::drag(ui, &resp, plot, egui::Id::new("bc-brush"), true) {
            brush::Drag::Pan(d) => pan += d.x,
            brush::Drag::Brushing => brushing = true,
            brush::Drag::Pick(brush::Pick::X(a, b)) => self.cwin = Some((from_x(a), from_x(b).max(from_x(a) + 120.0))),
            brush::Drag::Pick(brush::Pick::Y(a, b)) => self.cywin = Some((self.measure, from_y(b), from_y(a))),
            brush::Drag::Pick(brush::Pick::Both(r)) => {
                self.cwin = Some((from_x(r.left()), from_x(r.right()).max(from_x(r.left()) + 120.0)));
                self.cywin = Some((self.measure, from_y(r.bottom()), from_y(r.top())));
            }
            brush::Drag::None => {}
        }
        if pan != 0.0 {
            let dc = -(pan as f64) / plot.width().max(1.0) as f64 * (c1 - c0);
            self.cwin = Some((ax.back(c0 + dc), ax.back(c1 + dc)));
        }
        if self.cywin.is_some_and(|(m, ..)| m == self.measure) {
            p.text(plot.left_top() + egui::vec2(6.0, 2.0), Align2::LEFT_TOP, "zoomed up and down · double-click for all of it", FontId::proportional(10.0), theme::hush());
        }

        // Now, labelled on the axis.
        if now >= x0 && now <= x1 {
            let x = to_x(now);
            p.vline(x, plot.y_range(), Stroke::new(1.5, super::now_color().gamma_multiply(0.8)));
            p.text(egui::pos2(x - 3.0, plot.top() + 2.0), Align2::RIGHT_TOP, format!("now {}", fmt_date(now as i64 + cat.t0).rsplit(' ').next().unwrap_or("")), FontId::proportional(10.0), super::now_color());
        }

        // Hover reads every machine; a click pins the moment.
        let hover_x = resp.hover_pos().filter(|q| plot.contains(*q) && !brushing).map(|q| from_x(q.x));
        if let Some(px) = self.pinned.filter(|t| *t >= x0 && *t <= x1) {
            let x = to_x(px);
            p.vline(x, plot.y_range(), Stroke::new(1.5, theme::amber()));
            for c in &cv.boxes {
                if let Some(v) = self.measure.value(c, px) {
                    let y = to_y(v, y0, y1);
                    if plot.y_range().contains(y) {
                        p.circle_filled(egui::pos2(x, y), 3.5, c.color);
                    }
                }
            }
        }
        if let Some(hx) = hover_x {
            let x = to_x(hx);
            p.vline(x, plot.y_range(), Stroke::new(1.0, theme::muted().gamma_multiply(0.6)));
            egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), egui::Id::new("bc-tip"), egui::PopupAnchor::Pointer).show(|ui| {
                ui.label(RichText::new(format!("{} UTC", fmt_date(hx as i64 + cat.t0))).strong());
                for c in &cv.boxes {
                    let a = self.measure.value(c, hx).map_or("-".into(), |v| self.measure.fmt(v));
                    let b = self.overlay.and_then(|o| o.value(c, hx).map(|v| format!(" · {}", o.fmt(v)))).unwrap_or_default();
                    ui.label(RichText::new(format!("● {}  {a}{b}", c.name)).color(c.color));
                }
                ui.label(RichText::new("click to see what finished here").size(10.5).color(theme::hush()));
            });
        }
        if resp.clicked() {
            if let Some(hx) = hover_x {
                self.pinned = Some(hx);
            }
        }
        if resp.double_clicked() {
            self.cwin = None;
            self.cywin = None;
        }
        if resp.hovered() {
            let z = ui.input(|i| i.zoom_delta());
            if (z - 1.0).abs() > 1e-3 {
                if let Some(q) = resp.hover_pos() {
                    let cw = c1 - c0;
                    let c = ax.fwd(from_x(q.x));
                    let nw = (cw / z as f64).max(120.0);
                    let f = (c - c0) / cw;
                    self.cwin = Some((ax.back(c - f * nw), ax.back(c - f * nw + nw)));
                }
            }
        }

        // Under the chart: the moment, or the machines as a whole.
        ui.add_space(4.0);
        let mut focus = None;
        egui::ScrollArea::vertical().id_salt("bc-under").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| match self.pinned {
                Some(px) => self.moment(ui, cat, &cv, px, (w / plot.width().max(1.0) as f64 * 8.0).max(60.0)),
                None => focus = machines_table(ui, cat, &cv),
            });
        });
        // A line's name in the table focuses the curves on it: the machine
        // comparison 3D's lanes were for (operator, 2026-11-01: "remove 3D";
        // "toggle Intel, ARM, GPU, AMD ... without creating separate charts").
        if let Some(code) = focus {
            let split = self.br.split;
            if !self.br.branch.iter().any(|(d, _)| *d == split) {
                self.br.branch.push((split, code));
                self.cwin = None;
            }
        }
    }

    /// What a pinned moment shows: each machine there, what finished around
    /// it, and what was still compiling - the step that holds a curve flat.
    fn moment(&mut self, ui: &mut egui::Ui, cat: &Catalog, cv: &Curves, px: f64, near: f64) {
        let mut go: Option<f64> = None;
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} UTC", fmt_date(px as i64 + cat.t0))).font(theme::display(13.0)).color(theme::amber()));
            // Step to the finish before / after, across every machine.
            let prev = cv.boxes.iter().filter_map(|c| c.ends.iter().rev().find(|&&e| (e as f64) < px - 0.5).copied()).max();
            let next = cv.boxes.iter().filter_map(|c| c.ends.iter().find(|&&e| (e as f64) > px + 0.5).copied()).min();
            if theme::tool_if(ui, prev.is_some(), "◀", "the finish before").clicked() {
                go = prev.map(|e| e as f64);
            }
            if theme::tool_if(ui, next.is_some(), "▶", "the finish after").clicked() {
                go = next.map(|e| e as f64);
            }
            if theme::tool(ui, "✕", "unpin").clicked() {
                close = true;
            }
            ui.label(RichText::new(format!("± {} around it", fmt_dur(near as i64))).size(10.5).color(theme::hush()));
        });
        if let Some(g) = go {
            self.pinned = Some(g);
        }
        if close {
            self.pinned = None;
            return;
        }
        let mut open = self.inventory(ui, cat, px);
        // The package chips below are coloured by the env set they are built for.
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            // A square per manifest group, its sets on hover: the full names
            // were a paragraph of text under the curves (operator, 2026-11-06).
            ui.label(RichText::new("◐").size(11.5).color(theme::hush())).on_hover_text("chip colours: the env manifests a package is in; in none - by paradigm");
            for (g, (label, n)) in cat.sets.iter().enumerate() {
                ui.label(RichText::new("■").size(13.0).color(super::Catalog::group_color(g))).on_hover_text(format!("{label}\n{n} packages are in exactly these env manifests"));
            }
        });
        for c in &cv.boxes {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("● {}", c.name)).strong().color(c.color));
                let built = c.built(px);
                let mut bits = vec![format!("{} built", thousands(built as usize)), format!("{:.0}/h then", Measure::Rate.value(c, px).unwrap_or(0.0)), format!("{} of building", Measure::Hours.fmt(c.busy(px)))];
                if cv.priced {
                    bits.push(format!("{} spent", Measure::Spend.fmt(c.spend(px))));
                    if let Some(v) = Measure::PerPackage.value(c, px) {
                        bits.push(format!("{} a package", Measure::PerPackage.fmt(v)));
                    }
                }
                ui.label(RichText::new(bits.join(" · ")).size(11.5).color(theme::muted()));
            });
            // Still compiling at that moment, longest first.
            let mut busy: Vec<&(i64, i64, u32)> = c.steps.iter().filter(|s| (s.0 as f64) <= px && px < s.1 as f64).collect();
            busy.sort_by_key(|s| std::cmp::Reverse(s.1 - s.0));
            if !busy.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("compiling:").size(10.5).color(theme::hush()));
                    for s in busy.iter().take(4) {
                        let i = cat.st_i[s.2 as usize] as usize;
                        let r = super::chip(ui, &format!("{} {}", cat.items[i].n, fmt_dur(s.1 - s.0)), cat.set_color(i).unwrap_or(cat.kind[i].color()), false)
                            .on_hover_text(format!("{} of its {} done here - a long step like this is what flattens a curve", fmt_dur((px as i64 - s.0).max(0)), fmt_dur(s.1 - s.0)));
                        if r.clicked() {
                            open = Some(i);
                        }
                    }
                });
            }
            let a = c.steps.partition_point(|s| (s.1 as f64) < px - near);
            let b = c.steps.partition_point(|s| (s.1 as f64) <= px + near);
            if a < b {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(format!("finished ({}):", b - a)).size(10.5).color(theme::hush()));
                    for s in &c.steps[a..b.min(a + 40)] {
                        let i = cat.st_i[s.2 as usize] as usize;
                        let set = cat.items[i].s.join(", ");
                        let r = super::chip(ui, &cat.items[i].n, cat.set_color(i).unwrap_or(cat.kind[i].color()), false).on_hover_text(format!(
                            "{} · {} · took {} · done {} UTC",
                            cat.kind[i].long(),
                            if set.is_empty() { "in no env set".to_owned() } else { format!("for {set}") },
                            fmt_dur(s.1 - s.0),
                            fmt_date(s.1 + cat.t0)
                        ));
                        if r.clicked() {
                            open = Some(i);
                        }
                    }
                    if b - a > 40 {
                        ui.label(RichText::new(format!("+{} more - zoom in", b - a - 40)).size(10.5).color(theme::hush()));
                    }
                });
            }
        }
        if let Some(i) = open {
            self.open(i);
        }
    }

    /// Everything installed at the pinned moment, per lane: each package's
    /// first finished build there at or before it, counted by paradigm, and
    /// found by name (referral #780: "the full inventory installed at that
    /// moment"). The item a found chip opens, if clicked.
    fn inventory(&mut self, ui: &mut egui::Ui, cat: &Catalog, px: f64) -> Option<usize> {
        if self.inv.as_ref().is_none_or(|(h, _)| *h != cat.hash) {
            self.inv = Some((cat.hash.clone(), Arc::new(cat.installs_by_lane())));
        }
        let inv = self.inv.as_ref().map(|(_, v)| v.clone()).unwrap_or_default();
        let mut open = None;
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("Installed by then").strong().color(theme::text_strong()));
            ui.add(egui::TextEdit::singleline(&mut self.inv_q).hint_text("find a package").desired_width(170.0));
        });
        let q = self.inv_q.trim().to_lowercase();
        for (lane, list) in inv.iter() {
            let upto = list.partition_point(|(e, _)| (*e as f64) <= px);
            if upto == 0 {
                continue;
            }
            let mut by = [0usize; 8];
            for (_, i) in &list[..upto] {
                by[cat.kind[*i as usize] as usize] += 1;
            }
            let name = if lane == "cuda" { "x86_64 + CUDA" } else { lane.as_str() };
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(RichText::new(format!("{name} {}", thousands(upto))).monospace().strong().color(theme::text()));
                for k in super::Kind::ALL.into_iter().chain([super::Kind::Other]).filter(|k| by[*k as usize] > 0) {
                    ui.label(RichText::new(format!("{} {}", k.label(), thousands(by[k as usize]))).monospace().size(11.5).color(k.color()));
                }
            });
            if q.is_empty() {
                continue;
            }
            let hits: Vec<&(i64, u32)> = list[..upto].iter().filter(|(_, i)| cat.lname[*i as usize].contains(&q)).collect();
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if hits.is_empty() {
                    ui.label(RichText::new(format!("no \"{q}\" on {name} by then")).size(11.0).color(theme::hush()));
                }
                for (e, i) in hits.iter().take(40) {
                    let i = *i as usize;
                    let r = super::chip(ui, &cat.items[i].n, cat.set_color(i).unwrap_or(cat.kind[i].color()), false)
                        .on_hover_text(format!("{} · first built on {name} {} UTC", cat.kind[i].long(), fmt_date(e + cat.t0)));
                    if r.clicked() {
                        open = Some(i);
                    }
                }
                if hits.len() > 40 {
                    ui.label(RichText::new(format!("+{} more", hits.len() - 40)).size(10.5).color(theme::hush()));
                }
            });
        }
        ui.separator();
        open
    }

    // ── Queue ─────────────────────────────────────────────────────────────

    /// The machines and their power, as n-box.ndexr.io has them - for an
    /// ndexr admin at cove.ndexr.io only: anyone else's answer is a refusal,
    /// and then this draws nothing (boxes.rs).
    fn machines_panel(&mut self, ui: &mut egui::Ui) {
        let Some(Ok(boxes)) = self.boxes.lock().unwrap().clone() else { return };
        if boxes.is_empty() {
            return;
        }
        super::section(ui, "Machines · admin");
        let job = self.box_job.lock().unwrap().clone();
        egui::Grid::new("bq-boxes").striped(true).spacing(egui::vec2(12.0, 6.0)).show(ui, |ui| {
            for b in &boxes {
                let c = match b.state.as_str() {
                    "running" => theme::ok(),
                    "stale" | "failed" => theme::alt(),
                    "starting" => theme::amber(),
                    _ => theme::hush(),
                };
                ui.label(RichText::new(&b.server).monospace().size(12.0).color(theme::text_strong()));
                ui.label(RichText::new(format!("{} · {}", b.arch, b.instance)).size(11.0).color(theme::hush()));
                let step = if b.steps > 0 && !b.step.is_empty() { format!(" · step {}/{} {}", b.step_index, b.steps, b.step) } else { String::new() };
                let quiet = if b.state == "stale" { format!(" · log quiet {}", fmt_dur(b.quiet_s)) } else { String::new() };
                ui.label(RichText::new(format!("{}{}{step}{quiet}", if b.up { "● " } else { "○ " }, b.state)).size(11.5).color(c))
                    .on_hover_text(if b.up { "on, with an address - billing" } else { "powered off: its disk is kept" });
                ui.horizontal(|ui| {
                    let mut act = |verb: &str| self.box_act = Some((b.server.clone(), verb.to_owned()));
                    if ui.small_button("Kick off").on_hover_text("start the machine and `ndexr stack up` on it (a stale build is stopped first)").clicked() {
                        act("up");
                    }
                    if ui.add_enabled(b.up, egui::Button::new("Stop build").small()).on_hover_text("`ndexr stack down` on the box; it stays on").clicked() {
                        act("down");
                    }
                    if ui.add_enabled(b.up, egui::Button::new("Shut down").small()).on_hover_text("power it off, keeping its disk - the next Kick off resumes").clicked() {
                        act("shutdown");
                    }
                    // Terminate destroys the machine and its disk: a second click.
                    let armed = self.box_confirm.as_deref() == Some(b.server.as_str());
                    let label = if armed { RichText::new("Terminate - sure?").color(theme::alt()) } else { RichText::new("Terminate") };
                    if ui.small_button(label).on_hover_text("destroy the machine and its disk").clicked() {
                        if armed {
                            self.box_confirm = None;
                            self.box_act = Some((b.server.clone(), "terminate".into()));
                        } else {
                            self.box_confirm = Some(b.server.clone());
                        }
                    }
                });
                ui.end_row();
            }
        });
        if let Some(j) = job {
            // In words, not the raw job: where Kick off has got to.
            let b = self.box_job_server.as_deref().and_then(|s| boxes.iter().find(|b| b.server == s));
            let (what, done, failed) = crate::api::start_phase(&j, b);
            let c = if failed { theme::alt() } else if done { theme::ok() } else { theme::amber() };
            let secs = (ui.input(|i| i.time) - self.box_job_since).max(0.0) as u64;
            ui.horizontal(|ui| {
                if !done {
                    ui.add(egui::Spinner::new().size(12.0));
                }
                let who = self.box_job_server.as_deref().unwrap_or("");
                ui.label(RichText::new(format!("{who} · {what}")).size(11.5).strong().color(c));
                ui.label(RichText::new(format!("{}m {:02}s", secs / 60, secs % 60)).monospace().size(10.5).color(theme::hush()));
            });
            let last = j.lines.iter().rev().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("");
            if !last.is_empty() {
                ui.label(RichText::new(last).monospace().size(10.5).color(theme::hush()));
            }
        }
        ui.add_space(10.0);
    }

    pub(super) fn queue_view(&mut self, ui: &mut egui::Ui, cat: &Catalog) {
        let live = self.live.lock().unwrap().take();
        let Some(live) = live else {
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.spinner();
                ui.label(RichText::new("asking the machines what they are building…").color(theme::muted()));
            });
            return;
        };
        let mut open = None;
        match &live {
            Err(e) => {
                egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    ui.label(RichText::new(e).color(theme::alt()));
                });
            }
            Ok(l) => {
                let (ms, off): (Vec<&Machine>, Vec<&Machine>) = l.eta.as_ref().map(|e| e.machines.iter().partition(|m| l.machine_live(m))).unwrap_or_default();
                let (busy, idle): (Vec<&Machine>, Vec<&Machine>) = ms.into_iter().partition(|m| m.running.is_some() || !m.pending.is_empty());
                theme::strip(ui, 22.0, theme::bg(), |ui| {
                    ui.add_space(8.0);
                    let at = l.eta_at.unwrap_or(l.generated);
                    ui.label(RichText::new(format!("{} building · {} idle", busy.len(), idle.len())).size(11.5).color(theme::text()));
                    ui.label(RichText::new(format!("· estimates {}", super::fmt_ago(now_secs(), at))).size(10.5).color(theme::hush()))
                        .on_hover_text("`ndexr aggregate`'s builds.eta, the same numbers as build.ndexr.io's \"to build\" page; refreshed every 30 s here");
                });
                egui::ScrollArea::vertical().id_salt("bq").auto_shrink([false, false]).show(ui, |ui| {
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                        self.machines_panel(ui);
                        if busy.is_empty() {
                            ui.label(RichText::new("Nothing is building or queued right now.").color(theme::hush()));
                        }
                        for m in &busy {
                            machine_card(ui, cat, m, l.lane(&m.id), &mut open);
                            ui.add_space(10.0);
                        }
                        if !idle.is_empty() {
                            super::section(ui, "Nothing queued");
                            ui.horizontal_wrapped(|ui| {
                                for m in &idle {
                                    let c = if m.counts.failed > 0 { theme::alt() } else { theme::muted() };
                                    ui.label(RichText::new(format!("{} · {}/{}", m.id, m.counts.done, m.counts.total)).size(11.0).color(c))
                                        .on_hover_text(format!("{} · {} · {}{}", m.kind, m.arch, if m.plan.is_empty() { "-" } else { &m.plan }, if m.counts.failed > 0 { format!(" · {} failed", m.counts.failed) } else { String::new() }));
                                    ui.add_space(10.0);
                                }
                            });
                        }
                        // Off: one quiet line, named, so none seems to vanish.
                        if !off.is_empty() {
                            ui.add_space(8.0);
                            let names: Vec<String> = off.iter().map(|m| format!("{} ({})", m.id, m.mirror_age_s.map_or("never".into(), |a| format!("{} ago", fmt_dur(a))))).collect();
                            ui.label(RichText::new(format!("off: {}", names.join(" · "))).size(10.5).color(theme::hush()))
                                .on_hover_text(format!("not heard from in {} min - its last report is kept until it reports again", LIVE_S / 60));
                        }
                    });
                });
            }
        }
        *self.live.lock().unwrap() = Some(live);
        if let Some(i) = open {
            self.open(i);
        }
    }
}

/// The lines as a whole, under the chart until a moment is pinned: machines
/// with what they cost, any other split with its build time and its share.
/// Returns the line whose name was clicked, to focus on.
fn machines_table(ui: &mut egui::Ui, cat: &Catalog, cv: &Curves) -> Option<u16> {
    let mut focus = None;
    let machines = cv.boxes.iter().all(|c| c.machine.is_some());
    let all: usize = cv.boxes.iter().map(|c| c.ends.len()).sum();
    let all_secs: i64 = cv.boxes.iter().map(|c| c.secs.last().copied().unwrap_or(0)).sum();
    // A phone keeps what decides a machine's worth - built, failed, spent,
    // per package - rather than cut the right-hand columns off (IMG_8268).
    let narrow = ui.available_width() < 560.0;
    egui::Grid::new("bc-machines").striped(true).spacing(egui::vec2(if narrow { 12.0 } else { 16.0 }, 4.0)).show(ui, |ui| {
        let heads: &[&str] = if machines && narrow {
            &["machine", "built", "failed", "spent", "$ / package"]
        } else if machines {
            &["machine", "built", "failed", "building", "$/h", "spent", "$ / package", "per hour"]
        } else {
            &["", "built", "failed", "share", "build time", "share of time", "median build"]
        };
        for h in heads {
            ui.label(RichText::new(*h).size(12.0).color(theme::text()));
        }
        ui.end_row();
        for c in &cv.boxes {
            let n = c.ends.len();
            let busy = c.secs.last().copied().unwrap_or(0);
            match c.machine {
                Some(b) if machines => {
                    let hours = c.hours();
                    let spent = c.cum.last().copied().unwrap_or(0.0);
                    if ui.add(egui::Label::new(RichText::new(format!("● {}", c.name)).color(c.color)).sense(egui::Sense::click())).on_hover_text(format!("{} · {} - show only its line", cat.boxes.get(b).map_or("", |x| x.name.as_str()), if c.itype.is_empty() { "?" } else { &c.itype })).clicked() {
                        focus = Some(c.code);
                    }
                    ui.label(thousands(n));
                    ui.label(RichText::new(c.failed.to_string()).color(if c.failed > 0 { theme::alt() } else { theme::muted() }));
                    if !narrow {
                        ui.label(fmt_dur((hours * 3600.0) as i64));
                        ui.label(c.per_h.map_or("-".into(), |p| format!("${p:.2}")));
                    }
                    ui.label(if cv.priced { Measure::Spend.fmt(spent) } else { "-".into() });
                    ui.label(if cv.priced && n > 0 { Measure::PerPackage.fmt(spent / n as f64) } else { "-".into() });
                    if !narrow {
                        ui.label(if hours > 0.0 { format!("{:.0}", n as f64 / hours) } else { "-".into() });
                    }
                }
                _ => {
                    let mut d: Vec<i64> = c.steps.iter().map(|s| s.1 - s.0).collect();
                    d.sort_unstable();
                    if ui.add(egui::Label::new(RichText::new(format!("● {}", c.name)).color(c.color)).sense(egui::Sense::click())).on_hover_text("show only its line").clicked() {
                        focus = Some(c.code);
                    }
                    ui.label(thousands(n));
                    ui.label(RichText::new(c.failed.to_string()).color(if c.failed > 0 { theme::alt() } else { theme::muted() }));
                    ui.label(format!("{:.0}%", 100.0 * n as f64 / all.max(1) as f64));
                    ui.label(fmt_dur(busy));
                    ui.label(format!("{:.0}%", 100.0 * busy as f64 / all_secs.max(1) as f64));
                    ui.label(d.get(d.len() / 2).map_or("-".into(), |m| fmt_dur(*m)));
                }
            }
            ui.end_row();
        }
    });
    if !cv.priced && machines {
        ui.add_space(4.0);
        ui.label(RichText::new("Spend and $ / package light up once the instance prices arrive (builds.json).").size(10.5).color(theme::hush()));
    }
    focus
}

/// One machine with work: its progress, the step in flight against its
/// estimate, and what is left - each bar its own time and its dependencies'.
fn machine_card(ui: &mut egui::Ui, cat: &Catalog, m: &Machine, lane: Option<&Lane>, open: &mut Option<usize>) {
    // Times tick between the half-minute reads: the rollup's numbers were
    // true at m.generated, so now is that much later (operator, 2026-10-15:
    // "queue has seemed particularly frozen").
    let since = (now_secs() - m.generated).max(0) as f64;
    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
    egui::Frame::new().fill(theme::surface()).corner_radius(6).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(&m.id).font(theme::display(13.5)).color(theme::text_strong()));
            ui.label(RichText::new(format!("{} · {} · {}", m.arch, m.kind, if m.plan.is_empty() { &m.role } else { &m.plan })).size(11.0).color(theme::hush()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let left = m.remaining_s.filter(|s| *s > 0.0);
                let when = match (left, m.finish_at) {
                    (Some(s), _) => format!("about {} left", fmt_dur(s as i64)),
                    (None, Some(f)) if f > m.generated => format!("done about {} UTC", fmt_date(f)),
                    _ => "finishing".into(),
                };
                ui.label(RichText::new(when).size(11.5).color(theme::amber())).on_hover_text("the sum of what is left: each entry's own estimate plus the dependencies it still waits on");
            });
        });
        // Progress: done, failed, the rest.
        let total = m.counts.total.max(1) as f32;
        let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 8.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(r, 3.0, theme::surface_hi());
        let done_w = r.width() * m.counts.done as f32 / total;
        p.rect_filled(Rect::from_min_size(r.min, egui::vec2(done_w, r.height())), 3.0, theme::ok());
        let fail_w = r.width() * m.counts.failed as f32 / total;
        p.rect_filled(Rect::from_min_size(r.min + egui::vec2(done_w, 0.0), egui::vec2(fail_w, r.height())), 0.0, theme::alt());
        ui.label(RichText::new(format!("{} of {} done{}", m.counts.done, m.counts.total, if m.counts.failed > 0 { format!(" · {} failed", m.counts.failed) } else { String::new() })).size(10.5).color(theme::muted()))
            .on_hover_text("the EasyBuild plan (builds.eta); the R and Python packages each env set needs are the lane below");

        // The package lane: what the log says it is installing now.
        if let Some(l) = lane.filter(|l| l.of > 0) {
            ui.add_space(6.0);
            let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 6.0), Sense::hover());
            let p = ui.painter();
            p.rect_filled(r, 3.0, theme::surface_hi());
            p.rect_filled(Rect::from_min_size(r.min, egui::vec2(r.width() * (l.n as f32 / l.of.max(1) as f32).min(1.0), r.height())), 3.0, theme::primary());
            let quiet = (now_secs() - l.mtime).max(0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("packages {} of {}", l.n, l.of)).size(10.5).color(theme::muted()));
                if let Some(pkg) = &l.now {
                    let hit = find_item(cat, pkg);
                    let label = format!("{pkg} ({})", l.kind.as_deref().unwrap_or("?"));
                    let r = super::chip(ui, &label, hit.map_or(theme::primary(), |i| cat.set_color(i).unwrap_or(cat.kind[i].color())), true);
                    if r.on_hover_text("installing now - click to open it").clicked() {
                        *open = hit;
                    }
                }
                let (txt, c) = if quiet > 120 {
                    (format!("log quiet {} - a long compile; R and pip print a package's log when it finishes", fmt_dur(quiet)), theme::amber())
                } else {
                    (format!("log {} ago", fmt_dur(quiet)), theme::hush())
                };
                ui.label(RichText::new(txt).size(10.5).color(c));
            });
        }

        if let Some(run) = &m.running {
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("building now").size(10.5).color(theme::hush()));
                let hit = find_item(cat, &run.name);
                let label = format!("{} {}", run.name, run.version);
                let r = match hit {
                    Some(i) => super::chip(ui, &label, cat.kind[i].color(), true),
                    None => super::chip(ui, &label, theme::amber(), true),
                };
                let el = run.elapsed_s.unwrap_or(0.0) + since;
                let tip = match run.est_s {
                    Some(e) if el > e => format!("{} in, past its own ~{} estimate", fmt_dur(el as i64), fmt_dur(e as i64)),
                    Some(e) => format!("{} of ~{}", fmt_dur(el as i64), fmt_dur(e as i64)),
                    None => format!("{} in - never built before, so no estimate", fmt_dur(el as i64)),
                };
                if r.on_hover_text(&tip).clicked() {
                    *open = hit;
                }
                ui.label(RichText::new(tip).size(10.5).color(theme::muted()));
                if let Some(t) = run.target.as_ref().filter(|t| !t.is_empty()) {
                    let rem = run.target_remaining_s.map(|s| format!(", ~{} left", fmt_dur(s as i64))).unwrap_or_default();
                    ui.label(RichText::new(format!("for {t}{rem}")).size(10.5).color(theme::hush()))
                        .on_hover_text(format!("the entry this step is part of; it takes ~{} in all", run.target_est_s.map_or("?".into(), |s| fmt_dur(s as i64))));
                }
            });
            if let Some(e) = run.est_s.filter(|e| *e > 0.0) {
                let f = ((run.elapsed_s.unwrap_or(0.0) + since) / e).min(1.0) as f32;
                let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(320.0), 4.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, theme::surface_hi());
                ui.painter().rect_filled(Rect::from_min_size(r.min, egui::vec2(r.width() * f, 4.0)), 2.0, if f >= 1.0 { theme::alt() } else { theme::amber() });
            }
        }

        let rows: Vec<&Pending> = m.pending.iter().filter(|p| p.status != "running").collect();
        if !rows.is_empty() {
            ui.add_space(6.0);
            ui.label(RichText::new(format!("to build ({})", rows.len())).size(10.5).color(theme::hush()));
            let max = rows.iter().map(|p| p.own_s.unwrap_or(0.0) + p.deps_s.unwrap_or(0.0)).fold(60.0, f64::max);
            for pnd in rows.iter().take(60) {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), Sense::click());
                let hit = find_item(cat, &pnd.entry);
                let p = ui.painter_at(r);
                if resp.hovered() {
                    p.rect_filled(r, 2.0, theme::surface_hi());
                }
                let name_w = (r.width() * 0.42).min(260.0);
                let color = hit.map_or(theme::muted(), |i| cat.kind[i].color());
                p.text(egui::pos2(r.left() + 4.0, r.center().y), Align2::LEFT_CENTER, super::elide(&pnd.entry, (name_w / 6.4) as usize), FontId::proportional(11.0), color);
                let bar = Rect::from_min_max(egui::pos2(r.left() + name_w + 6.0, r.top() + 5.0), egui::pos2(r.right() - 70.0, r.bottom() - 5.0));
                let own = pnd.own_s.or(pnd.est_s).unwrap_or(0.0);
                let deps = pnd.deps_s.unwrap_or(0.0);
                let wf = |s: f64| bar.width() * (s / max) as f32;
                p.rect_filled(Rect::from_min_size(bar.min, egui::vec2(wf(own).max(1.0), bar.height())), 2.0, theme::amber());
                p.rect_filled(Rect::from_min_size(bar.min + egui::vec2(wf(own), 0.0), egui::vec2(wf(deps), bar.height())), 2.0, theme::net().gamma_multiply(0.7));
                p.text(egui::pos2(r.right() - 4.0, r.center().y), Align2::RIGHT_CENTER, fmt_dur((own + deps) as i64), FontId::proportional(10.5), theme::muted());
                let basis = match pnd.basis.as_str() {
                    "other-arch" => "estimated from the other architectures' builds of it",
                    "same-arch" | "history" => "estimated from its own builds here",
                    "engine" => "a container build, estimated from the last one",
                    "" => "no estimate basis given",
                    b => b,
                };
                let tip = format!(
                    "{}\nown ~{} (amber) · {} dependencies still to build ~{} (blue)\n{basis}{}",
                    pnd.entry,
                    fmt_dur(own as i64),
                    pnd.deps_n.unwrap_or(0.0) as i64,
                    fmt_dur(deps as i64),
                    pnd.cum_s.filter(|c| *c > 0.0).map(|c| format!("\nstarts after ~{} of the queue ahead of it", fmt_dur(c as i64))).unwrap_or_default()
                );
                if resp.on_hover_text(tip).clicked() {
                    *open = hit;
                }
            }
            if rows.len() > 60 {
                ui.label(RichText::new(format!("+{} more", rows.len() - 60)).size(10.5).color(theme::hush()));
            }
        }
    });
}

#[cfg(test)]
mod tests {

    #[test]
    fn off_machines_sharing_a_name_are_told_apart() {
        let m = |name: &str, arch: &str| Machine { name: name.into(), arch: arch.into(), ..Default::default() };
        let (a, b, c, d) = (m("containers", "x86_64"), m("containers", "aarch64"), m("svbt", "x86_64"), m("native", ""));
        let e = m("native", "");
        assert_eq!(off_names(&[&a, &b, &c, &d, &e]), ["containers (x86_64)", "containers (aarch64)", "svbt", "native ×2"]);
    }

    use super::*;

    #[test]
    fn a_machine_off_is_a_gap_not_a_flat_line() {
        // Two working stretches a day apart; a 10-minute pause inside the first.
        let active = merged(vec![(0, 3600), (4200, 7200), (90_000, 93_600)], ON_GAP);
        assert_eq!(active, vec![(0, 7200), (90_000, 93_600)]);
        let c = BoxCurve { name: "a".into(), code: 0, color: Color32::WHITE, machine: Some(0), steps: vec![], ends: vec![3600, 93_600], secs: vec![3600, 7200], failed: 0, spans: vec![], cum: vec![], itype: String::new(), per_h: None, first: 0, last: 93_600, active };
        assert!(c.on(3000.0, 3100.0) && c.on(3700.0, 3800.0), "a short pause is still at work");
        assert!(!c.on(20_000.0, 80_000.0), "the night it was off");
        assert!(c.on(89_000.0, 90_500.0), "picked up again");
    }

    #[test]
    fn queue_entries_name_their_software() {
        assert_eq!(entry_name("HISAT2-2.2.1-gompi-2024a.eb"), "hisat2");
        assert_eq!(entry_name("statsmodels"), "statsmodels");
        assert_eq!(entry_name("apptainer r4.6_py3.13-release"), "apptainer");
        assert_eq!(entry_name("SciPy-bundle-2024.05-gfbf-2024a.eb"), "scipy-bundle");
    }

    #[test]
    fn a_line_exists_only_while_its_machine_was_building() {
        let c = BoxCurve { name: "a".into(), code: 0, color: Color32::WHITE, machine: Some(0), steps: vec![(100, 200, 0)], ends: vec![200], secs: vec![100], failed: 0, spans: vec![(50, 300, 1.0)], cum: vec![250.0], itype: String::new(), per_h: None, first: 50, last: 300, active: vec![(50, 300)] };
        assert!(Measure::Built.value(&c, 10.0).is_none(), "before it began");
        assert!(Measure::Built.value(&c, 400.0).is_none(), "after it stopped");
        assert_eq!(Measure::Built.value(&c, 250.0), Some(1.0));
    }

    #[test]
    fn building_time_cuts_the_idle_days_out() {
        let mk = |active: Vec<(i64, i64)>| BoxCurve { name: "a".into(), code: 0, color: Color32::WHITE, machine: Some(0), steps: vec![], ends: vec![], secs: vec![], failed: 0, spans: vec![], cum: vec![], itype: String::new(), per_h: None, first: 0, last: 0, active };
        // Two machines: 0-100 and 50-150 overlap into one stretch; a day off; 90 000-90 100.
        let cv = Curves { key: Default::default(), boxes: vec![mk(vec![(0, 100), (90_000, 90_100)]), mk(vec![(50, 150)])], lo: 0, hi: 90_100, priced: false };
        let ax = Axis::new(&cv, false);
        assert_eq!(ax.fwd(120.0), 120.0);
        assert_eq!(ax.fwd(5_000.0), 150.0, "a moment in the cut gap sits where the gap was cut");
        assert_eq!(ax.fwd(90_050.0), 200.0, "the day off takes no width: 150 s of building, then 50 more");
        assert_eq!(ax.back(200.0), 90_050.0);
        assert_eq!(ax.back(ax.fwd(30.0)), 30.0);
        assert_eq!(ax.cuts().collect::<Vec<_>>(), vec![(150.0, 90_000.0)]);
        let cal = Axis::new(&cv, true);
        assert_eq!((cal.fwd(5_000.0), cal.back(5_000.0)), (5_000.0, 5_000.0));
    }

    #[test]
    fn axis_tops_are_round() {
        assert_eq!(nice_top(7.0), 10.0);
        assert_eq!(nice_top(1234.0), 2000.0);
        assert_eq!(nice_top(0.21), 0.25);
    }

    #[test]
    fn curves_count_a_step_once_and_price_overlaps_once() {
        // t0 is 15:00 UTC; the second run began at 15:10, while the first was going.
        let j = br#"{"schema":1,"generated":0,"hash":"h","t0":1791990000,
            "boxes":[{"name":"four-box-arm","arch":"aarch64","lane":"x"}],
            "runs":[{"id":"20261014T150000Z-a","box":0,"itype":"m7g","start":0,"end":0},
                    {"id":"20261014T151000Z-a","box":0,"itype":"m7g","start":0,"end":0}],
            "items":[{"k":"R","n":"a"},{"k":"R","n":"b"}],
            "steps":{"i":[0,0,1],"r":[0,1,1],"t":[1000,1000,5000],"d":[100,100,400],"s":"ddd"}}"#;
        let cat = Catalog::parse(j).unwrap();
        let prices: HashMap<String, f64> = [("m7g".to_owned(), 3.6)].into();
        let cv = curves(&cat, &Dims::new(&cat), Some(&prices), Dim::Machine, &[]);
        let c = &cv.boxes[0];
        assert_eq!(c.ends, vec![1100, 5400], "the inherited copy of a counts once");
        assert_eq!(c.built(2000.0), 1.0);
        // $3.60/h is $0.001/s; 0-1100 and 600-5400 overlap, so 0-5400 is paid once.
        assert!((c.spend(1100.0) - 1.1).abs() < 1e-9);
        assert!((c.spend(9999.0) - 5.4).abs() < 1e-9);
        assert!((Measure::PerPackage.value(c, 9999.0).is_none()), "under 50 built: no price per package yet");
    }
}
