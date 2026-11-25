//! The Software tab (right window; it was Packages until 2026-10-14).
//!
//! Operator, 2026-10-14: "a searchable tool which is a recreation of 2)
//! timeline in ndexr.io in a highly optimized way ... check at session start
//! for the data to completely recreate locally ... its 3d tree of time, arch,
//! paradigm ... when software is clicked its name, description, purpose".
//!
//! The work is done before it gets here: `ndexr aggregate`'s `software` spec
//! (ndexr.ndexr.io crate/src/aggregate/software.rs) joins every fleet build
//! step in `unibuild_steps` to the stack's own metadata and writes one
//! catalog. A session fetches it once, at start (`/software.json`, ETag'd, so
//! a browser keeps its copy and a reload costs a 304), and indexes it here.
//! From then on every keystroke, tree and gantt is local - nothing asks the
//! server again.
//!
//!   Find      fuzzy search over names, groups, purposes and descriptions,
//!             with filters typed into the same box: `k:r` `k:eb` `a:arm`
//!             `set:r4.6` `g:dev-libs` `is:failed` `is:unbuilt` `>10m` `<5s`
//!   Tree      the same matches as a tree: paradigm › group, arch › paradigm,
//!             the stack bottom up (Prefix → EasyBuild toolchains → env sets
//!             → site libraries), the site libraries top down, or by day built
//!   Timeline  ndexr.io's gantt for the matches: a row per software, a thin
//!             line per machine; drag across for a stretch of time, down for
//!             a set of rows (brush.rs), Shift+drag to pan, Ctrl+wheel to
//!             zoom, double-click a bar for its run
//!   Curves    (Builds) build.ndexr.io's curves and cost as one chart - see builds.rs
//!   Queue     (Builds) what each machine is building and has left
//!   Logs      (Builds) each build machine's log, live - see logs.rs
//!
//! Two tabs (operator, 2026-10-15): Packages (Find · Tree) and Builds (Curves
//! · Timeline · Queue). Installed went the same day - Find and Tree cover it.
//!
//! Then three (2026-10-15): Packages gained Manifests - every env manifest,
//! its records, two compared - and a manifest filter over Find and Tree
//! (manifests.rs); Install is the third tab, the desktop app building the
//! picked environments on the person's own machine (install.rs).
//!
//! A click on any software opens it: purpose, description, homepage, licence,
//! versions, arches, sets, its build times per arch and over time, what it is
//! built on and what is built on it - each a link, so the tree walks both ways.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, text::LayoutJob, Align, Align2, Color32, FontId, Key, Layout, Rect, RichText, Sense, Stroke, TextFormat, UiBuilder};
use serde::Deserialize;

use crate::api::Lang;
use crate::backend::Backend;
use crate::ui::kit::theme;

mod branches;
mod brush;
mod builds;
mod draft;
mod install;
mod logs;
mod manifests;


// ── the catalog, as the spec writes it ─────────────────────────────────────

#[derive(Deserialize)]
struct Raw {
    #[serde(default)]
    generated: i64,
    #[serde(default)]
    hash: String,
    #[serde(default)]
    t0: i64,
    #[serde(default)]
    boxes: Vec<RawBox>,
    #[serde(default)]
    runs: Vec<RawRun>,
    #[serde(default)]
    items: Vec<RawItem>,
    /// The manifests the steps' `m` index (catalogs from 2026-10-15 on).
    #[serde(default)]
    sets: Vec<String>,
    steps: RawSteps,
}

#[derive(Deserialize)]
struct RawBox {
    name: String,
    #[serde(default)]
    arch: String,
    #[serde(default)]
    lane: String,
}

#[derive(Deserialize)]
struct RawRun {
    #[serde(default)]
    id: String,
    #[serde(rename = "box")]
    bx: usize,
    #[serde(default)]
    itype: String,
    start: i64,
    end: i64,
}

#[derive(Deserialize)]
struct RawItem {
    k: String,
    n: String,
    #[serde(default)]
    g: String,
    #[serde(default)]
    v: Vec<String>,
    #[serde(default)]
    d: String,
    #[serde(default)]
    h: String,
    #[serde(default)]
    p: String,
    #[serde(default)]
    l: String,
    #[serde(default)]
    m: String,
    /// The language building it compiles (the spec's lang_*), "" in an older
    /// catalog.
    #[serde(default)]
    ln: String,
    #[serde(default)]
    a: Vec<String>,
    #[serde(default)]
    s: Vec<String>,
    #[serde(default)]
    dep: Vec<u32>,
    #[serde(default)]
    st: Option<Stats>,
}

#[derive(Deserialize, Default, Clone)]
struct Stats {
    #[serde(default)]
    n: u32,
    #[serde(default)]
    ok: u32,
    #[serde(default)]
    fail: u32,
    med: Option<i64>,
    min: Option<i64>,
    max: Option<i64>,
    #[serde(default)]
    tot: i64,
    #[serde(default)]
    arch: BTreeMap<String, i64>,
    first: Option<i64>,
    last: Option<i64>,
    #[serde(default)]
    lv: String,
}

#[derive(Deserialize, Default)]
struct RawSteps {
    #[serde(default)]
    i: Vec<u32>,
    #[serde(default)]
    r: Vec<u32>,
    #[serde(default)]
    t: Vec<i64>,
    #[serde(default)]
    d: Vec<i64>,
    #[serde(default)]
    s: String,
    /// Each step's manifest, an index into Raw::sets; -1 not recorded.
    #[serde(default)]
    m: Vec<i32>,
}

/// The paradigms, coloured as ndexr.io's gantt colours them (gantt.r
/// .KIND_COLOR, the hsl()s as RGB): the Prefix, EasyBuild, and the three
/// languages' package sets - R, Python and (2026-10-18) Rust crates. `Other`
/// is a kind this Cove does not know yet: shown as what it is, never passed
/// off as Python (every unknown kind used to read as Python).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum Kind {
    Prefix,
    EasyBuild,
    R,
    Python,
    Rust,
    /// Julia's General-registry packages (2026-10-21).
    Julia,
    /// The env/<set> modules: environments, their own layer - they compose
    /// R, Python and Rust sets at will, so they sit beside the languages,
    /// never inside one of them or inside EasyBuild (operator, 2026-10-18:
    /// "envs should be outside of the R, Py, Cargo builds ... language envs
    /// are their own layers"). In the catalog they are EasyBuild modules.
    Env,
    Other,
}

impl Kind {
    /// The chips: every kind but Other.
    const ALL: [Kind; 7] = [Kind::Prefix, Kind::EasyBuild, Kind::R, Kind::Python, Kind::Rust, Kind::Julia, Kind::Env];

    fn parse(s: &str) -> Kind {
        match s {
            "prefix" => Kind::Prefix,
            "easybuild" => Kind::EasyBuild,
            "R" => Kind::R,
            "Python" | "python" => Kind::Python,
            "Rust" | "rust" | "crate" => Kind::Rust,
            "Julia" | "julia" => Kind::Julia,
            "env" => Kind::Env,
            _ => Kind::Other,
        }
    }

    /// The catalog's own word for the kind (its `k`): what source.rs and
    /// names.rs are handed.
    pub fn key(self) -> &'static str {
        match self {
            Kind::Prefix => "prefix",
            Kind::EasyBuild => "easybuild",
            Kind::R => "R",
            Kind::Python => "Python",
            Kind::Rust => "Rust",
            Kind::Julia => "Julia",
            // An EasyBuild module to everything that reads its source.
            Kind::Env => "easybuild",
            Kind::Other => "other",
        }
    }

    /// A language's package set: R, Python or Rust.
    pub fn is_lang(self) -> bool {
        matches!(self, Kind::R | Kind::Python | Kind::Rust | Kind::Julia)
    }

    pub fn label(self) -> &'static str {
        match self {
            Kind::Prefix => "Prefix",
            Kind::EasyBuild => "EasyBuild",
            Kind::R => "R",
            Kind::Python => "Python",
            Kind::Rust => "Rust",
            Kind::Julia => "Julia",
            Kind::Env => "Env",
            Kind::Other => "Other",
        }
    }

    fn long(self) -> &'static str {
        match self {
            Kind::Prefix => "Gentoo Prefix",
            Kind::EasyBuild => "EasyBuild modules",
            Kind::R => "R packages",
            Kind::Python => "Python packages",
            Kind::Rust => "Rust crates",
            Kind::Julia => "Julia packages",
            Kind::Env => "Environments (ml env/<set>)",
            Kind::Other => "other software",
        }
    }

    pub fn color(self) -> Color32 {
        match self {
            Kind::Prefix => Color32::from_rgb(223, 143, 73),
            Kind::EasyBuild => Color32::from_rgb(78, 160, 218),
            Kind::R => Color32::from_rgb(71, 194, 112),
            Kind::Python => Color32::from_rgb(168, 121, 216),
            // Rust's own colour (#DEA584).
            Kind::Rust => Color32::from_rgb(0xde, 0xa5, 0x84),
            // The atlas's Julia layer colour.
            Kind::Julia => Color32::from_rgb(0xb0, 0x7c, 0xd8),
            // The atlas's Env layer colour, so the two views agree.
            Kind::Env => Color32::from_rgb(0x5e, 0xc4, 0xc0),
            Kind::Other => Color32::from_rgb(150, 150, 150),
        }
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// The catalog, indexed once for search: everything a frame needs is a
/// lookup, never a scan of the steps.
pub struct Catalog {
    generated: i64,
    hash: String,
    t0: i64,
    boxes: Vec<RawBox>,
    runs: Vec<RawRun>,
    items: Vec<RawItem>,
    kind: Vec<Kind>,
    /// Lower-case name, and name + group + purpose + description + sets.
    lname: Vec<String>,
    hay: Vec<String>,
    st_i: Vec<u32>,
    st_r: Vec<u32>,
    st_t: Vec<i64>,
    st_d: Vec<i64>,
    st_s: Vec<u8>,
    /// Each item's steps, oldest first; and what is built on it.
    by_item: Vec<Vec<u32>>,
    used_by: Vec<Vec<u32>>,
    counts: [usize; 8],
    total_secs: i64,
    max_med: i64,
    /// The env-set groups: which manifests list a package, as (label,
    /// packages), largest first - an index here is a colour (set_color).
    /// Grouped rather than one set each because the catalog's steps do not say
    /// which manifest a build was for, and most packages are in every set: a
    /// colour per first set painted nearly everything alike (2026-10-15).
    sets: Vec<(String, usize)>,
    /// Each item's group, None when no manifest lists it (Prefix, EasyBuild).
    set_group: Vec<Option<u16>>,
    /// The manifest each step was built for, where the box said so
    /// (unibuild_steps.env_set): an index into set_names, or -1.
    set_names: Vec<String>,
    st_m: Vec<i32>,
}

/// One generation of the stack: the runs of one month, on these machines.
#[derive(Clone, Debug, PartialEq)]
pub struct Deployment {
    pub label: String,
    pub lo: i64,
    pub hi: i64,
    pub runs: usize,
    pub boxes: Vec<String>,
}

const MONTH_NAMES: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

/// "Oct 14 · 03:00 UTC": a moment in a deployment's replay.
pub fn stamp(t: i64) -> String {
    let z = t.div_euclid(86_400) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let secs = t.rem_euclid(86_400);
    format!("{} {d} · {:02}:{:02} UTC", MONTHS[(m - 1) as usize], secs / 3600, secs % 3600 / 60)
}

/// (year, month 1-12) of a UTC instant.
fn year_month(t: i64) -> (i64, i64) {
    // Howard Hinnant's civil_from_days.
    let z = t.div_euclid(86_400) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { yoe + era * 400 + 1 } else { yoe + era * 400 }, m)
}

/// One software as the atlas (atlas.rs) reads it: the catalog's own fields,
/// borrowed, and the two directions of its dependency edges.
pub struct Sw<'a> {
    pub name: &'a str,
    pub kind: Kind,
    pub group: &'a str,
    pub purpose: &'a str,
    pub desc: &'a str,
    pub module: &'a str,
    pub versions: &'a [String],
    pub sets: &'a [String],
    pub deps: &'a [u32],
    /// The architectures it is built for (x86_64, aarch64, cuda).
    pub archs: &'a [String],
    /// Its homepage, as its package's own metadata gives it.
    pub home: &'a str,
}

