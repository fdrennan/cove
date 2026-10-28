//! Packages, the mode: a status and a live mode for every step of the
//! software supply chain (operator, 2026-10-24: "focus on improves status
//! and live modes for all steps in the software supply chain"; "a missing
//! package should look like a visible gap ... not an orange sentence buried
//! in a table"; "never conflate published, synchronized, installed, and
//! loaded").
//!
//! Top to bottom:
//! - Live: the ndexr operations running on this host and what each build
//!   machine is doing, now.
//! - The steps: planned, built, published, synchronized, installed, module,
//!   loaded - each with how many sets are there, behind or missing, and a
//!   pulse when an operation is moving it.
//! - Every set against every step, one cell each: there, behind or a gap.
//!   A cell opens what is there, what is not, and the operation that closes
//!   it (pkgflow.rs `stages` judges them all; this file only draws).
//! - A set opens its packages; a package, its trace: what it needs, what
//!   needs it, and the modules under it.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, RichText, Sense, Stroke};

use crate::api::{PackageFlow, PfRow, PfSet, PfStage, Query};
use crate::backend::Backend;
use crate::theme;

/// The steps, in order: pkgflow.rs's six stage keys, then loaded (the
/// sessions, which only the root server can count).
const STEPS: [(&str, &str, &str); 7] = [
    ("planned", "Planned", "the kit's manifest"),
    ("built", "Built", "a build machine or this host"),
    ("published", "Published", "S3 · repo.ndexr.io/env"),
    ("synced", "Synchronized", "the publish this host pulled"),
    ("installed", "Installed", "/opt/ndexr/site-library"),
    ("cvmfs", "Module", "env/<set> on CVMFS"),
    ("loaded", "Loaded", "Cove sessions"),
];

#[derive(Default)]
pub struct PkgView {
    slot: Arc<Mutex<Option<Result<PackageFlow, String>>>>,
    asked_at: f64,
    asked: Option<(String, String)>,
    set: String,
    package: String,
    /// The cell open: (set, step key).
    cell: Option<(String, String)>,
    only_diff: bool,
    find: String,
}

fn ago(at: i64) -> String {
    if at <= 0 {
        return "-".into();
    }
    let s = (crate::software::now_secs() - at).max(0);
    match s {
        0..=89 => format!("{s} s"),
        90..=5399 => format!("{} min", s / 60),
        5400..=172_799 => format!("{} h", s / 3600),
        _ => format!("{} days", s / 86400),
    }
}

fn state_color(state: &str) -> Color32 {
    match state {
        "ok" => theme::ok(),
        "behind" => theme::amber(),
        "gap" => theme::alt(),
        _ => theme::moss(),
    }
}

/// The loaded step for a set, from the session counts (none = an admin's view).
fn loaded(f: &PackageFlow, s: &PfSet) -> PfStage {
    let n: u32 = f.sessions.iter().filter(|u| u.module == s.module || u.module == s.set).map(|u| u.sessions).sum();
    let (state, value, note) = if f.sessions.is_empty() {
        ("none", "-".to_owned(), "session counts are an ndexr admin's".to_owned())
    } else if n > 0 {
        ("ok", n.to_string(), format!("{n} Cove sessions run in {}", s.module))
    } else {
        ("none", "0".to_owned(), format!("no Cove session runs in {}", s.module))
    };
    PfStage { key: "loaded".into(), state: state.into(), value, note, fix: String::new() }
}

fn step_of(f: &PackageFlow, s: &PfSet, key: &str) -> PfStage {
    if key == "loaded" {
        return loaded(f, s);
    }
    s.stages.iter().find(|g| g.key == key).cloned().unwrap_or_default()
}

/// An operation running now for this set and step ("" set = every set).
fn live_for<'a>(f: &'a PackageFlow, set: &str, key: &str) -> Option<&'a crate::api::PfLive> {
    f.live.iter().find(|l| l.stage == key && (l.set.is_empty() || l.set == set))
}

