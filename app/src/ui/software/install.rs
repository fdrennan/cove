//! Software › Install: the desktop app building the ndexr stack on the
//! person's own machine, the environments they pick.
//!
//! Operator, 2026-10-15: "the desktop version of cove will use the ndexr
//! commands to install the stack locally on their system, there should be a
//! window for that - and users should be able to select all or individually
//! which environments they would like in their stack, create tab under
//! software called Install, the users install sends build stats back to ndexr
//! to collect and display for the user inside their ide just like n-boxes do,
//! but they wouldnt set up s3 or push or anything like that yet - that comes
//! later as an option". And the same day: "the Install tab picks by manifest".
//!
//!   pick      the env manifests (manifests.rs's list): the kept R + Python
//!             combinations, the R / Python / purpose sets, each with its
//!             record counts and what is built here; All, or one by one
//!   Install   the ndexr CLI's own commands as one job (manifest.rs
//!             install_script): the folders made the person's (the one root
//!             step), the kit, the Prefix + EasyBuild layer if missing, the
//!             sets' modules, their packages - prebuilt binaries first if
//!             asked (`ndexr pkgs sync`, HTTPS, read only), then built. Never
//!             a publish, a promote or a push.
//!   progress  step i of n, the package i of n, the easyconfig building, and
//!             the log, every second while it runs; Stop
//!   reported  the person's machines as their account has them
//!             (api.ndexr.io/v1/builds, their key) - the same reports n-boxes
//!             send, off with one tick (no `stack report`, `--no-report`)
//!
//! Only the desktop app on Linux builds. A browser on a Cove server, or the
//! app on a cell, is told why not in the machine's words (host::info): the
//! stack there is ndexr's own, from CVMFS.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align, FontId, Layout, RichText, Sense};

use super::{fmt_ago, fmt_dur, now_secs, section, stat, stat_c, SoftwarePane};
use crate::api::{JobView, Query};
use crate::backend::Backend;
use crate::catalog::manifest::{self, InstallInfo, InstallRequest, MyBuilds, Progress};
use crate::ui::kit::theme;

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

pub struct Inst {
    info: Slot<InstallInfo>,
    info_asked: bool,
    job: Slot<JobView>,
    job_now: JobView,
    job_at: f64,
    job_waiting: bool,
    builds: Slot<MyBuilds>,
    builds_at: f64,
    /// The environments picked.
    pub picks: BTreeSet<String>,
    prebuilt: bool,
    report: bool,
    note: Option<String>,
    /// The log read for progress, by (lines, last line): read again only
    /// when it moved.
    read: (usize, String, Progress),
}

impl Default for Inst {
    fn default() -> Self {
        Inst {
            info: Default::default(),
            info_asked: false,
            job: Default::default(),
            job_now: JobView::default(),
            job_at: f64::NEG_INFINITY,
            job_waiting: false,
            builds: Default::default(),
            builds_at: f64::NEG_INFINITY,
            picks: BTreeSet::new(),
            prebuilt: true,
            report: true,
            note: None,
            read: (0, String::new(), Progress::default()),
        }
    }
}

impl Inst {
    fn tick(&mut self, backend: &Backend, ctx: &egui::Context, now: f64) {
        if !self.info_asked {
            self.info_asked = true;
            let slot = self.info.clone();
            backend.query(Query::InstallInfo, ctx, move |r: Result<InstallInfo, String>| *slot.lock().unwrap() = Some(r));
        }
        if let Some(r) = self.job.lock().unwrap().take() {
            self.job_waiting = false;
            match r {
                Ok(j) => {
                    let ended = self.job_now.running && !j.running;
                    self.job_now = j;
                    // A finished install has reported its last steps: ask
                    // the account again, and the folders may be ours now.
                    if ended {
                        self.builds_at = f64::NEG_INFINITY;
                        self.info_asked = false;
                    }
                }
                Err(e) => self.note = Some(e),
            }
        }
        let every = if self.job_now.running { 1.0 } else { 10.0 };
        if !self.job_waiting && now - self.job_at >= every {
            self.job_at = now;
            self.job_waiting = true;
            let slot = self.job.clone();
            backend.query(Query::InstallJob, ctx, move |r: Result<JobView, String>| *slot.lock().unwrap() = Some(r));
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(every));
        // The account's view of this machine moves on the CLI's reports
        // (every five minutes while building): a minute is often enough.
        let b_every = if self.job_now.running { 60.0 } else { 300.0 };
        if now - self.builds_at >= b_every {
            self.builds_at = now;
            let slot = self.builds.clone();
            backend.query(Query::MyBuilds, ctx, move |r: Result<MyBuilds, String>| *slot.lock().unwrap() = Some(r));
        }
    }