impl Catalog {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn sw(&self, i: usize) -> Sw<'_> {
        let it = &self.items[i];
        Sw {
            name: &it.n,
            kind: self.kind[i],
            group: &it.g,
            purpose: &it.p,
            desc: &it.d,
            module: &it.m,
            versions: &it.v,
            sets: &it.s,
            deps: &it.dep,
            archs: &it.a,
            home: &it.h,
        }
    }

    /// Software per paradigm (Kind order: Prefix, EasyBuild, R, Python, Rust, Julia, Env, Other), build steps
    /// recorded, machine-hours spent building, and machines - the public
    /// front's numbers (showcase.rs).
    pub fn summary(&self) -> ([usize; 8], usize, i64, usize) {
        (self.counts, self.st_i.len(), self.total_secs, self.boxes.len())
    }

    /// When the spec wrote this catalog, UTC seconds (when it was read).
    pub fn generated(&self) -> i64 {
        self.generated
    }

    /// The build machines: (name, architecture, lane).
    pub fn machines(&self) -> Vec<(&str, &str, &str)> {
        self.boxes.iter().map(|b| (b.name.as_str(), b.arch.as_str(), b.lane.as_str())).collect()
    }

    /// Known to the stack but never yet built anywhere: tried (failed or
    /// queued) and not succeeded - a star that does not exist yet (atlas.rs).
    /// Packages the pkgs lane installs carry no build record at all, so
    /// "no record" is not "not built".
    pub fn planned(&self, i: usize) -> bool {
        self.items.get(i).and_then(|it| it.st.as_ref()).is_some_and(|s| s.ok == 0)
    }

    /// The deployments the catalog records: the build machines' runs grouped
    /// by the UTC month they started in, oldest first. A deployment is one
    /// generation of the stack - October 2026 is the four-box machines
    /// building Gentoo Prefix, EasyBuild and the package sets from nothing.
    pub fn deployments(&self) -> Vec<Deployment> {
        let mut out: Vec<Deployment> = Vec::new();
        let mut runs: Vec<&RawRun> = self.runs.iter().filter(|r| r.start > 0).collect();
        runs.sort_by_key(|r| r.start);
        for r in runs {
            let (y, m) = year_month(r.start);
            let label = format!("{} {y}", MONTH_NAMES[(m - 1) as usize]);
            let bx = self.boxes.get(r.bx).map(|b| b.name.clone()).unwrap_or_default();
            match out.last_mut() {
                Some(d) if d.label == label => {
                    d.hi = d.hi.max(r.end.max(r.start));
                    d.runs += 1;
                    if !bx.is_empty() && !d.boxes.contains(&bx) {
                        d.boxes.push(bx);
                    }
                }
                _ => out.push(Deployment { label, lo: r.start, hi: r.end.max(r.start), runs: 1, boxes: if bx.is_empty() { Vec::new() } else { vec![bx] } }),
            }
        }
        // A step can finish after its run's recorded end (a run's last beat):
        // each deployment ends at its last step, so the replay reaches it.
        for d in out.iter_mut() {
            for k in 0..self.st_t.len().min(self.st_d.len()) {
                let (s, e) = (self.st_t[k] + self.t0, self.st_t[k] + self.st_d[k] + self.t0);
                if s >= d.lo && s <= d.hi {
                    d.hi = d.hi.max(e);
                }
            }
        }
        out
    }

    /// Each item's first finished build in [lo, hi] - on the box named `on`,
    /// or any box when None: (start, end), UTC seconds. None: not built in
    /// that window (there).
    pub fn first_builds(&self, lo: i64, hi: i64, on: Option<&str>) -> Vec<Option<(i64, i64)>> {
        let bx = on.and_then(|n| self.boxes.iter().position(|b| b.name == n));
        let mut out: Vec<Option<(i64, i64)>> = vec![None; self.items.len()];
        let n = self.st_i.len().min(self.st_t.len()).min(self.st_d.len()).min(self.st_s.len()).min(self.st_r.len());
        for k in 0..n {
            if self.st_s[k] != b'd' {
                continue;
            }
            let (s, e) = (self.st_t[k] + self.t0, self.st_t[k] + self.st_d[k] + self.t0);
            if s < lo || s > hi {
                continue;
            }
            if let Some(b) = bx {
                if self.runs.get(self.st_r[k] as usize).map(|r| r.bx) != Some(b) {
                    continue;
                }
            }
            let slot = &mut out[self.st_i[k] as usize];
            if slot.is_none_or(|(_, e0)| e < e0) {
                *slot = Some((s, e));
            }
        }
        out
    }

    /// What was installed when, per lane (x86_64, aarch64, cuda): each item's
    /// first finished build there as (end, item), seconds from t0, soonest
    /// first - so the inventory at any moment is a prefix (referral #780).
    pub fn installs_by_lane(&self) -> Vec<(String, Vec<(i64, u32)>)> {
        let mut lanes: Vec<String> = Vec::new();
        let lane_of: Vec<usize> = (0..self.boxes.len().max(1))
            .map(|b| {
                let l = self.box_arch(b);
                lanes.iter().position(|x| *x == l).unwrap_or_else(|| {
                    lanes.push(l);
                    lanes.len() - 1
                })
            })
            .collect();
        let mut first: Vec<Vec<Option<i64>>> = vec![vec![None; self.items.len()]; lanes.len()];
        let n = self.st_i.len().min(self.st_t.len()).min(self.st_d.len()).min(self.st_s.len()).min(self.st_r.len());
        for k in 0..n {
            if self.st_s[k] != b'd' {
                continue;
            }
            let e = self.st_t[k] + self.st_d[k];
            let Some(slot) = lane_of.get(self.box_of_step(k)).and_then(|l| first[*l].get_mut(self.st_i[k] as usize)) else { continue };
            if slot.is_none_or(|e0| e < e0) {
                *slot = Some(e);
            }
        }
        let mut out: Vec<(String, Vec<(i64, u32)>)> = lanes
            .into_iter()
            .zip(first)
            .map(|(l, f)| {
                let mut v: Vec<(i64, u32)> = f.into_iter().enumerate().filter_map(|(i, e)| Some((e?, i as u32))).collect();
                v.sort_unstable();
                (l, v)
            })
            .filter(|(_, v)| !v.is_empty())
            .collect();
        out.sort_by_key(|(l, v)| (std::cmp::Reverse(v.len()), l.clone()));
        out
    }

    /// The item a build machine's entry names (`GDAL-3.10.0-foss-2024a.eb`,
    /// `statsmodels`), as the Builds Queue matches them.
    pub fn find_entry(&self, entry: &str) -> Option<usize> {
        builds::find_item(self, entry)
    }

    /// Builds finished per UTC day for the last `days` days, oldest first:
    /// (day start, built, failed) - the public front's chart (showcase.rs).
    pub fn daily(&self, days: usize) -> Vec<(i64, usize, usize)> {
        let end = self.st_t.iter().zip(&self.st_d).map(|(t, d)| t + d + self.t0).max().unwrap_or(0);
        let last = end - end.rem_euclid(86_400);
        let first = last - 86_400 * (days as i64 - 1);
        let mut out: Vec<(i64, usize, usize)> = (0..days as i64).map(|k| (first + k * 86_400, 0, 0)).collect();
        for k in 0..self.st_t.len().min(self.st_d.len()).min(self.st_s.len()) {
            let at = self.st_t[k] + self.st_d[k] + self.t0;
            if at < first {
                continue;
            }
            let Some(slot) = out.get_mut(((at - first) / 86_400) as usize) else { continue };
            match self.st_s[k] {
                b'd' => slot.1 += 1,
                b'f' => slot.2 += 1,
                _ => {}
            }
        }
        out
    }

    /// How many in the catalog are built directly on item `i`.
    pub fn used_by_count(&self, i: usize) -> usize {
        self.used_by.get(i).map_or(0, Vec::len)
    }

    /// What the stack knows of item `i` right now, as one line: its edges,
    /// and its build record from the n-boxes - the front page's facts
    /// (operator, 2026-10-18: "the visualization is displaying the real time
    /// status of what we know of the stack"). Numbers come from here, never
    /// from a slide's own words, so they move as the machines build.
    pub fn live_facts(&self, i: usize) -> String {
        let it = &self.items[i];
        let mut out = vec![match (it.dep.len(), self.used_by_count(i)) {
            (0, u) => format!("on the kernel's floor · {u} built on it"),
            (d, u) => format!("built on {d} · {u} built on it"),
        }];
        if let Some(st) = &it.st {
            if st.n > 0 {
                let arches = if st.arch.is_empty() { String::new() } else { format!(" on {}", st.arch.keys().cloned().collect::<Vec<_>>().join(", ")) };
                let failed = if st.fail > 0 { format!(" ({} failed)", st.fail) } else { String::new() };
                out.push(format!("built {}×{arches}{failed}", st.n));
            }
            if let Some(m) = st.med {
                out.push(format!("median build {}", fmt_dur(m)));
            }
            if let Some(l) = st.last {
                out.push(format!("last {}", fmt_ago(now_secs(), l)));
            }
        } else {
            out.push("not built yet".to_owned());
        }
        out.join("  ·  ")
    }

    /// The catalog item of this paradigm and name, case-blind.
    pub fn find(&self, kind: &str, name: &str) -> Option<usize> {
        let want = Kind::parse(kind);
        let name = name.to_lowercase();
        (0..self.items.len()).find(|&i| self.kind[i] == want && self.lname[i] == name)
    }

    /// Changes whenever the spec rewrote the catalog with something new.
    pub fn hash(&self) -> &str {
        &self.hash
    }
}

/// "r4.6_py3.13-release" → "r4.6 rel"; "ndexr-app-r4.6.1" → "ndexr app".
pub(crate) fn short_set(s: &str) -> String {
    if s.starts_with("ndexr-app") {
        return "ndexr app".into();
    }
    let r = s.split('_').next().unwrap_or(s);
    let kind = if s.ends_with("-release") { "rel" } else if s.ends_with("-devel") { "dev" } else { s.rsplit('-').next().unwrap_or("") };
    format!("{r} {kind}")
}

/// A group of sets as words: every R/Python set is "all N sets".
fn group_label(sets: &[String], all_rp: &[String]) -> String {
    let rp: Vec<&String> = sets.iter().filter(|s| all_rp.contains(s)).collect();
    let other: Vec<String> = sets.iter().filter(|s| !all_rp.contains(s)).map(|s| short_set(s)).collect();
    let mut parts = Vec::new();
    if rp.len() == all_rp.len() && rp.len() > 1 {
        parts.push(format!("all {} sets", rp.len()));
    } else {
        parts.extend(rp.iter().map(|s| short_set(s)));
    }
    parts.extend(other);
    parts.join(" + ")
}

/// The env sets' colours (operator, 2026-10-15: "color the package builds by
/// the env manifest we are building for"): picked apart from the paradigm and
/// machine colours, so a chip or bar coloured by set reads as such.
const SET_COLORS: [Color32; 8] = [
    Color32::from_rgb(0x7f, 0xd1, 0xae),
    Color32::from_rgb(0xf2, 0x9e, 0x6b),
    Color32::from_rgb(0x8f, 0xa8, 0xf5),
    Color32::from_rgb(0xe7, 0x83, 0xb5),
    Color32::from_rgb(0xd9, 0xcf, 0x6b),
    Color32::from_rgb(0x6f, 0xc6, 0xe0),
    Color32::from_rgb(0xc4, 0x9a, 0xf0),
    Color32::from_rgb(0xa8, 0xc2, 0x7a),
];

/// Which kind of environment `env/<set>` is: one language's set, or a
/// composition of an R and a Python one (env/r<R>_py<P>-...), or a purpose set.
fn env_family(name: &str) -> &'static str {
    let set = name.trim_start_matches("env/");
    if set.starts_with("R/") {
        "R environments"
    } else if set.starts_with("Python/") {
        "Python environments"
    } else if set.starts_with("Rust/") {
        "Rust environments"
    } else if set.starts_with('r') && set.contains("_py") {
        "composed: R + Python"
    } else {
        "purpose sets"
    }
}

impl Catalog {
    pub(crate) fn parse(bytes: &[u8]) -> Result<Catalog, String> {
        let raw: Raw = serde_json::from_slice(bytes).map_err(|e| format!("the catalog did not read: {e}"))?;
        let n = raw.items.len();
        let kind: Vec<Kind> = raw
            .items
            .iter()
            .map(|it| match Kind::parse(&it.k) {
                Kind::EasyBuild if it.n.starts_with("env/") || it.g == "env sets" => Kind::Env,
                k => k,
            })
            .collect();
        let lname: Vec<String> = raw.items.iter().map(|it| it.n.to_lowercase()).collect();
        let hay: Vec<String> = raw
            .items
            .iter()
            .map(|it| format!("{} {} {} {} {} {}", it.n, it.g, it.p, it.d, it.s.join(" "), it.m).to_lowercase())
            .collect();
        let mut by_item = vec![Vec::new(); n];
        for (k, &i) in raw.steps.i.iter().enumerate() {
            if let Some(v) = by_item.get_mut(i as usize) {
                v.push(k as u32);
            }
        }
        let mut used_by = vec![Vec::new(); n];
        for (i, it) in raw.items.iter().enumerate() {
            for &d in &it.dep {
                if let Some(v) = used_by.get_mut(d as usize) {
                    v.push(i as u32);
                }
            }
        }
        let mut counts = [0usize; 8];
        for k in &kind {
            counts[*k as usize] += 1;
        }
        let total_secs = raw.items.iter().filter_map(|it| it.st.as_ref()).map(|s| s.tot).sum();
        let max_med = raw.items.iter().filter_map(|it| it.st.as_ref()?.med).max().unwrap_or(1).max(1);
        // A run that a newer run on the same box replaced never finishes its
        // in-flight steps, and the spec stretches every one of them to
        // `generated`: on 2026-10-14 the boxes restarted three times and 194
        // of 197 "building now" bars were such leftovers, drawn as lines
        // across the whole day. Those steps stopped when the next run began.
        let began: Vec<Option<i64>> = raw.runs.iter().map(|r| run_began(&r.id).map(|t| t - raw.t0)).collect();
        let superseded_at = |r: usize| -> Option<i64> {
            let (bx, at) = (raw.runs.get(r)?.bx, began[r]?);
            raw.runs.iter().zip(&began).filter(|(o, b)| o.bx == bx && b.is_some_and(|b| b > at)).filter_map(|(_, b)| *b).min()
        };
        let mut st_s: Vec<u8> = raw.steps.s.bytes().collect();
        // Which steps the spec called running, before any is retired below.
        let was_running: Vec<bool> = st_s.iter().map(|&c| c == b'r').collect();
        let mut st_d = raw.steps.d;
        for k in 0..st_s.len().min(st_d.len()) {
            if st_s[k] == b'r' {
                if let Some(next) = raw.steps.r.get(k).and_then(|&r| superseded_at(r as usize)) {
                    st_s[k] = b's';
                    st_d[k] = st_d[k].min(next - raw.steps.t[k]).max(0);
                }
            }
        }
        // And within a live run: it goes through the same packages once per
        // env set, and an attempt that never got its done record stayed
        // "building" - 45 of 47 on 2026-10-15, bars from midnight to now while
        // the machines installed other things (operator: "more long lines
        // which dont make sense"). A step has stopped once the same package
        // started again on that machine, or once LATER_DONE steps that began
        // after it have finished there (packages install one at a time). It
        // ends where the machine visibly moved on.
        const LATER_DONE: usize = 3;
        let box_of = |k: usize| raw.steps.r.get(k).and_then(|&r| raw.runs.get(r as usize)).map_or(0, |r| r.bx);
        let n = st_s.len().min(st_d.len()).min(raw.steps.t.len()).min(raw.steps.i.len());
        let mut done_starts: HashMap<usize, Vec<i64>> = HashMap::new();
        let mut starts: HashMap<(usize, u32), Vec<i64>> = HashMap::new();
        for k in 0..n {
            let (b, t) = (box_of(k), raw.steps.t[k]);
            starts.entry((b, raw.steps.i[k])).or_default().push(t);
            if st_s[k] == b'd' {
                done_starts.entry(b).or_default().push(t);
            }
        }
        for v in done_starts.values_mut() {
            v.sort_unstable();
        }
        for k in 0..n {
            // A step retired with its run above may still have been left
            // behind long before that run ended: the earliest end wins.
            if !was_running[k] {
                continue;
            }
            let (b, t) = (box_of(k), raw.steps.t[k]);
            let again = starts.get(&(b, raw.steps.i[k])).and_then(|v| v.iter().copied().filter(|&x| x > t).min());
            let moved_on = done_starts.get(&b).and_then(|v| {
                let at = v.partition_point(|&x| x <= t);
                (v.len() - at >= LATER_DONE).then(|| v[at])
            });
            // Whichever came first: a retry hours later must not stretch a
            // step the machine left behind long before (grpcio, midnight to
            // 11:00 on 2026-10-15).
            let end = match (again, moved_on) {
                (Some(a), Some(m)) => Some(a.min(m)),
                (a, m) => a.or(m),
            };
            if let Some(end) = end {
                st_s[k] = b's';
                st_d[k] = st_d[k].min(end - t).max(0);
            }
        }
        // The env-set groups (see Catalog::sets), largest first.
        let mut all_rp: Vec<String> = raw.items.iter().flat_map(|it| it.s.iter().cloned()).filter(|s| !s.starts_with("ndexr-app")).collect();
        all_rp.sort();
        all_rp.dedup();
        let mut by_key: BTreeMap<Vec<String>, usize> = BTreeMap::new();
        for it in &raw.items {
            if !it.s.is_empty() {
                let mut k = it.s.clone();
                k.sort();
                *by_key.entry(k).or_default() += 1;
            }
        }
        let mut groups: Vec<(Vec<String>, usize)> = by_key.into_iter().collect();
        groups.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let set_group: Vec<Option<u16>> = raw
            .items
            .iter()
            .map(|it| {
                let mut k = it.s.clone();
                k.sort();
                groups.iter().position(|g| g.0 == k).map(|p| p as u16)
            })
            .collect();
        let sets: Vec<(String, usize)> = groups.iter().map(|(k, n)| (group_label(k, &all_rp), *n)).collect();
        Ok(Catalog {
            generated: raw.generated,
            hash: raw.hash,
            t0: raw.t0,
            st_s,
            st_i: raw.steps.i,
            st_r: raw.steps.r,
            st_t: raw.steps.t,
            st_d,
            boxes: raw.boxes,
            runs: raw.runs,
            items: raw.items,
            kind,
            lname,
            hay,
            by_item,
            used_by,
            counts,
            total_secs,
            max_med,
            sets,
            set_group,
            set_names: raw.sets,
            st_m: raw.steps.m,
        })
    }

    /// The manifest step `k` was built for: as the box recorded it, else the
    /// one manifest that lists its package; None when that cannot be told.
    fn step_set(&self, k: usize) -> Option<&str> {
        if let Some(&m) = self.st_m.get(k).filter(|m| **m >= 0) {
            return self.set_names.get(m as usize).map(String::as_str);
        }
        let it = &self.items[*self.st_i.get(k)? as usize];
        match it.s.as_slice() {
            [one] if self.kind[self.st_i[k] as usize].is_lang() => Some(one.as_str()),
            _ => None,
        }
    }

