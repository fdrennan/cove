//! The Git window (operator, 2026-10-30: "just create a basic git viewer in
//! cove instead for now, think pycharm"): the project's versions down the
//! left, the changes not saved yet on top; the files the picked one changed,
//! under its message, in the middle; and the picked file's diff on the right,
//! each line numbered on both sides and coloured by what happened to it. On
//! a phone the three come one at a time, each with its way back.
//!
//! Every project is a git repository: the autosave service and Save commit
//! to it (host.rs versions_at), so this reads the same history Versions did.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, FontId, RichText, Sense};

use crate::api::{GitFile, Query, Version};
use crate::backend::Backend;
use crate::ui::kit::theme;

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

/// What the window asks of its owner, which holds the history and Restore.
pub enum GitAct {
    None,
    Restore(String),
    Refresh,
}

#[derive(Default)]
pub struct GitView {
    host: String,
    /// The version picked: "" the changes not saved yet.
    sha: Option<String>,
    files: Slot<Vec<GitFile>>,
    file: Option<GitFile>,
    diff: Slot<Diff>,
    /// On a phone, which pane shows: 0 the history, 1 the files, 2 the diff.
    step: u8,
    confirm: Option<String>,
}

/// A diff as drawn: its lines, and the widest one, for the horizontal scroll.
#[derive(Default)]
pub struct Diff {
    lines: Vec<Line>,
    widest: usize,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Hunk,
    Add,
    Del,
    Same,
    Note,
}

#[derive(Debug)]
struct Line {
    kind: Kind,
    old: Option<u32>,
    new: Option<u32>,
    text: String,
}

const LOG_ROW: f32 = 46.0;
const FILE_ROW: f32 = 30.0;
const MONO: f32 = 12.5;