/// A package's state across the four host-side steps, kept apart.
fn row_gap(r: &PfRow) -> Option<&'static str> {
    if !r.requested.is_empty() && r.published.is_empty() && r.synced.is_empty() {
        Some("asked for, never published")
    } else if r.local.is_empty() && !(r.synced.is_empty() && r.published.is_empty()) {
        Some("not installed")
    } else if !r.synced.is_empty() && r.local != r.synced {
        Some("installed differs from the synchronized")
    } else if !r.published.is_empty() && !r.synced.is_empty() && r.published != r.synced {
        Some("synchronized differs from the published")
    } else if !r.requested.is_empty() && !r.local.is_empty() && r.requested != r.local {
        Some("installed differs from the asked")
    } else {
        None
    }
}

impl PkgView {
    fn ask(&mut self, backend: &Backend, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let want = (self.set.clone(), self.package.clone());
        let live = self.slot.lock().unwrap().as_ref().is_some_and(|r| r.as_ref().is_ok_and(|f| !f.live.is_empty() || f.live_machines.iter().any(|m| m.active)));
        // Every 5 s while something runs, every 20 s when all is still.
        if self.asked.as_ref() == Some(&want) && now - self.asked_at < if live { 5.0 } else { 20.0 } {
            return;
        }
        self.asked_at = now;
        self.asked = Some(want.clone());
        let slot = self.slot.clone();
        backend.query(Query::PackageFlow { set: want.0, package: want.1 }, ctx, move |r: Result<PackageFlow, String>| {
            *slot.lock().unwrap() = Some(r);
        });
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend) {
        self.ask(backend, ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        let got = self.slot.lock().unwrap().clone();
        let f = match got {
            Some(Ok(f)) => f,
            Some(Err(e)) => {
                ui.label(RichText::new(e).size(13.5).color(theme::alt()));
                return;
            }
            None => {
                ui.label(RichText::new("reading the kit, the build machines, S3, this host and CVMFS …").size(13.5).color(theme::text()));
                return;
            }
        };
        let t = ctx.input(|i| i.time);
        egui::ScrollArea::vertical().id_salt("pkgs").auto_shrink([false, false]).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("PACKAGES").size(12.5).strong().color(theme::primary()));
                let rev = if f.stack_revision.is_empty() { String::new() } else { format!(" · CVMFS revision {}", f.stack_revision) };
                ui.label(RichText::new(format!("{} ({}){rev} · read {} ago", f.host, f.triple, ago(f.at))).size(13.0).color(theme::text()));
            });
            ui.add_space(6.0);
            verdict(ui, ctx, &f);
            ui.add_space(8.0);
            live_strip(ui, &f, t);
            ui.add_space(10.0);
            self.steps(ui, &f, t);
            // The open cell's card above the table, where the eye is: under
            // it, the card fell below a laptop's screen.
            if let Some((set, key)) = self.cell.clone() {
                if let Some(s) = f.sets.iter().find(|s| s.set == set) {
                    ui.add_space(8.0);
                    self.cell_card(ui, ctx, &f, s, &key);
                }
            }
            ui.add_space(10.0);
            self.table(ui, &f, t);
            if let Some(s) = f.sets.iter().find(|s| s.set == self.set) {
                ui.add_space(10.0);
                self.packages(ui, &f, s);
            }
        });
    }

    /// One block per step: how many sets are there, behind, missing; a
    /// pulse when an operation is moving it now.
    fn steps(&mut self, ui: &mut egui::Ui, f: &PackageFlow, t: f64) {
        let n = STEPS.len() as f32;
        let w = ((ui.available_width() - 6.0 * (n - 1.0) - 2.0) / n).max(132.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
            for (key, name, place) in STEPS {
                let states: Vec<String> = f.sets.iter().map(|s| step_of(f, s, key).state).collect();
                let count = |x: &str| states.iter().filter(|s| *s == x).count();
                let (ok, behind, gap) = (count("ok"), count("behind"), count("gap"));
                let live = f.live.iter().any(|l| l.stage == key) || (key == "built" && f.live_machines.iter().any(|m| m.active));
                let worst = if gap > 0 { "gap" } else if behind > 0 { "behind" } else if ok > 0 { "ok" } else { "none" };
                // Top-down inside: the wrapping row around the blocks otherwise
                // laid each block's lines side by side, a letter per line
                // (screenshot, 2026-10-24). Width less margin and stroke.
                ui.allocate_ui_with_layout(egui::vec2(w, 84.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                egui::Frame::new().fill(theme::surface()).stroke(Stroke::new(1.5, state_color(worst))).corner_radius(6).inner_margin(8.0).show(ui, |ui| {
                    ui.set_width(w - 19.0);
                    ui.set_height(64.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(name).size(14.0).strong().color(theme::text_strong()));
                        if live {
                            pulse(ui, t, theme::primary());
                        }
                    });
                    ui.label(RichText::new(place).size(12.0).color(theme::text()));
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if ok > 0 {
                            ui.label(RichText::new(format!("{ok} there")).size(13.0).color(theme::ok()));
                        }
                        if behind > 0 {
                            ui.label(RichText::new(format!("{behind} behind")).size(13.0).color(theme::amber()));
                        }
                        if gap > 0 {
                            ui.label(RichText::new(format!("{gap} missing")).size(13.0).strong().color(theme::alt()));
                        }
                        if ok + behind + gap == 0 {
                            ui.label(RichText::new("-").size(13.0).color(theme::text()));
                        }
                    });
                });
                });
            }
        });
    }

    /// Every set against every step.
    fn table(&mut self, ui: &mut egui::Ui, f: &PackageFlow, t: f64) {
        let name_w = 170.0;
        let cell_w = ((ui.available_width() - name_w) / STEPS.len() as f32).clamp(64.0, 150.0);
        egui::ScrollArea::horizontal().id_salt("pkgs-table").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_sized([name_w, 20.0], egui::Label::new(RichText::new("set").size(12.5).strong().color(theme::text())));
                for (_, name, _) in STEPS {
                    ui.add_sized([cell_w, 20.0], egui::Label::new(RichText::new(name).size(12.5).strong().color(theme::text())));
                }
            });
            let mut group = "";
            for s in &f.sets {
                let g = if s.records > 0 { "language sets" } else { "combined sets: what `ml` loads today" };
                if g != group {
                    group = g;
                    ui.add_space(4.0);
                    ui.label(RichText::new(g.to_uppercase()).size(11.5).strong().color(theme::primary()));
                }
                ui.horizontal(|ui| {
                    let on = self.set == s.set;
                    let b = egui::Button::selectable(on, RichText::new(&s.set).size(13.0).color(theme::text_strong())).min_size(egui::vec2(name_w, 26.0));
                    if ui.add_sized([name_w, 26.0], b).on_hover_text("its packages").clicked() {
                        self.set = if on { String::new() } else { s.set.clone() };
                        self.package.clear();
                    }
                    // A count only where it changes along the chain: a set
                        // that went through untouched reads as one band, and
                        // where it changes, or breaks, is what shows.
                    let mut prev = String::new();
                    for (key, _, _) in STEPS {
                        let st = step_of(f, s, key);
                        let quiet = st.state == "ok" && st.value == prev;
                        prev = st.value.clone();
                        let open = self.cell.as_ref().is_some_and(|(cs, ck)| cs == &s.set && ck == key);
                        if cell(ui, cell_w, &st, quiet, live_for(f, &s.set, key).is_some(), open, t).clicked() {
                            self.cell = if open { None } else { Some((s.set.clone(), key.to_owned())) };
                        }
                    }
                });
            }
        });
    }

    /// What a cell holds: the step's note, its record, and the operation
    /// that closes a gap.
    fn cell_card(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, f: &PackageFlow, s: &PfSet, key: &str) {
        let st = step_of(f, s, key);
        let name = STEPS.iter().find(|x| x.0 == key).map_or(key, |x| x.1);
        egui::Frame::new().fill(theme::surface()).stroke(Stroke::new(1.5, state_color(&st.state))).corner_radius(6).inner_margin(10.0).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{} · {name}", s.set)).font(theme::display(16.0)).color(theme::text_strong()));
                let word = match st.state.as_str() {
                    "ok" => "there",
                    "behind" => "behind",
                    "gap" => "missing",
                    _ => "not expected",
                };
                ui.label(RichText::new(word).size(13.5).strong().color(state_color(&st.state)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("×").clicked() {
                        self.cell = None;
                    }
                });
            });
            ui.add(egui::Label::new(RichText::new(&st.note).size(13.5).color(theme::text())).wrap());
            if let Some(l) = live_for(f, &s.set, key) {
                ui.label(RichText::new(format!("running now: {} (pid {}) for {}", l.what, l.pid, ago(l.since))).size(13.0).color(theme::primary()));
            }
            // The record behind the step.
            let line = |ui: &mut egui::Ui, k: &str, v: String| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(k).size(12.5).color(theme::text()));
                    ui.label(RichText::new(v).monospace().size(12.5).color(theme::text_strong()));
                });
            };
            match key {
                "planned" => line(ui, "manifest sha256", s.manifest_sha256.clone()),
                "built" => {
                    line(ui, "by", format!("{} · {}", s.built.host, s.built.at));
                    line(ui, "manifest", s.built.manifest_sha256.clone());
                    if s.built.failed > 0 {
                        line(ui, "failed", s.built.failed.to_string());
                    }
                    for m in f.live_machines.iter().filter(|m| m.active) {
                        line(ui, "building now", format!("{} · {} · {} of {}", m.name, m.running, m.done, m.total));
                    }
                }
                "published" => {
                    for p in &s.published {
                        if p.found {
                            line(ui, &p.triple, format!("{} packages · {} · {} · ndexr {} · manifest {}", p.packages, p.published_at, p.host, p.cli_version, p.manifest_sha256.get(..12).unwrap_or("")));
                        } else {
                            line(ui, &p.triple, "nothing published".into());
                        }
                    }
                    line(ui, "repo", format!("{}/{}/{}/current.json", f.repo, f.triple, s.set));
                }
                "synced" => line(ui, "from", format!("{} · {} · {} packages · manifest {}", s.synced.host, s.synced.at, s.synced.packages, s.synced.manifest_sha256.get(..12).unwrap_or(""))),
                "installed" => line(ui, "library", format!("{} · {} packages · changed {} ago", s.library, s.local, ago(s.local_at))),
                "cvmfs" => {
                    line(ui, "module", s.module.clone());
                    if !s.load.is_empty() {
                        line(ui, "it sets", s.load.clone());
                    }
                }
                _ => {}
            }
            if !st.fix.is_empty() {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("closes it").size(12.5).strong().color(theme::text()));
                    ui.label(RichText::new(&st.fix).monospace().size(13.0).color(theme::text_strong()));
                    if ui.button("Copy").clicked() {
                        ctx.copy_text(st.fix.clone());
                    }
                });
            }
        });
    }

    /// A set's packages at each host-side step, kept apart; a click traces one.
    fn packages(&mut self, ui: &mut egui::Ui, f: &PackageFlow, s: &PfSet) {
        let rows: Vec<&PfRow> = f.rows.iter().collect();
        let differing = rows.iter().filter(|r| row_gap(r).is_some()).count();
        ui.label(RichText::new(format!("{} · {} packages", s.set, rows.len())).font(theme::display(16.0)).color(theme::text_strong()));
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.only_diff, RichText::new(format!("only the {differing} that differ")).size(13.0));
            ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("find a package").desired_width(180.0));
        });
        if let Some(tr) = f.trace.as_ref().filter(|tr| tr.name == self.package) {
            let mut go: Option<String> = None;
            egui::Frame::new().fill(theme::surface()).stroke(Stroke::new(1.0, theme::pane_brd())).corner_radius(6).inner_margin(10.0).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&tr.name).font(theme::display(16.0)).color(theme::text_strong()));
                    if let Some(r) = rows.iter().find(|r| r.name == tr.name) {
                        ui.label(RichText::new(format!("asked {} · published {} · synchronized {} · installed {}", dash(&r.requested), dash(&r.published), dash(&r.synced), dash(&r.local))).size(13.0).color(theme::text()));
                    }
                });
                if !tr.built.is_empty() {
                    ui.label(RichText::new(format!("its build step here: {}", tr.built)).size(13.0).color(theme::text()));
                }
                ui.label(RichText::new(format!("needs {}", tr.deps.len())).size(12.5).strong().color(theme::text()));
                ui.horizontal_wrapped(|ui| {
                    for (d, v) in &tr.deps {
                        let t = RichText::new(if v.is_empty() { format!("{d} (not in the set)") } else { format!("{d} {v}") }).size(12.5).color(if v.is_empty() { theme::text() } else { theme::text_strong() });
                        if ui.add(egui::Button::new(t)).clicked() && !v.is_empty() {
                            go = Some(d.clone());
                        }
                    }
                });
                ui.label(RichText::new(format!("needed by {}", tr.needed_by.len())).size(12.5).strong().color(theme::text()));
                ui.horizontal_wrapped(|ui| {
                    for d in &tr.needed_by {
                        if ui.button(RichText::new(d).size(12.5)).clicked() {
                            go = Some(d.clone());
                        }
                    }
                });
                ui.label(RichText::new("under it").size(12.5).strong().color(theme::text()));
                ui.label(RichText::new(tr.toolchain.join(" · ")).monospace().size(12.5).color(theme::text_strong()));
            });
            if let Some(d) = go {
                self.package = d;
            }
            ui.add_space(6.0);
        }
        let find = self.find.to_lowercase();
        egui::ScrollArea::horizontal().id_salt("pkgs-rows").show(ui, |ui| {
            egui::Grid::new("pkgs-rows-grid").striped(true).spacing(egui::vec2(16.0, 3.0)).show(ui, |ui| {
                for h in ["package", "asked", "published", "synchronized", "installed", ""] {
                    ui.label(RichText::new(h).size(12.5).strong().color(theme::text()));
                }
                ui.end_row();
                for r in rows.iter().filter(|r| (!self.only_diff || row_gap(r).is_some()) && (find.is_empty() || r.name.to_lowercase().contains(&find))).take(1000) {
                    let on = self.package == r.name;
                    if ui.add(egui::Button::selectable(on, RichText::new(&r.name).size(13.0).color(theme::text_strong()))).on_hover_text("trace it").clicked() {
                        self.package = if on { String::new() } else { r.name.clone() };
                    }
                    for v in [&r.requested, &r.published, &r.synced, &r.local] {
                        ui.label(RichText::new(dash(v)).monospace().size(12.5).color(theme::text()));
                    }
                    let g = row_gap(r);
                    ui.label(RichText::new(g.unwrap_or("")).size(12.5).strong().color(theme::alt()));
                    ui.end_row();
                }
            });
        });
    }
}