    /// Item `i`'s language, falling back to its paradigm's in an older catalog.
    fn lang(&self, i: usize) -> &str {
        match self.items[i].ln.as_str() {
            "" => match self.kind[i] {
                Kind::R => "R",
                Kind::Python => "Python",
                Kind::Rust => "Rust",
                Kind::Julia => "Julia",
                _ => "unknown",
            },
            l => l,
        }
    }

    /// Env-set group `g`'s colour.
    fn group_color(g: usize) -> Color32 {
        SET_COLORS[g % SET_COLORS.len()]
    }

    /// Item `i`'s env-set colour - its group's; None when no manifest lists
    /// it (Prefix, most EasyBuild modules).
    fn set_color(&self, i: usize) -> Option<Color32> {
        self.set_group.get(i).copied().flatten().map(|g| Catalog::group_color(g as usize))
    }

    fn med(&self, i: usize) -> Option<i64> {
        self.items[i].st.as_ref()?.med
    }

    fn box_of_step(&self, k: usize) -> usize {
        self.runs.get(self.st_r[k] as usize).map_or(0, |r| r.bx)
    }

    /// x86_64, aarch64, or cuda for the GPU box (it shares x86_64).
    fn box_arch(&self, b: usize) -> String {
        match self.boxes.get(b) {
            Some(x) if x.lane.contains("CUDA") => "cuda".into(),
            Some(x) => x.arch.clone(),
            None => String::new(),
        }
    }
}

// ── the search box's little language ──────────────────────────────────────

#[derive(Default, Clone, PartialEq)]
struct Filter {
    terms: Vec<String>,
    kinds: u8,
    arch: Vec<String>,
    set: Vec<String>,
    group: Vec<String>,
    failed: bool,
    built: Option<bool>,
    longer: Option<i64>,
    shorter: Option<i64>,
}

/// "10m" → 600, "90" / "90s" → 90, "2h" → 7200, "1d" → 86400.
fn parse_secs(s: &str) -> Option<i64> {
    let s = s.trim();
    let (num, mul) = match s.chars().last()? {
        's' => (&s[..s.len() - 1], 1),
        'm' => (&s[..s.len() - 1], 60),
        'h' => (&s[..s.len() - 1], 3600),
        'd' => (&s[..s.len() - 1], 86400),
        _ => (s, 1),
    };
    num.parse::<f64>().ok().map(|v| (v * mul as f64) as i64)
}

fn parse_filter(q: &str, chips: u8) -> Filter {
    let mut f = Filter { kinds: chips, ..Default::default() };
    for tok in q.split_whitespace() {
        let t = tok.to_lowercase();
        let (key, val) = t.split_once(':').unwrap_or(("", t.as_str()));
        match key {
            "k" | "kind" => {
                for v in val.split(',') {
                    let k = match v {
                        "p" | "prefix" | "portage" | "gentoo" => Some(Kind::Prefix),
                        "e" | "eb" | "easybuild" | "module" | "modules" => Some(Kind::EasyBuild),
                        "r" => Some(Kind::R),
                        "py" | "python" | "pip" => Some(Kind::Python),
                        "rs" | "rust" | "crate" | "crates" | "cargo" => Some(Kind::Rust),
                        "jl" | "julia" => Some(Kind::Julia),
                        "env" | "envs" | "environment" | "environments" => Some(Kind::Env),
                        _ => None,
                    };
                    if let Some(k) = k {
                        f.kinds |= k.bit();
                    }
                }
            }
            "a" | "arch" => f.arch.push(val.to_owned()),
            "set" | "env" | "s" => f.set.push(val.to_owned()),
            "g" | "group" | "tc" | "cat" => f.group.push(val.to_owned()),
            "is" => match val {
                "failed" | "fail" | "broken" => f.failed = true,
                "built" => f.built = Some(true),
                "unbuilt" | "untimed" | "never" => f.built = Some(false),
                _ => {}
            },
            _ if t.starts_with('>') => f.longer = parse_secs(&t[1..]),
            _ if t.starts_with('<') => f.shorter = parse_secs(&t[1..]),
            _ if t == "failed" || t == "!" => f.failed = true,
            _ => f.terms.push(t),
        }
    }
    f
}

/// Where `term` shows in `name` (char indices): a run of it, else its
/// letters in order (fuzzy). The score says how good the fit is.
fn fit(name: &str, term: &str) -> Option<(i32, Vec<usize>)> {
    if term.is_empty() {
        return Some((0, vec![]));
    }
    if let Some(at) = name.find(term) {
        let start = name[..at].chars().count();
        let len = term.chars().count();
        let score = if name == term {
            1000
        } else if at == 0 {
            700
        } else if name[..at].ends_with(['/', '-', '_', '.', ' ']) {
            550
        } else {
            420
        };
        return Some((score - name.chars().count() as i32, (start..start + len).collect()));
    }
    let mut hits = Vec::new();
    let mut want = term.chars().peekable();
    for (ci, c) in name.chars().enumerate() {
        if want.peek() == Some(&c) {
            hits.push(ci);
            want.next();
        }
    }
    if want.peek().is_some() {
        return None;
    }
    let spread = (hits.last().unwrap_or(&0) - hits.first().unwrap_or(&0)) as i32;
    Some((200 - spread * 4 - *hits.first().unwrap_or(&0) as i32, hits))
}

fn score(cat: &Catalog, i: usize, f: &Filter) -> Option<i32> {
    let it = &cat.items[i];
    if f.kinds != 0 && f.kinds & cat.kind[i].bit() == 0 {
        return None;
    }
    if !f.arch.iter().all(|a| it.a.iter().any(|x| x.to_lowercase().contains(a.as_str()))) {
        return None;
    }
    if !f.set.iter().all(|s| it.s.iter().any(|x| x.to_lowercase().contains(s.as_str())) || it.n.to_lowercase().contains(&format!("env/{s}"))) {
        return None;
    }
    if !f.group.iter().all(|g| it.g.to_lowercase().contains(g.as_str())) {
        return None;
    }
    let st = it.st.as_ref();
    if f.failed && st.map_or(true, |s| s.fail == 0) {
        return None;
    }
    if let Some(b) = f.built {
        if st.is_some_and(|s| s.ok > 0) != b {
            return None;
        }
    }
    let med = st.and_then(|s| s.med);
    if f.longer.is_some_and(|l| med.map_or(true, |m| m < l)) || f.shorter.is_some_and(|l| med.map_or(true, |m| m > l)) {
        return None;
    }
    let mut total = 0;
    for t in &f.terms {
        match fit(&cat.lname[i], t) {
            Some((s, _)) => total += s,
            None if cat.hay[i].contains(t.as_str()) => total += 60,
            None => return None,
        }
    }
    Some(total)
}

// ── the pane ───────────────────────────────────────────────────────────────

/// `Software::ui`'s group for the Builds mode: everything about the stack,
/// what it has and how it got built, in one place (operator, 2026-10-23:
/// "things regarding the software stack and anything behind the users site
/// goes into builds now").
pub const STACK: usize = 3;

/// Two tabs (operator, 2026-10-15: "maybe we combine all of that under two
/// tabs in software - you decide"): Packages - what the stack has - and
/// Builds - how it got built and what is building (builds.rs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum View {
    Find,
    Tree,
    Manifests,
    Curves,
    Timeline,
    Queue,
    Logs,
    Install,
}

impl View {
    fn builds(self) -> bool {
        matches!(self, View::Curves | View::Timeline | View::Queue | View::Logs)
    }

    /// The Builds mode's group: 0 Packages, 1 Environments, 2 Builds.
    fn tab_whole(self) -> usize {
        match self {
            View::Find | View::Tree => 0,
            View::Manifests => 1,
            _ => 2,
        }
    }

    /// Which toolbar tab a view sits under: Packages, Builds, Install.
    fn tab(self) -> usize {
        match self {
            View::Find | View::Tree | View::Manifests => 0,
            View::Install => 2,
            _ => 1,
        }
    }

    /// The search box and the paradigm chips filter these.
    fn searched(self) -> bool {
        matches!(self, View::Find | View::Tree | View::Timeline)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    Best,
    Name,
    Slowest,
    Recent,
    Builds,
    Failures,
}

impl Sort {
    const ALL: [(Sort, &'static str); 6] = [
        (Sort::Best, "best match"),
        (Sort::Name, "name"),
        (Sort::Slowest, "slowest to build"),
        (Sort::Recent, "built most recently"),
        (Sort::Builds, "most builds"),
        (Sort::Failures, "most failures"),
    ];
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Order {
    Paradigm,
    Arch,
    Stack,
    Libraries,
    Day,
}

impl Order {
    const ALL: [(Order, &'static str, &'static str); 5] = [
        (Order::Paradigm, "paradigm › group", "Prefix / EasyBuild / R / Python, then category, toolchain or env set"),
        (Order::Arch, "arch › paradigm › group", "x86_64, aarch64 and the GPU box, each a whole stack"),
        (Order::Stack, "the stack, bottom up", "Gentoo Prefix → EasyBuild by toolchain → env sets → their site libraries"),
        (Order::Libraries, "site libraries, top down", "each env set: the modules it loads, its R packages, its Python packages"),
        (Order::Day, "by day built", "the last day each was built, then paradigm"),
    ];
}

pub enum Action {
    /// Code for a console: `ml("BLAST+/2.16.0-gompi-2024a")`, `library(x)`.
    Run(Lang, String),
}

#[derive(Default)]
struct Node {
    kids: BTreeMap<String, Node>,
    items: Vec<u32>,
    count: usize,
    secs: i64,
}

impl Node {
    fn add(&mut self, path: &[String], i: u32, secs: i64) {
        self.count += 1;
        self.secs += secs;
        match path.split_first() {
            None => self.items.push(i),
            Some((head, rest)) => self.kids.entry(head.clone()).or_default().add(rest, i, secs),
        }
    }
}

type Loaded = Arc<Mutex<Option<Result<Arc<Catalog>, String>>>>;

pub struct SoftwarePane {
    /// The public view (view.rs): no Manifests (it asks a session), nothing
    /// that runs.
    pub public: bool,
    /// EasyBuild modules' easyconfigs, by "name/version", once asked; and
    /// the version picked to read, by item.
    easyconfigs: Arc<Mutex<HashMap<String, Result<crate::api::EasyconfigView, String>>>>,
    ec_asked: std::collections::HashSet<String>,
    ec_pick: HashMap<usize, String>,
    slot: Loaded,
    asked: bool,
    pub view: View,
    /// The view each of the Builds mode's three groups was last on, so
    /// coming back to a group comes back to it.
    pkg_view: View,
    build_view: View,
    query: String,
    chips: u8,
    sort: Sort,
    order: Order,
    by_box: bool,
    /// The timeline coloured by env set (by_box off).
    by_set: bool,
    sel: Option<usize>,
    back: Vec<usize>,
    results: Vec<u32>,
    results_key: Option<(String, u8, u8, String)>,
    tree: Option<((String, u8, u8, String), Node)>,
    /// The gantt's time window, seconds after t0; None fits the matches.
    window: Option<(f64, f64)>,
    find_view: (f32, f32),
    follow_sel: bool,
    focus_search: bool,
    seed: u64,
    deps_all: bool,
    used_all: bool,
    /// The Builds tab's dimensions, focus, Branches and 3D (branches.rs).
    br: branches::State,
    /// The Builds tab (builds.rs): /builds.json and when it was last asked,
    /// the curves worked out for this catalog, and the chart's state.
    live: builds::LiveSlot,
    live_asked: f64,
    /// The build machines, for an admin at cove.ndexr.io (boxes.rs): None
    /// until asked, Err for everyone else - and then nothing is drawn.
    boxes: builds::BoxesSlot,
    /// An action picked in the Queue, sent on the next tick; a terminate
    /// waiting for its second click; the last action's job.
    box_act: Option<(String, String)>,
    box_confirm: Option<String>,
    box_job: std::sync::Arc<std::sync::Mutex<Option<crate::api::JobView>>>,
    /// The machine that job is for, when it was pressed and last asked - so
    /// Kick off is watched every 2 s until it ends (api.rs start_phase).
    box_job_server: Option<String>,
    box_job_since: f64,
    box_job_poll: f64,
    curves: Option<Arc<builds::Curves>>,
    measure: builds::Measure,
    overlay: Option<builds::Measure>,
    pinned: Option<f64>,
    /// installs_by_lane for the catalog of this hash, and the moment's
    /// package search (builds.rs inventory).
    inv: Option<(String, Arc<Vec<(String, Vec<(i64, u32)>)>>)>,
    inv_q: String,
    cwin: Option<(f64, f64)>,
    /// Curves across the calendar; off (the default), across building time -
    /// every stretch no machine was building cut out (operator, 2026-10-28:
    /// "the hours built over time matter more than the time between builds").
    calendar: bool,
    /// The curves' vertical window, for the measure it was drawn in.
    cywin: Option<(builds::Measure, f64, f64)>,
    /// Rows a vertical drag on the gantt picked; None shows every match.
    gantt_rows: Option<Vec<u32>>,
    /// When the catalog was last fetched again (refresh), and a fetch is out.
    refreshed: f64,
    refreshing: Arc<std::sync::atomic::AtomicBool>,
    /// The Builds › Logs view (logs.rs).
    logs: logs::Logs,
    /// The manifests, the Manifests view and the Packages filter
    /// (manifests.rs); the Install tab (install.rs).
    man: manifests::Man,
    inst: install::Inst,
    /// The view each tab was last on: Packages, Builds, Install, and the
    /// Builds mode's whole stack (Packages' views and Builds' in one switch).
    last: [View; 4],
}

impl Default for SoftwarePane {
    fn default() -> Self {
        SoftwarePane {
            public: false,
            easyconfigs: Default::default(),
            ec_asked: Default::default(),
            ec_pick: Default::default(),
            slot: Default::default(),
            asked: false,
            view: View::Find,
            pkg_view: View::Find,
            build_view: View::Curves,
            query: String::new(),
            chips: 0,
            sort: Sort::Best,
            order: Order::Paradigm,
            by_box: false,
            by_set: false,
            sel: None,
            back: Vec::new(),
            results: Vec::new(),
            results_key: None,
            tree: None,
            window: None,
            find_view: (0.0, 0.0),
            follow_sel: false,
            focus_search: false,
            seed: 0x9e37_79b9_7f4a_7c15,
            deps_all: false,
            used_all: false,
            br: Default::default(),
            live: Default::default(),
            live_asked: f64::NEG_INFINITY,
            boxes: Default::default(),
            box_act: None,
            box_confirm: None,
            box_job: Default::default(),
            box_job_server: None,
            box_job_since: 0.0,
            box_job_poll: f64::NEG_INFINITY,
            refreshed: 0.0,
            refreshing: Default::default(),
            curves: None,
            measure: builds::Measure::Built,
            overlay: Some(builds::Measure::Spend),
            pinned: None,
            inv: None,
            inv_q: String::new(),
            cwin: None,
            calendar: false,
            cywin: None,
            gantt_rows: None,
            logs: Default::default(),
            man: Default::default(),
            inst: Default::default(),
            last: [View::Find, View::Curves, View::Install, View::Curves],
        }
    }
}

impl SoftwarePane {
    /// Fetch the catalog - at session start, so it is ready when the tab
    /// opens - unless it is here or on its way.
    pub fn load(&mut self, backend: &Backend, ctx: &egui::Context) {
        if self.asked {
            return;
        }
        self.asked = true;
        let slot = self.slot.clone();
        backend.software(ctx, move |r| {
            // Parsed and indexed off the UI thread on the desktop; in a
            // browser the callback is the page's own, once per session.
            let parsed = r.and_then(|bytes| Catalog::parse(&bytes)).map(Arc::new);
            *slot.lock().unwrap() = Some(parsed);
        });
    }

    /// Fetch the catalog again now and then while a Builds view shows, and
    /// swap it in only if it changed: once per session, the Curves and the
    /// Timeline stood still while the machines built on (operator,
    /// 2026-10-15: "not sure if logs and queue get updated at all in real
    /// time"). The spec rewrites it about every minute; a browser revalidates
    /// by ETag, and the same hash is not parsed again.
    pub fn refresh(&mut self, backend: &Backend, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        ctx.request_repaint_after(std::time::Duration::from_secs(120));
        if !self.asked || now - self.refreshed < 120.0 || self.refreshing.swap(true, std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        self.refreshed = now;
        let (slot, busy) = (self.slot.clone(), self.refreshing.clone());
        let have = match &*slot.lock().unwrap() {
            Some(Ok(c)) => format!("\"hash\":\"{}\"", c.hash),
            _ => String::new(),
        };
        backend.software(ctx, move |r| {
            busy.store(false, std::sync::atomic::Ordering::Relaxed);
            let Ok(bytes) = r else { return };
            if !have.is_empty() && bytes.windows(have.len()).any(|w| w == have.as_bytes()) {
                return;
            }
            if let Ok(c) = Catalog::parse(&bytes) {
                *slot.lock().unwrap() = Some(Ok(Arc::new(c)));
            }
        });
    }

    fn reload(&mut self, backend: &Backend, ctx: &egui::Context) {
        *self.slot.lock().unwrap() = None;
        self.asked = false;
        self.results_key = None;
        self.tree = None;
        self.load(backend, ctx);
    }

    fn open(&mut self, i: usize) {
        if self.sel != Some(i) {
            if let Some(s) = self.sel {
                self.back.push(s);
                if self.back.len() > 64 {
                    self.back.remove(0);
                }
            }
            self.sel = Some(i);
            self.deps_all = false;
            self.used_all = false;
        }
        self.follow_sel = true;
    }

    /// The catalog once it has arrived (the atlas draws the stack from it).
    pub fn catalog(&self) -> Option<Arc<Catalog>> {
        match &*self.slot.lock().unwrap() {
            Some(Ok(c)) => Some(c.clone()),
            _ => None,
        }
    }

    /// Packages › Find, opened on software `i` (the atlas's "Open in Software").
    pub fn reveal(&mut self, i: usize) {
        self.last[0] = View::Find;
        self.last[STACK] = View::Find;
        if self.view.tab() == 0 {
            self.view = View::Find;
        }
        self.open(i);
    }

    /// Put a filter in the search box (a chip or a breadcrumb was clicked).
    fn search_for(&mut self, q: String) {
        self.query = q;
        self.focus_search = true;
    }

    fn next_rand(&mut self) -> u64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        self.seed
    }

    /// The catalog's hash, and the manifest filter when Packages applies it:
    /// what the results and the tree were built for.
    fn data_key(&self, cat: &Catalog) -> String {
        if self.view.tab() == 0 {
            format!("{}|{}", cat.hash, self.man.filter_sig())
        } else {
            cat.hash.clone()
        }
    }

    fn refresh_results(&mut self, cat: &Catalog) {
        let key = (self.query.clone(), self.chips, self.sort as u8, self.data_key(cat));
        if self.results_key.as_ref() == Some(&key) {
            return;
        }
        let f = parse_filter(&self.query, self.chips);
        // Packages › Find and Tree, restricted to the picked manifests'
        // records (operator, 2026-10-15: "the different manifests really do
        // become an important filter").
        let allowed = if self.view.tab() == 0 { self.man.allowed() } else { None };
        let mut hits: Vec<(u32, i32)> = (0..cat.items.len())
            .filter(|&i| allowed.as_ref().map_or(true, |a| manifests::admits(a, cat, i)))
            .filter_map(|i| score(cat, i, &f).map(|s| (i as u32, s)))
            .collect();
        let st = |i: u32| cat.items[i as usize].st.clone().unwrap_or_default();
        match self.sort {
            Sort::Best if f.terms.is_empty() => hits.sort_by(|a, b| cat.lname[a.0 as usize].cmp(&cat.lname[b.0 as usize])),
            Sort::Best => hits.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| cat.lname[a.0 as usize].cmp(&cat.lname[b.0 as usize]))),
            Sort::Name => hits.sort_by(|a, b| cat.lname[a.0 as usize].cmp(&cat.lname[b.0 as usize])),
            Sort::Slowest => hits.sort_by_key(|h| std::cmp::Reverse(st(h.0).med.unwrap_or(-1))),
            Sort::Recent => hits.sort_by_key(|h| std::cmp::Reverse(st(h.0).last.unwrap_or(0))),
            Sort::Builds => hits.sort_by_key(|h| std::cmp::Reverse(st(h.0).n)),
            Sort::Failures => hits.sort_by_key(|h| std::cmp::Reverse((st(h.0).fail, st(h.0).n))),
        }
        self.results = hits.into_iter().map(|h| h.0).collect();
        self.results_key = Some(key);
        self.window = None;
    }