impl GitView {
    /// The window's body. `versions` is the project's history, newest
    /// first; `said` the last restore's word.
    pub fn ui(&mut self, ui: &mut egui::Ui, backend: &Backend, host: &str, versions: &Option<Result<Vec<Version>, String>>, said: &Option<Result<String, String>>) -> GitAct {
        let ctx = ui.ctx().clone();
        if self.host != host {
            *self = GitView { host: host.to_owned(), ..Default::default() };
        }
        // The unsaved changes are where a person starts, as in PyCharm.
        if self.sha.is_none() {
            self.pick(Some(String::new()), backend, &ctx);
        }
        let mut act = GitAct::None;
        match said {
            Some(Ok(m)) => {
                ui.label(RichText::new(format!("✓ {m}")).size(13.0).color(theme::ok()));
            }
            Some(Err(e)) => {
                ui.label(RichText::new(format!("Not restored: {e}")).size(13.0).color(theme::alt()));
            }
            None => {}
        }
        let list: &[Version] = match versions {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("reading its history…").size(13.0).color(theme::text()));
                });
                return act;
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).size(13.0).color(theme::alt()));
                return act;
            }
            Some(Ok(v)) => v,
        };
        let narrow = ui.available_width() < 760.0;
        if narrow {
            match self.step {
                0 => self.log_pane(ui, list, backend, &ctx, &mut act),
                1 => {
                    if back(ui, "← history") {
                        self.step = 0;
                    }
                    self.files_pane(ui, list, backend, &ctx, &mut act);
                }
                _ => {
                    if back(ui, "← files") {
                        self.step = 1;
                    }
                    self.diff_pane(ui);
                }
            }
            return act;
        }
        let w = ui.available_width();
        egui::Panel::left("git-log").resizable(true).default_size((w * 0.3).clamp(260.0, 420.0)).show(ui, |ui| self.log_pane(ui, list, backend, &ctx, &mut act));
        egui::Panel::left("git-files").resizable(true).default_size((w * 0.24).clamp(220.0, 360.0)).show(ui, |ui| self.files_pane(ui, list, backend, &ctx, &mut act));
        egui::CentralPanel::default().show(ui, |ui| self.diff_pane(ui));
        act
    }

    /// Pick a version ("" the unsaved changes): its files are asked for, and
    /// the diff waits for a file.
    fn pick(&mut self, sha: Option<String>, backend: &Backend, ctx: &egui::Context) {
        self.sha = sha.clone();
        self.file = None;
        self.confirm = None;
        *self.diff.lock().unwrap() = None;
        let Some(sha) = sha else { return };
        let slot = self.files.clone();
        *slot.lock().unwrap() = None;
        backend.query(Query::GitChanges { host: self.host.clone(), sha }, ctx, move |r: Result<Vec<GitFile>, String>| {
            *slot.lock().unwrap() = Some(r);
        });
    }

    fn pick_file(&mut self, f: GitFile, backend: &Backend, ctx: &egui::Context) {
        let slot = self.diff.clone();
        *slot.lock().unwrap() = None;
        let q = Query::GitDiff { host: self.host.clone(), sha: self.sha.clone().unwrap_or_default(), path: f.path.clone(), old: f.old.clone() };
        self.file = Some(f);
        backend.query(q, ctx, move |r: Result<String, String>| {
            *slot.lock().unwrap() = Some(r.map(|t| parse(&t)));
        });
    }

    /// The history: the unsaved changes, then every version, a line of dots
    /// down the left joining them as PyCharm's log does.
    fn log_pane(&mut self, ui: &mut egui::Ui, list: &[Version], backend: &Backend, ctx: &egui::Context, act: &mut GitAct) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("History").size(14.0).strong().color(theme::text_strong()));
            ui.label(RichText::new(format!("{}", list.len())).size(12.5).color(theme::muted()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::tool(ui, "⟳", "read the history and the changes again").clicked() {
                    *act = GitAct::Refresh;
                    let sha = self.sha.clone();
                    self.pick(sha, backend, ctx);
                }
            });
        });
        ui.separator();
        let now = crate::ui::software::now_secs();
        let rows = list.len() + 1;
        let mut picked = None;
        egui::ScrollArea::vertical().id_salt("git-log").auto_shrink([false, false]).show_rows(ui, LOG_ROW, rows, |ui, range| {
            for i in range {
                let (sha, title, sub) = if i == 0 {
                    (String::new(), "Changes not saved yet".to_owned(), "saved as a version within 30 minutes, or by Save".to_owned())
                } else {
                    let v = &list[i - 1];
                    (v.sha.clone(), first_line(&v.message).to_owned(), format!("{} · {} ago · {}", v.author, crate::ui::software::fmt_dur((now - v.when).max(0)), &v.sha[..v.sha.len().min(7)]))
                };
                let on = self.sha.as_deref() == Some(sha.as_str());
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), LOG_ROW), Sense::click());
                let p = ui.painter();
                if on {
                    p.rect_filled(rect, 6.0, theme::primary().gamma_multiply(0.16));
                } else if resp.hovered() {
                    p.rect_filled(rect, 6.0, theme::surface());
                }
                // The lane: one line through every version, a dot on each.
                let x = rect.left() + 12.0;
                let lane = theme::primary().gamma_multiply(0.55);
                let top = if i == 0 { rect.center().y } else { rect.top() };
                let bottom = if i + 1 == rows { rect.center().y } else { rect.bottom() };
                if rows > 1 {
                    p.line_segment([egui::pos2(x, top), egui::pos2(x, bottom)], egui::Stroke::new(2.0, lane));
                }
                if i == 0 {
                    p.circle(rect.left_center() + egui::vec2(12.0, 0.0), 5.0, theme::bg(), egui::Stroke::new(2.0, theme::amber()));
                } else {
                    p.circle_filled(egui::pos2(x, rect.center().y), if i == 1 { 5.5 } else { 4.5 }, theme::primary());
                }
                let tx = rect.left() + 28.0;
                let tw = rect.right() - tx - 6.0;
                let title_g = ui.painter().layout(title, FontId::proportional(13.5), if i == 0 { theme::amber() } else { theme::text_strong() }, f32::INFINITY);
                let sub_g = ui.painter().layout(sub, FontId::proportional(11.5), theme::muted(), f32::INFINITY);
                let clip = egui::Rect::from_min_max(egui::pos2(tx, rect.top()), egui::pos2(tx + tw, rect.bottom()));
                let p = ui.painter().with_clip_rect(clip);
                p.galley(egui::pos2(tx, rect.top() + 6.0), title_g, Color32::WHITE);
                p.galley(egui::pos2(tx, rect.top() + 26.0), sub_g, Color32::WHITE);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    picked = Some(sha);
                }
            }
        });
        if let Some(sha) = picked {
            self.pick(Some(sha), backend, ctx);
            self.step = 1;
        }
    }

    /// The picked version: its message, whose and when, Restore, and the
    /// files it changed with their lines added and removed.
    fn files_pane(&mut self, ui: &mut egui::Ui, list: &[Version], backend: &Backend, ctx: &egui::Context, act: &mut GitAct) {
        let sha = self.sha.clone().unwrap_or_default();
        let v = list.iter().position(|v| v.sha == sha).map(|k| (k, &list[k]));
        match v {
            Some((k, v)) => {
                ui.add(egui::Label::new(RichText::new(&v.message).size(13.5).strong().color(theme::text_strong())).wrap());
                ui.label(RichText::new(format!("{} · {} UTC", v.author, crate::ui::software::fmt_date(v.when))).size(12.0).color(theme::text()));
                ui.horizontal(|ui| {
                    if ui.add(egui::Label::new(RichText::new(&v.sha[..v.sha.len().min(10)]).monospace().size(12.0).color(theme::muted())).sense(Sense::click())).on_hover_text("copy the whole id").clicked() {
                        ui.ctx().copy_text(v.sha.clone());
                    }
                    // The newest is what is there now; any other can come back.
                    if k > 0 {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if self.confirm.as_deref() == Some(sha.as_str()) {
                                if ui.add(theme::primary_button("Restore it")).on_hover_text("what is there now is saved as a version first, so nothing is lost").clicked() {
                                    *act = GitAct::Restore(sha.clone());
                                    self.confirm = None;
                                }
                                if ui.button("Cancel").clicked() {
                                    self.confirm = None;
                                }
                            } else if ui.button("Restore").on_hover_text("put this version back - what is there now is saved as a version first").clicked() {
                                self.confirm = Some(sha.clone());
                            }
                        });
                    }
                });
            }
            None => {
                ui.label(RichText::new("Changes not saved yet").size(13.5).strong().color(theme::amber()));
            }
        }
        ui.separator();
        let files = self.files.lock().unwrap().clone();
        let files = match files {
            None => {
                ui.spinner();
                return;
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).size(12.5).color(theme::alt()));
                return;
            }
            Some(Ok(f)) => f,
        };
        if files.is_empty() {
            ui.label(RichText::new(if sha.is_empty() { "Nothing changed since the last version." } else { "This version changed no files." }).size(12.5).color(theme::text()));
            return;
        }
        let (add, del) = files.iter().fold((0, 0), |(a, d), f| (a + f.added.max(0), d + f.removed.max(0)));
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("{} files", files.len())).size(12.5).color(theme::text_strong()));
            ui.label(RichText::new(format!("+{add}")).size(12.5).color(theme::ok()));
            ui.label(RichText::new(format!("−{del}")).size(12.5).color(theme::alt()));
        });
        let mut picked = None;
        egui::ScrollArea::vertical().id_salt("git-files").auto_shrink([false, false]).show_rows(ui, FILE_ROW, files.len(), |ui, range| {
            for i in range {
                let f = &files[i];
                let on = self.file.as_ref().is_some_and(|x| x.path == f.path);
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), FILE_ROW), Sense::click());
                if on {
                    ui.painter().rect_filled(rect, 5.0, theme::primary().gamma_multiply(0.16));
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 5.0, theme::surface());
                }
                let (letter, c) = status_mark(&f.status);
                let p = ui.painter().with_clip_rect(rect);
                p.text(rect.left_center() + egui::vec2(8.0, 0.0), egui::Align2::LEFT_CENTER, letter, FontId::monospace(12.5), c);
                let counts = if f.added < 0 { "bin".to_owned() } else { format!("+{} −{}", f.added, f.removed) };
                let cg = p.layout_no_wrap(counts, FontId::proportional(11.5), theme::muted());
                let cw = cg.size().x;
                p.galley(egui::pos2(rect.right() - cw - 6.0, rect.center().y - cg.size().y / 2.0), cg, Color32::WHITE);
                // The name strong, its folder after it, quieter.
                let (dir, name) = f.path.rsplit_once('/').map_or(("", f.path.as_str()), |(d, n)| (d, n));
                let mut job = egui::text::LayoutJob::default();
                job.append(name, 0.0, egui::TextFormat::simple(FontId::proportional(13.0), if f.status == "D" { theme::muted() } else { theme::text_strong() }));
                if !dir.is_empty() {
                    job.append(&format!("  {dir}"), 0.0, egui::TextFormat::simple(FontId::proportional(11.5), theme::muted()));
                }
                let tx = rect.left() + 28.0;
                let clip = egui::Rect::from_min_max(egui::pos2(tx, rect.top()), egui::pos2(rect.right() - cw - 12.0, rect.bottom()));
                let g = ui.painter().layout_job(job);
                ui.painter().with_clip_rect(clip).galley(egui::pos2(tx, rect.center().y - g.size().y / 2.0), g, Color32::WHITE);
                let tip = if f.old.is_empty() { f.path.clone() } else { format!("{} ← {}", f.path, f.old) };
                if resp.on_hover_text(tip).clicked() {
                    picked = Some(f.clone());
                }
            }
        });
        if let Some(f) = picked {
            self.pick_file(f, backend, ctx);
            self.step = 2;
        }
    }

    /// The picked file's diff: old and new line numbers, then the line,
    /// added on green and removed on red, each hunk headed by where it is.
    fn diff_pane(&mut self, ui: &mut egui::Ui) {
        let Some(f) = self.file.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Pick a file to see what changed in it.").size(13.0).color(theme::text()));
            });
            return;
        };
        ui.horizontal(|ui| {
            let (letter, c) = status_mark(&f.status);
            ui.label(RichText::new(letter).monospace().size(13.0).color(c));
            ui.add(egui::Label::new(RichText::new(&f.path).monospace().size(13.0).color(theme::text_strong())).truncate());
            if !f.old.is_empty() {
                ui.label(RichText::new(format!("← {}", f.old)).monospace().size(12.0).color(theme::muted()));
            }
        });
        ui.separator();
        let diff = self.diff.lock().unwrap();
        let d = match diff.as_ref() {
            None => {
                ui.spinner();
                return;
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).size(12.5).color(theme::alt()));
                return;
            }
            Some(Ok(d)) => d,
        };
        if d.lines.is_empty() {
            ui.label(RichText::new("No lines to show: the file only changed its mode, or is empty.").size(12.5).color(theme::text()));
            return;
        }
        let font = FontId::monospace(MONO);
        let (row_h, cw) = ui.fonts_mut(|g| (g.row_height(&font) + 3.0, g.glyph_width(&font, '0')));
        let digits = d.lines.iter().filter_map(|l| l.old.max(l.new)).max().unwrap_or(1).to_string().len().max(3) as f32;
        let gutter = cw * (digits * 2.0 + 3.0) + 16.0;
        let width = (gutter + cw * (d.widest as f32 + 2.0)).max(ui.available_width());
        egui::ScrollArea::both().id_salt("git-diff").auto_shrink([false, false]).show_rows(ui, row_h, d.lines.len(), |ui, range| {
            for l in &d.lines[range] {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, row_h), Sense::hover());
                let p = ui.painter();
                let (bg, fg, mark) = match l.kind {
                    Kind::Add => (theme::ok().gamma_multiply(0.16), theme::text_strong(), "+"),
                    Kind::Del => (theme::alt().gamma_multiply(0.16), theme::text_strong(), "−"),
                    Kind::Hunk => (theme::primary().gamma_multiply(0.10), theme::primary(), ""),
                    Kind::Note => (Color32::TRANSPARENT, theme::muted(), ""),
                    Kind::Same => (Color32::TRANSPARENT, theme::text(), ""),
                };
                if bg != Color32::TRANSPARENT {
                    p.rect_filled(rect, 0.0, bg);
                }
                let y = rect.center().y;
                let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
                let nc = theme::muted();
                p.text(egui::pos2(rect.left() + cw * digits + 4.0, y), egui::Align2::RIGHT_CENTER, num(l.old), font.clone(), nc);
                p.text(egui::pos2(rect.left() + cw * (digits * 2.0 + 1.0) + 8.0, y), egui::Align2::RIGHT_CENTER, num(l.new), font.clone(), nc);
                let mark_c = if l.kind == Kind::Add { theme::ok() } else { theme::alt() };
                p.text(egui::pos2(rect.left() + gutter - cw * 1.5, y), egui::Align2::LEFT_CENTER, mark, font.clone(), mark_c);
                p.text(egui::pos2(rect.left() + gutter, y), egui::Align2::LEFT_CENTER, &l.text, font.clone(), fg);
            }
        });
    }
}