fn dash(v: &str) -> &str {
    if v.is_empty() { "-" } else { v }
}

/// A beating dot: something is moving this now.
fn pulse(ui: &mut egui::Ui, t: f64, c: Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
    let k = (0.5 + 0.5 * (t * 4.0).sin()) as f32;
    ui.painter().circle_filled(r.center(), 3.0 + 2.0 * k, c.gamma_multiply(0.5 + 0.5 * k));
}

/// One step of one set: a block in its state's colour holding the value;
/// a gap is an empty block with a red outline, so it reads as a hole.
fn cell(ui: &mut egui::Ui, w: f32, st: &PfStage, quiet: bool, live: bool, open: bool, t: f64) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 26.0), Sense::click());
    let r = rect.shrink2(egui::vec2(3.0, 2.0));
    let c = state_color(&st.state);
    let p = ui.painter();
    match st.state.as_str() {
        "gap" => {
            p.rect_stroke(r, 4, Stroke::new(2.0, c), egui::StrokeKind::Inside);
            p.text(r.center(), egui::Align2::CENTER_CENTER, "missing", egui::FontId::proportional(12.5), c);
        }
        "none" => {
            p.text(r.center(), egui::Align2::CENTER_CENTER, &st.value, egui::FontId::proportional(12.5), theme::text());
        }
        _ => {
            p.rect_filled(r, 4, c.gamma_multiply(0.22));
            p.rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(4.0, r.height())), 2, c);
            if !quiet {
                p.text(r.center(), egui::Align2::CENTER_CENTER, &st.value, egui::FontId::proportional(12.5), theme::text_strong());
            }
        }
    }
    if live {
        let k = (0.5 + 0.5 * (t * 4.0).sin()) as f32;
        p.rect_stroke(r.expand(1.0), 5, Stroke::new(1.5, theme::primary().gamma_multiply(0.4 + 0.6 * k)), egui::StrokeKind::Outside);
    }
    if open {
        p.rect_stroke(r.expand(2.0), 5, Stroke::new(2.0, theme::text_strong()), egui::StrokeKind::Outside);
    }
    let resp = resp.on_hover_text(if st.fix.is_empty() { st.note.clone() } else { format!("{}\ncloses it: {}", st.note, st.fix) });
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// The chain in words, from the same states the table draws: how far every
/// language set has come, the first step where it breaks and why, what is
/// behind, and the one command that closes the break (operator, 2026-10-24,
/// the Packages view "hard to understand": nine identical rows of numbers
/// ending in red, and nothing saying what that meant).
fn verdict(ui: &mut egui::Ui, ctx: &egui::Context, f: &PackageFlow) {
    let (lines, fix) = verdict_lines(f);
    if lines.is_empty() {
        return;
    }
    let broken = !fix.is_empty();
    egui::Frame::new().fill(theme::surface()).stroke(Stroke::new(1.5, if broken { theme::alt() } else { theme::ok() })).corner_radius(6).inner_margin(10.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        for (i, l) in lines.iter().enumerate() {
            let t = RichText::new(l).size(if i == 0 { 15.5 } else { 14.0 });
            ui.add(egui::Label::new(if i == 0 { t.strong().color(theme::text_strong()) } else { t.color(theme::text_strong()) }).wrap());
        }
        if broken {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("closes it").size(13.5).strong().color(theme::text_strong()));
                ui.label(RichText::new(&fix).monospace().size(13.5).color(theme::text_strong()));
                if ui.button("Copy").clicked() {
                    ctx.copy_text(fix.clone());
                }
            });
        }
    });
}

