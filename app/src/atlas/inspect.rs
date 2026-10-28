//! The atlas's inspection view: one node's real relations laid flat, what it
//! stands on to the left, the node in the middle, what stands on it to the
//! right (operator, 2026-10-21: "The environments should become the
//! organizing layer for inspecting connections ... A useful inspection
//! arrangement would place EasyBuild on one side, the environment in the
//! middle, and its package groups on the other side"). Drawn over the stack,
//! so no camera angle hides a line; every row is one edge of the scene, or a
//! group of them, counted, that opens into those edges.

use super::*;
use std::collections::HashSet;

/// One side's rows for one relation.
pub(super) struct Section {
    pub left: bool,
    pub rel: Rel,
    pub title: String,
    /// Edges it stands for (members of open and closed groups alike).
    pub count: usize,
    pub rows: Vec<Row>,
}

pub(super) struct Row {
    pub label: String,
    pub note: String,
    pub color: Color32,
    pub go: Go,
    /// 0: an item or a group; 1: a member of the open group above it.
    pub depth: u8,
    pub tip: String,
}

#[derive(Clone, PartialEq)]
pub(super) enum Go {
    Node(u32),
    Toggle(String),
    Nothing,
}

/// A side's groups open one at a time by this key.
pub(super) fn group_key(n: u32, left: bool, rel: Rel, layer: Layer, group: &str) -> String {
    format!("{n}|{left}|{rel:?}|{layer:?}|{group}")
}

/// Past this many, a section is shown by group, each with its count.
const GROUP_PAST: usize = 10;

