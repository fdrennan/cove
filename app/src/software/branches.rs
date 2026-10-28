//! The Builds tab's dimensions, its focus and its depth (operator,
//! 2026-10-15: "color by paradigm and ideally have a 3d perspective - paradigm
//! and manifest and language are colors or groups ... so we can see other than
//! just arch stats ... creating branches in the builds section which look tree
//! like and where we can look at different parts of that tree").
//!
//!   Dims      every build step coded once per catalog on five dimensions -
//!             machine, arch, paradigm, manifest, language - so a split, a
//!             colour or a filter is a lookup, never a string compare.
//!   Branch    a path down those dimensions (arm › R › r4.6 rel): the focus.
//!             Curves and 3D show only the builds under it. (The Branches tree
//!             view was removed, 2026-10-18: "Branches tab under builds can be
//!             completely removed".)
//!   3D        the cumulative curves in perspective: one lane per value of one
//!             dimension, each lane stacked by the colours of another. Drag
//!             turns it, hover reads it, a click focuses a lane.
//!
//! The steps are the catalog's (ndexr.ndexr.io aggregate/software.rs). The
//! manifest is the box's own record (unibuild_steps.env_set) where it sent one,
//! else the single manifest that lists the package; language is the spec's
//! `ln`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, RichText, Sense, Shape, Stroke};

use super::{box_color, fmt_date, short_arch, short_box, short_set, thousands, Catalog, Kind, SoftwarePane};
use crate::theme;

// ── the dimensions ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Dim {
    Machine,
    Arch,
    Paradigm,
    Manifest,
    Language,
}

impl Dim {
    pub const ALL: [(Dim, &'static str, &'static str); 5] = [
        (Dim::Machine, "machine", "each build machine"),
        (Dim::Arch, "arch", "x86_64, aarch64, and the GPU box's CUDA lane"),
        (Dim::Paradigm, "paradigm", "Gentoo Prefix, EasyBuild, R, Python"),
        (Dim::Manifest, "manifest", "the env manifest an R or Python build was for; the stack's own builds are 'the stack'"),
        (Dim::Language, "language", "what building it compiles: C/C++, Rust, Fortran… or R / Python alone"),
    ];

    pub fn label(self) -> &'static str {
        Dim::ALL.iter().find(|d| d.0 == self).map_or("", |d| d.1)
    }

    fn ix(self) -> usize {
        self as usize
    }
}

/// The values that mean "not known" sort last and draw grey.
const QUIET: [&str; 4] = ["the stack", "unrecorded", "unknown", "meta"];

/// A language's colour: fixed, so Rust is Rust on every chart.
fn lang_color(l: &str) -> Color32 {
    match l {
        "C/C++" => Color32::from_rgb(0x6f, 0x9f, 0xd8),
        "Rust" => Kind::Rust.color(),
        "R" => Kind::R.color(),
        "Python" => Kind::Python.color(),
        "Fortran" => Color32::from_rgb(0xd9, 0xcf, 0x6b),
        "Go" => Color32::from_rgb(0x6f, 0xd6, 0xd6),
        "Java" => Color32::from_rgb(0xe7, 0x83, 0xb5),
        "Perl" => Color32::from_rgb(0xa8, 0xc2, 0x7a),
        "binary" => Color32::from_rgb(0x9a, 0x9a, 0xa8),
        _ => theme::hush(),
    }
}

/// One dimension, coded: its values and each step's value.
struct Coded {
    names: Vec<String>,
    colors: Vec<Color32>,
    code: Vec<u16>,
}

/// Every step on every dimension, worked out once per catalog.
pub struct Dims {
    hash: String,
    by: [Coded; 5],
    /// The steps that count: a run lists the steps it inherited from the runs
    /// before it, so each (machine, software, start) once - as the curves do.
    pub unique: Vec<u32>,
}