/// The verdict's sentences and the fix for its break ("" when nothing breaks).
fn verdict_lines(f: &PackageFlow) -> (Vec<String>, String) {
    let sets: Vec<&PfSet> = f.sets.iter().filter(|s| s.records > 0).collect();
    if sets.is_empty() {
        return (Vec::new(), String::new());
    }
    let n = sets.len();
    let name = |k: &str| -> &'static str { STEPS.iter().find(|x| x.0 == k).map_or("", |x| x.1) };
    let mut lines = Vec::new();
    let mut fix = String::new();
    let mut whole = Vec::new();
    for (key, _, _) in STEPS.iter().filter(|x| x.0 != "loaded") {
        let st: Vec<(&PfSet, PfStage)> = sets.iter().map(|s| (*s, step_of(f, s, key))).collect();
        let gaps: Vec<&(&PfSet, PfStage)> = st.iter().filter(|(_, g)| g.state == "gap").collect();
        let behind: Vec<&(&PfSet, PfStage)> = st.iter().filter(|(_, g)| g.state == "behind").collect();
        if !gaps.is_empty() && fix.is_empty() {
            let (s0, g0) = gaps[0];
            let who = if gaps.len() == n { format!("none of the {n} language sets") } else { format!("{} of the {n} language sets ({})", gaps.len(), gaps.iter().map(|(s, _)| s.set.as_str()).collect::<Vec<_>>().join(", ")) };
            let what = if *key == "cvmfs" {
                format!("so `ml {}` fails for everyone on the stack", s0.module)
            } else {
                g0.note.clone()
            };
            lines.push(format!("The chain breaks at {}: {who} {} - {what}.", name(key), if *key == "cvmfs" { "has its module on CVMFS" } else { "is there" }));
            fix = g0.fix.clone();
        } else if gaps.is_empty() && behind.is_empty() && fix.is_empty() {
            whole.push(name(key));
        }
        if !behind.is_empty() {
            let (_, b0) = behind[0];
            lines.push(format!("{}: {} of {n} behind - {}.", name(key), behind.len(), b0.note));
        }
    }
    let head = if whole.is_empty() {
        format!("{n} language sets.")
    } else {
        format!("All {n} language sets are {} here.", whole.iter().map(|w| w.to_lowercase()).collect::<Vec<_>>().join(", "))
    };
    lines.insert(0, if fix.is_empty() && lines.is_empty() { format!("{head} Nothing is missing.") } else { head });
    (lines, fix)
}

