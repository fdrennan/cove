//! "This computer": the ndexr stack installed here and the build making it,
//! in Builds and in Software (operator, 2026-11-14: "installed local must
//! have tools for monitoring the local install through the packages/builds
//! sections"). Read from the build's own files on this machine
//! (stack.rs `local_install`) - nothing asked of ndexr.io, so it works with
//! no connection at all.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::api::{JobView, LocalInstall, LocalStep, Query};
use crate::backend::Backend;
use crate::ui::kit::theme::{self, State};

type Slot<T> = Arc<Mutex<Option<T>>>;

#[derive(Default)]
pub struct LocalPane {
    data: Slot<Result<LocalInstall, String>>,
    asked: f64,
    find: String,
    log_open: bool,
    note: Slot<Result<JobView, String>>,
}

impl LocalPane {
    /// Whether there is anything to show: a build started here, or a stack
    /// built or copied here in use. The CernVM-FS mount is not this
    /// computer's own - it had put "This computer" and all 545 of its modules
    /// at the top of Builds (operator, 2026-11-17: "in the builds section
    /// there is a lot of unnecessary text").
    pub fn has_install(&self) -> bool {
        matches!(&*self.data.lock().unwrap(), Some(Ok(v)) if !v.state.is_empty() || v.root_is_dir)
    }

    /// Ask again every 5 s, every 2 s while the build runs.
    pub fn poll(&mut self, backend: &Backend, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let busy = matches!(&*self.data.lock().unwrap(), Some(Ok(v)) if v.alive || v.state == "running");
        if self.asked > 0.0 && now - self.asked < if busy { 2.0 } else { 5.0 } {
            return;
        }
        self.asked = now;
        let slot = self.data.clone();
        backend.query(Query::LocalInstall, ctx, move |r: Result<LocalInstall, String>| *slot.lock().unwrap() = Some(r));
        ctx.request_repaint_after(std::time::Duration::from_secs(if busy { 2 } else { 5 }));
    }