    /// One of the three tabs - Packages (0), Builds (1), Install (2) - or
    /// the Builds mode's whole stack (`STACK`), each keeping the view it was
    /// left on, so two of them side by side in the dock each show their own.
    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, group: usize) -> Option<Action> {
        let group = group.min(STACK);
        if group == STACK {
            self.view = self.last[STACK];
        } else if self.view.tab() != group {
            self.last[self.view.tab()] = self.view;
            self.view = self.last[group];
        }
        let act = self.ui_view(ui, ctx, backend, group == STACK);
        self.last[group] = self.view;
        act
    }

    fn ui_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, whole: bool) -> Option<Action> {
        // Install has no views or search of its own: no toolbar over it.
        if self.view == View::Install {
            self.install_view(ui, ctx, backend);
            return None;
        }
        self.load(backend, ctx);
        if self.view.builds() {
            self.refresh(backend, ctx);
        }
        let have = self.slot.lock().unwrap().clone();
        let mut action = None;
        let mut reload = false;
        let mut lucky = false;
        // ── toolbar: the search box, the paradigms, the views ──────────────
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            // The views as a segmented switch, spaced apart (operator, 2026-10-14:
            // "Find, Tree, Timeline, and Installed were quite close to one another").
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.spacing_mut().button_padding = egui::vec2(10.0, 2.0);
            // The tab's own views (Packages, Builds and Install are tabs of
            // their own beside App and Plots, 2026-10-17), one segmented style
            // (operator, 2026-10-15: "should look consistent with find tree").
            if self.view == View::Install {
                return;
            }
            const FIND: (View, &str, &str) = (View::Find, "Find", "search everything the stack builds");
            const TREE: (View, &str, &str) = (View::Tree, "Tree", "the matches as a tree: paradigm, arch, the stack bottom up, the site libraries top down, the day built");
            const MANIFESTS: (View, &str, &str) = (View::Manifests, "Manifests", "every env manifest: what it loads, its R and Python records, what is built; compare two");
            // The Builds mode, around three questions (operator, 2026-11-01):
            // what software there is, which environments hold it, how it was
            // built. One group at a time, then that group's own views - not
            // eight views side by side.
            if whole {
                match self.view {
                    View::Find | View::Tree => self.pkg_view = self.view,
                    v if v.builds() => self.build_view = v,
                    _ => {}
                }
                let groups = [
                    (self.pkg_view, "Packages", "every package the stack builds: search it, see where each one is and how it was built"),
                    (View::Manifests, "Environments", "every environment's manifest: what it holds, two compared, and a draft of changes to propose"),
                    (self.build_view, "Builds", "how it got built: the curves, the timeline, the queue and the logs"),
                ];
                for (v, name, tip) in groups {
                    if sized_segment(ui, name, tip, self.view.tab_whole() == v.tab_whole(), 13.5).clicked() {
                        self.view = v;
                    }
                }
                let own: &[(View, &str, &str)] = match self.view.tab_whole() {
                    0 => &[(View::Find, "List", "the matches, best first"), (View::Tree, "Tree", "the matches as a tree: paradigm, arch, the stack bottom up, the site libraries top down, the day built")],
                    2 => &[
                        (View::Curves, "Curves", "packages built, build time and what they cost, over time"),
                        (View::Timeline, "Timeline", "the gantt: a row per software, a line per machine"),
                        (View::Queue, "Queue", "what each machine is building now and what it has left"),
                        (View::Logs, "Logs", "each build machine's log, live as it builds"),
                    ],
                    _ => &[],
                };
                if !own.is_empty() {
                    theme::tool_sep(ui);
                    for &(v, name, tip) in own {
                        if sized_segment(ui, name, tip, self.view == v, 11.5).clicked() {
                            self.view = v;
                        }
                    }
                }
            }
            let views: &[(View, &str, &str)] = if whole {
                &[]
            } else if self.view.builds() {
                &[
                    (View::Curves, "Curves", "packages built, build time and what they cost, over time - a line per machine, arch, paradigm, manifest or language: click a moment to see what finished"),
                    (View::Timeline, "Timeline", "the gantt for the matches: a row per software, a line per machine; drag across for a stretch of time, down for a set of rows, Shift+drag to pan, Ctrl+wheel to zoom"),
                    (View::Queue, "Queue", "what each machine is building now and what it has left, by how long"),
                    (View::Logs, "Logs", "each build machine's log, live as it builds"),
                ]
            } else {
                &[FIND, TREE, MANIFESTS]
            };
            for &(v, name, tip) in views {
                if self.public && v == View::Manifests {
                    continue;
                }
                if segment(ui, name, tip, self.view == v).clicked() {
                    self.view = v;
                }
            }
            if !self.view.searched() {
                return;
            }
            ui.add_space(6.0);
            theme::tool_sep(ui);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.add_space(4.0);
                reload = theme::tool(ui, "⟳", "fetch the catalog again (it is rebuilt from the build database every few minutes)").clicked();
                lucky = theme::tool(ui, "⚄", "surprise me: open a random match").clicked();
                let w = ui.available_width().clamp(80.0, 420.0);
                let edit = egui::TextEdit::singleline(&mut self.query)
                    .hint_text("search software…  k:r  a:arm  set:r4.6  g:GCCcore  is:failed  >10m")
                    .desired_width(w);
                let r = ui.add(edit);
                if self.focus_search {
                    r.request_focus();
                    self.focus_search = false;
                }
                if r.has_focus() {
                    let (down, up, esc) = ui.input(|i| (i.key_pressed(Key::ArrowDown), i.key_pressed(Key::ArrowUp), i.key_pressed(Key::Escape)));
                    if esc {
                        self.query.clear();
                    }
                    if down || up {
                        let pos = self.sel.and_then(|s| self.results.iter().position(|&r| r as usize == s));
                        let next = match (pos, down) {
                            (None, _) => 0,
                            (Some(p), true) => (p + 1).min(self.results.len().saturating_sub(1)),
                            (Some(p), false) => p.saturating_sub(1),
                        };
                        if let Some(&i) = self.results.get(next) {
                            self.open(i as usize);
                        }
                    }
                }
            });
        });
        if reload {
            self.reload(backend, ctx);
        }
        // Neither needs the catalog: the manifests and the install are the
        // machine's (api.rs Query::Manifests, Query::Install…).
        match self.view {
            View::Install => {
                self.install_view(ui, ctx, backend);
                return None;
            }
            View::Manifests => {
                let cat = match &have {
                    Some(Ok(c)) => Some(c.clone()),
                    _ => None,
                };
                self.manifests_view(ui, ctx, backend, cat.as_deref());
                return None;
            }
            _ => {}
        }
        if self.view.tab() == 0 {
            self.man.load(backend, ctx);
            self.man.fetch_filter(backend, ctx);
        }
        let cat = match have {
            None => {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spinner();
                    ui.label(RichText::new("fetching the software catalog…").color(theme::muted()));
                });
                return None;
            }
            Some(Err(e)) => {
                egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    ui.label(RichText::new(e).color(theme::alt()));
                    ui.label(RichText::new("The catalog is built on the ndexr host by `ndexr aggregate` (spec `software`).").size(11.5).color(theme::hush()));
                });
                return None;
            }
            Some(Ok(c)) => c,
        };
        self.refresh_results(&cat);
        if lucky && !self.results.is_empty() {
            let k = (self.next_rand() % self.results.len() as u64) as usize;
            let i = self.results[k] as usize;
            self.open(i);
        }

        // ── the paradigm chips and the line that says what is shown ─────────
        if self.view.searched() { theme::strip(ui, 22.0, theme::surface(), |ui| {
            ui.add_space(6.0);
            for k in Kind::ALL.into_iter().filter(|k| cat.counts[*k as usize] > 0) {
                // No chips set means everything: all of them are lit.
                let on = self.chips == 0 || self.chips & k.bit() != 0;
                let r = chip(ui, &format!("{} {}", k.label(), thousands(cat.counts[k as usize])), k.color(), on);
                let tip = if self.chips == k.bit() {
                    "click again for everything".to_owned()
                } else if self.chips == 0 {
                    format!("only {}", k.long())
                } else if on {
                    format!("hide {}", k.long())
                } else {
                    format!("add {}", k.long())
                };
                if r.on_hover_text(tip).clicked() {
                    self.chips = pick_chip(self.chips, k.bit());
                }
            }
            if self.view.tab() == 0 {
                theme::tool_sep(ui);
                self.man.filter_picker(ui);
            }
            theme::tool_sep(ui);
            let all = cat.items.len();
            let shown = self.results.len();
            let words = if shown == all {
                format!("{} software · {} builds · {} of build time", thousands(all), thousands(cat.st_i.len()), fmt_dur(cat.total_secs))
            } else {
                format!("{} of {} match", thousands(shown), thousands(all))
            };
            ui.label(RichText::new(words).size(11.0).color(theme::muted()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("catalog {}", fmt_ago(now_secs(), cat.generated))).size(10.5).color(theme::hush()))
                    .on_hover_text(format!("built {} UTC from the fleet's build database · {}", fmt_date(cat.generated), cat.hash));
            });
        }); }

        // ── body: the view, and the open software beside or under it ───────
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let wide = rect.width() >= 720.0;
        let (main, detail) = match self.sel {
            None => (rect, None),
            Some(_) if wide => {
                let w = (rect.width() * 0.42).clamp(300.0, 520.0);
                let split = rect.right() - w;
                (rect.with_max_x(split - 1.0), Some(rect.with_min_x(split)))
            }
            Some(_) => {
                let split = rect.top() + rect.height() * 0.5;
                (rect.with_max_y(split - 1.0), Some(rect.with_min_y(split)))
            }
        };
        region(ui, main, |ui| match self.view {
            View::Find => self.find(ui, &cat),
            View::Tree => self.tree(ui, &cat),
            View::Timeline => self.timeline(ui, &cat),
            View::Curves => {
                self.live_tick(backend, ctx);
                self.curves_view(ui, &cat)
            }
            View::Queue => {
                self.live_tick(backend, ctx);
                self.queue_view(ui, &cat)
            }
            View::Logs => self.logs_view(ui, backend, ctx),
            // Drawn before the catalog is needed (above).
            View::Manifests | View::Install => {}
        });
        if let Some(d) = detail {
            let p = ui.painter();
            if wide {
                p.vline(d.left(), d.y_range(), Stroke::new(1.0, theme::pane_brd()));
            } else {
                p.hline(d.x_range(), d.top(), Stroke::new(1.0, theme::pane_brd()));
            }
            region(ui, d.shrink2(egui::vec2(0.0, 1.0)).with_min_x(d.left() + 1.0), |ui| {
                action = self.detail(ui, ctx, backend, &cat);
            });
        }
        action
    }

    // ── Find ───────────────────────────────────────────────────────────────

    fn find(&mut self, ui: &mut egui::Ui, cat: &Catalog) {
        theme::strip(ui, 22.0, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("sort").size(11.0).color(theme::hush()));
            let now = Sort::ALL.iter().find(|s| s.0 == self.sort).map_or("", |s| s.1);
            egui::ComboBox::from_id_salt("sw-sort").selected_text(RichText::new(now).size(11.5)).width(150.0).show_ui(ui, |ui| {
                for (s, name) in Sort::ALL {
                    ui.selectable_value(&mut self.sort, s, name);
                }
            });
        });
        let row_h = 24.0;
        let n = self.results.len();
        if n == 0 {
            ui.add_space(14.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Nothing matches. Try fewer letters, or drop a filter.").color(theme::hush())));
            return;
        }
        let mut area = egui::ScrollArea::vertical().id_salt("sw-find").auto_shrink([false, false]);
        if std::mem::take(&mut self.follow_sel) {
            if let Some(pos) = self.sel.and_then(|s| self.results.iter().position(|&r| r as usize == s)) {
                let (off, h) = self.find_view;
                let y = pos as f32 * row_h;
                if y < off {
                    area = area.vertical_scroll_offset(y);
                } else if y + row_h > off + h && h > 0.0 {
                    area = area.vertical_scroll_offset(y + row_h - h);
                }
            }
        }
        let terms = parse_filter(&self.query, 0).terms;
        let mut picked = None;
        ui.spacing_mut().item_spacing.y = 0.0;
        let out = area.show_rows(ui, row_h, n, |ui, range| {
            for pos in range {
                let i = self.results[pos] as usize;
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
                item_row(ui, r, cat, i, self.sel == Some(i), resp.hovered(), &terms);
                if resp.clicked() {
                    picked = Some(i);
                }
            }
        });
        self.find_view = (out.state.offset.y, out.inner_rect.height());
        if let Some(i) = picked {
            self.open(i);
            self.follow_sel = false;
        }
    }

    // ── Tree ───────────────────────────────────────────────────────────────

    fn paths(cat: &Catalog, i: usize, order: Order) -> Vec<Vec<String>> {
        let it = &cat.items[i];
        let k = cat.kind[i];
        let homes: Vec<String> = match k {
            k if k.is_lang() && !it.s.is_empty() => it.s.clone(),
            k if k.is_lang() => vec!["(no env set)".into()],
            Kind::Env => vec![env_family(&it.n).into()],
            _ => vec![it.g.clone()],
        };
        let arches: Vec<String> = if it.a.is_empty() { vec!["(no arch)".into()] } else { it.a.clone() };
        let mut out = Vec::new();
        match order {
            Order::Paradigm => {
                for h in &homes {
                    out.push(vec![k.long().to_string(), h.clone()]);
                }
            }
            Order::Arch => {
                for a in &arches {
                    for h in &homes {
                        out.push(vec![a.clone(), k.long().to_string(), h.clone()]);
                    }
                }
            }
            Order::Stack => match k {
                Kind::Prefix => out.push(vec!["1 · Gentoo Prefix (the compatibility layer)".into(), it.g.clone()]),
                Kind::Env => out.push(vec!["4 · environments (ml env/<set>)".into(), env_family(&it.n).into()]),
                Kind::EasyBuild => out.push(vec!["2 · EasyBuild, by toolchain".into(), it.g.clone()]),
                _ => {
                    for h in &homes {
                        out.push(vec!["3 · site libraries".into(), h.clone(), k.long().to_string()]);
                    }
                }
            },
            Order::Libraries => match k {
                Kind::R | Kind::Python | Kind::Rust | Kind::Julia => {
                    for h in &homes {
                        out.push(vec![h.clone(), k.long().to_string()]);
                    }
                }
                Kind::EasyBuild => {
                    // The modules an env set loads, under that set.
                    for &u in &cat.used_by[i] {
                        let user = &cat.items[u as usize].n;
                        if let Some(set) = user.strip_prefix("env/") {
                            out.push(vec![set.to_string(), "modules it loads".into()]);
                        }
                    }
                }
                Kind::Env => out.push(vec![it.n.trim_start_matches("env/").to_string()]),
                Kind::Prefix | Kind::Other => {}
            },
            Order::Day => {
                let day = it.st.as_ref().and_then(|s| s.last).map(|t| fmt_day(t)).unwrap_or_else(|| "never timed".into());
                out.push(vec![day, k.long().to_string()]);
            }
        }
        out
    }

    fn tree(&mut self, ui: &mut egui::Ui, cat: &Catalog) {
        theme::strip(ui, 22.0, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("grow by").size(11.0).color(theme::hush()));
            let now = Order::ALL.iter().find(|o| o.0 == self.order).map_or("", |o| o.1);
            egui::ComboBox::from_id_salt("sw-order").selected_text(RichText::new(now).size(11.5)).width(190.0).show_ui(ui, |ui| {
                for (o, name, tip) in Order::ALL {
                    ui.selectable_value(&mut self.order, o, name).on_hover_text(tip);
                }
            });
        });
        let key = (self.query.clone(), self.chips, self.order as u8, self.data_key(cat));
        if self.tree.as_ref().map(|t| &t.0) != Some(&key) {
            let mut root = Node::default();
            for &i in &self.results {
                let secs = cat.med(i as usize).unwrap_or(0);
                for p in Self::paths(cat, i as usize, self.order) {
                    root.add(&p, i, secs);
                }
            }
            self.tree = Some((key, root));
        }
        let Some((_, root)) = self.tree.take() else { return };
        let searching = !self.query.trim().is_empty() || self.chips != 0;
        let salt = egui::Id::new(("sw-tree", self.order as u8, if searching { self.query.clone() } else { String::new() }, self.chips));
        let mut picked = None;
        egui::ScrollArea::vertical().id_salt("sw-tree-scroll").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(6, 4)).show(ui, |ui| {
                if root.kids.is_empty() && root.items.is_empty() {
                    ui.label(RichText::new("Nothing matches.").color(theme::hush()));
                }
                let open_all = searching && root.count <= 400;
                for (name, kid) in &root.kids {
                    tree_node(ui, cat, name, kid, salt.with(name), 0, open_all, self.sel, &mut picked);
                }
                for &i in &root.items {
                    if leaf(ui, cat, i as usize, self.sel == Some(i as usize)).clicked() {
                        picked = Some(i as usize);
                    }
                }
            });
        });
        self.tree = Some((self.tree_key_dummy(), root));
        if let Some(i) = picked {
            self.open(i);
        }
    }

    /// The tree's key is only read to compare: put back the one it was built with.
    fn tree_key_dummy(&self) -> (String, u8, u8, String) {
        let hash = self.results_key.as_ref().map(|k| k.3.clone()).unwrap_or_default();
        (self.query.clone(), self.chips, self.order as u8, hash)
    }

    // ── Timeline ───────────────────────────────────────────────────────────

    fn timeline(&mut self, ui: &mut egui::Ui, cat: &Catalog) {
        let mut rows: Vec<u32> = self.results.iter().copied().filter(|&i| !cat.by_item[i as usize].is_empty()).collect();
        let all_rows = rows.len();
        if let Some(keep) = &self.gantt_rows {
            let picked: Vec<u32> = rows.iter().copied().filter(|i| keep.contains(i)).collect();
            // A new search that shares none of the picked rows shows its own.
            if picked.is_empty() {
                self.gantt_rows = None;
            } else {
                rows = picked;
            }
        }
        // The span of every bar the matches have.
        let (mut lo, mut hi) = (i64::MAX, i64::MIN);
        for &i in &rows {
            for &k in &cat.by_item[i as usize] {
                let k = k as usize;
                lo = lo.min(cat.st_t[k]);
                hi = hi.max(cat.st_t[k] + cat.st_d[k]);
            }
        }
        if rows.is_empty() || lo > hi {
            theme::strip(ui, 22.0, theme::bg(), |_| {});
            ui.add_space(14.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("None of the matches has a timed build.").color(theme::hush())));
            return;
        }
        let full = (lo as f64, (hi.max(lo + 60)) as f64);
        let (mut x0, mut x1) = self.window.unwrap_or(full);
        let newest_run = cat.runs.iter().max_by_key(|r| r.start).map(|r| ((r.start - cat.t0) as f64, (r.end - cat.t0) as f64));
        let now = (now_secs() - cat.t0) as f64;
        let latest = (hi as f64).max(now);
        theme::strip(ui, 22.0, theme::bg(), |ui| {
            ui.add_space(6.0);
            if theme::tool(ui, "Fit", "every build of the matches").clicked() {
                self.window = None;
            }
            if self.gantt_rows.is_some() && theme::tool(ui, &format!("✕ {} of {all_rows} rows", rows.len()), "the rows a drag picked - click for every match").clicked() {
                self.gantt_rows = None;
            }
            if let Some(r) = newest_run {
                if theme::tool(ui, "Newest run", "the latest run of any machine").clicked() {
                    self.window = Some(r);
                }
            }
            for (name, secs) in [("24 h", 86400.0), ("7 d", 7.0 * 86400.0)] {
                if theme::tool(ui, name, &format!("the last {name} of building")).clicked() {
                    self.window = Some((latest - secs, latest));
                }
            }
            if theme::tool(ui, "−", "zoom out").clicked() {
                let (c, w) = ((x0 + x1) / 2.0, (x1 - x0) * 1.6);
                self.window = Some((c - w / 2.0, c + w / 2.0));
            }
            if theme::tool(ui, "+", "zoom in").clicked() {
                let (c, w) = ((x0 + x1) / 2.0, (x1 - x0) / 1.6);
                self.window = Some((c - w / 2.0, c + w / 2.0));
            }
            theme::tool_sep(ui);
            ui.label(RichText::new("colour").size(11.0).color(theme::hush()));
            let mut mode = if self.by_set { 2 } else if self.by_box { 1 } else { 0 };
            ui.selectable_value(&mut mode, 0, RichText::new("paradigm").size(11.5));
            ui.selectable_value(&mut mode, 1, RichText::new("machine").size(11.5));
            ui.selectable_value(&mut mode, 2, RichText::new("env set").size(11.5)).on_hover_text("which env manifests list each R and Python library; grey is in none (Prefix, EasyBuild). The build records do not yet say which manifest a single build was for");
            (self.by_box, self.by_set) = (mode == 1, mode == 2);
            if self.by_set {
                for (g, (label, n)) in cat.sets.iter().enumerate() {
                    ui.label(RichText::new("■").color(Catalog::group_color(g)).size(11.0));
                    ui.label(RichText::new(label).size(10.5).color(theme::muted())).on_hover_text(format!("{n} packages are in exactly these manifests"));
                }
            }
            if self.by_box {
                for (b, bx) in cat.boxes.iter().enumerate() {
                    ui.label(RichText::new("■").color(box_color(b)).size(11.0));
                    ui.label(RichText::new(short_box(&bx.name)).size(10.5).color(theme::muted()));
                }
            }
        });
        if let Some(w) = self.window {
            (x0, x1) = w;
            // A time window shows only what built in it (operator, 2026-10-18:
            // "if we selected a time range that only those things which exist
            // within that time range are displayed"): every match had kept its
            // row, empty in the window, so the latest builds across paradigms
            // were lost among hundreds of blank rows.
            rows.retain(|&i| {
                cat.by_item[i as usize].iter().any(|&k| {
                    let (t, d) = (cat.st_t[k as usize] as f64, cat.st_d[k as usize] as f64);
                    t + d >= x0 && t <= x1
                })
            });
            if rows.is_empty() {
                ui.add_space(14.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("Nothing of the matches built in this window - Fit shows all of them.").color(theme::hush())));
                return;
            }
        }
        let label_w = 170.0_f32.min(ui.available_width() * 0.35);
        let nb = cat.boxes.len().max(1);
        let row_h = (6.0 + 3.0 * nb as f32).clamp(14.0, 24.0);

        // The time axis, pinned above the rows.
        let (axis, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 18.0), Sense::hover());
        let plot = Rect::from_min_max(egui::pos2(axis.left() + label_w, axis.top()), egui::pos2(axis.right() - 8.0, axis.bottom()));
        let to_x = |t: f64| plot.left() + ((t - x0) / (x1 - x0).max(1.0)) as f32 * plot.width();
        {
            let p = ui.painter();
            p.hline(axis.x_range(), axis.bottom() - 0.5, Stroke::new(1.0, theme::pane_brd()));
            let step = tick_step((x1 - x0) / (plot.width() / 90.0).max(1.0) as f64);
            let mut t = ((x0 + cat.t0 as f64) / step).ceil() * step - cat.t0 as f64;
            while t <= x1 {
                let x = to_x(t);
                p.vline(x, axis.bottom() - 4.0..=axis.bottom(), Stroke::new(1.0, theme::hush()));
                let abs = t as i64 + cat.t0;
                let text = if step >= 86400.0 { fmt_day(abs) } else { fmt_date(abs) };
                p.text(egui::pos2(x + 2.0, axis.top() + 2.0), Align2::LEFT_TOP, text, FontId::proportional(10.0), theme::hush());
                t += step;
            }
            // Now (operator, 2026-10-15: "need to know current time with a
            // vertical bar"): labelled here, drawn through every row below.
            if now >= x0 && now <= x1 {
                let x = to_x(now);
                p.vline(x, axis.y_range(), Stroke::new(1.5, now_color()));
                p.text(egui::pos2(x - 3.0, axis.top() + 2.0), Align2::RIGHT_TOP, "now", FontId::proportional(10.0), now_color());
            }
        }

        // Rows: a software each, a thin line per machine inside it.
        let mut picked = None;
        let mut zoom_to: Option<(f64, f64)> = None;
        let mut pan = 0.0_f32;
        let mut zoom: Option<(f32, f32)> = None;
        let mut hover: Option<(usize, usize)> = None;
        // Drag selects (brush.rs): the row being dragged on, and every row
        // drawn, so a vertical stretch can say which software it covered.
        let mut refit = false;
        let mut drag_resp: Option<egui::Response> = None;
        let mut drawn: Vec<(f32, f32, u32)> = Vec::new();
        ui.spacing_mut().item_spacing.y = 0.0;
        let gantt = egui::ScrollArea::vertical().id_salt("sw-gantt").auto_shrink([false, false]).scroll_source(egui::containers::scroll_area::ScrollSource { drag: egui::containers::scroll_area::DragScroll::Never, ..Default::default() }).show_rows(ui, row_h, rows.len(), |ui, range| {
            for pos in range {
                let i = rows[pos] as usize;
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click_and_drag());
                drawn.push((r.top(), r.bottom(), i as u32));
                let p = ui.painter_at(r);
                let selected = self.sel == Some(i);
                if selected {
                    p.rect_filled(r, 0.0, theme::surface_hi());
                } else if pos % 2 == 1 {
                    p.rect_filled(r, 0.0, theme::surface().gamma_multiply(0.5));
                }
                let k = cat.kind[i];
                p.circle_filled(egui::pos2(r.left() + 8.0, r.center().y), 3.0, k.color());
                let name = elide(&cat.items[i].n, ((label_w - 20.0) / 6.4) as usize);
                p.text(egui::pos2(r.left() + 16.0, r.center().y), Align2::LEFT_CENTER, name, FontId::proportional(11.0), if selected { theme::text_strong() } else { theme::text() });
                let lane_h = ((row_h - 4.0) / nb as f32).max(2.0);
                let pointer = resp.hover_pos();
                for &kk in &cat.by_item[i] {
                    let kk = kk as usize;
                    let (t, d) = (cat.st_t[kk] as f64, cat.st_d[kk] as f64);
                    if t + d < x0 || t > x1 {
                        continue;
                    }
                    let b = cat.box_of_step(kk);
                    let xa = to_x(t).max(plot.left());
                    let xb = to_x(t + d).min(plot.right()).max(xa + 1.5);
                    let y = r.top() + 2.0 + b as f32 * lane_h;
                    let bar = Rect::from_min_max(egui::pos2(xa, y), egui::pos2(xb, y + lane_h - 0.5));
                    let fill = match cat.st_s[kk] {
                        b'f' => theme::alt(),
                        b'r' => theme::amber(),
                        b's' => theme::hush(),
                        _ if self.by_set => cat.set_color(i).unwrap_or(theme::hush().gamma_multiply(0.6)),
                        _ if self.by_box => box_color(b),
                        _ => k.color(),
                    };
                    p.rect_filled(bar, 1.0, fill);
                    if pointer.is_some_and(|q| bar.expand2(egui::vec2(2.0, 1.0)).contains(q)) {
                        hover = Some((i, kk));
                    }
                }
                if now >= x0 && now <= x1 {
                    p.vline(to_x(now), r.y_range(), Stroke::new(1.0, now_color().gamma_multiply(0.7)));
                }
                if resp.clicked() {
                    picked = Some(i);
                }
                if resp.double_clicked() {
                    if let Some((_, kk)) = hover {
                        let run = &cat.runs[cat.st_r[kk] as usize];
                        zoom_to = Some(((run.start - cat.t0) as f64, (run.end - cat.t0) as f64));
                    } else {
                        // Double-click on nothing: every match, all of time.
                        refit = true;
                    }
                }
                if resp.dragged() || resp.drag_stopped() {
                    drag_resp = Some(resp.clone());
                }
                if resp.hovered() {
                    // A sideways scroll (trackpad, Shift+wheel) pans.
                    pan += ui.input(|inp| inp.smooth_scroll_delta.x);
                    let z = ui.input(|inp| inp.zoom_delta());
                    if (z - 1.0).abs() > 1e-3 {
                        if let Some(q) = pointer {
                            zoom = Some((z, q.x));
                        }
                    }
                }
            }
        });
        let w = (x1 - x0).max(1.0);
        let from_x = |px: f32| x0 + ((px - plot.left()) / plot.width().max(1.0)) as f64 * w;
        let area = Rect::from_x_y_ranges(plot.x_range(), gantt.inner_rect.y_range());
        let rows_in = |y0: f32, y1: f32| -> Vec<u32> { drawn.iter().filter(|(a, b, _)| *b > y0 && *a < y1).map(|r| r.2).collect() };
        let mut brushing = false;
        if let Some(resp) = &drag_resp {
            match brush::drag(ui, resp, area, egui::Id::new("sw-gantt-brush"), true) {
                brush::Drag::Pan(d) => pan += d.x,
                brush::Drag::Brushing => brushing = true,
                brush::Drag::Pick(brush::Pick::X(a, b)) => self.window = Some((from_x(a), from_x(b).max(from_x(a) + 30.0))),
                brush::Drag::Pick(brush::Pick::Y(a, b)) => {
                    let keep = rows_in(a, b);
                    if !keep.is_empty() {
                        self.gantt_rows = Some(keep);
                        self.window = None;
                    }
                }
                brush::Drag::Pick(brush::Pick::Both(r)) => {
                    let keep = rows_in(r.top(), r.bottom());
                    if !keep.is_empty() {
                        self.gantt_rows = Some(keep);
                    }
                    self.window = Some((from_x(r.left()), from_x(r.right()).max(from_x(r.left()) + 30.0)));
                }
                brush::Drag::None => {}
            }
        }
        if let Some((i, kk)) = hover.filter(|_| !brushing) {
            let b = cat.box_of_step(kk);
            let run = &cat.runs[cat.st_r[kk] as usize];
            let status = match cat.st_s[kk] {
                b'd' => "built",
                b'f' => "failed",
                b'r' => "building now",
                _ => "stopped",
            };
            let abs = cat.st_t[kk] + cat.t0;
            egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), egui::Id::new("sw-gantt-tip"), egui::PopupAnchor::Pointer).show(|ui| {
                ui.label(RichText::new(&cat.items[i].n).strong().color(cat.kind[i].color()));
                ui.label(format!("{status} in {} on {}", fmt_dur(cat.st_d[kk]), cat.boxes.get(b).map_or("?", |x| x.name.as_str())));
                ui.label(RichText::new(format!("started {} UTC · run {} · {}", fmt_date(abs), run.id, run.itype)).size(11.0).color(theme::muted()));
            });
        }
        if pan != 0.0 {
            let dt = -(pan as f64) / plot.width().max(1.0) as f64 * w;
            let (a, b) = self.window.unwrap_or((x0, x1));
            self.window = Some((a + dt, b + dt));
        }
        if let Some((z, at)) = zoom {
            let c = x0 + ((at - plot.left()) / plot.width().max(1.0)) as f64 * w;
            let nw = (w / z as f64).max(30.0);
            let f = (c - x0) / w;
            self.window = Some((c - f * nw, c - f * nw + nw));
        }
        if refit {
            self.window = None;
            self.gantt_rows = None;
        }
        if let Some(zw) = zoom_to {
            let pad = (zw.1 - zw.0).max(60.0) * 0.03;
            self.window = Some((zw.0 - pad, zw.1 + pad));
        }
        if let Some(i) = picked {
            self.open(i);
        }
    }

    // ── one software, opened ───────────────────────────────────────────────

    /// An EasyBuild module's easyconfig, read in place (operator, 2026-10-19:
    /// "if we have an easybuild configuration for something we need to be able
    /// to see it"): the installed module's own copy, else the suite's recipe.
    /// One version at a time - the module's default first.
    fn easyconfig_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, i: usize, it: &RawItem) {
        // env/<set> is EasyBuild's `env` at version <set>; env/R/<v> is
        // `env/R` (the host maps it to env-R) at <v>.
        let (name, versions): (String, Vec<String>) = match it.n.rsplit_once('/') {
            Some((n, v)) => (n.to_owned(), vec![v.to_owned()]),
            None => (it.n.clone(), it.v.clone()),
        };
        if versions.is_empty() {
            return;
        }
        let default = it.m.split_once('/').map(|(_, v)| v.to_owned()).filter(|v| versions.contains(v)).unwrap_or_else(|| versions.last().cloned().unwrap_or_default());
        let ver = self.ec_pick.get(&i).cloned().filter(|v| versions.contains(v)).unwrap_or(default);
        let key = format!("{name}/{ver}");
        if self.ec_asked.insert(key.clone()) {
            let (slot, k) = (self.easyconfigs.clone(), key.clone());
            backend.query(crate::api::Query::Easyconfig { name: name.clone(), version: ver.clone() }, ctx, move |r: Result<crate::api::EasyconfigView, String>| {
                slot.lock().unwrap().insert(k, r);
            });
        }
        ui.add_space(8.0);
        section(ui, "Easyconfig");
        if versions.len() > 1 {
            ui.horizontal_wrapped(|ui| {
                for v in versions.iter().rev() {
                    if ui.selectable_label(*v == ver, RichText::new(v).monospace().size(11.0)).clicked() {
                        self.ec_pick.insert(i, v.clone());
                    }
                }
            });
        }
        let got = self.easyconfigs.lock().unwrap().get(&key).cloned();
        match got {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new(format!("reading {name}-{ver}.eb…")).size(11.5).color(theme::muted()));
                });
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).size(11.5).color(theme::hush()));
            }
            Some(Ok(v)) => {
                ui.horizontal_wrapped(|ui| {
                    let whence = if v.built { "the copy installed with the module - what it was built from" } else { "the suite's recipe - not installed on this machine" };
                    ui.label(RichText::new(&v.path).monospace().size(10.5).color(theme::muted())).on_hover_text(whence);
                    if theme::tool(ui, "copy", "copy the easyconfig").clicked() {
                        ctx.copy_text(v.text.clone());
                    }
                });
                egui::Frame::new().fill(theme::surface()).corner_radius(4).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt(("sw-easyconfig", i)).max_height(380.0).show(ui, |ui| {
                        let mut text = v.text.as_str();
                        ui.add(egui::TextEdit::multiline(&mut text).code_editor().font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                    });
                });
            }
        }
    }

    fn detail(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, cat: &Catalog) -> Option<Action> {
        let i = self.sel?;
        if i >= cat.items.len() {
            self.sel = None;
            return None;
        }
        let it = &cat.items[i];
        let k = cat.kind[i];
        let mut action = None;
        let mut go: Option<usize> = None;
        let mut close = false;
        let mut back = false;
        let mut search: Option<String> = None;
        theme::strip(ui, 22.0, theme::bg(), |ui| {
            ui.add_space(4.0);
            if theme::tool_if(ui, !self.back.is_empty(), "←", "back to the one before").clicked() {
                back = true;
            }
            // Breadcrumbs: each a filter.
            let arch = it.a.first().cloned().unwrap_or_default();
            if !arch.is_empty() && crumb(ui, &arch, &format!("everything built for {arch}")).clicked() {
                search = Some(format!("a:{arch}"));
            }
            if crumb(ui, k.label(), &format!("every {}", k.long())).clicked() {
                search = Some(format!("k:{}", k.label().to_lowercase()));
            }
            let home = if k.is_lang() { it.s.first().cloned().unwrap_or_else(|| it.g.clone()) } else { it.g.clone() };
            if !home.is_empty() && crumb(ui, &home, "everything in it").clicked() {
                search = Some(if k.is_lang() && !it.s.is_empty() { format!("set:{home} k:{}", k.label().to_lowercase()) } else { format!("g:{home}") });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(4.0);
                close = theme::tool(ui, "✕", "close").clicked();
            });
        });
        egui::ScrollArea::vertical().id_salt(("sw-detail", i)).auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&it.n).font(theme::display(18.0)).color(theme::text_strong()));
                    chip(ui, k.label(), k.color(), true);
                    if let Some(st) = &it.st {
                        if !st.lv.is_empty() {
                            ui.label(RichText::new(&st.lv).monospace().size(11.5).color(theme::muted()));
                        }
                    }
                });
                if !it.p.is_empty() {
                    ui.label(RichText::new(&it.p).size(13.0).color(theme::primary()));
                }
                if !it.d.is_empty() {
                    ui.add_space(2.0);
                    ui.label(RichText::new(&it.d).size(12.0).color(theme::text()));
                }
                ui.add_space(6.0);
                // How to use it, in the console it belongs to.
                let use_code = match k {
                    Kind::EasyBuild | Kind::Env if !it.m.is_empty() => Some((Lang::R, format!("ml(\"{}\")", it.m))),
                    Kind::R => Some((Lang::R, format!("library({})", it.n))),
                    Kind::Julia => Some((Lang::Julia, format!("using {}", it.n))),
                    // A crate goes into the project's Cargo.toml, in the terminal.
                    Kind::Rust => Some((Lang::Shell, match it.v.first().filter(|v| !v.is_empty()) {
                        Some(v) => format!("cargo add {}@{v}", it.n),
                        None => format!("cargo add {}", it.n),
                    })),
                    _ => None,
                };
                if let Some((lang, code)) = use_code {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&code).monospace().size(12.0).color(theme::ok()));
                        if ui.add(theme::primary_button("Use")).on_hover_text(if k == Kind::R {
                            "attach the package in the R console"
                        } else if k == Kind::Julia {
                            "load the package in the Julia console"
                        } else if k == Kind::Rust {
                            "add the crate to this project's Cargo.toml (cargo add, in the terminal)"
                        } else {
                            "load the module into the R console: ml() puts its programs and libraries on this R session's PATH, as `ml` does in a shell"
                        }).clicked() {
                            action = Some(Action::Run(lang, code.clone()));
                        }
                        if theme::tool(ui, "copy", "copy").clicked() {
                            ctx.copy_text(code.clone());
                        }
                    });
                    ui.add_space(4.0);
                }
                egui::Grid::new(("sw-facts", i)).num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
                    let key = |ui: &mut egui::Ui, s: &str| ui.label(RichText::new(s).size(11.0).color(theme::hush()));
                    if !it.v.is_empty() {
                        key(ui, if it.v.len() == 1 { "version" } else { "versions" });
                        ui.label(RichText::new(it.v.iter().rev().take(12).cloned().collect::<Vec<_>>().join("  ·  ")).monospace().size(11.0).color(theme::text()));
                        ui.end_row();
                    }
                    if !it.m.is_empty() {
                        key(ui, "module");
                        ui.label(RichText::new(&it.m).monospace().size(11.0));
                        ui.end_row();
                    }
                    if !it.a.is_empty() {
                        key(ui, "built for");
                        ui.horizontal_wrapped(|ui| {
                            for a in &it.a {
                                if chip(ui, a, theme::net(), false).on_hover_text(format!("everything built for {a}")).clicked() {
                                    search = Some(format!("a:{a}"));
                                }
                            }
                        });
                        ui.end_row();
                    }
                    if !it.s.is_empty() {
                        key(ui, "env sets");
                        ui.horizontal_wrapped(|ui| {
                            for s in &it.s {
                                if chip(ui, s, theme::amber(), false).on_hover_text(format!("everything in {s} (ml env/{s})")).clicked() {
                                    search = Some(format!("set:{s}"));
                                }
                            }
                        });
                        ui.end_row();
                    }
                    if !it.g.is_empty() {
                        key(ui, "group");
                        if ui.link(RichText::new(&it.g).size(11.5)).clicked() {
                            search = Some(format!("g:{}", it.g));
                        }
                        ui.end_row();
                    }
                    if !it.l.is_empty() {
                        key(ui, "licence");
                        ui.label(RichText::new(&it.l).size(11.5).color(theme::muted()));
                        ui.end_row();
                    }
                    if it.h.starts_with("http") {
                        key(ui, "homepage");
                        ui.hyperlink_to(RichText::new(elide(&it.h, 60)).size(11.5), &it.h);
                        ui.end_row();
                    }
                });

                if matches!(k, Kind::EasyBuild | Kind::Env) {
                    self.easyconfig_section(ui, ctx, backend, i, it);
                }

                // Build times.
                ui.add_space(8.0);
                section(ui, "Build times");
                match &it.st {
                    None => {
                        ui.label(RichText::new("Never timed by the fleet: it is in the stack, but no build of it was recorded (a copy carried in, or built before timing began).").size(11.5).color(theme::hush()));
                    }
                    Some(st) => {
                        ui.horizontal_wrapped(|ui| {
                            stat(ui, &st.med.map(fmt_dur).unwrap_or_else(|| "—".into()), "median");
                            stat(ui, &st.min.map(fmt_dur).unwrap_or_else(|| "—".into()), "fastest");
                            stat(ui, &st.max.map(fmt_dur).unwrap_or_else(|| "—".into()), "slowest");
                            stat(ui, &st.n.to_string(), "builds");
                            if st.fail > 0 {
                                stat_c(ui, &st.fail.to_string(), "failed", theme::alt());
                            }
                            stat(ui, &fmt_dur(st.tot), "in all");
                        });
                        if let Some(l) = st.last {
                            ui.label(RichText::new(format!("last built {} ({} UTC) · first {}", fmt_ago(now_secs(), l), fmt_date(l), st.first.map(fmt_day).unwrap_or_default())).size(11.0).color(theme::muted()));
                        }
                        if !st.arch.is_empty() {
                            ui.add_space(4.0);
                            let most = st.arch.values().copied().max().unwrap_or(1).max(1);
                            for (a, s) in &st.arch {
                                ui.horizontal(|ui| {
                                    ui.add_sized([62.0, 14.0], egui::Label::new(RichText::new(a).size(11.0).color(theme::muted())));
                                    let w = (ui.available_width() - 70.0).max(40.0);
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(w, 10.0), Sense::hover());
                                    ui.painter().rect_filled(r, 2.0, theme::surface_hi());
                                    let f = (*s as f32 / most as f32).clamp(0.02, 1.0);
                                    ui.painter().rect_filled(r.with_max_x(r.left() + r.width() * f), 2.0, k.color());
                                    ui.label(RichText::new(fmt_dur(*s)).size(11.0).monospace());
                                });
                            }
                        }
                        ui.add_space(6.0);
                        history(ui, cat, i);
                    }
                }

                // Bottom up and top down.
                ui.add_space(8.0);
                let deps = &it.dep;
                section(ui, &format!("Built on · {}", deps.len()));
                if deps.is_empty() {
                    ui.label(RichText::new(if k == Kind::Prefix { "Nothing in the stack below it." } else { "No recorded dependencies." }).size(11.0).color(theme::hush()));
                } else {
                    let show = if self.deps_all { deps.len() } else { deps.len().min(40) };
                    ui.horizontal_wrapped(|ui| {
                        for &d in &deps[..show] {
                            if link_chip(ui, cat, d as usize).clicked() {
                                go = Some(d as usize);
                            }
                        }
                        if show < deps.len() && theme::tool(ui, &format!("+{} more", deps.len() - show), "show them all").clicked() {
                            self.deps_all = true;
                        }
                    });
                }
                ui.add_space(6.0);
                let users = &cat.used_by[i];
                section(ui, &format!("Built on it · {}", users.len()));
                if users.is_empty() {
                    ui.label(RichText::new("Nothing in the catalog names it as a dependency.").size(11.0).color(theme::hush()));
                } else {
                    let show = if self.used_all { users.len() } else { users.len().min(40) };
                    ui.horizontal_wrapped(|ui| {
                        for &u in &users[..show] {
                            if link_chip(ui, cat, u as usize).clicked() {
                                go = Some(u as usize);
                            }
                        }
                        if show < users.len() && theme::tool(ui, &format!("+{} more", users.len() - show), "show them all").clicked() {
                            self.used_all = true;
                        }
                    });
                }
            });
        });
        if close {
            self.sel = None;
            self.back.clear();
        } else if back {
            if let Some(b) = self.back.pop() {
                self.sel = Some(b);
                self.follow_sel = true;
            }
        } else if let Some(g) = go {
            self.open(g);
        }
        if let Some(q) = search {
            self.search_for(q);
        }
        action
    }
}

