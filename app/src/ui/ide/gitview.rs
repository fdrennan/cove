//! The Git window (operator, 2026-10-30: "just create a basic git viewer in
//! cove instead for now, think pycharm"; 2026-11-25: "a refined, compact Git
//! diff and patch viewer that feels like a natural extension of the
//! editor"): the project's versions down the left; the files the picked one
//! changed as a folder tree; and the picked file's diff - side by side,
//! unified or inline - with its changes to step through, unchanged code
//! folded, an overview ruler, and stage, unstage, revert, copy and export
//! at the file or the hunk. On a phone the three come one at a time.
//!
//! This file is the interaction: what is picked, what is asked of git, what
//! a click does. Parsing and laying out a diff are project/diff; drawing it
//! is gitview/render, gitview/ruler and gitview/tree; git itself runs in
//! host/git.rs.

mod render;
mod ruler;
mod tree;

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, FontId, RichText, Sense};

use crate::api::{GitFile, Query, Version};
use crate::backend::Backend;
use crate::project::diff::model::{self, Folds, Model};
use crate::project::diff::patch::{self, FilePatch};
use crate::ui::kit::bar::{self, Weight};
use crate::ui::kit::icons::ctl;
use crate::ui::kit::theme;
use render::{FoldAct, HunkAct, Intent, Mode};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

/// What the window asks of its owner, which holds the history and Restore.
pub enum GitAct {
    None,
    Restore(String),
    Refresh,
}

/// A file's diff, read once: the patch and its model.
pub struct Loaded {
    file: FilePatch,
    model: Model,
}

/// What a revert waits on: the person saying yes.
#[derive(Clone, Copy, PartialEq)]
enum Target {
    File,
    Hunk(usize),
}

/// The Apply a patch dialog.
#[derive(Default)]
struct Apply {
    text: String,
    said: Slot<String>,
    opened: Arc<Mutex<Option<String>>>,
}

#[derive(Default)]
pub struct GitView {
    host: String,
    /// The version picked: "" the changes not saved yet.
    sha: Option<String>,
    files: Slot<Vec<GitFile>>,
    file: Option<GitFile>,
    diff: Slot<Loaded>,
    /// On a phone, which pane shows: 0 the history, 1 the files, 2 the diff.
    step: u8,
    confirm: Option<String>,
    tree: tree::Tree,
    mode: Option<Mode>,
    folds: Folds,
    /// The change the navigation is at.
    current: Option<usize>,
    /// A scroll the page takes next frame, and where it was last.
    jump: Option<f32>,
    view: (f32, f32, f32),
    /// The last stage, unstage, revert or apply, as git said it.
    said: Slot<String>,
    revert: Option<Target>,
    apply: Option<Apply>,
    /// After an action: find the file again when the list comes back, in
    /// whichever part it is now.
    follow: bool,
}

const LOG_ROW: f32 = 46.0;
const MODE_KEY: &str = "git-diff-mode";