impl Dims {
    pub fn new(cat: &Catalog) -> Dims {
        let n = cat.st_i.len().min(cat.st_t.len()).min(cat.st_d.len()).min(cat.st_s.len());
        let mut seen: HashSet<(usize, u32, i64)> = HashSet::new();
        let mut unique = Vec::with_capacity(n);
        for k in 0..n {
            if seen.insert((cat.box_of_step(k), cat.st_i[k], cat.st_t[k])) {
                unique.push(k as u32);
            }
        }
        // Machine and arch keep the catalog's order, so a machine has the
        // colour the Timeline gives it; the others go by how many builds.
        let machine = Coded {
            names: cat.boxes.iter().map(|b| short_box(&b.name).to_owned()).collect(),
            colors: (0..cat.boxes.len()).map(box_color).collect(),
            code: (0..n).map(|k| cat.box_of_step(k) as u16).collect(),
        };
        let value = |d: Dim, k: usize| -> String {
            let i = cat.st_i[k] as usize;
            match d {
                Dim::Arch => short_arch(&cat.box_arch(cat.box_of_step(k))).to_owned(),
                Dim::Paradigm => cat.kind[i].label().to_owned(),
                Dim::Manifest => match (cat.step_set(k), cat.kind[i]) {
                    (Some(s), _) => short_set(s),
                    (None, k) if k.is_lang() => "unrecorded".to_owned(),
                    (None, _) => "the stack".to_owned(),
                },
                Dim::Language => cat.lang(i).to_owned(),
                Dim::Machine => unreachable!(),
            }
        };
        let code = |d: Dim| -> Coded {
            let vals: Vec<String> = (0..n).map(|k| value(d, k)).collect();
            let mut count: HashMap<&str, usize> = HashMap::new();
            for &k in &unique {
                *count.entry(vals[k as usize].as_str()).or_default() += 1;
            }
            for v in &vals {
                count.entry(v.as_str()).or_default();
            }
            let mut names: Vec<&str> = count.keys().copied().collect();
            match d {
                Dim::Paradigm => names.sort_by_key(|n| Kind::ALL.iter().position(|k| k.label() == *n)),
                Dim::Arch => names.sort(),
                _ => names.sort_by(|a, b| QUIET.contains(a).cmp(&QUIET.contains(b)).then(count[b].cmp(&count[a])).then(a.cmp(b))),
            }
            let ix: HashMap<&str, u16> = names.iter().enumerate().map(|(p, n)| (*n, p as u16)).collect();
            let colors = names
                .iter()
                .enumerate()
                .map(|(p, name)| match d {
                    _ if QUIET.contains(name) => theme::hush(),
                    Dim::Paradigm => Kind::ALL.iter().find(|k| k.label() == *name).map_or(theme::hush(), |k| k.color()),
                    Dim::Language => lang_color(name),
                    Dim::Arch => match *name {
                        "x86" => theme::primary(),
                        "arm" => theme::amber(),
                        "cuda" => theme::ok(),
                        _ => box_color(p + 3),
                    },
                    _ => Catalog::group_color(p),
                })
                .collect();
            let code = vals.iter().map(|v| ix[v.as_str()]).collect();
            Coded { names: names.into_iter().map(str::to_owned).collect(), colors, code }
        };
        let by = [machine, code(Dim::Arch), code(Dim::Paradigm), code(Dim::Manifest), code(Dim::Language)];
        Dims { hash: cat.hash.clone(), by, unique }
    }

    pub fn code(&self, d: Dim, k: usize) -> u16 {
        self.by[d.ix()].code[k]
    }

    pub fn name(&self, d: Dim, c: u16) -> &str {
        self.by[d.ix()].names.get(c as usize).map_or("?", String::as_str)
    }

    pub fn color(&self, d: Dim, c: u16) -> Color32 {
        self.by[d.ix()].colors.get(c as usize).copied().unwrap_or(theme::hush())
    }

    pub fn values(&self, d: Dim) -> usize {
        self.by[d.ix()].names.len()
    }