/// A plain button with an arrow, on a phone, back one pane.
fn back(ui: &mut egui::Ui, word: &str) -> bool {
    let b = ui.add(egui::Button::new(RichText::new(word).size(14.0)).min_size(egui::vec2(0.0, 36.0))).clicked();
    ui.add_space(4.0);
    b
}

fn first_line(s: &str) -> &str {
    s.lines().next().unwrap_or("")
}

/// git's letter for what happened to a file, and its colour.
fn status_mark(s: &str) -> (&'static str, Color32) {
    match s {
        "A" => ("A", theme::ok()),
        "D" => ("D", theme::alt()),
        "R" => ("R", theme::amber()),
        "C" => ("C", theme::amber()),
        "?" => ("?", theme::warn()),
        "T" => ("T", theme::muted()),
        _ => ("M", theme::primary()),
    }
}

/// A unified diff into lines to draw. Headers before the first hunk are
/// dropped but for what a person needs (binary, new or deleted, renamed);
/// each hunk restarts the numbers at its "@@ -old +new @@".
fn parse(diff: &str) -> Diff {
    let mut lines = Vec::new();
    let (mut o, mut n, mut in_hunk) = (0u32, 0u32, false);
    for raw in diff.lines() {
        let l = raw.replace('\t', "    ");
        if l.starts_with("@@") {
            let mut t = l.split_whitespace().skip(1);
            let start = |w: Option<&str>| w.and_then(|w| w[1..].split(',').next()?.parse::<u32>().ok()).unwrap_or(1);
            o = start(t.next());
            n = start(t.next());
            in_hunk = true;
            lines.push(Line { kind: Kind::Hunk, old: None, new: None, text: l });
            continue;
        }
        if l.starts_with("diff --git") {
            in_hunk = false;
            continue;
        }
        if !in_hunk {
            let said = ["Binary files", "new file mode", "deleted file mode", "rename from", "rename to", "old mode", "new mode"];
            if said.iter().any(|s| l.starts_with(s)) {
                lines.push(Line { kind: Kind::Note, old: None, new: None, text: l });
            }
            continue;
        }
        let (kind, old, new, text) = match l.chars().next() {
            Some('+') => {
                n += 1;
                (Kind::Add, None, Some(n - 1), l[1..].to_owned())
            }
            Some('-') => {
                o += 1;
                (Kind::Del, Some(o - 1), None, l[1..].to_owned())
            }
            Some('\\') => (Kind::Note, None, None, l),
            _ => {
                o += 1;
                n += 1;
                (Kind::Same, Some(o - 1), Some(n - 1), l.get(1..).unwrap_or("").to_owned())
            }
        };
        lines.push(Line { kind, old, new, text });
    }
    let widest = lines.iter().map(|l| l.text.chars().count()).max().unwrap_or(0);
    Diff { lines, widest }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_diff_is_numbered_on_both_sides() {
        let d = parse("diff --git a/x b/x\nindex 1..2 100644\n--- a/x\n+++ b/x\n@@ -3,3 +3,4 @@ fn f\n a\n-b\n+B\n+C\n c\n\\ No newline at end of file\n");
        let k: Vec<_> = d.lines.iter().map(|l| (l.kind, l.old, l.new, l.text.as_str())).collect();
        assert_eq!(
            k,
            vec![
                (Kind::Hunk, None, None, "@@ -3,3 +3,4 @@ fn f"),
                (Kind::Same, Some(3), Some(3), "a"),
                (Kind::Del, Some(4), None, "b"),
                (Kind::Add, None, Some(4), "B"),
                (Kind::Add, None, Some(5), "C"),
                (Kind::Same, Some(5), Some(6), "c"),
                (Kind::Note, None, None, "\\ No newline at end of file"),
            ]
        );
        assert_eq!(d.widest, "\\ No newline at end of file".len(), "the note is the widest line");
    }

    #[test]
    fn a_new_and_a_binary_file_say_so() {
        let d = parse("diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+one\n+two\ndiff --git a/p.png b/p.png\nBinary files a/p.png and b/p.png differ\n");
        let k: Vec<_> = d.lines.iter().map(|l| (l.kind, l.new)).collect();
        assert_eq!(k, vec![(Kind::Note, None), (Kind::Hunk, None), (Kind::Add, Some(1)), (Kind::Add, Some(2)), (Kind::Note, None)]);
    }
}