/// What runs now: this host's ndexr operations and each build machine.
fn live_strip(ui: &mut egui::Ui, f: &PackageFlow, t: f64) {
    egui::Frame::new().fill(theme::surface()).stroke(Stroke::new(1.0, theme::pane_brd())).corner_radius(6).inner_margin(8.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(10.0, 6.0);
            ui.label(RichText::new("LIVE").size(12.5).strong().color(theme::primary()));
            let active: Vec<&crate::api::PfLiveMachine> = f.live_machines.iter().filter(|m| m.active).collect();
            if f.live.is_empty() && active.is_empty() {
                ui.label(RichText::new("nothing running: no ndexr operation on this host, every build machine idle").size(13.0).color(theme::text()));
            }
            for l in &f.live {
                pulse(ui, t, theme::primary());
                let set = if l.set.is_empty() { "every set".to_owned() } else { l.set.clone() };
                ui.label(RichText::new(format!("{} · {set} · {}", l.what, ago(l.since))).size(13.0).color(theme::text_strong()));
            }
            for m in active {
                pulse(ui, t, theme::ok());
                let left = m.remaining_s.map_or(String::new(), |s| format!(" · about {} left", ago(crate::software::now_secs() - s).trim_end_matches(" ago")));
                ui.label(RichText::new(format!("{} building {} · {} of {}{left}", m.name, if m.running.is_empty() { "-" } else { &m.running }, m.done, m.total)).size(13.0).color(theme::text_strong()));
            }
            let idle = f.live_machines.iter().filter(|m| !m.active).count();
            if idle > 0 && (!f.live.is_empty() || f.live_machines.iter().any(|m| m.active)) {
                ui.label(RichText::new(format!("{idle} machines idle")).size(13.0).color(theme::text()));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_verdict_names_the_break_and_its_fix() {
        let stage = |key: &str, state: &str, value: &str| PfStage { key: key.into(), state: state.into(), value: value.into(), note: format!("{key} note"), fix: if state == "gap" { "ndexr stack cvmfs publish --from-s3".into() } else { String::new() } };
        let set = |name: &str| PfSet {
            set: name.into(),
            module: format!("env/{name}"),
            records: 10,
            stages: vec![stage("planned", "ok", "10"), stage("built", "ok", "30"), stage("published", "ok", "30"), stage("synced", "behind", "30"), stage("installed", "ok", "30"), stage("cvmfs", "gap", "no module")],
            ..Default::default()
        };
        let f = PackageFlow { sets: vec![set("R/4.6.1"), set("Python/3.13")], ..Default::default() };
        let (lines, fix) = verdict_lines(&f);
        assert_eq!(lines[0], "All 2 language sets are planned, built, published, installed here.", "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("The chain breaks at Module: none of the 2 language sets has its module on CVMFS - so `ml env/R/4.6.1` fails")), "{lines:?}");
        assert!(lines.iter().any(|l| l.starts_with("Synchronized: 2 of 2 behind")), "{lines:?}");
        assert_eq!(fix, "ndexr stack cvmfs publish --from-s3");
    }

    #[test]
    fn four_states_stay_apart() {
        let r = |q: &str, p: &str, y: &str, l: &str| PfRow { name: "x".into(), requested: q.into(), published: p.into(), synced: y.into(), local: l.into(), ..Default::default() };
        assert_eq!(row_gap(&r("1", "1", "1", "1")), None);
        assert_eq!(row_gap(&r("", "2", "2", "2")), None, "a dependency, everywhere");
        assert_eq!(row_gap(&r("1", "", "", "")), Some("asked for, never published"));
        assert_eq!(row_gap(&r("1", "1", "1", "")), Some("not installed"));
        assert_eq!(row_gap(&r("1", "1", "1", "0.9")), Some("installed differs from the synchronized"));
        assert_eq!(row_gap(&r("2", "1", "2", "2")), Some("synchronized differs from the published"));
    }
}