    /// Step `k` lies under the branch.
    pub fn under(&self, branch: &[(Dim, u16)], k: usize) -> bool {
        branch.iter().all(|&(d, c)| self.code(d, k) == c)
    }
}

// ── the state the views share ──────────────────────────────────────────────

pub struct State {
    pub dims: Option<Arc<Dims>>,
    /// What the curves split into lines by.
    pub split: Dim,
    /// The focus: a path down the dimensions.
    pub branch: Vec<(Dim, u16)>,
    lanes: Dim,
    color: Dim,
    hours: bool,
    yaw: f32,
    pitch: f32,
}

impl Default for State {
    fn default() -> Self {
        State {
            dims: None,
            split: Dim::Machine,
            branch: Vec::new(),
            lanes: Dim::Machine,
            color: Dim::Paradigm,
            hours: false,
            yaw: YAW,
            pitch: PITCH,
        }
    }
}

/// A quarter-turn from the first angle (operator, 2026-10-16: "3d view needs
/// to rotate 90 degrees ... with the most done towards the back"): time runs
/// away from the viewer, so each lane's cumulative curve rises toward the back
/// and the tall, late part never hides the early part in front of it.
const YAW: f32 = -0.55 + std::f32::consts::FRAC_PI_2;
const PITCH: f32 = 0.42;
const BINS: usize = 96;

impl State {
    pub fn dims_for(&mut self, cat: &Catalog) -> Arc<Dims> {
        match &self.dims {
            Some(d) if d.hash == cat.hash => d.clone(),
            _ => {
                let d = Arc::new(Dims::new(cat));
                // A new catalog may code the values differently.
                if self.dims.is_some() {
                    self.branch.clear();
                }
                self.dims = Some(d.clone());
                d
            }
        }
    }

    /// The key the curves are cached under: split and focus.
    pub fn sig(&self) -> String {
        format!("{:?}{:?}", self.split, self.branch)
    }
}

/// A dimension picker: the five as a small segmented switch.
fn dim_pick(ui: &mut egui::Ui, now: &mut Dim, skip: &[Dim]) -> bool {
    let mut changed = false;
    for (d, name, tip) in Dim::ALL {
        if skip.contains(&d) {
            continue;
        }
        let on = *now == d;
        let t = RichText::new(name).size(11.5).color(if on { theme::text_strong() } else { theme::muted() });
        if ui.add(egui::Button::new(t).fill(if on { theme::surface_hi() } else { Color32::TRANSPARENT }).corner_radius(4)).on_hover_text(tip).clicked() && !on {
            *now = d;
            changed = true;
        }
    }
    changed
}

/// The focus as breadcrumbs: every crumb steps back up to it, ✕ clears.
pub fn branch_strip(ui: &mut egui::Ui, dims: &Dims, branch: &mut Vec<(Dim, u16)>) {
    theme::strip(ui, 22.0, theme::surface(), |ui| {
        ui.add_space(6.0);
        ui.spacing_mut().item_spacing.x = 4.0;
        let mut cut: Option<usize> = None;
        let all = ui.add(egui::Button::new(RichText::new("all builds").size(11.5).color(if branch.is_empty() { theme::text_strong() } else { theme::muted() })).frame(false));
        if all.on_hover_text("every build: the focus is the whole fleet").clicked() {
            cut = Some(0);
        }
        for (n, &(d, c)) in branch.iter().enumerate() {
            ui.label(RichText::new("›").size(11.0).color(theme::hush()));
            let r = super::chip(ui, &format!("{} {}", d.label(), dims.name(d, c)), dims.color(d, c), n + 1 == branch.len());
            if r.on_hover_text("back up to here").clicked() {
                cut = Some(n + 1);
            }
        }
        if !branch.is_empty() && theme::tool(ui, "✕", "the whole fleet again").clicked() {
            cut = Some(0);
        }
        if let Some(n) = cut {
            branch.truncate(n);
        }
        if branch.is_empty() {
            ui.label(RichText::new("· focus a branch from 3D").size(10.5).color(theme::hush()));
        }
    });
}