    fn progress(&mut self) -> &Progress {
        let lines = &self.job_now.lines;
        let last = lines.last().cloned().unwrap_or_default();
        if self.read.0 != lines.len() || self.read.1 != last {
            self.read = (lines.len(), last, manifest::progress(lines));
        }
        &self.read.2
    }

    fn start(&mut self, backend: &Backend, ctx: &egui::Context) {
        let req = InstallRequest { sets: self.picks.iter().cloned().collect(), prebuilt: self.prebuilt, report: self.report };
        let slot = self.job.clone();
        self.job_waiting = true;
        backend.query(Query::Install { install: req }, ctx, move |r: Result<JobView, String>| *slot.lock().unwrap() = Some(r));
        self.job_at = ctx.input(|i| i.time);
    }

    fn stop(&mut self, backend: &Backend, ctx: &egui::Context) {
        let slot = self.job.clone();
        self.job_waiting = true;
        backend.query(Query::InstallStop, ctx, move |r: Result<JobView, String>| *slot.lock().unwrap() = Some(r));
    }
}

/// A thin bar, `f` of the way along.
fn bar(ui: &mut egui::Ui, f: f32, c: egui::Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().min(520.0), 5.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(r, 2.0, theme::moss());
    p.rect_filled(r.with_max_x(r.left() + r.width() * f.clamp(0.0, 1.0)), 2.0, c);
}