impl GitView {
    /// The window's body. `versions` is the project's history, newest
    /// first; `said` the last restore's word.
    pub fn ui(&mut self, ui: &mut egui::Ui, backend: &Backend, host: &str, versions: &Option<Result<Vec<Version>, String>>, said: &Option<Result<String, String>>) -> GitAct {
        let ctx = ui.ctx().clone();
        if self.host != host {
            *self = GitView { host: host.to_owned(), ..Default::default() };
        }
        if self.mode.is_none() {
            self.mode = Some(ctx.data_mut(|d| d.get_persisted::<Mode>(egui::Id::new(MODE_KEY))).unwrap_or_default());
        }
        // The unsaved changes are where a person starts, as in PyCharm.
        if self.sha.is_none() {
            self.pick(Some(String::new()), backend, &ctx);
        }
        self.follow_file(backend, &ctx);
        self.apply_window(&ctx, backend);
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
        #[cfg(target_arch = "wasm32")]
        web_sys::console::log_1(&format!("DIAG avail={:?} clip={:?} max={:?}", ui.available_rect_before_wrap(), ui.clip_rect(), ui.max_rect()).into());
        let narrow = ui.available_width() < 760.0;
        if narrow {
            match self.step {
                0 => theme::pane(ui, |ui| self.log_pane(ui, list, backend, &ctx, &mut act)),
                1 => {
                    if back(ui, "← history") {
                        self.step = 0;
                    }
                    theme::pane(ui, |ui| self.files_pane(ui, list, backend, &ctx, &mut act));
                }
                _ => {
                    if back(ui, "← files") {
                        self.step = 1;
                    }
                    theme::pane(ui, |ui| self.diff_pane(ui, backend, &ctx, true));
                }
            }
            return act;
        }
        let w = ui.available_width();
        // Three windows as the IDE's are: rounded panes, each headed by its
        // own bar, a gap between (operator, 2026-11-26: "the git window
        // should be more beautiful ... how the bars look").
        let gap = (theme::WIN_GAP / 2) as i8;
        let side = egui::Frame::NONE.inner_margin(egui::Margin { left: 0, right: gap, top: 0, bottom: 2 });
        egui::Panel::left("git-log").resizable(true).show_separator_line(false).frame(side).default_size((w * 0.22).clamp(220.0, 340.0)).show(ui, |ui| theme::pane(ui, |ui| self.log_pane(ui, list, backend, &ctx, &mut act)));
        let mid = egui::Frame::NONE.inner_margin(egui::Margin { left: gap, right: gap, top: 0, bottom: 2 });
        egui::Panel::left("git-files").resizable(true).show_separator_line(false).frame(mid).default_size((w * 0.2).clamp(210.0, 320.0)).show(ui, |ui| theme::pane(ui, |ui| self.files_pane(ui, list, backend, &ctx, &mut act)));
        let last = egui::Frame::NONE.inner_margin(egui::Margin { left: gap, right: 0, top: 0, bottom: 2 });
        egui::CentralPanel::default().frame(last).show(ui, |ui| theme::pane(ui, |ui| self.diff_pane(ui, backend, &ctx, false)));
        act
    }

    /// Pick a version ("" the unsaved changes): its files are asked for -
    /// for "", in their parts, staged and not - and the diff waits for a file.
    fn pick(&mut self, sha: Option<String>, backend: &Backend, ctx: &egui::Context) {
        self.sha = sha.clone();
        self.file = None;
        self.confirm = None;
        self.revert = None;
        *self.diff.lock().unwrap() = None;
        self.ask_files(backend, ctx);
    }

    fn ask_files(&self, backend: &Backend, ctx: &egui::Context) {
        let Some(sha) = self.sha.clone() else { return };
        let slot = self.files.clone();
        *slot.lock().unwrap() = None;
        let q = if sha.is_empty() { Query::GitStatus { host: self.host.clone() } } else { Query::GitChanges { host: self.host.clone(), sha } };
        backend.query(q, ctx, move |r: Result<Vec<GitFile>, String>| {
            *slot.lock().unwrap() = Some(r);
        });
    }

    fn pick_file(&mut self, f: GitFile, backend: &Backend, ctx: &egui::Context) {
        let slot = self.diff.clone();
        *slot.lock().unwrap() = None;
        let q = Query::GitDiff { host: self.host.clone(), sha: self.sha.clone().unwrap_or_default(), path: f.path.clone(), old: f.old.clone(), area: f.area.clone(), full: true };
        if self.file.as_ref().is_none_or(|x| x.path != f.path || x.area != f.area) {
            self.folds = Folds::default();
            self.current = None;
            self.jump = Some(0.0);
        }
        self.revert = None;
        self.file = Some(f);
        backend.query(q, ctx, move |r: Result<String, String>| {
            *slot.lock().unwrap() = Some(r.map(|t| {
                let file = patch::parse(&t).into_iter().next().unwrap_or_default();
                let model = model::build(&file);
                Loaded { file, model }
            }));
        });
    }