/// `n`'s relations, by side and kind, as rows: each edge of the scene once;
/// a long list by its layer and group, open where `open` says.
pub(super) fn sections(sc: &Scene, n: u32, open: &HashSet<String>, cat: Option<&Catalog>) -> Vec<Section> {
    let nd = &sc.nodes[n as usize];
    let mut out = Vec::new();
    for left in [true, false] {
        let mut by: Vec<(Rel, Vec<u32>)> = Vec::new();
        let edges = if left { &sc.up[n as usize] } else { &sc.down[n as usize] };
        for &e in edges {
            let e = &sc.edges[e as usize];
            let other = if left { e.to } else { e.from };
            match by.iter_mut().find(|x| x.0 == e.rel) {
                Some(x) => x.1.push(other),
                None => by.push((e.rel, vec![other])),
            }
        }
        // Membership first, then what it is built on: the env sets are the
        // layer this view is organised round.
        by.sort_by_key(|x| match x.0 {
            Rel::Member => 0,
            Rel::Joins => 0,
            Rel::Uses => 1,
            Rel::Dep => 2,
            Rel::Works => 3,
            Rel::Sub => 4,
            Rel::Floor => 5,
            Rel::Serves | Rel::Reach => 1,
            Rel::Proxy | Rel::Tunnel | Rel::Dns => 2,
            Rel::Runs => 5,
        });
        for (rel, mut items) in by {
            items.sort_by(|a, b| sc.nodes[*a as usize].label.cmp(&sc.nodes[*b as usize].label));
            items.dedup();
            let (from, to) = rel.words();
            let title = if left { from } else { to }.to_owned();
            let note_of = |k: u32| -> String {
                let m = &sc.nodes[k as usize];
                let pair = if left { (n, k) } else { (k, n) };
                match sc.used.get(&pair) {
                    Some(v) if rel == Rel::Uses => v.clone(),
                    _ => m.layer.label().to_owned(),
                }
            };
            let tip_of = |k: u32| -> String {
                let m = &sc.nodes[k as usize];
                let (a, b) = if left { (&nd.label, &m.label) } else { (&m.label, &nd.label) };
                let what = match rel {
                    Rel::Member => "is in the env set's manifest and site library - membership, not a dependency",
                    Rel::Uses => "loads this EasyBuild module - from the set's easyconfig",
                    Rel::Dep => "depends on it - the catalog's own dependency",
                    Rel::Floor => "stands on it - the stack's shape",
                    Rel::Works => "works in this env set - its .ndexr-env",
                    Rel::Sub => "is a subdomain of it",
                    Rel::Joins => "joins this language set - the kit's combos.txt",
                    Rel::Serves => "is answered by it - the domain's nginx target",
                    Rel::Proxy => "hands it the requests that arrive for its domains",
                    Rel::Runs => "runs on it",
                    Rel::Dns => "has its DNS answered there",
                    Rel::Reach => "resolves to it - where requests for it arrive",
                    Rel::Tunnel => "forwards what it receives down to it",
                };
                format!("{a} → {b}: {what}")
            };
            let count = items.len();
            let mut rows = Vec::new();
            if count <= GROUP_PAST {
                for k in items {
                    let m = &sc.nodes[k as usize];
                    rows.push(Row { label: m.label.clone(), note: note_of(k), color: sc.tint(m), go: Go::Node(k), depth: 0, tip: tip_of(k) });
                }
            } else {
                // By layer and group, largest first; an open group lists its
                // members under it.
                let mut groups: Vec<((Layer, String), Vec<u32>)> = Vec::new();
                for k in items {
                    let m = &sc.nodes[k as usize];
                    let g = (m.layer, m.group.clone());
                    match groups.iter_mut().find(|x| x.0 == g) {
                        Some(x) => x.1.push(k),
                        None => groups.push((g, vec![k])),
                    }
                }
                groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0 .1.cmp(&b.0 .1)));
                for ((layer, group), members) in groups {
                    let key = group_key(n, left, rel, layer, &group);
                    let is_open = open.contains(&key);
                    let name = if group.is_empty() { layer.label().to_owned() } else { format!("{} · {group}", layer.label()) };
                    let tip = format!("{} {} - {}", members.len(), if is_open { "shown below; click to fold" } else { "click to list them" }, title);
                    rows.push(Row { label: format!("{} {name}", if is_open { "▾" } else { "▸" }), note: members.len().to_string(), color: layer.color(), go: Go::Toggle(key), depth: 0, tip });
                    if is_open {
                        for k in members {
                            let m = &sc.nodes[k as usize];
                            rows.push(Row { label: m.label.clone(), note: note_of(k), color: sc.tint(m), go: Go::Node(k), depth: 1, tip: tip_of(k) });
                        }
                    }
                }
            }
            out.push(Section { left, rel, title, count, rows });
        }
    }
    // A package listed in sets that have no module record in the catalog
    // (the r4.x_py3.y compositions): named, not drawn as edges.
    if let (Some(cat), Some(i)) = (cat, nd.sw.filter(|_| matches!(nd.layer, Layer::R | Layer::Python | Layer::Rust))) {
        let known: HashSet<&str> = sc.nodes.iter().filter(|m| matches!(m.layer, Layer::Env | Layer::Combo)).filter_map(|m| env_set_name(&m.label)).collect();
        let rest: Vec<&String> = cat.sw(i as usize).sets.iter().filter(|s| !known.contains(s.as_str())).collect();
        if !rest.is_empty() {
            let rows = rest
                .iter()
                .map(|s| Row { label: (*s).clone(), note: "no module".into(), color: Layer::Env.color().gamma_multiply(0.45), go: Go::Nothing, depth: 0, tip: format!("{} lists {s}, a set with no module record in the catalog: there is no star to draw it to", nd.label) })
                .collect();
            out.push(Section { left: true, rel: Rel::Member, title: "listed in sets without a module".into(), count: rest.len(), rows });
        }
    }
    out
}

/// What the person did in the view, applied once it is drawn.
enum Do {
    Go(u32),
    Toggle(String),
    Back,
    Overview,
    InStack,
    Isolate,
}

