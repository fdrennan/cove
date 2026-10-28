//! Chaos Lab: the Lorenz system (Lorenz 1963, "Deterministic Nonperiodic
//! Flow"), solved here in Rust every frame and drawn as it goes.
//!
//!   dx/dt = σ (y − x)      dy/dt = x (ρ − z) − y      dz/dt = x y − β z
//!
//! Two runs start a hair apart. For a while they look like one; then the gap
//! grows about e-fold every 1.1 time units (the largest Lyapunov exponent,
//! ≈ 0.906 at σ 10, ρ 28, β 8/3) until they are on different wings of the
//! butterfly. That is the butterfly effect, measured: the plot under the
//! attractor is that gap, on a log scale.
//!
//! The same experiment is the Shiny starter's (module.r), in base R.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

pub type V3 = [f64; 3];

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Params {
    pub sigma: f64,
    pub rho: f64,
    pub beta: f64,
}

impl Default for Params {
    fn default() -> Self {
        Params { sigma: 10.0, rho: 28.0, beta: 8.0 / 3.0 }
    }
}

pub fn lorenz(p: &Params, s: V3) -> V3 {
    [p.sigma * (s[1] - s[0]), s[0] * (p.rho - s[2]) - s[1], s[0] * s[1] - p.beta * s[2]]
}

/// One classical Runge-Kutta (RK4) step of size `h`.
pub fn rk4(p: &Params, s: V3, h: f64) -> V3 {
    let add = |a: V3, b: V3, k: f64| [a[0] + b[0] * k, a[1] + b[1] * k, a[2] + b[2] * k];
    let k1 = lorenz(p, s);
    let k2 = lorenz(p, add(s, k1, h / 2.0));
    let k3 = lorenz(p, add(s, k2, h / 2.0));
    let k4 = lorenz(p, add(s, k3, h));
    [0, 1, 2].map(|i| s[i] + h / 6.0 * (k1[i] + 2.0 * k2[i] + 2.0 * k3[i] + k4[i]))
}

fn dist(a: V3, b: V3) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

const H: f64 = 0.005;
const TRAIL: usize = 2500;
const A: Color32 = Color32::from_rgb(90, 200, 250);
const B: Color32 = Color32::from_rgb(255, 150, 80);
/// The night the visuals sit in, top and bottom of its gradient.
pub const DEEP: Color32 = Color32::from_rgb(5, 7, 13);
const DEEP_TOP: Color32 = Color32::from_rgb(14, 20, 38);

pub struct Chaos {
    pub p: Params,
    /// Steps per frame.
    speed: f64,
    /// How far apart the two runs start, as log10 of the x difference.
    gap_exp: f64,
    running: bool,
    t: f64,
    a: Vec<V3>,
    b: Vec<V3>,
    /// (time, log10 gap), for the plot underneath.
    gaps: Vec<(f64, f64)>,
    turn: f32,
    /// The whole attractor for the current parameters, drawn faintly behind
    /// the two runs so its shape is there from the first frame; and the
    /// parameters it was drawn for.
    ghost: Vec<V3>,
    ghost_p: Option<Params>,
    /// The controls card: open or folded to its title (None = not decided
    /// yet: open on a desk, folded on a phone, so the attractor comes first).
    card: Option<bool>,
}

impl Default for Chaos {
    fn default() -> Self {
        let mut c = Chaos { p: Params::default(), speed: 8.0, gap_exp: -6.0, running: false, t: 0.0, a: Vec::new(), b: Vec::new(), gaps: Vec::new(), turn: 0.6, ghost: Vec::new(), ghost_p: None, card: None };
        c.reset();
        c
    }
}

impl Chaos {
    fn reset(&mut self) {
        // Already on the attractor: 20 time units on from (1, 1, 1) at
        // Lorenz's values. From (1, 1, 1) itself the two runs spent their
        // first seconds on screen in one slow spiral, one over the other.
        let mut start = [1.0, 1.0, 1.0];
        for _ in 0..4000 {
            start = rk4(&Params::default(), start, H);
        }
        self.a = vec![start];
        self.b = vec![[start[0] + 10f64.powf(self.gap_exp), start[1], start[2]]];
        self.gaps.clear();
        self.t = 0.0;
    }