    /// After an action the list comes back new: the file shown stays shown,
    /// in the part it is in now (all of it staged, it is under Staged).
    fn follow_file(&mut self, backend: &Backend, ctx: &egui::Context) {
        if !self.follow {
            return;
        }
        let Some(Ok(files)) = self.files.lock().unwrap().clone() else { return };
        self.follow = false;
        let Some(cur) = self.file.clone() else { return };
        let same = files.iter().find(|f| f.path == cur.path && f.area == cur.area).or_else(|| files.iter().find(|f| f.path == cur.path)).cloned();
        match same {
            Some(f) => self.pick_file(f, backend, ctx),
            None => {
                self.file = None;
                *self.diff.lock().unwrap() = None;
            }
        }
    }

    /// A patch put to work, then the list and the diff read again.
    fn run(&mut self, action: &str, patch: String, backend: &Backend, ctx: &egui::Context) {
        let said = self.said.clone();
        *said.lock().unwrap() = None;
        self.revert = None;
        self.follow = true;
        let (files, host, sha) = (self.files.clone(), self.host.clone(), self.sha.clone().unwrap_or_default());
        let (b2, c2) = (backend.clone(), ctx.clone());
        backend.query(Query::GitPatch { host: self.host.clone(), action: action.to_owned(), patch }, ctx, move |r: Result<String, String>| {
            *said.lock().unwrap() = Some(r);
            *files.lock().unwrap() = None;
            let q = if sha.is_empty() { Query::GitStatus { host } } else { Query::GitChanges { host, sha } };
            b2.query(q, &c2, move |r: Result<Vec<GitFile>, String>| {
                *files.lock().unwrap() = Some(r);
            });
        });
    }

    /// What can be done to this diff's hunks, by where it came from.
    fn acts(&self) -> Vec<HunkAct> {
        let area = self.file.as_ref().map_or("", |f| f.area.as_str());
        match (self.sha.as_deref().unwrap_or(""), area) {
            ("", "staged") => vec![HunkAct::Unstage, HunkAct::Copy],
            ("", _) => vec![HunkAct::Stage, HunkAct::Revert, HunkAct::Copy],
            // A version's change, taken back out of the files as they are.
            _ => vec![HunkAct::Revert, HunkAct::Copy],
        }
    }

