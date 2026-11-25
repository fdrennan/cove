//! The changed files as a folder tree: folders open and close, a folder of
//! one folder reads as one path (`src/ui/kit`), and each file carries its
//! status letter and its lines added and removed, quietly. The changes not
//! saved yet come in sections - staged, not staged - since a file can be in
//! both.

use std::collections::{BTreeMap, HashSet};

use eframe::egui::{self, Color32, FontId, Sense, Stroke};

use crate::api::GitFile;
use crate::ui::kit::theme;

#[derive(Default)]
pub struct Tree {
    /// Folders closed, by section and path.
    closed: HashSet<String>,
}

#[derive(Default)]
struct Node {
    dirs: BTreeMap<String, Node>,
    files: Vec<usize>,
    added: i64,
    removed: i64,
}

impl Node {
    fn insert(&mut self, parts: &[&str], i: usize, f: &GitFile) {
        self.added += f.added.max(0);
        self.removed += f.removed.max(0);
        match parts {
            [_] | [] => self.files.push(i),
            [d, rest @ ..] => self.dirs.entry((*d).to_owned()).or_default().insert(rest, i, f),
        }
    }
}

/// The sections a list of files falls in: for the changes not saved yet,
/// by `area`; for a version, one.
fn sections(files: &[GitFile]) -> Vec<(&'static str, Vec<usize>)> {
    let pick = |a: &[&str]| (0..files.len()).filter(|&i| a.contains(&files[i].area.as_str())).collect::<Vec<_>>();
    if files.iter().all(|f| f.area.is_empty()) {
        return vec![("", (0..files.len()).collect())];
    }
    [("Staged", pick(&["staged"])), ("Not staged", pick(&["unstaged", "untracked"]))].into_iter().filter(|(_, v)| !v.is_empty()).collect()
}

/// git's letter for what happened to a file, and its ink: quiet, so the
/// names lead.
pub fn status_mark(s: &str) -> (&'static str, Color32) {
    let (l, c) = match s {
        "A" => ("A", theme::ok()),
        "D" => ("D", theme::alt()),
        "R" => ("R", theme::amber()),
        "C" => ("C", theme::amber()),
        "?" => ("U", theme::ok()),
        "T" => ("T", theme::muted()),
        _ => ("M", theme::primary()),
    };
    (l, c.gamma_multiply(0.85))
}