impl SoftwarePane {
    // ── 3D ─────────────────────────────────────────────────────────────────

    pub(super) fn depth_view(&mut self, ui: &mut egui::Ui, cat: &Catalog) {
        let dims = self.br.dims_for(cat);
        branch_strip(ui, &dims, &mut self.br.branch);
        theme::strip(ui, 22.0, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = 3.0;
            ui.label(RichText::new("lanes").size(11.0).color(theme::hush()));
            let mut lanes = self.br.lanes;
            dim_pick(ui, &mut lanes, &[]);
            theme::tool_sep(ui);
            ui.label(RichText::new("colour").size(11.0).color(theme::hush()));
            let mut color = self.br.color;
            dim_pick(ui, &mut color, &[lanes]);
            if lanes == color {
                color = Dim::ALL.iter().map(|d| d.0).find(|d| *d != lanes).unwrap_or(Dim::Paradigm);
            }
            (self.br.lanes, self.br.color) = (lanes, color);
            theme::tool_sep(ui);
            for (on, name, tip) in [(false, "built", "packages built so far"), (true, "build time", "hours of building so far")] {
                let t = RichText::new(name).size(11.5).color(if self.br.hours == on { theme::text_strong() } else { theme::muted() });
                if ui.add(egui::Button::new(t).fill(if self.br.hours == on { theme::surface_hi() } else { Color32::TRANSPARENT }).corner_radius(4)).on_hover_text(tip).clicked() {
                    self.br.hours = on;
                }
            }
            theme::tool_sep(ui);
            for (name, yaw, pitch) in [("Angled", YAW, PITCH), ("Front", std::f32::consts::FRAC_PI_2, 0.08), ("Above", -0.2 + std::f32::consts::FRAC_PI_2, 1.25)] {
                if theme::tool(ui, name, "turn the view to this; drag on the chart to turn it yourself").clicked() {
                    (self.br.yaw, self.br.pitch) = (yaw, pitch);
                }
            }
        });