    /// The history: the unsaved changes, then every version, a line of dots
    /// down the left joining them as PyCharm's log does.
    fn log_pane(&mut self, ui: &mut egui::Ui, list: &[Version], backend: &Backend, ctx: &egui::Context, act: &mut GitAct) {
        let mut tb = bar::Toolbar::top(ui);
        if bar::glyph(tb.right(), crate::ui::kit::icons::Glyph::Refresh, "read the history and the changes again", true).clicked() {
            *act = GitAct::Refresh;
            let sha = self.sha.clone();
            self.pick(sha, backend, ctx);
        }
        bar::title(&mut tb.left(), "History", &list.len().to_string());
        let now = crate::ui::software::now_secs();
        let rows = list.len() + 1;
        let mut picked = None;
        egui::ScrollArea::vertical().id_salt("git-log").auto_shrink([false, false]).show_rows(ui, LOG_ROW, rows, |ui, range| {
            for i in range {
                let (sha, title, who, short) = if i == 0 {
                    (String::new(), "Changes not saved yet".to_owned(), "saved within 30 minutes, or by Save".to_owned(), String::new())
                } else {
                    let v = &list[i - 1];
                    (v.sha.clone(), first_line(&v.message).to_owned(), format!("{} · {} ago", v.author, crate::ui::software::fmt_dur((now - v.when).max(0))), v.sha[..v.sha.len().min(7)].to_owned())
                };
                let on = self.sha.as_deref() == Some(sha.as_str());
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), LOG_ROW), Sense::click());
                let p = ui.painter();
                // Picked: a soft wash and the accent along its edge, as the
                // tab in front is marked - not a block of colour.
                if on {
                    p.rect_filled(rect, 0.0, theme::primary().gamma_multiply(0.08));
                    p.vline(rect.left() + 1.0, rect.y_range(), egui::Stroke::new(2.0, theme::primary()));
                } else if resp.hovered() {
                    p.rect_filled(rect, 0.0, theme::surface());
                }
                // The lane: a hairline through every version, a dot on each.
                let x = rect.left() + 16.0;
                let top = if i == 0 { rect.center().y } else { rect.top() };
                let bottom = if i + 1 == rows { rect.center().y } else { rect.bottom() };
                if rows > 1 {
                    p.line_segment([egui::pos2(x, top), egui::pos2(x, bottom)], egui::Stroke::new(1.5, theme::lift(theme::moss(), 0.12)));
                }
                let c = egui::pos2(x, rect.center().y);
                if i == 0 {
                    p.circle(c, 4.5, theme::bg(), egui::Stroke::new(1.5, theme::amber()));
                } else {
                    p.circle_filled(c, 4.5, theme::bg());
                    p.circle_filled(c, 3.2, if on { theme::primary() } else { theme::primary().gamma_multiply(0.7) });
                }
                let tx = rect.left() + 32.0;
                let right = rect.right() - 10.0;
                let sha_g = ui.painter().layout_no_wrap(short, FontId::monospace(10.5), theme::hush());
                let mut job = egui::text::LayoutJob::single_section(title, egui::TextFormat::simple(FontId::proportional(13.0), if i == 0 { theme::amber() } else if on { theme::text_strong() } else { theme::text() }));
                job.wrap = egui::text::TextWrapping::truncate_at_width(right - tx);
                let title_g = ui.painter().layout_job(job);
                let mut sub = egui::text::LayoutJob::single_section(who, egui::TextFormat::simple(FontId::proportional(11.5), theme::muted()));
                sub.wrap = egui::text::TextWrapping::truncate_at_width((right - tx - sha_g.size().x - 8.0).max(20.0));
                let sub_g = ui.painter().layout_job(sub);
                let p = ui.painter().with_clip_rect(rect);
                p.galley(egui::pos2(tx, rect.top() + 7.0), title_g, Color32::WHITE);
                let sub_y = rect.top() + 26.0;
                p.galley(egui::pos2(tx, sub_y), sub_g, Color32::WHITE);
                p.galley(egui::pos2(right - sha_g.size().x, sub_y + 1.0), sha_g, Color32::WHITE);
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

    /// The picked version: its message, whose and when, Restore; then the
    /// files it changed as a tree.
    fn files_pane(&mut self, ui: &mut egui::Ui, list: &[Version], backend: &Backend, ctx: &egui::Context, act: &mut GitAct) {
        let sha = self.sha.clone().unwrap_or_default();
        let v = list.iter().position(|v| v.sha == sha).map(|k| (k, &list[k]));
        let count = match &*self.files.lock().unwrap() {
            Some(Ok(f)) => f.len().to_string(),
            _ => String::new(),
        };
        let mut tb = bar::Toolbar::top(ui);
        match v {
            Some((k, v)) => {
                // The newest is what is there now; any other can come back,
                // asked twice: Restore, then Restore it.
                if k > 0 {
                    let r = tb.right();
                    if self.confirm.as_deref() == Some(sha.as_str()) {
                        if bar::button(r, &ctl::REVERT.tip("what is there now is saved as a version first, so nothing is lost"), Weight::Primary).clicked() {
                            *act = GitAct::Restore(sha.clone());
                            self.confirm = None;
                        }
                        if bar::glyph(r, crate::ui::kit::icons::Glyph::Close, "keep things as they are", true).clicked() {
                            self.confirm = None;
                        }
                    } else if bar::glyph(r, crate::ui::kit::icons::Glyph::Undo, "Restore: put this version back - what is there now is saved as a version first", true).clicked() {
                        self.confirm = Some(sha.clone());
                    }
                }
                bar::title(&mut tb.left(), &v.sha[..v.sha.len().min(7)], &count);
                // The version: its message and whose and when, under the bar.
                egui::Frame::NONE.inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&v.message).size(13.0).color(theme::text_strong())).wrap());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} · {} UTC", v.author, crate::ui::software::fmt_date(v.when))).size(11.5).color(theme::muted()));
                        if ui.add(egui::Label::new(RichText::new("copy id").size(11.5).color(theme::primary())).sense(Sense::click())).on_hover_text(v.sha.clone()).clicked() {
                            ui.ctx().copy_text(v.sha.clone());
                        }
                    });
                    if self.confirm.as_deref() == Some(sha.as_str()) {
                        ui.label(RichText::new("Restore puts this version back. What is there now is saved as a version first.").size(12.0).color(theme::warn()));
                    }
                });
                ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), egui::Stroke::new(1.0, theme::pane_brd()));
            }
            None => {
                if bar::button(tb.right(), &ctl::APPLY_PATCH, Weight::Icon).clicked() {
                    self.apply = Some(Apply::default());
                }
                bar::title(&mut tb.left(), "Changes", &count);
            }
        }
        ui.add_space(4.0);
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
        let picked = self.file.as_ref().map(|f| (f.area.clone(), f.path.clone()));
        if let Some(i) = self.tree.show(ui, &files, picked.as_ref().map(|(a, p)| (a.as_str(), p.as_str()))) {
            self.pick_file(files[i].clone(), backend, ctx);
            self.step = 2;
        }
    }

    /// The picked file's diff: one bar over it, the page, the ruler beside.
    fn diff_pane(&mut self, ui: &mut egui::Ui, backend: &Backend, ctx: &egui::Context, narrow: bool) {
        let Some(f) = self.file.clone() else {
            ui.centered_and_justified(|ui| {
                ui.label(RichText::new("Pick a file to see what changed in it.").size(13.0).color(theme::text()));
            });
            return;
        };
        let loaded = self.diff.clone();
        let guard = loaded.lock().unwrap();
        let d = match guard.as_ref() {
            None => {
                drop(guard);
                ui.spinner();
                return;
            }
            Some(Err(e)) => {
                let e = e.clone();
                drop(guard);
                ui.label(RichText::new(e).size(12.5).color(theme::alt()));
                return;
            }
            Some(Ok(d)) => d,
        };
        // A side-by-side diff does not fit a phone.
        let mode = match self.mode.unwrap_or_default() {
            Mode::Split if narrow => Mode::Unified,
            m => m,
        };
        let acts = self.acts();
        let file_patch = patch::write(&d.file, &d.file.hunks);
        let mut ask: Option<(&'static str, String)> = None;

        // ── the bar ──
        let mut tb = bar::Toolbar::top(ui);
        {
            let ui = tb.right();
            bar::more(ui, "more for this file", |ui| {
                if bar::menu_item(ui, &ctl::COPY_PATCH, !d.file.hunks.is_empty()) {
                    ui.ctx().copy_text(file_patch.clone());
                }
                if bar::menu_item(ui, &ctl::EXPORT_PATCH, !d.file.hunks.is_empty()) {
                    let name = format!("{}.patch", f.path.rsplit('/').next().unwrap_or("change"));
                    crate::ui::kit::pick::save_text(&name, file_patch.clone());
                }
                if acts.contains(&HunkAct::Revert) && bar::menu_item(ui, &ctl::REVERT.tip("take every change in this file out of it"), !d.file.hunks.is_empty()) {
                    self.revert = Some(Target::File);
                }
            });
            if acts.contains(&HunkAct::Stage) && bar::button_tip(ui, !d.file.hunks.is_empty(), &ctl::STAGE, Weight::Secondary, "stage the whole file").clicked() {
                ask = Some(("stage", file_patch.clone()));
            }
            if acts.contains(&HunkAct::Unstage) && bar::button_tip(ui, !d.file.hunks.is_empty(), &ctl::UNSTAGE, Weight::Secondary, "unstage the whole file").clicked() {
                ask = Some(("unstage", file_patch.clone()));
            }
            theme::tool_sep(ui);
            if bar::toggle(ui, &ctl::UNFOLD, self.folds.all, Weight::Icon).clicked() {
                self.folds.all = !self.folds.all;
            }
            let n = d.model.changes.len();
            if bar::button_if(ui, n > 0, &ctl::NEXT_CHANGE, Weight::Icon).clicked() {
                self.step_change(true, &d.model);
            }
            if bar::button_if(ui, n > 0, &ctl::PREV_CHANGE, Weight::Icon).clicked() {
                self.step_change(false, &d.model);
            }
            if n > 0 {
                let word = match self.current {
                    Some(c) => format!("{} of {n}", c + 1),
                    None if n == 1 => "1 change".to_owned(),
                    None => format!("{n} changes"),
                };
                ui.label(RichText::new(word).size(11.5).color(theme::hush()));
            }
            theme::tool_sep(ui);
            for (c, m) in [(&ctl::DIFF_INLINE, Mode::Inline), (&ctl::DIFF_UNIFIED, Mode::Unified), (&ctl::DIFF_SPLIT, Mode::Split)] {
                if m == Mode::Split && narrow {
                    continue;
                }
                if bar::toggle(ui, c, mode == m, Weight::Icon).clicked() && mode != m {
                    self.mode = Some(m);
                    ctx.data_mut(|dd| dd.insert_persisted(egui::Id::new(MODE_KEY), m));
                    self.jump = Some(0.0);
                }
            }
        }
        {
            let mut left = tb.left();
            let (letter, c) = tree::status_mark(&f.status);
            left.label(RichText::new(letter).monospace().size(12.0).color(c));
            let (dir, name) = f.path.rsplit_once('/').map_or(("", f.path.as_str()), |(d, n)| (d, n));
            let mut job = egui::text::LayoutJob::default();
            if !dir.is_empty() {
                job.append(&format!("{dir}/"), 0.0, egui::TextFormat::simple(FontId::monospace(12.0), theme::muted()));
            }
            job.append(name, 0.0, egui::TextFormat::simple(FontId::monospace(12.0), theme::text_strong()));
            left.add(egui::Label::new(job).truncate()).on_hover_text(if f.old.is_empty() { f.path.clone() } else { format!("{} ← {}", f.path, f.old) });
            if !f.old.is_empty() {
                left.label(RichText::new(format!("← {}", f.old)).monospace().size(11.0).color(theme::hush()));
            }
            left.label(RichText::new(format!("+{}", d.model.added)).monospace().size(11.0).color(theme::ok().gamma_multiply(0.85)));
            left.label(RichText::new(format!("−{}", d.model.removed)).monospace().size(11.0).color(theme::alt().gamma_multiply(0.85)));
        }

        // ── what git said, and a revert waiting for yes ──
        let said = self.said.lock().unwrap().clone();
        if let Some(r) = &said {
            let (t, c) = match r {
                Ok(m) => (format!("✓ {m}"), theme::ok()),
                Err(e) => (e.clone(), theme::alt()),
            };
            ui.add(egui::Label::new(RichText::new(t).size(12.0).color(c)).truncate());
        }
        if let Some(t) = self.revert {
            let what = match t {
                Target::File => format!("every change in {}", f.path),
                Target::Hunk(h) => format!("the change at line {}", d.model.hunks.get(h).map_or(0, |h| h.new_start)),
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("Take {what} out of the file? Its last saved version keeps it.")).size(12.5).color(theme::warn()));
                if ui.add(theme::primary_button("Revert")).clicked() {
                    let p = match t {
                        Target::File => file_patch.clone(),
                        Target::Hunk(h) => patch::write(&d.file, &d.model.hunks[h..=h]),
                    };
                    ask = Some(("revert", p));
                }
                if ui.button("Cancel").clicked() {
                    self.revert = None;
                }
            });
        }

        // ── the page and its ruler ──
        if d.model.rows.is_empty() {
            ui.add_space(8.0);
            for n in d.file.notes.iter().chain(d.file.binary.then_some(&String::new())) {
                if !n.is_empty() {
                    ui.label(RichText::new(n).monospace().size(12.0).color(theme::text()));
                }
            }
            if d.file.notes.is_empty() {
                ui.label(RichText::new("Nothing to show here now.").size(12.5).color(theme::text()));
            }
            drop(guard);
            if let Some((a, p)) = ask {
                self.run(a, p, backend, ctx);
            }
            return;
        }
        let lines = render::lines(&d.model, &self.folds, mode);
        let look = render::Look::of(&f.path);
        let row_h = render::Metrics::of(ui, &d.model).row_h;
        // Keys: F7 and Alt+↓ the next change, Shift+F7 and Alt+↑ the one before.
        if ctx.memory(|m| m.focused().is_none()) {
            let (next, prev) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::F7) || i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowDown),
                    i.consume_key(egui::Modifiers::SHIFT, egui::Key::F7) || i.consume_key(egui::Modifiers::ALT, egui::Key::ArrowUp),
                )
            });
            if next || prev {
                self.step_change(next, &d.model);
            }
        }
        // A change stepped to: its line a quarter down the view.
        if let Some(c) = self.current.filter(|_| self.jump.is_some_and(|j| j < 0.0)) {
            let li = d.model.changes.get(c).and_then(|ch| render::line_of_row(&lines, ch.rows.start)).unwrap_or(0);
            self.jump = Some((li as f32 * row_h - self.view.1 * 0.25).max(0.0));
        }
        let full = ui.available_rect_before_wrap();
        let page_rect = full.with_max_x(full.right() - ruler::WIDTH);
        let ruler_rect = full.with_min_x(full.right() - ruler::WIDTH);
        let jump = self.jump.take();
        let mut page_ui = ui.new_child(egui::UiBuilder::new().max_rect(page_rect).layout(egui::Layout::top_down(egui::Align::Min)));
        let (intent, view) = render::page(&mut page_ui, &d.model, &lines, mode, &look, &acts, jump, self.current);
        self.view = view;
        if let Some(y) = ruler::show(ui, ruler_rect, &d.model, &lines, row_h, view) {
            self.jump = Some(y);
        }
        ui.allocate_rect(full, Sense::hover());
        match intent {
            Some(Intent::Hunk(h, a)) => {
                let p = patch::write(&d.file, &d.model.hunks[h..=h]);
                match a {
                    HunkAct::Stage => ask = Some(("stage", p)),
                    HunkAct::Unstage => ask = Some(("unstage", p)),
                    HunkAct::Revert => self.revert = Some(Target::Hunk(h)),
                    HunkAct::Copy => ctx.copy_text(p),
                }
            }
            Some(Intent::Fold(run, a)) => {
                let e = self.folds.open.entry(run).or_insert((0, 0));
                match a {
                    FoldAct::Up => e.0 += render::FOLD_STEP,
                    FoldAct::Down => e.1 += render::FOLD_STEP,
                    FoldAct::All => *e = (usize::MAX / 4, usize::MAX / 4),
                }
            }
            None => {}
        }
        drop(guard);
        if let Some((a, p)) = ask {
            self.run(a, p, backend, ctx);
        }
    }

    /// To the next change (or the one before), round the file; the page
    /// scrolls to it next frame.
    fn step_change(&mut self, forward: bool, m: &Model) {
        let from = self.current.and_then(|c| m.changes.get(c)).map_or(if forward { 0 } else { usize::MAX }, |c| c.rows.start);
        let from = if self.current.is_none() && forward { 0 } else { from };
        let next = if self.current.is_none() && forward { (!m.changes.is_empty()).then_some(0) } else { model::next_change(m, from, forward) };
        if let Some(c) = next {
            self.current = Some(c);
            // Marked to be placed once the lines are laid out (diff_pane).
            self.jump = Some(-1.0);
        }
    }

    /// Apply a patch: pasted, or opened from a file; checked, then applied
    /// to the project's files.
    fn apply_window(&mut self, ctx: &egui::Context, backend: &Backend) {
        let Some(a) = self.apply.as_mut() else { return };
        if let Some(t) = a.opened.lock().unwrap().take() {
            a.text = t;
        }
        let mut open = true;
        let mut go: Option<&'static str> = None;
        egui::Window::new("Apply a patch")
            .id(egui::Id::new("git-apply"))
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .default_size(egui::vec2(640.0, 420.0))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if bar::button(ui, &ctl::APPLY_PATCH.tip("open a .patch or .diff file"), Weight::Secondary).clicked() {
                        let slot = a.opened.clone();
                        let pick = async move {
                            let Some(f) = crate::ui::kit::pick::files("Open a patch", false).await.and_then(|v| v.into_iter().next()) else { return };
                            *slot.lock().unwrap() = Some(String::from_utf8_lossy(&f.read().await).into_owned());
                        };
                        #[cfg(not(target_arch = "wasm32"))]
                        std::thread::spawn(move || pollster::block_on(pick));
                        #[cfg(target_arch = "wasm32")]
                        wasm_bindgen_futures::spawn_local(pick);
                    }
                    let files = patch::parse(&a.text);
                    if !files.is_empty() {
                        let names: Vec<&str> = files.iter().map(|f| f.name()).collect();
                        ui.add(egui::Label::new(RichText::new(names.join(" · ")).monospace().size(11.5).color(theme::muted())).truncate());
                    }
                });
                egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                    ui.add(egui::TextEdit::multiline(&mut a.text).font(FontId::monospace(12.0)).desired_width(f32::INFINITY).desired_rows(14).hint_text("Paste a patch - git diff's output, or a .patch file's"));
                });
                if let Some(r) = a.said.lock().unwrap().clone() {
                    let (t, c) = match r {
                        Ok(m) => (format!("✓ {m}"), theme::ok()),
                        Err(e) => (e, theme::alt()),
                    };
                    ui.label(RichText::new(t).size(12.5).color(c));
                }
                ui.horizontal(|ui| {
                    let some = !a.text.trim().is_empty();
                    if ui.add_enabled(some, egui::Button::new("Check")).on_hover_text("would it apply to the files as they are?").clicked() {
                        go = Some("check");
                    }
                    if ui.add_enabled(some, theme::primary_button("Apply")).on_hover_text("apply it to the project's files; stage it after, as any change").clicked() {
                        go = Some("apply");
                    }
                });
            });
        if let Some(action) = go {
            let slot = a.said.clone();
            *slot.lock().unwrap() = None;
            let text = a.text.clone();
            if action == "apply" {
                self.follow = true;
            }
            let (files, host) = (self.files.clone(), self.host.clone());
            let (b2, c2) = (backend.clone(), ctx.clone());
            backend.query(Query::GitPatch { host: self.host.clone(), action: action.into(), patch: text }, ctx, move |r: Result<String, String>| {
                let ok = r.is_ok();
                *slot.lock().unwrap() = Some(r);
                if ok && action == "apply" {
                    *files.lock().unwrap() = None;
                    b2.query(Query::GitStatus { host }, &c2, move |r: Result<Vec<GitFile>, String>| {
                        *files.lock().unwrap() = Some(r);
                    });
                }
            });
        }
        if !open {
            self.apply = None;
        }
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