// ── drawing pieces ─────────────────────────────────────────────────────────

fn region<R>(ui: &mut egui::Ui, rect: Rect, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    ui.scope_builder(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)), |ui| {
        ui.set_clip_rect(rect.intersect(ui.clip_rect()));
        add(ui)
    })
    .inner
}

/// A small pill: filled when `on`, outlined otherwise.
/// A paradigm chip clicked (operator, 2026-10-15): from everything, it shows
/// that one in isolation; an unlit one is added; the lone lit one again brings
/// everything back; a lit one among several is dropped. 0 is everything.
/// One button of the toolbar's segmented switches: Packages | Builds and the
/// views under each look alike.
pub fn segment(ui: &mut egui::Ui, name: &str, tip: &str, on: bool) -> egui::Response {
    sized_segment(ui, name, tip, on, 12.0)
}

/// `segment` at a size: the Builds mode's three groups larger than the views
/// under each, so which question is being answered reads first.
fn sized_segment(ui: &mut egui::Ui, name: &str, tip: &str, on: bool, size: f32) -> egui::Response {
    let t = RichText::new(name).size(size).color(if on { theme::text_strong() } else { theme::muted() });
    let b = egui::Button::new(t)
        .fill(if on { theme::surface_hi() } else { Color32::TRANSPARENT })
        .stroke(Stroke::new(1.0, if on { theme::pane_brd() } else { Color32::TRANSPARENT }))
        .corner_radius(4);
    ui.add(b).on_hover_text(tip)
}