        // The lanes' stacked curves, binned over the focus's span.
        let (lanes_d, color_d, hours) = (self.br.lanes, self.br.color, self.br.hours);
        let (nl, nc) = (dims.values(lanes_d), dims.values(color_d));
        let (mut lo, mut hi) = (i64::MAX, i64::MIN);
        let picked: Vec<usize> = dims.unique.iter().map(|&k| k as usize).filter(|&k| cat.st_s[k] == b'd' && dims.under(&self.br.branch, k)).collect();
        for &k in &picked {
            lo = lo.min(cat.st_t[k] + cat.st_d[k]);
            hi = hi.max(cat.st_t[k] + cat.st_d[k]);
        }
        if picked.is_empty() {
            ui.add_space(14.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Nothing finished under this focus.").color(theme::hush())));
            return;
        }
        let hi = hi.max(lo + 60);
        let bin = |t: i64| ((((t - lo) as f64) / (hi - lo) as f64) * (BINS - 1) as f64).round() as usize;
        // grid[lane][colour][bin], cumulative.
        let mut grid = vec![vec![vec![0f64; BINS]; nc]; nl];
        for &k in &picked {
            let v = if hours { cat.st_d[k] as f64 / 3600.0 } else { 1.0 };
            grid[dims.code(lanes_d, k) as usize][dims.code(color_d, k) as usize][bin(cat.st_t[k] + cat.st_d[k]).min(BINS - 1)] += v;
        }
        for lane in &mut grid {
            for c in lane.iter_mut() {
                for b in 1..BINS {
                    c[b] += c[b - 1];
                }
            }
        }
        let mut lanes: Vec<usize> = (0..nl).filter(|&l| grid[l].iter().any(|c| c[BINS - 1] > 0.0)).collect();
        // Front to back: the machines in the operator's order - "start with
        // gpu, then the old amd and then arm and intel ... with the most done
        // towards the back" (2026-10-16), which is least built to most; any
        // other split, least built first. Tall lanes stand behind short ones.
        let total = |l: usize| grid[l].iter().map(|c| c[BINS - 1]).sum::<f64>();
        let rank = |l: usize| -> usize {
            if lanes_d != Dim::Machine {
                return 0;
            }
            let n = dims.name(lanes_d, l as u16).to_ascii_lowercase();
            ["gpu", "amd", "arm", "intel"].iter().position(|m| n.contains(m)).unwrap_or(4)
        };
        lanes.sort_by(|&a, &b| rank(a).cmp(&rank(b)).then(total(a).total_cmp(&total(b))));
        let colors: Vec<usize> = (0..nc).filter(|&c| lanes.iter().any(|&l| grid[l][c][BINS - 1] > 0.0)).collect();
        let ymax = lanes.iter().map(|&l| colors.iter().map(|&c| grid[l][c][BINS - 1]).sum::<f64>()).fold(0.0, f64::max).max(1e-9);
        let fmt = |v: f64| if hours { format!("{v:.0} h") } else { thousands(v.round() as usize) };

        // The legend.
        theme::strip(ui, 20.0, theme::bg(), |ui| {
            ui.add_space(8.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            for &c in &colors {
                ui.label(RichText::new("■").size(11.0).color(dims.color(color_d, c as u16)));
                ui.label(RichText::new(dims.name(color_d, c as u16)).size(10.5).color(theme::muted()));
            }
            ui.label(RichText::new(format!("· drag to turn · click a lane to focus its {}", lanes_d.label())).size(10.5).color(theme::hush()));
        });

        let avail = ui.available_rect_before_wrap();
        let (rect, resp) = ui.allocate_exact_size(avail.size().max(egui::vec2(200.0, 200.0)), Sense::click_and_drag());
        if resp.dragged() {
            let d = resp.drag_delta();
            self.br.yaw += d.x * 0.008;
            self.br.pitch = (self.br.pitch + d.y * 0.006).clamp(0.02, 1.4);
        }
        if resp.double_clicked() {
            (self.br.yaw, self.br.pitch) = (YAW, PITCH);
        }
        let (yaw, pitch) = (self.br.yaw, self.br.pitch);
        let (cy, sy, cp, sp) = (yaw.cos(), yaw.sin(), pitch.cos(), pitch.sin());
        let scale = rect.width().min(rect.height() * 1.6) * 0.62;
        let center = rect.center() + egui::vec2(0.0, rect.height() * 0.08);
        // World: x time 0..1, y value 0..1, z lane 0..1 (front to back).
        let proj = |x: f32, y: f32, z: f32| -> (Pos2, f32) {
            let (x, y, z) = (x - 0.5, y * 0.55 - 0.2, (z - 0.5) * 0.9);
            let (x1, z1) = (x * cy - z * sy, x * sy + z * cy);
            // Tilted to look DOWN on the floor: a lane further back sits
            // higher on screen. With the signs the other way round the
            // camera looked up from under it and the curves hung upside down
            // (operator, 2026-10-16: "the diagram for cove in 3d is upside down").
            let (y2, z2) = (y * cp + z1 * sp, z1 * cp - y * sp);
            let s = 2.4 / (2.4 + z2);
            (center + egui::vec2(x1 * s * scale, -y2 * s * scale), z2)
        };
        let lane_z = |n: usize| if lanes.len() > 1 { n as f32 / (lanes.len() - 1) as f32 } else { 0.5 };
        let p = ui.painter_at(rect);
        p.rect_filled(rect, 0.0, theme::bg());
        // The floor: the time axis at both ends, a line along each lane.
        let floor = Stroke::new(1.0, theme::pane_brd().gamma_multiply(0.7));
        for z in [0.0, 1.0] {
            p.line_segment([proj(0.0, 0.0, z).0, proj(1.0, 0.0, z).0], floor);
        }
        for x in [0.0, 1.0] {
            p.line_segment([proj(x, 0.0, 0.0).0, proj(x, 0.0, 1.0).0], floor);
        }
        let step = super::tick_step((hi - lo) as f64 / 6.0);
        let mut t = ((lo + cat.t0) as f64 / step).ceil() * step - cat.t0 as f64;
        while t <= hi as f64 {
            let x = ((t - lo as f64) / (hi - lo) as f64) as f32;
            let (a, b) = (proj(x, 0.0, 0.0).0, proj(x, 0.0, 1.0).0);
            p.line_segment([a, b], Stroke::new(1.0, theme::pane_brd().gamma_multiply(0.35)));
            let abs = t as i64 + cat.t0;
            p.text(a + egui::vec2(0.0, 4.0), Align2::CENTER_TOP, if step >= 86400.0 { super::fmt_day(abs) } else { fmt_date(abs) }, FontId::proportional(9.5), theme::hush());
            t += step;
        }
        // The value axis at the back left.
        for q in 1..=4 {
            let y = q as f32 / 4.0;
            let (a, b) = (proj(0.0, y, 1.0).0, proj(1.0, y, 1.0).0);
            p.line_segment([a, b], Stroke::new(1.0, theme::pane_brd().gamma_multiply(0.25)));
            p.text(a - egui::vec2(4.0, 0.0), Align2::RIGHT_CENTER, fmt(ymax * q as f64 / 4.0), FontId::proportional(9.5), theme::hush());
        }
        // Lanes back to front, each a stack of curtains.
        let mut order: Vec<(usize, f32)> = lanes.iter().enumerate().map(|(n, _)| (n, proj(0.5, 0.0, lane_z(n)).1)).collect();
        order.sort_by(|a, b| b.1.total_cmp(&a.1));
        let hover = resp.hover_pos();
        let mut near: Option<(f32, usize, usize)> = None;
        for &(n, _) in &order {
            let l = lanes[n];
            let z = lane_z(n);
            let mut base = vec![0f64; BINS];
            let mut mesh = egui::Mesh::default();
            let mut tops: Vec<(Vec<Pos2>, Color32)> = Vec::new();
            for &c in &colors {
                let col = dims.color(color_d, c as u16);
                let fill = col.gamma_multiply(0.78);
                let top: Vec<f64> = (0..BINS).map(|b| base[b] + grid[l][c][b]).collect();
                if top.iter().zip(&base).all(|(t, b)| t <= b) {
                    continue;
                }
                let lo_pts: Vec<Pos2> = (0..BINS).map(|b| proj(b as f32 / (BINS - 1) as f32, (base[b] / ymax) as f32, z).0).collect();
                let hi_pts: Vec<Pos2> = (0..BINS).map(|b| proj(b as f32 / (BINS - 1) as f32, (top[b] / ymax) as f32, z).0).collect();
                for b in 0..BINS - 1 {
                    let i = mesh.vertices.len() as u32;
                    for q in [lo_pts[b], lo_pts[b + 1], hi_pts[b + 1], hi_pts[b]] {
                        mesh.colored_vertex(q, fill);
                    }
                    mesh.add_triangle(i, i + 1, i + 2);
                    mesh.add_triangle(i, i + 2, i + 3);
                }
                tops.push((hi_pts, col));
                base = top;
            }
            p.add(Shape::mesh(mesh));
            for (pts, col) in tops {
                p.add(Shape::line(pts, Stroke::new(1.2, col)));
            }
            // The lane's name at its far end, and the nearest point to the pointer.
            let end = proj(1.0, (base[BINS - 1] / ymax) as f32, z).0;
            p.text(end + egui::vec2(6.0, 0.0), Align2::LEFT_CENTER, format!("{} {}", dims.name(lanes_d, l as u16), fmt(base[BINS - 1])), FontId::proportional(10.5), dims.color(lanes_d, l as u16));
            if let Some(h) = hover {
                for b in 0..BINS {
                    let q = proj(b as f32 / (BINS - 1) as f32, (base[b] / ymax) as f32, z).0;
                    let d = q.distance(h);
                    if d < 24.0 && near.is_none_or(|x| d < x.0) {
                        near = Some((d, l, b));
                    }
                }
            }
        }
        if let Some((_, l, b)) = near {
            let t = lo + ((hi - lo) as f64 * b as f64 / (BINS - 1) as f64) as i64;
            egui::Tooltip::always_open(ui.ctx().clone(), ui.layer_id(), egui::Id::new("d3-tip"), egui::PopupAnchor::Pointer).show(|ui| {
                ui.label(RichText::new(format!("{} {} · {} UTC", lanes_d.label(), dims.name(lanes_d, l as u16), fmt_date(t + cat.t0))).strong());
                for &c in colors.iter().rev() {
                    let v = grid[l][c][b];
                    if v > 0.0 {
                        ui.label(RichText::new(format!("■ {}  {}", dims.name(color_d, c as u16), fmt(v))).color(dims.color(color_d, c as u16)));
                    }
                }
                ui.label(RichText::new(format!("click to focus this {}", lanes_d.label())).size(10.5).color(theme::hush()));
            });
            if resp.clicked() && !self.br.branch.iter().any(|(d, _)| *d == lanes_d) {
                self.br.branch.push((lanes_d, l as u16));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Catalog {
        let j = br#"{"schema":1,"generated":0,"hash":"h","t0":1791990000,
            "boxes":[{"name":"four-box-arm","arch":"aarch64","lane":"x"},{"name":"four-box-intel","arch":"x86_64","lane":"y"}],
            "runs":[{"id":"20261014T150000Z-a","box":0,"itype":"m7g","start":0,"end":0},
                    {"id":"20261014T150000Z-b","box":1,"itype":"m7i","start":0,"end":0}],
            "sets":["r4.6_py3.13-release"],
            "items":[{"k":"R","n":"a","ln":"C/C++","s":["r4.6_py3.13-release","r4.5_py3.12-release"]},{"k":"easybuild","n":"GCC","ln":"C/C++"},{"k":"Python","n":"p"}],
            "steps":{"i":[0,0,1,2,0],"r":[0,0,1,1,1],"t":[10,10,20,30,40],"d":[5,5,50,5,5],"s":"dddfd","m":[0,0,-1,-1,-1]}}"#;
        Catalog::parse(j).unwrap()
    }

    #[test]
    fn steps_code_on_every_dimension() {
        let c = cat();
        let d = Dims::new(&c);
        assert_eq!(d.unique, vec![0, 2, 3, 4], "the inherited copy counts once");
        assert_eq!(d.name(Dim::Machine, d.code(Dim::Machine, 2)), "intel");
        assert_eq!(d.name(Dim::Arch, d.code(Dim::Arch, 0)), "arm");
        assert_eq!(d.name(Dim::Paradigm, d.code(Dim::Paradigm, 2)), "EasyBuild");
        assert_eq!(d.name(Dim::Manifest, d.code(Dim::Manifest, 0)), "r4.6 rel", "the box's record");
        assert_eq!(d.name(Dim::Manifest, d.code(Dim::Manifest, 4)), "unrecorded", "in two manifests, none recorded");
        assert_eq!(d.name(Dim::Manifest, d.code(Dim::Manifest, 2)), "the stack");
        assert_eq!(d.name(Dim::Language, d.code(Dim::Language, 3)), "Python", "an older catalog falls back to the paradigm");
        let r = d.code(Dim::Paradigm, 0);
        assert!(d.under(&[(Dim::Paradigm, r)], 4) && !d.under(&[(Dim::Paradigm, r)], 2));
    }

}