    fn step(&mut self) {
        for _ in 0..self.speed.round().max(1.0) as usize {
            let (a, b) = (*self.a.last().unwrap(), *self.b.last().unwrap());
            let (a, b) = (rk4(&self.p, a, H), rk4(&self.p, b, H));
            self.a.push(a);
            self.b.push(b);
            self.t += H;
            self.gaps.push((self.t, dist(a, b).max(1e-16).log10()));
        }
        for v in [&mut self.a, &mut self.b] {
            if v.len() > TRAIL {
                v.drain(..v.len() - TRAIL);
            }
        }
        if self.gaps.len() > 6000 {
            self.gaps.drain(..self.gaps.len() - 6000);
        }
    }

    /// The whole window is the attractor; the controls float over it
    /// (`bare`: H pressed, nothing over it at all). Space pauses.
    pub fn ui(&mut self, ui: &mut egui::Ui, bare: bool) {
        let ctx = ui.ctx().clone();
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            self.running = !self.running;
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(DEEP)).show(ui, |ui| self.field(ui, bare));
        if !bare {
            let mut open = *self.card.get_or_insert(ctx.content_rect().width() >= 760.0);
            crate::app::float_card(&ctx, "chaos-card", "Chaos Lab", &mut open, |ui| self.controls(ui));
            self.card = Some(open);
        }
        if self.running {
            self.step();
            ctx.request_repaint();
        }
    }

    fn controls(&mut self, ui: &mut egui::Ui) {
        ui.label("Two runs of the Lorenz system start a hair apart. Move a slider and watch them part.");
        ui.add_space(8.0);
        let before = self.p;
        ui.add(egui::Slider::new(&mut self.p.sigma, 0.1..=30.0).text("σ  (how fast the air turns over)"));
        ui.add(egui::Slider::new(&mut self.p.rho, 0.1..=60.0).text("ρ  (how hard it is heated)"));
        ui.add(egui::Slider::new(&mut self.p.beta, 0.1..=8.0).text("β  (the cell's shape)"));
        ui.add(egui::Slider::new(&mut self.speed, 1.0..=20.0).text("speed"));
        let gap = self.gap_exp;
        ui.add(egui::Slider::new(&mut self.gap_exp, -12.0..=-1.0).text("starting gap, 10^x"));
        if self.gap_exp != gap {
            self.reset();
        }
        if self.p != before && self.p.rho < 1.0 {
            ui.label("Below ρ = 1 every run settles at the origin: no chaos.");
        }
        ui.horizontal(|ui| {
            // Paused until asked (operator, 2026-10-25: "chaos lab templates
            // need to wait to start when pushed").
            let word = if self.running { "Pause" } else if self.t == 0.0 { "Start" } else { "Run" };
            if ui.button(word).clicked() {
                self.running = !self.running;
            }
            if ui.button("Reset").clicked() {
                self.reset();
            }
            if ui.button("Classic values").clicked() {
                self.p = Params::default();
                self.reset();
            }
        });
        ui.add_space(8.0);
        let now = dist(*self.a.last().unwrap(), *self.b.last().unwrap());
        ui.label(RichText::new(format!("t = {:.1}   gap = {:.2e}", self.t, now)).monospace());
        if let Some(rate) = self.growth() {
            ui.label(format!("The gap grows about e-fold every {:.2} time units (fitted to the plot below).", 1.0 / rate));
        }
        ui.add_space(8.0);
        egui::CollapsingHeader::new("Built with Cove").show(ui, |ui| built_with(ui));
    }

    /// The early, straight part of log(gap) against time: its slope is the
    /// growth rate - an estimate of the largest Lyapunov exponent.
    fn growth(&self) -> Option<f64> {
        let pts: Vec<(f64, f64)> = self.gaps.iter().copied().filter(|(_, g)| *g < -0.5).collect();
        if pts.len() < 200 {
            return None;
        }
        let n = pts.len() as f64;
        let (mt, mg) = (pts.iter().map(|p| p.0).sum::<f64>() / n, pts.iter().map(|p| p.1).sum::<f64>() / n);
        let cov: f64 = pts.iter().map(|p| (p.0 - mt) * (p.1 - mg)).sum();
        let var: f64 = pts.iter().map(|p| (p.0 - mt).powi(2)).sum();
        (var > 0.0).then(|| cov / var * std::f64::consts::LN_10).filter(|r| *r > 0.05)
    }

    /// 60 time units of one run, its first 5 dropped (the way in).
    fn ghost(&mut self) {
        if self.ghost_p == Some(self.p) {
            return;
        }
        let mut s = [1.0, 1.0, 1.0];
        self.ghost.clear();
        for i in 0..12_000 {
            s = rk4(&self.p, s, H);
            if i >= 1000 && i % 2 == 0 {
                self.ghost.push(s);
            }
        }
        self.ghost_p = Some(self.p);
    }

    /// The attractor, edge to edge: a deep gradient and a faint field of
    /// stars behind, the whole attractor as a ghost, the two runs as glowing
    /// trails, and the gap between them as a small glass plot in a corner.
    fn field(&mut self, ui: &mut egui::Ui, bare: bool) {
        self.ghost();
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        // Not started yet: a tap on the field starts it (on a phone the card
        // with the Start button begins folded).
        if resp.clicked() && !self.running && self.t == 0.0 {
            self.running = true;
        }
        let rect = resp.rect;
        backdrop(&painter, rect);
        if resp.dragged() {
            self.turn += resp.drag_delta().x * 0.01;
        } else if self.running {
            self.turn += 0.002;
        }
        let narrow = rect.width() < 760.0;
        // The attractor is ~60 wide and ~50 tall in its own units: fill the
        // window with it, a margin all round, nudged clear of the plot.
        let scale = (rect.width() / 62.0).min(rect.height() / 58.0);
        let centre = rect.center() + Vec2::new(0.0, if narrow { -rect.height() * 0.08 } else { -rect.height() * 0.03 });
        let (c, s) = (self.turn.cos(), self.turn.sin());
        let proj = |v: V3| -> Pos2 {
            let x = v[0] as f32 * c - v[1] as f32 * s;
            centre + Vec2::new(x * scale, -(v[2] as f32 - 25.0) * scale)
        };
        let ghost: Vec<Pos2> = self.ghost.iter().map(|v| proj(*v)).collect();
        painter.add(egui::Shape::line(ghost, Stroke::new(1.0, Color32::from_rgb(120, 150, 210).gamma_multiply(0.16))));
        for (run, col) in [(&self.a, A), (&self.b, B)] {
            let n = run.len();
            for (i, w) in run.windows(2).enumerate() {
                let fade = (i as f32 / n as f32).powf(1.6);
                let (p, q) = (proj(w[0]), proj(w[1]));
                // A soft halo under a bright core: the trail glows.
                painter.line_segment([p, q], Stroke::new(5.0, col.gamma_multiply(0.06 * fade)));
                painter.line_segment([p, q], Stroke::new(1.6, col.gamma_multiply(0.12 + 0.88 * fade)));
            }
            if let Some(h) = run.last() {
                let h = proj(*h);
                painter.circle_filled(h, 18.0, col.gamma_multiply(0.08));
                painter.circle_filled(h, 10.0, col.gamma_multiply(0.22));
                painter.circle_filled(h, 4.5, Color32::WHITE.lerp_to_gamma(col, 0.4));
            }
        }
        if !bare {
            let w = if narrow { rect.width() - 24.0 } else { (rect.width() * 0.32).clamp(280.0, 460.0) };
            let plot = if narrow {
                Rect::from_min_size(rect.left_top() + Vec2::new(12.0, 64.0), Vec2::new(w, 96.0))
            } else {
                Rect::from_min_size(Pos2::new(rect.left() + 16.0, rect.bottom() - 16.0 - 140.0), Vec2::new(w, 140.0))
            };
            self.gap_plot(&painter, plot);
            if !narrow {
                painter.text(rect.right_bottom() - Vec2::new(16.0, 14.0), Align2::RIGHT_BOTTOM, "drag to turn it · space pauses · H hides everything", FontId::proportional(13.0), Color32::from_gray(150));
            }
        }
    }

    fn gap_plot(&self, painter: &egui::Painter, r: Rect) {
        painter.rect_filled(r, 12.0, Color32::from_rgba_unmultiplied(10, 13, 22, 190));
        painter.rect_stroke(r, 12.0, Stroke::new(1.0, Color32::from_white_alpha(26)), egui::StrokeKind::Inside);
        painter.text(r.left_top() + Vec2::new(12.0, 8.0), Align2::LEFT_TOP, "how far apart the runs are (log scale)", FontId::proportional(13.0), Color32::from_gray(210));
        let r = r.shrink2(Vec2::new(12.0, 10.0)).with_min_y(r.top() + 28.0);
        let (Some(first), Some(last)) = (self.gaps.first(), self.gaps.last()) else { return };
        let span = (last.0 - first.0).max(1e-9);
        let (lo, hi) = (-12.5f64, 2.0f64);
        let at = |(t, g): (f64, f64)| Pos2::new(r.left() + ((t - first.0) / span) as f32 * r.width(), r.bottom() - ((g.clamp(lo, hi) - lo) / (hi - lo)) as f32 * r.height());
        let pts: Vec<Pos2> = self.gaps.iter().step_by((self.gaps.len() / 600).max(1)).map(|g| at(*g)).collect();
        painter.add(egui::Shape::line(pts, Stroke::new(1.6, Color32::from_rgb(240, 220, 120))));
    }
}