fn pick_chip(chips: u8, bit: u8) -> u8 {
    let all = Kind::ALL.iter().fold(0, |a, k| a | k.bit());
    let next = match chips {
        0 => bit,
        c if c == bit => 0,
        c => c ^ bit,
    };
    if next == all { 0 } else { next }
}

fn chip(ui: &mut egui::Ui, text: &str, color: Color32, on: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), FontId::proportional(11.0), if on { theme::bg() } else { color });
    let (r, resp) = ui.allocate_exact_size(galley.size() + egui::vec2(14.0, 4.0), Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(r, 8.0, color);
    } else {
        p.rect_filled(r, 8.0, if resp.hovered() { color.gamma_multiply(0.25) } else { color.gamma_multiply(0.10) });
        p.rect_stroke(r, 8.0, Stroke::new(1.0, color.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    }
    p.galley(r.min + egui::vec2(7.0, 2.0), galley, color);
    resp
}

fn crumb(ui: &mut egui::Ui, text: &str, tip: &str) -> egui::Response {
    ui.label(RichText::new("›").size(11.0).color(theme::hush()));
    ui.add(egui::Button::new(RichText::new(text).size(11.5).color(theme::muted())).frame(false)).on_hover_text(tip)
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).font(theme::display(12.5)).color(theme::text_strong()));
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 3.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, theme::pane_brd()));
}