    /// The panel. `packages`: the Software view, where the modules lead; in
    /// Builds the build leads.
    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, packages: bool) {
        self.poll(backend, ctx);
        let Some(got) = self.data.lock().unwrap().clone() else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading this computer's stack");
            });
            return;
        };
        let v = match got {
            Ok(v) => v,
            Err(e) => {
                theme::state_mark(ui, State::Failed, &format!("Could not read this computer's stack: {e}"));
                return;
            }
        };
        egui::Frame::new().fill(theme::surface()).stroke(egui::Stroke::new(1.0, theme::moss())).corner_radius(8).inner_margin(12).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("This computer").size(15.0).strong().color(theme::text_strong()));
                let (st, words) = build_state(&v);
                theme::state_mark(ui, st, &words);
            });
            ui.add(egui::Label::new(RichText::new(format!("Stack in use: {}{}", v.root, if v.root_is_dir { "  (built here)" } else { "" })).monospace().size(11.5).color(theme::text())).wrap());
            // Builds: the build. Packages: the build, then its modules, folded -
            // the names are what Packages lists anyway.
            self.build(ui, ctx, backend, &v);
            if packages {
                ui.add_space(6.0);
                egui::CollapsingHeader::new(RichText::new(format!("{} modules installed", v.modules.len())).size(12.5)).id_salt("local-modules-fold").show(ui, |ui| self.modules(ui, &v));
            }
        });
    }

    fn build(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, v: &LocalInstall) {
        if v.state.is_empty() {
            ui.label(RichText::new(format!("No build has run in {}.", v.dir)).size(12.5).color(theme::text()));
        } else {
            if !v.steps.is_empty() {
                let at = v.steps.iter().position(|s| *s == v.step);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (i, s) in v.steps.iter().enumerate() {
                        let st = match at {
                            Some(a) if i < a => State::Good,
                            Some(a) if i == a && v.state == "failed" => State::Failed,
                            Some(a) if i == a => State::Active,
                            None if v.state == "done" => State::Good,
                            _ => State::Idle,
                        };
                        theme::state_mark(ui, st, s);
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} steps done", v.done)).size(12.5).color(theme::text_strong()));
                if v.failed > 0 {
                    ui.label(RichText::new(format!("· {} failed", v.failed)).size(12.5).color(State::Failed.color()));
                }
                if !v.running.is_empty() {
                    let now: Vec<String> = v.running.iter().map(step_name).collect();
                    ui.label(RichText::new(format!("· building {}", now.join(", "))).size(12.5).color(State::Active.color()));
                }
            });
            for f in v.failures.iter().take(5) {
                theme::state_mark(ui, State::Failed, &format!("{} failed", step_name(f)));
            }
            if !v.recent.is_empty() {
                egui::CollapsingHeader::new(RichText::new(format!("Finished lately ({})", v.recent.len())).size(12.5)).id_salt("local-recent").show(ui, |ui| {
                    for s in &v.recent {
                        let secs = (s.finished - s.started).max(0);
                        ui.label(RichText::new(format!("{}  ·  {}:{:02}", step_name(s), secs / 60, secs % 60)).monospace().size(11.5).color(theme::text()));
                    }
                });
            }
            if !v.tail.is_empty() {
                egui::CollapsingHeader::new(RichText::new("The build's log, last lines").size(12.5)).id_salt("local-log").open(self.log_open.then_some(true)).show(ui, |ui| {
                    theme::log_scroll(egui::ScrollArea::vertical().max_height(220.0).stick_to_bottom(true), ui, |ui| {
                        theme::selectable_lines(ui, &v.tail, egui::FontId::monospace(11.0), theme::text(), true);
                    });
                });
                self.log_open = false;
            }
        }
        ui.horizontal_wrapped(|ui| {
            let running = v.alive || v.state == "running" || v.state == "starting";
            let word = if v.state.is_empty() { "Build the stack here" } else if v.state == "done" { "Build again (only what changed)" } else { "Resume the build" };
            if !running && ui.add(egui::Button::new(word)).on_hover_text("ndexr stack up --dir: no root and no ndexr servers; it resumes where it stopped").clicked() {
                let note = self.note.clone();
                backend.query(Query::StackBuild { report: false }, ctx, move |r: Result<JobView, String>| *note.lock().unwrap() = Some(r));
                self.asked = 0.0;
            }
            if ui.button("Check again").on_hover_text(format!("in a terminal: ndexr stack logs --dir {} -f", v.dir)).clicked() {
                self.asked = 0.0;
            }
        });
        if let Some(n) = self.note.lock().unwrap().clone() {
            match n {
                Ok(j) => theme::state_mark(ui, if j.ok == Some(false) { State::Failed } else { State::Active }, &j.what),
                Err(e) => theme::state_mark(ui, State::Failed, &e),
            };
        }
    }

    fn modules(&mut self, ui: &mut egui::Ui, v: &LocalInstall) {
        ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("find a module: R, Python, GDAL…").desired_width(220.0));
        let f = self.find.trim().to_lowercase();
        let hits: Vec<&String> = v.modules.iter().filter(|m| f.is_empty() || m.to_lowercase().contains(&f)).collect();
        if hits.is_empty() {
            if !v.modules.is_empty() {
                ui.label(RichText::new("None match.").size(12.5).color(theme::text()));
            }
            return;
        }
        egui::ScrollArea::vertical().id_salt("local-modules").max_height(180.0).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(10.0, 2.0);
                for m in hits.iter().take(400) {
                    ui.label(RichText::new(m.as_str()).monospace().size(11.5).color(theme::text()));
                }
            });
            if hits.len() > 400 {
                ui.label(RichText::new(format!("and {} more - narrow the search", hits.len() - 400)).size(12.0).color(theme::text()));
            }
        });
    }
}

fn step_name(s: &LocalStep) -> String {
    if s.version.is_empty() { s.name.clone() } else { format!("{}/{}", s.name, s.version) }
}

/// The build's state as a mark and its words.
pub fn build_state(v: &LocalInstall) -> (State, String) {
    let step = |w: &str| if v.step.is_empty() { w.to_owned() } else { format!("{w} · {}", v.step) };
    match v.state.as_str() {
        "" if v.modules.is_empty() => (State::Idle, "No stack built here".into()),
        "" => (State::Good, format!("{} modules", v.modules.len())),
        "running" | "starting" if v.alive => (State::Active, step("Building")),
        "running" | "starting" => (State::Attention, step("Stopped without finishing - resume it")),
        "waiting" => (State::Attention, step("Waiting for a fixed release")),
        "done" => (State::Good, "Built".into()),
        "failed" => (State::Failed, step(&format!("Failed{}", v.rc.map(|r| format!(" (exit {r})")).unwrap_or_default()))),
        "stopped" => (State::Idle, step("Stopped")),
        other => (State::Idle, other.to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::build_state;
    use crate::api::LocalInstall;
    use crate::ui::kit::theme::State;

    #[test]
    fn a_build_says_what_it_is_doing() {
        let v = |state: &str, alive: bool| LocalInstall { state: state.into(), step: "suite".into(), alive, ..Default::default() };
        assert_eq!(build_state(&v("running", true)), (State::Active, "Building · suite".into()));
        assert_eq!(build_state(&v("running", false)).0, State::Attention, "a dead supervisor is not building");
        assert_eq!(build_state(&LocalInstall { state: "failed".into(), step: "prefix".into(), rc: Some(2), ..Default::default() }), (State::Failed, "Failed (exit 2) · prefix".into()));
        assert_eq!(build_state(&v("done", false)), (State::Good, "Built".into()));
        assert_eq!(build_state(&LocalInstall::default()).1, "No stack built here");
    }
}