impl Atlas {
    /// Where the view goes: the canvas between the rail and the card, on a
    /// desk, when the focus is a star of the stack or a bundle and the person
    /// has not asked to see it in the stack instead.
    pub(super) fn inspect_area(&self, rect: Rect, compact: bool) -> Option<(Rect, u32)> {
        if compact || self.immersive || self.tour.is_some() {
            return None;
        }
        let sc = self.scene.as_ref()?;
        let n = match self.focus() {
            Focus::Node(n) => n,
            Focus::Bundle(b) => sc.bundles.get(b as usize)?.env,
            _ => return None,
        };
        let nd = sc.nodes.get(n as usize)?;
        if !matches!(nd.layer, Layer::Env | Layer::R | Layer::Python | Layer::Rust | Layer::Julia | Layer::Combo | Layer::EasyBuild | Layer::Prefix) || self.flat != Some(n) {
            return None;
        }
        let pad = 14.0;
        let area = Rect::from_min_max(Pos2::new(rect.left() + pad + 250.0 + pad, rect.top() + pad + 46.0), Pos2::new(rect.right() - pad - 360.0 - pad, rect.bottom() - pad - 26.0));
        (area.width() >= 440.0 && area.height() >= 260.0).then_some((area, n))
    }

    pub(super) fn inspect(&mut self, ui: &mut egui::Ui, area: Rect, n: u32, cat: Option<&Catalog>) {
        let Some(sc) = self.scene.as_ref() else { return };
        let Some(nd) = sc.nodes.get(n as usize) else { return };
        let secs = sections(sc, n, &self.open, cat);
        let mut todo: Option<Do> = None;
        ui.interact(area, ui.id().with("atlas-inspect"), Sense::click_and_drag());
        let painter = ui.painter_at(area);
        // Over the stack and its photograph: the lines here are data only.
        painter.rect_filled(area, theme::WIN_RADIUS, theme::bg().gamma_multiply(if self.isolate { 1.0 } else { 0.95 }));
        painter.rect_stroke(area, theme::WIN_RADIUS, Stroke::new(1.0, theme::pane_brd()), egui::StrokeKind::Inside);

        // ── the bar: what this is, and the ways out ────────────────────────
        let bar = Rect::from_min_size(area.min + egui::vec2(12.0, 8.0), egui::vec2(area.width() - 24.0, 26.0));
        let mut b = ui.new_child(UiBuilder::new().max_rect(bar).layout(egui::Layout::left_to_right(egui::Align::Center)));
        b.label(RichText::new("INSPECT").size(10.0).color(theme::hush()));
        b.add(egui::Label::new(RichText::new(&nd.label).size(13.0).strong().color(theme::text_strong())).truncate());
        b.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("‹ Back").on_hover_text("where you were before this (Esc)").clicked() {
                todo = Some(Do::Back);
            }
            if ui.button("Overview").on_hover_text("the whole stack - this one stays marked").clicked() {
                todo = Some(Do::Overview);
            }
            if ui.button("Show in the stack").on_hover_text("its lines in the 3D stack, fitted to the view").clicked() {
                todo = Some(Do::InStack);
            }
            if ui.selectable_label(self.isolate, "Isolate").on_hover_text("in the stack, hide everything it does not touch").clicked() {
                todo = Some(Do::Isolate);
            }
        });
        // The key: membership never looks like dependency.
        let key_y = bar.bottom() + 12.0;
        let mut x = area.left() + 16.0;
        for (rel, text) in [(Rel::Uses, "uses (easyconfig)"), (Rel::Dep, "depends on"), (Rel::Member, "member of an env set")] {
            let (a, c) = (Pos2::new(x, key_y), Pos2::new(x + 26.0, key_y));
            stroke_rel(&painter, &[a, c], rel, Stroke::new(1.6, theme::muted()));
            let g = painter.text(Pos2::new(x + 32.0, key_y), Align2::LEFT_CENTER, text, FontId::proportional(11.0), theme::muted());
            x = g.right() + 18.0;
        }

        // ── the three columns ──────────────────────────────────────────────
        let body = Rect::from_min_max(Pos2::new(area.left() + 12.0, key_y + 14.0), Pos2::new(area.right() - 12.0, area.bottom() - 10.0));
        let side_w = (body.width() * 0.36).min(320.0);
        let lrect = Rect::from_min_size(body.min, egui::vec2(side_w, body.height()));
        let rrect = Rect::from_min_max(Pos2::new(body.right() - side_w, body.top()), body.max);
        let mid_w = (rrect.left() - lrect.right() - 80.0).clamp(120.0, 240.0);
        let lefts: Vec<&Section> = secs.iter().filter(|s| s.left).collect();
        let rights: Vec<&Section> = secs.iter().filter(|s| !s.left).collect();
        // The middle: the node, named, with what each side holds.
        let lines: Vec<String> = secs.iter().map(|s| format!("{} {}", s.count, s.title)).collect();
        let name_g = painter.layout(nd.label.clone(), theme::display(16.0), theme::text_strong(), mid_w - 20.0);
        let what = if nd.group.is_empty() { nd.layer.label().to_owned() } else { format!("{} · {}", nd.layer.label(), nd.group) };
        let what_g = painter.layout(what, FontId::proportional(11.0), theme::muted(), mid_w - 20.0);
        let sum_g = painter.layout(lines.join("\n"), FontId::proportional(11.0), theme::text(), mid_w - 20.0);
        let h = name_g.size().y + what_g.size().y + sum_g.size().y + 30.0;
        let mid = Rect::from_center_size(Pos2::new((lrect.right() + rrect.left()) / 2.0, body.center().y), egui::vec2(mid_w, h));
        let c = sc.tint(nd);
        painter.rect_filled(mid, 8, theme::surface_hi());
        painter.rect_stroke(mid, 8, Stroke::new(1.6, c), egui::StrokeKind::Inside);
        let mut y = mid.top() + 10.0;
        let gx = mid.left() + 10.0;
        let (nh, wh) = (name_g.size().y, what_g.size().y);
        painter.galley(Pos2::new(gx, y), name_g, c);
        y += nh + 2.0;
        painter.galley(Pos2::new(gx, y), what_g, theme::muted());
        y += wh + 8.0;
        painter.galley(Pos2::new(gx, y), sum_g, theme::text());

        let mut placed: Vec<(Rect, bool, Rel, Color32, u8, bool, Rect)> = Vec::new();
        for (secs, rect, left) in [(lefts, lrect, true), (rights, rrect, false)] {
            let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
            let clip = rect;
            egui::ScrollArea::vertical().id_salt(("atlas-inspect-col", left, n)).auto_shrink([false, false]).max_height(rect.height()).show(&mut child, |ui| {
                if secs.is_empty() {
                    ui.label(RichText::new(if left { "stands on nothing in the catalog" } else { "nothing in the catalog stands on it" }).size(11.5).color(theme::hush()));
                }
                for s in secs {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("{} · {}", s.title.to_uppercase(), s.count)).size(10.0).color(theme::hush()));
                    for r in &s.rows {
                        let w = ui.available_width();
                        let (rr, resp) = ui.allocate_exact_size(egui::vec2(w, 21.0), if r.go == Go::Nothing { Sense::hover() } else { Sense::click() });
                        let indent = if r.depth == 1 { 16.0 } else { 0.0 };
                        let row = Rect::from_min_max(Pos2::new(rr.left() + if left { 0.0 } else { indent }, rr.top()), Pos2::new(rr.right() - if left { indent } else { 0.0 }, rr.bottom()));
                        let p = ui.painter();
                        if resp.hovered() {
                            p.rect_filled(row, 4, theme::surface_hi());
                        }
                        p.circle_filled(Pos2::new(row.left() + 8.0, row.center().y), 4.0, r.color);
                        let note = p.text(Pos2::new(row.right() - 6.0, row.center().y), Align2::RIGHT_CENTER, &r.note, FontId::proportional(10.5), theme::muted());
                        let mut job = egui::text::LayoutJob::simple_singleline(r.label.clone(), FontId::proportional(12.0), if r.go == Go::Nothing { theme::muted() } else { theme::text() });
                        job.wrap = egui::text::TextWrapping::truncate_at_width((note.left() - row.left() - 26.0).max(30.0));
                        let g = p.layout_job(job);
                        p.galley(Pos2::new(row.left() + 18.0, row.center().y - g.size().y / 2.0), g, theme::text());
                        let resp = resp.on_hover_text(&r.tip);
                        if resp.clicked() {
                            todo = Some(match &r.go {
                                Go::Node(k) => Do::Go(*k),
                                Go::Toggle(key) => Do::Toggle(key.clone()),
                                Go::Nothing => continue,
                            });
                        }
                        if resp.hovered() && r.go != Go::Nothing {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        placed.push((row, left, s.rel, r.color, r.depth, resp.hovered(), clip));
                    }
                }
            });
        }

        // ── the lines: each row to the node; an open group's members out of
        // the group's own line, so a bundle parts where it arrives ─────────
        let lp = ui.painter_at(body);
        let any_hover = placed.iter().any(|p| p.5);
        let mut parent: Option<Pos2> = None;
        for &(row, left, rel, color, depth, hov, clip) in &placed {
            let edge = if left { row.right_center() } else { row.left_center() };
            if depth == 0 {
                parent = Some(edge);
            }
            if !clip.contains(edge) {
                continue;
            }
            let alpha = if hov { 235 } else if any_hover { 45 } else { 120 };
            let st = Stroke::new(if hov { 2.2 } else { 1.2 }, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha));
            let pts = if depth == 0 {
                let to = if left { mid.left_center() } else { mid.right_center() };
                let (a, b) = if left { (edge, to) } else { (to, edge) };
                let dx = (b.x - a.x) * 0.5;
                egui::epaint::CubicBezierShape::from_points_stroke([a, a + egui::vec2(dx, 0.0), b - egui::vec2(dx, 0.0), b], false, Color32::TRANSPARENT, st).flatten(Some(0.5))
            } else {
                // From the group's line, a short branch out to the member.
                let Some(p) = parent.filter(|p| clip.contains(*p)) else { continue };
                let out = if left { 14.0 } else { -14.0 };
                vec![p, p + egui::vec2(out, 0.0), Pos2::new(p.x + out, edge.y), edge]
            };
            stroke_rel(&lp, &pts, rel, st);
        }

        // A pass egui will run again (a tooltip sizing itself) sees the same
        // click: act on the kept pass only, or a group opened and shut in
        // one frame and a click landed twice (screenshots, 2026-10-21).
        if ui.ctx().will_discard() {
            todo = None;
        }
        match todo {
            Some(Do::Go(k)) => self.go(Focus::Node(k)),
            Some(Do::Toggle(key)) => {
                if !self.open.remove(&key) {
                    self.open.insert(key);
                }
            }
            Some(Do::Back) => self.back(),
            Some(Do::Overview) => {
                self.last_pick = Some(n);
                self.go(Focus::All);
            }
            Some(Do::InStack) => {
                self.flat = None;
                self.aim();
            }
            Some(Do::Isolate) => self.isolate = !self.isolate,
            None => {}
        }
    }
}

/// A relation's line: membership dashed, the rest solid.
pub(super) fn stroke_rel(p: &egui::Painter, pts: &[Pos2], rel: Rel, st: Stroke) {
    if rel == Rel::Member {
        p.extend(egui::Shape::dashed_line(pts, st, 5.0, 4.0));
    } else {
        p.add(egui::Shape::line(pts.to_vec(), st));
    }
}