fn stat(ui: &mut egui::Ui, value: &str, what: &str) {
    stat_c(ui, value, what, theme::text_strong());
}

fn stat_c(ui: &mut egui::Ui, value: &str, what: &str, c: Color32) {
    ui.vertical(|ui| {
        ui.label(RichText::new(value).font(theme::display(14.0)).color(c));
        ui.label(RichText::new(what).size(10.0).color(theme::hush()));
    });
    ui.add_space(8.0);
}

fn link_chip(ui: &mut egui::Ui, cat: &Catalog, i: usize) -> egui::Response {
    let k = cat.kind[i];
    chip(ui, &cat.items[i].n, k.color(), false).on_hover_text(format!("{} · {}", k.long(), if cat.items[i].p.is_empty() { &cat.items[i].g } else { &cat.items[i].p }))
}

/// One row of Find: paradigm dot, the name with what matched lit, version,
/// group - and on the right where it is built and how long it takes.
fn item_row(ui: &egui::Ui, r: Rect, cat: &Catalog, i: usize, selected: bool, hovered: bool, terms: &[String]) {
    let p = ui.painter_at(r);
    let it = &cat.items[i];
    let k = cat.kind[i];
    if selected {
        p.rect_filled(r, 0.0, theme::surface_hi());
        p.vline(r.left() + 1.0, r.y_range(), Stroke::new(2.0, theme::amber()));
    } else if hovered {
        p.rect_filled(r, 0.0, theme::surface_hi().gamma_multiply(0.6));
    }
    p.circle_filled(egui::pos2(r.left() + 11.0, r.center().y), 3.5, k.color());
    // Right side first: it decides how much the name may have.
    let right_w = if r.width() > 520.0 { 210.0 } else { 96.0 };
    let rx = r.right() - right_w;
    if let Some(st) = &it.st {
        if let Some(m) = st.med {
            let f = ((m as f32 + 1.0).ln() / (cat.max_med as f32 + 1.0).ln()).clamp(0.03, 1.0);
            let bar = Rect::from_min_size(egui::pos2(rx + right_w - 92.0, r.center().y - 3.0), egui::vec2(36.0 * f, 6.0));
            p.rect_filled(bar, 2.0, k.color().gamma_multiply(0.7));
            p.text(egui::pos2(r.right() - 8.0, r.center().y), Align2::RIGHT_CENTER, fmt_dur(m), FontId::monospace(10.5), theme::muted());
        }
        if st.fail > 0 {
            p.text(egui::pos2(rx + right_w - 98.0, r.center().y), Align2::RIGHT_CENTER, "✗", FontId::proportional(11.0), theme::alt());
        }
    }
    if right_w > 100.0 {
        let mut x = rx;
        for a in it.a.iter().take(3) {
            let g = p.layout_no_wrap(short_arch(a).to_owned(), FontId::proportional(9.5), theme::net());
            let b = Rect::from_min_size(egui::pos2(x, r.center().y - 7.0), g.size() + egui::vec2(8.0, 3.0));
            p.rect_stroke(b, 6.0, Stroke::new(1.0, theme::net().gamma_multiply(0.45)), egui::StrokeKind::Inside);
            p.galley(b.min + egui::vec2(4.0, 1.5), g, theme::net());
            x = b.right() + 3.0;
        }
    }
    // The name, lit where the search matched it.
    let avail = (rx - r.left() - 30.0).max(40.0);
    let name_chars = (avail * 0.55 / 6.6) as usize;
    let name = elide(&it.n, name_chars.max(8));
    let hits = terms.iter().find_map(|t| fit(&name.to_lowercase(), t).map(|f| f.1)).unwrap_or_default();
    let mut job = LayoutJob::default();
    for (ci, c) in name.chars().enumerate() {
        let lit = hits.contains(&ci);
        job.append(
            &c.to_string(),
            0.0,
            TextFormat {
                font_id: FontId::proportional(12.5),
                color: if lit { theme::amber() } else if selected { theme::text_strong() } else { theme::text() },
                underline: if lit { Stroke::new(1.0, theme::amber()) } else { Stroke::NONE },
                ..Default::default()
            },
        );
    }
    let g = ui.painter().layout_job(job);
    let nw = g.size().x;
    p.galley(egui::pos2(r.left() + 22.0, r.center().y - g.size().y / 2.0), g, theme::text());
    // Then the version and the group, in what is left.
    let mut x = r.left() + 22.0 + nw + 8.0;
    let ver = it.st.as_ref().map(|s| s.lv.clone()).filter(|v| !v.is_empty()).or_else(|| it.v.last().cloned()).unwrap_or_default();
    let left = |x: f32| ((rx - x - 6.0) / 6.0).max(0.0) as usize;
    if !ver.is_empty() && left(x) > 4 {
        let r2 = p.text(egui::pos2(x, r.center().y), Align2::LEFT_CENTER, elide(&ver, left(x).min(22)), FontId::monospace(10.5), theme::muted());
        x = r2.right() + 8.0;
    }
    let what = if it.p.is_empty() || matches!(k, Kind::Prefix | Kind::EasyBuild | Kind::Env) { it.g.clone() } else { it.p.clone() };
    if left(x) > 4 {
        p.text(egui::pos2(x, r.center().y), Align2::LEFT_CENTER, elide(&what, left(x)), FontId::proportional(11.0), theme::hush());
    }
}

fn leaf(ui: &mut egui::Ui, cat: &Catalog, i: usize, selected: bool) -> egui::Response {
    let it = &cat.items[i];
    ui.horizontal(|ui| {
        let (d, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), Sense::hover());
        ui.painter().circle_filled(d.center(), 3.0, cat.kind[i].color());
        let r = ui.selectable_label(selected, RichText::new(&it.n).size(12.0));
        if let Some(m) = cat.med(i) {
            ui.label(RichText::new(fmt_dur(m)).monospace().size(10.5).color(theme::hush()));
        }
        r
    })
    .inner
}

#[allow(clippy::too_many_arguments)]
fn tree_node(ui: &mut egui::Ui, cat: &Catalog, name: &str, node: &Node, id: egui::Id, depth: usize, open_all: bool, sel: Option<usize>, picked: &mut Option<usize>) {
    let head = RichText::new(format!("{name}   {} · {}", thousands(node.count), fmt_dur(node.secs))).size(12.0).color(if depth == 0 { theme::text_strong() } else { theme::text() });
    egui::CollapsingHeader::new(head).id_salt(id).default_open(open_all || (depth == 0 && node.count <= 12)).show(ui, |ui| {
        for (k, kid) in &node.kids {
            tree_node(ui, cat, k, kid, id.with(k), depth + 1, open_all, sel, picked);
        }
        let cap = 600;
        for &i in node.items.iter().take(cap) {
            if leaf(ui, cat, i as usize, sel == Some(i as usize)).clicked() {
                *picked = Some(i as usize);
            }
        }
        if node.items.len() > cap {
            ui.label(RichText::new(format!("… and {} more - narrow the search", node.items.len() - cap)).size(11.0).color(theme::hush()));
        }
    });
}

/// Every build of one software over time: x when it started, y how long it
/// took (log), a dot per build in its machine's colour, failures as ✗.
fn history(ui: &mut egui::Ui, cat: &Catalog, i: usize) {
    let ks = &cat.by_item[i];
    if ks.is_empty() {
        return;
    }
    let (lo, hi) = ks.iter().fold((i64::MAX, i64::MIN), |(a, b), &k| (a.min(cat.st_t[k as usize]), b.max(cat.st_t[k as usize])));
    let dmax = ks.iter().map(|&k| cat.st_d[k as usize]).max().unwrap_or(1).max(1) as f32;
    let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 96.0), Sense::hover());
    let p = ui.painter_at(r);
    p.rect_filled(r, 4.0, theme::surface().gamma_multiply(0.8));
    let plot = r.shrink2(egui::vec2(10.0, 10.0));
    let span = (hi - lo).max(1) as f32;
    let to = |t: i64, d: i64| {
        let x = if hi == lo { plot.center().x } else { plot.left() + (t - lo) as f32 / span * plot.width() };
        let y = plot.bottom() - ((d as f32 + 1.0).ln() / (dmax + 1.0).ln()) * plot.height();
        egui::pos2(x, y)
    };
    let mut near: Option<(f32, usize)> = None;
    for &k in ks {
        let k = k as usize;
        let at = to(cat.st_t[k], cat.st_d[k]);
        let b = cat.box_of_step(k);
        match cat.st_s[k] {
            b'f' => {
                p.text(at, Align2::CENTER_CENTER, "✗", FontId::proportional(11.0), theme::alt());
            }
            _ => {
                p.circle_filled(at, 3.0, box_color(b));
            }
        }
        if let Some(q) = resp.hover_pos() {
            let dist = at.distance(q);
            if dist < 14.0 && near.is_none_or(|n| dist < n.0) {
                near = Some((dist, k));
            }
        }
    }
    p.text(r.left_top() + egui::vec2(6.0, 4.0), Align2::LEFT_TOP, format!("longest {}", fmt_dur(dmax as i64)), FontId::proportional(9.5), theme::hush());
    p.text(r.right_bottom() - egui::vec2(6.0, 4.0), Align2::RIGHT_BOTTOM, format!("{} → {}", fmt_day(lo + cat.t0), fmt_day(hi + cat.t0)), FontId::proportional(9.5), theme::hush());
    if let Some((_, k)) = near {
        let b = cat.box_of_step(k);
        resp.on_hover_ui_at_pointer(|ui| {
            ui.label(format!("{} on {}", fmt_dur(cat.st_d[k]), cat.boxes.get(b).map_or("?", |x| x.name.as_str())));
            ui.label(RichText::new(format!("{} UTC · {}", fmt_date(cat.st_t[k] + cat.t0), cat.box_arch(b))).size(11.0).color(theme::muted()));
        });
    }
    ui.horizontal_wrapped(|ui| {
        for (b, bx) in cat.boxes.iter().enumerate() {
            if ks.iter().any(|&k| cat.box_of_step(k as usize) == b) {
                ui.label(RichText::new("●").color(box_color(b)).size(10.0));
                ui.label(RichText::new(short_box(&bx.name)).size(10.5).color(theme::muted()));
            }
        }
    });
}

