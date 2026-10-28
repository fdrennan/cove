//! The first screen: the ndexr stack as a field of stars, a small cousin of
//! Cove's atlas. Every package the stack builds is a star in its layer's band
//! (the compatibility layer at the bottom, each language at the top), packages
//! of one group cluster together, and picking one draws what it needs and
//! what needs it. Live: the server reads the stack's catalog (catalog.rs).
//!
//! A place to start, not a finished thing - change the layout, the colours or
//! the question it answers, and make it yours.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Vec2};

use crate::api::AtlasView;

/// One colour per layer, bottom of the stack first.
const COLORS: [Color32; 6] = [
    Color32::from_rgb(150, 160, 175), // Gentoo Prefix
    Color32::from_rgb(110, 170, 255), // EasyBuild
    Color32::from_rgb(90, 200, 250),  // R
    Color32::from_rgb(255, 210, 90),  // Python
    Color32::from_rgb(190, 140, 255), // Julia
    Color32::from_rgb(255, 140, 90),  // Rust
];
const BAND: f32 = 300.0;
const WIDTH: f32 = 2400.0;

#[derive(Default)]
pub struct Atlas {
    /// Where each star sits, in world units; laid once per catalog.
    pos: Vec<Pos2>,
    /// For each star, the stars that need it.
    used_by: Vec<Vec<u32>>,
    laid: usize,
    pan: Vec2,
    zoom: f32,
    fit: bool,
    pub picked: Option<usize>,
    search: String,
    /// Layers switched off in the legend.
    hidden: [bool; 6],
    /// The floating card: open or folded (None = open on a desk, folded on
    /// a phone), and the star it last opened for.
    card_open: Option<bool>,
    card_open_for: Option<usize>,
}

fn color(layer: u8) -> Color32 {
    COLORS[(layer as usize).min(COLORS.len() - 1)]
}

impl Atlas {
    /// Each layer a band; in it, each group a spiral of its packages, the
    /// groups side by side, biggest first, scaled to the band's width. A
    /// language's packages carry no group of their own ("R packages", all
    /// 600), so each joins the constellation of the most-needed package it
    /// needs in its own layer - rlang's, serde's - or stands alone.
    fn lay(&mut self, v: &AtlasView) {
        let n = v.stars.len();
        self.pos = vec![Pos2::ZERO; n];
        self.used_by = vec![Vec::new(); n];
        for (i, s) in v.stars.iter().enumerate() {
            for d in &s.deps {
                self.used_by[*d as usize].push(i as u32);
            }
        }
        for layer in 0..v.layers.len() as u8 {
            let members: Vec<usize> = (0..n).filter(|i| v.stars[*i].layer == layer).collect();
            let one_group = members.windows(2).all(|w| v.stars[w[0]].group == v.stars[w[1]].group);
            let key = |i: usize| -> String {
                if !one_group {
                    return v.stars[i].group.clone();
                }
                // The hub: the star itself when others in its layer need it,
                // else the most-needed thing it needs in its layer.
                let mut best = (self.used_by[i].len(), i);
                for d in &v.stars[i].deps {
                    let d = *d as usize;
                    if v.stars[d].layer == layer && self.used_by[d].len() > best.0 {
                        best = (self.used_by[d].len(), d);
                    }
                }
                if best.0 == 0 { "standalone".to_owned() } else { v.stars[best.1].name.clone() }
            };
            let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
            for i in members {
                let k = key(i);
                match groups.iter_mut().find(|(g, _)| *g == k) {
                    Some((_, m)) => m.push(i),
                    None => groups.push((k, vec![i])),
                }
            }
            groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
            // Clusters left to right, wrapping onto a new row at the band's
            // width; then the whole layer is scaled evenly to fit its band, so
            // every cluster stays round.
            let radius = |k: usize| 9.0 * (k as f32).sqrt() + 6.0;
            let mut placed: Vec<(Pos2, usize)> = Vec::new();
            let (mut x, mut row_y, mut row_h, mut right) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for (gi, (_, members)) in groups.iter().enumerate() {
                let r = radius(members.len());
                if x > 0.0 && x + 2.0 * r > WIDTH {
                    x = 0.0;
                    row_y += row_h + 14.0;
                    row_h = 0.0;
                }
                placed.push((Pos2::new(x + r, row_y + r), gi));
                x += 2.0 * r + 14.0;
                right = right.max(x);
                row_h = row_h.max(2.0 * r);
            }
            let (w, h) = (right.max(1.0), (row_y + row_h).max(1.0));
            let k = (WIDTH / w).min(BAND * 0.86 / h).min(1.0);
            let y = (v.layers.len() as f32 - 1.0 - layer as f32) * BAND;
            let origin = Pos2::new((WIDTH - w * k) / 2.0, y - h * k / 2.0);
            for (c, gi) in placed {
                let c = origin + c.to_vec2() * k;
                for (j, i) in groups[gi].1.iter().enumerate() {
                    // The golden angle: an even, organic fill of the disc.
                    let a = j as f32 * 2.399_963;
                    let d = 9.0 * k * (j as f32 + 0.5).sqrt();
                    self.pos[*i] = c + Vec2::new(a.cos(), a.sin()) * d;
                }
            }
        }
        self.laid = n;
        self.fit = true;
    }