/// The night sky behind a visual: a gradient from deep blue to near black,
/// and a fixed scatter of faint stars (the same every frame).
pub fn backdrop(painter: &egui::Painter, rect: Rect) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), DEEP_TOP);
    mesh.colored_vertex(rect.right_top(), DEEP_TOP);
    mesh.colored_vertex(rect.right_bottom(), DEEP);
    mesh.colored_vertex(rect.left_bottom(), DEEP);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(mesh);
    let mut seed: u32 = 0x9e37_79b9;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as f32 / u32::MAX as f32
    };
    for _ in 0..220 {
        let p = Pos2::new(rect.left() + next() * rect.width(), rect.top() + next() * rect.height());
        let b = next();
        painter.circle_filled(p, 0.6 + 0.9 * b, Color32::from_white_alpha((18.0 + 60.0 * b * b) as u8));
    }
}

/// The app's own facts, compiled in: what it is, what it runs on, what it
/// was built from. Cove's build records add the artifact's identity.
fn built_with(ui: &mut egui::Ui) {
    ui.label(format!("{} {}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")));
    ui.label(format!("running on {} ({})", std::env::consts::ARCH, std::env::consts::OS));
    ui.label("Rust, compiled to WebAssembly in the browser and to a native binary on a desktop");
    ui.add_space(4.0);
    ui.label(RichText::new("Dependencies (Cargo.toml, pinned to the stack's Rust set)").strong());
    for line in include_str!("../Cargo.toml").lines() {
        let l = line.trim();
        if let Some((name, rest)) = l.split_once(" = ") {
            if let Some(v) = rest.split('"').nth(1).filter(|v| v.starts_with('=')) {
                ui.label(RichText::new(format!("{name} {}", &v[1..])).monospace());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rk4_stays_on_the_attractor_and_two_runs_part() {
        let p = Params::default();
        let (mut a, mut b) = ([1.0, 1.0, 1.0], [1.0 + 1e-8, 1.0, 1.0]);
        let mut at30 = 0.0;
        for i in 1..=8000 {
            a = rk4(&p, a, H);
            b = rk4(&p, b, H);
            assert!(a.iter().all(|x| x.is_finite() && x.abs() < 100.0), "left the attractor at step {i}: {a:?}");
            if i == 6000 {
                at30 = dist(a, b);
            }
        }
        // Measured (2026-10-24, the same RK4 in Python): a 1e-8 start is
        // 1.359e-01 apart at t = 30 and the attractor's size by t = 35.
        assert!((at30 - 0.1359).abs() < 0.001, "gap at t = 30: {at30}");
        assert!(dist(a, b) > 1.0, "gap at t = 40: {}", dist(a, b));
    }

    #[test]
    fn the_fitted_growth_is_near_the_lyapunov_exponent() {
        let mut c = Chaos::default();
        c.speed = 20.0;
        for _ in 0..400 {
            c.step();
        }
        let r = c.growth().expect("enough of the gap to fit");
        // ≈ 0.906 for the classic values; one run's fit is rougher than that.
        assert!((0.5..1.4).contains(&r), "growth {r}");
    }

    #[test]
    fn below_rho_one_it_settles() {
        let p = Params { rho: 0.5, ..Params::default() };
        let mut s = [1.0, 1.0, 1.0];
        for _ in 0..20000 {
            s = rk4(&p, s, H);
        }
        assert!(s.iter().all(|x| x.abs() < 1e-3), "{s:?}");
    }
}