impl SoftwarePane {
    pub(super) fn install_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend) {
        let now = ctx.input(|i| i.time);
        self.man.load(backend, ctx);
        self.inst.tick(backend, ctx, now);
        let info = self.inst.info.lock().unwrap().clone();
        let listing = self.man.listing();
        let builds = self.inst.builds.lock().unwrap().clone();
        let running = self.inst.job_now.running;
        let mut go = false;
        let mut stop = false;

        // ── where it builds, and who it reports to ─────────────────────────
        let can = matches!(&info, Some(Ok(i)) if i.unsupported.is_empty());
        theme::strip(ui, 22.0, theme::surface(), |ui| {
            ui.add_space(10.0);
            match &info {
                None => {
                    ui.spinner();
                    ui.label(RichText::new("asking this machine…").size(11.0).color(theme::muted()));
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e).size(11.0).color(theme::alt()));
                }
                Some(Ok(i)) if !i.unsupported.is_empty() => {
                    ui.label(RichText::new("not on this machine").size(11.0).color(theme::amber()));
                }
                Some(Ok(i)) => {
                    ui.label(RichText::new(format!("builds into {}", i.dir)).monospace().size(11.0).color(theme::text()))
                        .on_hover_text(format!("the stack's folder (the path every binary is built for); the packages go to {}", i.pkgs_root));
                    theme::tool_sep(ui);
                    ui.label(RichText::new(format!("with {}", i.cli)).monospace().size(10.5).color(theme::hush()));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(8.0);
                let (words, c) = match (&builds, &info) {
                    (_, Some(Ok(i))) if !i.key => ("not signed in - nothing is reported".to_owned(), theme::hush()),
                    (Some(Ok(b)), _) if !b.owner.is_empty() && self.inst.report => (format!("reporting to {}", b.owner), theme::ok()),
                    (_, _) if !self.inst.report => ("not reporting".to_owned(), theme::hush()),
                    _ => ("reporting to your ndexr account".to_owned(), theme::muted()),
                };
                ui.label(RichText::new(words).size(10.5).color(c))
                    .on_hover_text("What goes: package names, versions, times and status, with this machine's OS, arch, CPUs and memory - what n-boxes send. No logs, no paths beyond the stack folder.");
            });
        });

        if let Some(Ok(i)) = &info {
            if !i.unsupported.is_empty() {
                egui::Frame::new().inner_margin(egui::Margin::same(14)).show(ui, |ui| {
                    ui.label(RichText::new("Install").font(theme::display(15.0)).color(theme::text_strong()));
                    ui.add_space(4.0);
                    ui.label(RichText::new(&i.unsupported).color(theme::text()));
                    ui.add_space(6.0);
                    ui.label(RichText::new("Packages › Manifests shows what each environment holds.").size(11.5).color(theme::hush()));
                });
                if let Some(Ok(b)) = &builds {
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(14, 4)).show(ui, |ui| reported(ui, b));
                }
                return;
            }
        }

        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let wide = rect.width() >= 760.0;
        let (left, right) = if wide {
            let w = (rect.width() * 0.4).clamp(300.0, 460.0);
            (rect.with_max_x(rect.left() + w), rect.with_min_x(rect.left() + w + 1.0))
        } else {
            let h = (rect.height() * 0.45).max(180.0);
            (rect.with_max_y(rect.top() + h), rect.with_min_y(rect.top() + h + 1.0))
        };
        if wide {
            ui.painter().vline(right.left() - 0.5, rect.y_range(), egui::Stroke::new(1.0, theme::pane_brd()));
        }

        // ── the environments, picked ─────────────────────────────────────
        super::region(ui, left, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 8, bottom: 6 }).show(ui, |ui| {
                ui.horizontal(|ui| {
                    section(ui, "Environments");
                });
                let Some(Ok(l)) = &listing else {
                    match &listing {
                        Some(Err(e)) => {
                            ui.label(RichText::new(e).color(theme::alt()));
                        }
                        _ => {
                            ui.spinner();
                        }
                    }
                    return;
                };
                let shown: Vec<manifest::Summary> = self.man.shown(l).into_iter().cloned().collect();
                ui.horizontal(|ui| {
                    let all = !shown.is_empty() && shown.iter().all(|s| self.inst.picks.contains(&s.name));
                    if ui.add_enabled(!running, egui::Button::new(RichText::new(if all { "None" } else { "All" }).size(11.5))).clicked() {
                        if all {
                            self.inst.picks.clear();
                        } else {
                            self.inst.picks.extend(shown.iter().map(|s| s.name.clone()));
                        }
                    }
                    let mut show_all = self.man.show_all;
                    let hidden = l.sets.iter().filter(|s| !s.kept).count();
                    if hidden > 0 && ui.checkbox(&mut show_all, RichText::new(format!("every kit set (+{hidden})")).size(11.0)).changed() {
                        self.man.show_all = show_all;
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let n = shown.iter().filter(|s| self.inst.picks.contains(&s.name)).fold([0; 4], |mut a, s| {
                            a.iter_mut().zip(s.counts()).for_each(|(a, n)| *a += n);
                            a
                        });
                        ui.label(RichText::new(format!("{} picked · {}", self.inst.picks.len(), manifest::count_words(n))).size(10.5).color(theme::muted()));
                    });
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical().id_salt("in-sets").auto_shrink([false, true]).max_height(ui.available_height() - 92.0).show(ui, |ui| {
                    let mut fam_was = "";
                    let mut sorted = shown.clone();
                    sorted.sort_by_key(|s| match manifest::family(&s.name) {
                        "R + Python" => 0,
                        "R" => 1,
                        "Python" => 2,
                        "Rust" => 3,
                        _ => 4,
                    });
                    for s in &sorted {
                        let fam = manifest::family(&s.name);
                        if fam != fam_was {
                            ui.add_space(4.0);
                            ui.label(RichText::new(fam).size(10.5).color(theme::hush()));
                            fam_was = fam;
                        }
                        let mut on = self.inst.picks.contains(&s.name);
                        ui.horizontal(|ui| {
                            let r = ui.add_enabled(!running, egui::Checkbox::new(&mut on, RichText::new(&s.name).monospace().size(12.0)));
                            let mut tip = format!("{} · {}", s.env_module, manifest::count_words(s.counts()));
                            if !s.modules.is_empty() {
                                tip.push_str(&format!("\nbuilds on {}", s.modules.join(", ")));
                            }
                            if r.on_hover_text(tip).changed() {
                                if on {
                                    self.inst.picks.insert(s.name.clone());
                                } else {
                                    self.inst.picks.remove(&s.name);
                                }
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                match &s.built {
                                    Some(b) => ui.label(RichText::new(format!("here: {}", manifest::count_words(b.counts()))).size(10.5).color(if b.busy { theme::amber() } else { theme::ok() })),
                                    None => ui.label(RichText::new(manifest::count_words(s.counts())).size(10.5).color(theme::hush())),
                                };
                            });
                        });
                    }
                });
                ui.add_space(6.0);
                ui.add_enabled_ui(!running, |ui| {
                    ui.checkbox(&mut self.inst.prebuilt, RichText::new("fetch prebuilt packages first (repo.ndexr.io/env, read only), build the rest").size(11.5))
                        .on_hover_text("`ndexr pkgs sync <set>`, then `ndexr pkgs build <set>` builds only what is still missing. Off: every package is built here.");
                    let key = matches!(&info, Some(Ok(i)) if i.key);
                    ui.add_enabled(key, egui::Checkbox::new(&mut self.inst.report, RichText::new("send the build steps to my ndexr account").size(11.5)))
                        .on_hover_text(if key { "the timeline every five minutes (`ndexr stack report`) and each package (`ndexr pkgs build`) - off: neither, and `--no-report`" } else { "sign in first (account menu)" });
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    if running {
                        if ui.button(RichText::new("■ Stop").color(theme::alt())).on_hover_text("stop the install and the builds it started; Install again resumes - what is built stays built").clicked() {
                            stop = true;
                        }
                    } else {
                        let ok = can && !self.inst.picks.is_empty();
                        if ui.add_enabled(ok, theme::primary_button("Install")).on_hover_text("lay the kit, build what is missing, then the picked environments' packages - hours the first time").clicked() {
                            go = true;
                        }
                    }
                    if let Some(Ok(i)) = &info {
                        if !i.needs_root.is_empty() {
                            let how = match i.root_by.as_str() {
                                "pkexec" => "asks for your password once",
                                "sudo" | "root" => "uses sudo once",
                                _ => "needs root once - Install says the command",
                            };
                            ui.label(RichText::new(format!("making {} yours {how}", i.needs_root.join(", "))).size(10.5).color(theme::amber()));
                        }
                    }
                });
                if let Some(n) = &self.inst.note {
                    ui.label(RichText::new(n).size(11.0).color(theme::alt()));
                }
            });
        });
        if go {
            self.inst.note = None;
            self.inst.start(backend, ctx);
        }
        if stop {
            self.inst.stop(backend, ctx);
        }

        // ── how far it got, its log, and what the account has ─────────────
        super::region(ui, right, |ui| {
            egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 10, top: 8, bottom: 6 }).show(ui, |ui| {
                let job = self.inst.job_now.clone();
                let p = self.inst.progress().clone();
                section(ui, if job.what.is_empty() { "Progress" } else { &job.what });
                if job.what.is_empty() {
                    ui.label(RichText::new("Nothing installed from here yet. Pick environments and Install - the steps and the log show here.").size(11.5).color(theme::hush()));
                } else {
                    ui.horizontal(|ui| {
                        let (word, c) = match (job.running, job.ok) {
                            (true, _) => ("installing", theme::amber()),
                            (false, Some(true)) => ("done", theme::ok()),
                            (false, Some(false)) => ("stopped or failed", theme::alt()),
                            _ => ("", theme::hush()),
                        };
                        stat_c(ui, word, "state", c);
                        if let Some((i, n, what)) = &p.step {
                            stat(ui, &format!("{i} of {n}"), what);
                        }
                        if p.built + p.skipped > 0 {
                            stat(ui, &format!("{} built", p.built), &format!("{} done before", p.skipped));
                        }
                        if p.ok + p.failed > 0 {
                            stat_c(ui, &format!("{} ok", p.ok), &format!("{} failed", p.failed), if p.failed > 0 { theme::alt() } else { theme::ok() });
                        }
                    });
                    if let Some((i, n, _)) = &p.step {
                        bar(ui, *i as f32 / (*n).max(1) as f32, theme::primary());
                    }
                    if let Some((pkg, lang, i, n)) = &p.package {
                        ui.label(RichText::new(format!("installing {pkg} ({lang}) - {i} of {n}")).size(11.5).color(theme::text()));
                        bar(ui, *i as f32 / (*n).max(1) as f32, super::manifests::lang_kind(manifest::Lang::of(lang)).color());
                    } else if let Some(ec) = &p.easyconfig {
                        ui.label(RichText::new(format!("building {ec}")).monospace().size(11.5).color(theme::text()));
                    }
                    ui.add_space(6.0);
                    let font = FontId::monospace(11.0);
                    let row_h = ui.fonts_mut(|f| f.row_height(&font)) + 1.0;
                    let h = (ui.available_height() - 150.0).max(120.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    egui::ScrollArea::both().id_salt("in-log").max_height(h).auto_shrink([false, true]).stick_to_bottom(true).show_rows(ui, row_h, job.lines.len(), |ui, range| {
                        for l in &job.lines[range] {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(l.len() as f32 * 6.7 + 12.0), row_h), Sense::hover());
                            ui.painter().text(egui::pos2(r.left() + 4.0, r.center().y), egui::Align2::LEFT_CENTER, l, font.clone(), super::logs::tone(l));
                        }
                    });
                }
                ui.add_space(8.0);
                match &builds {
                    None => {
                        ui.spinner();
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).size(11.0).color(theme::alt()));
                    }
                    Some(Ok(b)) => reported(ui, b),
                }
            });
        });
    }
}