impl Tree {
    /// Draw the files; returns the one clicked (its index in `files`).
    /// `picked` is the one shown, by area and path.
    pub fn show(&mut self, ui: &mut egui::Ui, files: &[GitFile], picked: Option<(&str, &str)>) -> Option<usize> {
        let mut clicked = None;
        let row_h = if crate::ui::kit::device::coarse() { 34.0 } else { 24.0 };
        egui::ScrollArea::vertical().id_salt("git-tree").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for (title, idx) in sections(files) {
                let mut root = Node::default();
                for &i in &idx {
                    let parts: Vec<&str> = files[i].path.split('/').collect();
                    root.insert(&parts, i, &files[i]);
                }
                if !title.is_empty() {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::hover());
                    let p = ui.painter();
                    p.text(rect.left_center() + egui::vec2(6.0, 2.0), egui::Align2::LEFT_CENTER, title.to_uppercase(), FontId::proportional(10.5), theme::hush());
                    p.text(rect.right_center() - egui::vec2(8.0, -2.0), egui::Align2::RIGHT_CENTER, idx.len().to_string(), FontId::proportional(10.5), theme::hush());
                }
                self.node(ui, &root, title, 0, files, picked, row_h, &mut clicked);
            }
        });
        clicked
    }

    #[allow(clippy::too_many_arguments)]
    fn node(&mut self, ui: &mut egui::Ui, node: &Node, key: &str, depth: usize, files: &[GitFile], picked: Option<(&str, &str)>, row_h: f32, clicked: &mut Option<usize>) {
        let indent = |d: usize| 8.0 + d as f32 * 12.0;
        for (name, child) in &node.dirs {
            // A folder holding one folder and nothing else reads as one path.
            let (mut label, mut n) = (name.clone(), child);
            while n.files.is_empty() && n.dirs.len() == 1 {
                let (k, c) = n.dirs.iter().next().expect("one");
                label = format!("{label}/{k}");
                n = c;
            }
            let k = format!("{key}/{label}");
            let open = !self.closed.contains(&k);
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
            if ui.is_rect_visible(rect) {
                let p = ui.painter_at(rect);
                if resp.hovered() {
                    p.rect_filled(rect, 4.0, theme::surface());
                }
                let x = rect.left() + indent(depth);
                let c = egui::pos2(x + 4.0, rect.center().y);
                let pts = if open { vec![c + egui::vec2(-3.5, -1.5), c + egui::vec2(0.0, 2.0), c + egui::vec2(3.5, -1.5)] } else { vec![c + egui::vec2(-1.5, -3.5), c + egui::vec2(2.0, 0.0), c + egui::vec2(-1.5, 3.5)] };
                p.add(egui::Shape::line(pts, Stroke::new(1.3, theme::hush())));
                let used = counts(&p, rect, n.added, n.removed);
                name_in(&p, egui::pos2(x + 14.0, rect.center().y), rect.right() - used - 6.0, &label, theme::muted());
            }
            if resp.clicked() {
                if open {
                    self.closed.insert(k.clone());
                } else {
                    self.closed.remove(&k);
                }
            }
            if open {
                self.node(ui, n, &k, depth + 1, files, picked, row_h, clicked);
            }
        }
        for &i in &node.files {
            let f = &files[i];
            let on = picked == Some((f.area.as_str(), f.path.as_str()));
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
            if ui.is_rect_visible(rect) {
                let p = ui.painter_at(rect);
                if on {
                    p.rect_filled(rect, 4.0, theme::primary().gamma_multiply(0.14));
                } else if resp.hovered() {
                    p.rect_filled(rect, 4.0, theme::surface());
                }
                let x = rect.left() + indent(depth);
                let (letter, c) = status_mark(&f.status);
                p.text(egui::pos2(x + 4.0, rect.center().y), egui::Align2::CENTER_CENTER, letter, FontId::monospace(11.0), c);
                let name = f.path.rsplit('/').next().unwrap_or(&f.path);
                let ink = if f.status == "D" { theme::muted() } else if on { theme::text_strong() } else { theme::text() };
                // The counts first; the name gets the room left, cut with "…".
                let used = if f.added < 0 {
                    p.text(rect.right_center() - egui::vec2(8.0, 0.0), egui::Align2::RIGHT_CENTER, "bin", FontId::proportional(10.5), theme::hush()).width() + 8.0
                } else {
                    counts(&p, rect, f.added, f.removed)
                };
                name_in(&p, egui::pos2(x + 14.0, rect.center().y), rect.right() - used - 6.0, name, ink);
            }
            let tip = if f.old.is_empty() { f.path.clone() } else { format!("{} ← {}", f.path, f.old) };
            if resp.on_hover_text(tip).clicked() {
                *clicked = Some(i);
            }
        }
    }
}

/// A name from `at` up to `right`, cut with "…" when it does not fit.
fn name_in(p: &egui::Painter, at: egui::Pos2, right: f32, name: &str, ink: Color32) {
    let mut job = egui::text::LayoutJob::single_section(name.to_owned(), egui::TextFormat::simple(FontId::proportional(12.5), ink));
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - at.x).max(12.0));
    let g = p.layout_job(job);
    p.galley(egui::pos2(at.x, at.y - g.size().y / 2.0), g, ink);
}

/// "+12 −3" at the row's right, small and quiet, each in a muted tint.
/// Returns the width it took.
fn counts(p: &egui::Painter, rect: egui::Rect, added: i64, removed: i64) -> f32 {
    let font = FontId::monospace(10.5);
    let mut x = rect.right() - 8.0;
    for (n, c, sign) in [(removed, theme::alt(), "−"), (added, theme::ok(), "+")] {
        if n <= 0 {
            continue;
        }
        let g = p.layout_no_wrap(format!("{sign}{n}"), font.clone(), c.gamma_multiply(0.8));
        x -= g.size().x;
        p.galley(egui::pos2(x, rect.center().y - g.size().y / 2.0), g, c);
        x -= 6.0;
    }
    rect.right() - x
}