    /// World to screen.
    fn at(&self, rect: Rect, p: Pos2) -> Pos2 {
        rect.center() + (p.to_vec2() + self.pan) * self.zoom
    }

    /// The stack edge to edge; its card floats over it (`bare`: H pressed,
    /// nothing over it).
    pub fn ui(&mut self, ui: &mut egui::Ui, v: &AtlasView, bare: bool) {
        if self.laid != v.stars.len() {
            self.lay(v);
        }
        let ctx = ui.ctx().clone();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(crate::chaos::DEEP)).show(ui, |ui| self.field(ui, v, bare));
        if !bare {
            let mut open = *self.card_open.get_or_insert(ctx.content_rect().width() >= 760.0 || self.picked.is_some());
            if self.picked.is_some() && self.card_open_for != self.picked {
                // A star picked opens the card on it, phone included.
                open = true;
                self.card_open_for = self.picked;
            }
            crate::app::float_card(&ctx, "atlas-card", "The ndexr stack", &mut open, |ui| self.card(ui, v));
            self.card_open = Some(open);
        }
    }

    fn field(&mut self, ui: &mut egui::Ui, v: &AtlasView, bare: bool) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let rect = resp.rect;
        crate::chaos::backdrop(&painter, rect);
        if self.fit {
            let h = v.layers.len() as f32 * BAND;
            // 90% of the window: the tab pill and the card float over the
            // edges, and the stack's top band ran under them at 100%.
            self.zoom = (rect.width() / (WIDTH + 200.0)).min(rect.height() / (h + 60.0)) * 0.9;
            self.pan = -Vec2::new(WIDTH / 2.0, (h - BAND) / 2.0);
            self.fit = false;
        }
        if resp.dragged() {
            self.pan += resp.drag_delta() / self.zoom;
        }
        if resp.hovered() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let f = pinch * (scroll * 0.0015).exp();
            if f != 1.0 {
                if let Some(m) = resp.hover_pos() {
                    let before = (m - rect.center()) / self.zoom - self.pan;
                    self.zoom = (self.zoom * f).clamp(0.05, 12.0);
                    self.pan = (m - rect.center()) / self.zoom - before;
                }
            }
        }
        // The layers' bands and names.
        for (l, name) in v.layers.iter().enumerate() {
            let y = (v.layers.len() - 1 - l) as f32 * BAND;
            let top = self.at(rect, Pos2::new(-100.0, y - BAND / 2.0));
            let bottom = self.at(rect, Pos2::new(WIDTH + 100.0, y + BAND / 2.0));
            let band = Rect::from_min_max(top, bottom);
            painter.rect_filled(band, 0.0, color(l as u8).gamma_multiply(if l % 2 == 0 { 0.035 } else { 0.06 }));
            let n = v.stars.iter().filter(|s| s.layer as usize == l).count();
            painter.text(Pos2::new(rect.left() + 12.0, band.top().max(rect.top()) + 8.0), Align2::LEFT_TOP, format!("{name}  {n}"), FontId::proportional(14.0), color(l as u8));
        }
        let pointer = resp.hover_pos();
        let mut nearest: Option<(usize, f32)> = None;
        let lit = self.picked;
        // The picked star's needs (out) and users (in), under the stars.
        if let Some(p) = lit {
            let from = self.at(rect, self.pos[p]);
            for d in &v.stars[p].deps {
                painter.line_segment([from, self.at(rect, self.pos[*d as usize])], Stroke::new(1.2, color(v.stars[*d as usize].layer).gamma_multiply(0.7)));
            }
            for u in &self.used_by[p] {
                painter.line_segment([from, self.at(rect, self.pos[*u as usize])], Stroke::new(1.0, Color32::WHITE.gamma_multiply(0.35)));
            }
        }
        let q = self.search.trim().to_lowercase();
        for (i, s) in v.stars.iter().enumerate() {
            if self.hidden[s.layer as usize % 6] {
                continue;
            }
            let c = self.at(rect, self.pos[i]);
            if !rect.expand(10.0).contains(c) {
                continue;
            }
            let r = ((1.6 + 0.5 * (self.used_by[i].len() as f32).sqrt()) * self.zoom.sqrt() * 1.6).clamp(1.2, 14.0);
            let dim = !q.is_empty() && !s.name.to_lowercase().contains(&q);
            let col = if dim { color(s.layer).gamma_multiply(0.15) } else { color(s.layer) };
            painter.circle_filled(c, r * 2.4, col.gamma_multiply(0.12));
            painter.circle_filled(c, r, col);
            if s.failed > 0 && s.ok == 0 {
                painter.circle_stroke(c, r + 2.0, Stroke::new(1.2, Color32::from_rgb(255, 90, 90)));
            }
            if Some(i) == lit {
                painter.circle_stroke(c, r + 4.0, Stroke::new(2.0, Color32::WHITE));
            }
            if let Some(m) = pointer {
                let d = m.distance(c);
                if d < r.max(6.0) && nearest.is_none_or(|(_, b)| d < b) {
                    nearest = Some((i, d));
                }
            }
            if self.zoom > 1.6 && !dim {
                painter.text(c + Vec2::new(r + 3.0, 0.0), Align2::LEFT_CENTER, &s.name, FontId::proportional(11.0), Color32::from_gray(200));
            }
        }
        if let Some((i, _)) = nearest {
            let s = &v.stars[i];
            resp.clone().on_hover_text(format!("{}\n{} · {}", s.name, v.layers.get(s.layer as usize).cloned().unwrap_or_default(), s.purpose));
        }
        if resp.clicked() {
            self.picked = nearest.map(|(i, _)| i);
        }
        if !bare && rect.width() >= 760.0 {
            painter.text(rect.right_bottom() - Vec2::new(16.0, 14.0), Align2::RIGHT_BOTTOM, "drag to move · scroll or pinch to zoom · click a star · H hides everything", FontId::proportional(13.0), Color32::from_gray(150));
        }
    }

    fn card(&mut self, ui: &mut egui::Ui, v: &AtlasView) {
        ui.label(format!("{} packages, each built from source on the ndexr build machines.", v.stars.len()));
        ui.add_space(6.0);
        for (l, name) in v.layers.iter().enumerate() {
            let n = v.stars.iter().filter(|s| s.layer as usize == l).count();
            let on = !self.hidden[l];
            let t = RichText::new(format!("●  {name}  {n}")).color(if on { color(l as u8) } else { Color32::from_gray(110) });
            if ui.selectable_label(on, t).on_hover_text("show or hide this layer").clicked() {
                self.hidden[l] = !self.hidden[l];
            }
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("find a package").desired_width(180.0));
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                let q = self.search.trim().to_lowercase();
                self.picked = v.stars.iter().position(|s| s.name.to_lowercase() == q).or_else(|| v.stars.iter().position(|s| s.name.to_lowercase().contains(&q)));
                if let Some(p) = self.picked {
                    self.pan = -self.pos[p].to_vec2();
                    self.zoom = self.zoom.max(2.0);
                }
            }
            if ui.button("Whole stack").clicked() {
                self.fit = true;
            }
        });
        ui.separator();
        let Some(p) = self.picked else {
            ui.label("Pick a star to see what it is, what it needs and what needs it.");
            return;
        };
        let s = v.stars[p].clone();
        ui.label(RichText::new(&s.name).size(18.0).strong().color(color(s.layer)));
        ui.label(format!("{} · {}", v.layers.get(s.layer as usize).cloned().unwrap_or_default(), s.group));
        if !s.purpose.is_empty() {
            ui.label(&s.purpose);
        }
        if !s.archs.is_empty() {
            ui.label(format!("built for {}", s.archs.join(", ")));
        }
        if s.ok + s.failed > 0 {
            ui.label(format!("{} builds ok, {} failed", s.ok, s.failed));
        }
        let mut go = None;
        ui.add_space(6.0);
        ui.label(RichText::new(format!("Needs {}", s.deps.len())).strong());
        for d in s.deps.iter().take(40) {
            let t = &v.stars[*d as usize];
            if ui.link(RichText::new(&t.name).color(color(t.layer))).clicked() {
                go = Some(*d as usize);
            }
        }
        ui.add_space(6.0);
        let users = self.used_by[p].clone();
        ui.label(RichText::new(format!("Needed by {}", users.len())).strong());
        for u in users.iter().take(40) {
            let t = &v.stars[*u as usize];
            if ui.link(RichText::new(&t.name).color(color(t.layer))).clicked() {
                go = Some(*u as usize);
            }
        }
        if users.len() > 40 {
            ui.label(format!("and {} more", users.len() - 40));
        }
        if let Some(g) = go {
            self.picked = Some(g);
            self.pan = -self.pos[g].to_vec2();
        }
    }
}