/// The colour of "now" on the Timeline and the Curves.
fn now_color() -> Color32 {
    theme::primary()
}

fn box_color(b: usize) -> Color32 {
    let c: [Color32; 6] = [theme::primary(), theme::amber(), theme::net(), theme::alt(), theme::ok(), Color32::from_rgb(0xb9, 0x9c, 0xe0)];
    c[b % c.len()]
}

fn short_box(name: &str) -> &str {
    name.strip_prefix("four-box-").or_else(|| name.strip_prefix("n-box-")).unwrap_or(name)
}

fn short_arch(a: &str) -> &str {
    match a {
        "x86_64" => "x86",
        "aarch64" => "arm",
        other => other,
    }
}

// ── words and numbers ──────────────────────────────────────────────────────

fn elide(s: &str, n: usize) -> String {
    if s.chars().count() > n {
        format!("{}…", s.chars().take(n.saturating_sub(1)).collect::<String>())
    } else {
        s.to_owned()
    }
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// As ndexr.io's gantt says it: 7s, 3m 05s, 2h 10m.
pub fn fmt_dur(s: i64) -> String {
    let s = s.max(0);
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {:02}s", s / 60, s % 60)
    } else if s < 100 * 3600 {
        format!("{}h {:02}m", s / 3600, (s % 3600) / 60)
    } else {
        format!("{}d {:02}h", s / 86400, (s % 86400) / 3600)
    }
}

fn fmt_ago(now: i64, t: i64) -> String {
    let d = (now - t).max(0);
    if d < 90 {
        "just now".into()
    } else if d < 5400 {
        format!("{} min ago", d / 60)
    } else if d < 2 * 86400 {
        format!("{} h ago", d / 3600)
    } else {
        format!("{} d ago", d / 86400)
    }
}

/// Days since 1970 → (year, month, day): Howard Hinnant's civil_from_days.
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + if m <= 2 { 1 } else { 0 }, m, d)
}

/// (year, month, day) → days since 1970: Hinnant's days_from_civil.
fn days(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// When a run began, from its id (`20261014T154149Z-<host>`). The run's own
/// `start` is the box's first step, inherited, so it cannot order runs.
fn run_began(id: &str) -> Option<i64> {
    let s = id.get(..16)?;
    let n = |a: usize, b: usize| s.get(a..b)?.parse::<i64>().ok();
    if s.as_bytes()[8] != b'T' || s.as_bytes()[15] != b'Z' {
        return None;
    }
    Some(days(n(0, 4)?, n(4, 6)?, n(6, 8)?) * 86400 + n(9, 11)? * 3600 + n(11, 13)? * 60 + n(13, 15)?)
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

fn fmt_day(t: i64) -> String {
    let (y, m, d) = civil(t.div_euclid(86400));
    format!("{y}-{m:02}-{d:02}")
}

pub(crate) fn fmt_date(t: i64) -> String {
    let (_, m, d) = civil(t.div_euclid(86400));
    let s = t.rem_euclid(86400);
    format!("{} {} {:02}:{:02}", MONTHS[(m - 1) as usize], d, s / 3600, (s % 3600) / 60)
}

/// A tick spacing near `want` seconds that reads well on a clock.
fn tick_step(want: f64) -> f64 {
    const STEPS: [f64; 14] = [10.0, 30.0, 60.0, 300.0, 600.0, 1800.0, 3600.0, 3.0 * 3600.0, 6.0 * 3600.0, 12.0 * 3600.0, 86400.0, 2.0 * 86400.0, 7.0 * 86400.0, 30.0 * 86400.0];
    STEPS.iter().copied().find(|s| *s >= want).unwrap_or(30.0 * 86400.0)
}

pub(crate) fn now_secs() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        (js_sys::Date::now() / 1000.0) as i64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_month_of_runs_is_one_deployment_replayed_by_first_build() {
        // Two runs in October on two boxes, one in November.
        let j = r#"{"generated":0,"hash":"h","t0":1791000000,
            "boxes":[{"name":"four-box-intel","arch":"x86_64"},{"name":"four-box-arm","arch":"aarch64"}],
            "runs":[{"box":0,"start":1791000000,"end":1791000500},{"box":1,"start":1791000100,"end":1791000900},{"box":0,"start":1793600000,"end":1793600100}],
            "items":[{"k":"prefix","n":"sys-libs/glibc"},{"k":"easybuild","n":"GCCcore"}],
            "steps":{"i":[0,0,1,1],"r":[0,1,0,2],"t":[0,100,200,2600000],"d":[50,60,100,10],"s":"dddd"}}"#;
        let c = super::Catalog::parse(j.as_bytes()).unwrap();
        let d = c.deployments();
        assert_eq!(d.iter().map(|x| (x.label.as_str(), x.runs)).collect::<Vec<_>>(), [("October 2026", 2), ("November 2026", 1)]);
        assert_eq!(d[0].boxes, ["four-box-intel", "four-box-arm"]);
        let any = c.first_builds(d[0].lo, d[0].hi, None);
        assert_eq!(any, [Some((1791000000, 1791000050)), Some((1791000200, 1791000300))]);
        let arm = c.first_builds(d[0].lo, d[0].hi, Some("four-box-arm"));
        assert_eq!(arm, [Some((1791000100, 1791000160)), None]);
        assert_eq!(super::stamp(1791000000), "Oct 3 · 04:00 UTC");
    }

    use super::*;

    #[test]
    fn a_chip_isolates_adds_and_the_lone_one_restores_everything() {
        let (p, r) = (Kind::Prefix.bit(), Kind::R.bit());
        assert_eq!(pick_chip(0, r), r);
        assert_eq!(pick_chip(r, r), 0);
        assert_eq!(pick_chip(r, p), r | p);
        assert_eq!(pick_chip(r | p, p), r);
        let six = r | p | Kind::Python.bit() | Kind::Rust.bit() | Kind::Julia.bit() | Kind::Env.bit();
        assert_eq!(pick_chip(six, Kind::EasyBuild.bit()), 0);
        assert_eq!(parse_filter("DuckDB k:jl", 0).kinds, Kind::Julia.bit());
        assert_eq!(parse_filter("serde k:rs", 0).kinds, Kind::Rust.bit());
        assert_eq!((Kind::parse("Rust"), Kind::parse("conda")), (Kind::Rust, Kind::Other));
    }

    #[test]
    fn environments_are_their_own_layer_by_family() {
        assert_eq!(parse_filter("k:env", 0).kinds, Kind::Env.bit());
        assert_eq!(env_family("env/R/4.6.1-bioc3.23"), "R environments");
        assert_eq!(env_family("env/Python/3.13.15"), "Python environments");
        assert_eq!(env_family("env/Rust/1.96.0"), "Rust environments");
        assert_eq!(env_family("env/r4.6_py3.13-release"), "composed: R + Python");
        assert_eq!(env_family("env/genomics"), "purpose sets");
    }

    #[test]
    fn the_search_language() {
        let f = parse_filter("blast k:eb a:arm set:r4.6 is:failed >10m", 0);
        assert_eq!(f.terms, vec!["blast"]);
        assert_eq!(f.kinds, Kind::EasyBuild.bit());
        assert_eq!(f.arch, vec!["arm"]);
        assert_eq!(f.set, vec!["r4.6"]);
        assert!(f.failed);
        assert_eq!(f.longer, Some(600));
        assert_eq!(parse_secs("2h"), Some(7200));
        assert_eq!(parse_secs("45"), Some(45));
    }

    #[test]
    fn fits_rank_exact_then_prefix_then_inside_then_fuzzy() {
        let exact = fit("jsonlite", "jsonlite").unwrap().0;
        let prefix = fit("jsonlite", "json").unwrap().0;
        let inside = fit("rjsonio", "json").unwrap().0;
        let fuzzy = fit("dev-libs/openssl", "dlo").unwrap();
        assert!(exact > prefix && prefix > inside && inside > fuzzy.0);
        assert_eq!(fuzzy.1, vec![0, 4, 9]);
        assert!(fit("numpy", "pandas").is_none());
    }

    #[test]
    fn env_set_groups_read_as_words() {
        let all: Vec<String> = ["r4.5_py3.12-devel", "r4.5_py3.12-release", "r4.6_py3.13-devel", "r4.6_py3.13-release"].map(String::from).to_vec();
        assert_eq!(group_label(&all, &all), "all 4 sets");
        let mut with_app = all.clone();
        with_app.insert(0, "ndexr-app-r4.6.1".into());
        assert_eq!(group_label(&with_app, &all), "all 4 sets + ndexr app");
        assert_eq!(group_label(&["ndexr-app-r4.6.1".into(), "r4.6_py3.13-release".into()], &all), "r4.6 rel + ndexr app");
        assert_eq!(short_set("r4.5_py3.12-devel"), "r4.5 dev");
    }

    #[test]
    fn dates_and_durations() {
        assert_eq!(fmt_day(0), "1970-01-01");
        assert_eq!(fmt_day(1_792_000_000), "2026-10-14");
        assert_eq!(fmt_dur(7), "7s");
        assert_eq!(fmt_dur(185), "3m 05s");
        assert_eq!(fmt_dur(7800), "2h 10m");
        assert_eq!(thousands(54297), "54,297");
    }

    #[test]
    fn a_replaced_run_stops_building() {
        assert_eq!(run_began("20261014T154149Z-builder-1"), Some(1_791_992_509));
        assert_eq!(run_began("r1"), None);
        // Box 0 ran twice; the first run's step was still "r" when the second began.
        let j = br#"{"schema":1,"generated":1791999000,"hash":"h","t0":1791990000,
            "boxes":[{"name":"a","arch":"x86_64","lane":"x"},{"name":"b","arch":"aarch64","lane":"y"}],
            "runs":[{"id":"20261014T150000Z-a","box":0,"itype":"m","start":0,"end":1791999000},
                    {"id":"20261014T160000Z-a","box":0,"itype":"m","start":0,"end":1791999000},
                    {"id":"20261014T150000Z-b","box":1,"itype":"m","start":0,"end":1791999000}],
            "items":[{"k":"R","n":"V8"}],
            "steps":{"i":[0,0,0],"r":[0,1,2],"t":[1000,4000,1000],"d":[8000,5000,8000],"s":"rrr"}}"#;
        let c = Catalog::parse(j).unwrap();
        assert_eq!(c.st_s, b"srr");
        assert_eq!(c.st_d, vec![(1_791_993_600 - 1_791_990_000) - 1000, 5000, 8000]);
    }

    #[test]
    fn a_step_tried_again_or_passed_by_has_stopped() {
        // One run on one box: "a" started at 0 and never finished, then "a"
        // again at 100 (the next env set); "b" at 10 never finished, then
        // three others started after it and finished; "c" at 500 is live.
        let j = br#"{"schema":1,"generated":1791999000,"hash":"h","t0":1791990000,
            "boxes":[{"name":"a","arch":"x86_64","lane":"x"}],
            "runs":[{"id":"20261014T150000Z-a","box":0,"itype":"m","start":0,"end":1791999000}],
            "items":[{"k":"R","n":"a"},{"k":"R","n":"b"},{"k":"R","n":"c"},{"k":"R","n":"d"}],
            "steps":{"i":[0,0,1,3,3,3,2],"r":[0,0,0,0,0,0,0],"t":[0,100,10,20,30,40,500],"d":[9000,5,9000,5,5,5,8500],"s":"rdrdddr"}}"#;
        let c = Catalog::parse(j).unwrap();
        assert_eq!(c.st_s, b"sdsdddr");
        // "a" ends at 20 - the machine moved on then - not at its retry at 100.
        assert_eq!((c.st_d[0], c.st_d[2]), (20, 10), "each ends at the earliest sign the machine moved on");
    }

    /// The moment's inventory: per lane, each item once, at its first finished
    /// build there; a failed or later build changes nothing (#780).
    #[test]
    fn installs_are_first_finishes_per_lane() {
        let j = br#"{"schema":1,"generated":1791999000,"hash":"h","t0":1791990000,
            "boxes":[{"name":"a","arch":"x86_64","lane":"x"},{"name":"g","arch":"x86_64","lane":"x86_64 + CUDA"},{"name":"b","arch":"aarch64","lane":"y"}],
            "runs":[{"id":"20261014T150000Z-a","box":0,"itype":"m","start":0,"end":1791999000},
                    {"id":"20261014T150000Z-g","box":1,"itype":"m","start":0,"end":1791999000},
                    {"id":"20261014T150000Z-b","box":2,"itype":"m","start":0,"end":1791999000}],
            "items":[{"k":"R","n":"x"},{"k":"Python","n":"y"}],
            "steps":{"i":[0,0,1,0,1],"r":[0,0,0,1,2],"t":[0,200,10,0,0],"d":[100,50,5,30,7],"s":"dddfd"}}"#;
        let c = Catalog::parse(j).unwrap();
        let inv = c.installs_by_lane();
        let lane = |l: &str| inv.iter().find(|(n, _)| n == l).map(|(_, v)| v.clone());
        assert_eq!(lane("x86_64"), Some(vec![(15, 1), (100, 0)]));
        assert_eq!(lane("aarch64"), Some(vec![(7, 1)]));
        assert_eq!(lane("cuda"), None, "a failed build installs nothing");
    }

    /// Against the live catalog: the counts ndexr.io measured for #780.
    #[test]
    #[ignore]
    fn installs_match_the_measured_catalog() {
        let c = Catalog::parse(&std::fs::read("/srv/ndexr/software/catalog.json").unwrap()).unwrap();
        for (lane, v) in c.installs_by_lane() {
            for at in [1_791_920_220i64 /* 2026-10-13 19:37 */, 1_793_856_420 /* 11-05 05:27 */, 1_794_098_400 /* 11-08 00:40 */] {
                let n = v.partition_point(|(e, _)| e + c.t0 <= at);
                let mut by = [0usize; 8];
                for (_, i) in &v[..n] {
                    by[c.kind[*i as usize] as usize] += 1;
                }
                println!("{lane} {} {n} {by:?}", fmt_date(at));
            }
        }
    }

    #[test]
    fn a_catalog_indexes() {
        let j = br#"{"schema":1,"generated":10,"hash":"h","t0":100,
            "boxes":[{"name":"four-box-arm","arch":"aarch64","lane":"aarch64-unknown-linux-gnu"}],
            "runs":[{"id":"r1","box":0,"itype":"m7g","start":100,"end":200,"n":2}],
            "items":[{"k":"easybuild","n":"zlib","g":"GCCcore-13.3.0","v":["1.3.1-GCCcore-13.3.0"],"a":["aarch64"],"st":{"n":1,"ok":1,"fail":0,"med":30,"tot":30,"arch":{"aarch64":30},"lv":"1.3.1-GCCcore-13.3.0"}},
                     {"k":"R","n":"jsonlite","g":"R packages","s":["r4.6_py3.13-release"],"dep":[0],"p":"A JSON parser"}],
            "steps":{"i":[0,1],"r":[0,0],"t":[0,40],"d":[30,5],"s":"df"}}"#;
        let c = Catalog::parse(j).unwrap();
        assert_eq!(c.used_by[0], vec![1]);
        assert_eq!(c.by_item[1], vec![1]);
        assert_eq!(c.counts, [0, 1, 1, 0, 0, 0, 0, 0]);
        assert!(score(&c, 1, &parse_filter("json set:r4.6", 0)).is_some());
        assert!(score(&c, 1, &parse_filter("parser", 0)).is_some());
        assert!(score(&c, 0, &parse_filter("k:r", 0)).is_none());
        assert_eq!(SoftwarePane::paths(&c, 1, Order::Stack), vec![vec!["3 · site libraries".to_string(), "r4.6_py3.13-release".into(), "R packages".into()]]);
    }
}