/// The person's machines as their account has them, this one first.
fn reported(ui: &mut egui::Ui, b: &MyBuilds) {
    section(ui, "Your builds, as ndexr has them");
    if !b.error.is_empty() {
        ui.label(RichText::new(&b.error).size(11.0).color(theme::hush()));
        return;
    }
    if b.machines.is_empty() {
        ui.label(RichText::new(format!("No builds reported to {} yet - an install here sends its steps as it goes.", if b.owner.is_empty() { "your account" } else { &b.owner })).size(11.0).color(theme::hush()));
        return;
    }
    let now = now_secs();
    let mut ms = b.machines.clone();
    ms.sort_by_key(|m| (m.id != b.this_machine, -(m.seen as i64)));
    for m in &ms {
        ui.horizontal(|ui| {
            let mine = m.id == b.this_machine;
            let name = if m.host.is_empty() { m.id.chars().take(8).collect() } else { m.host.clone() };
            ui.label(RichText::new(if mine { format!("{name} (this machine)") } else { name }).monospace().size(11.5).color(if mine { theme::text_strong() } else { theme::text() }));
            if !m.system.is_empty() {
                ui.label(RichText::new(&m.system).size(10.5).color(theme::hush()));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let seen = if m.seen > 0.0 { fmt_ago(now, m.seen as i64) } else { String::new() };
                ui.label(RichText::new(seen).size(10.5).color(theme::hush()));
                let took = if m.until > m.started { fmt_dur((m.until - m.started) as i64) } else { String::new() };
                let c = if m.failed > 0 { theme::alt() } else if m.running > 0 { theme::amber() } else { theme::ok() };
                ui.label(RichText::new(format!("{} of {} done · {} failed · {} running · {took}", m.done, m.steps, m.failed, m.running)).size(10.5).color(c));
            });
        });
    }
}
