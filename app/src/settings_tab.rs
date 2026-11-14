//! Settings: the open project's app settings, in the place the Account tab
//! held (operator, 2026-10-28: "where we have account that should be more like
//! the app settings that you see inside of Posit Connect ... the deployed
//! environment variables, the permissions, the visibility"). The account
//! itself moved to the Files panel's Account button (account_tab.rs).
//!
//! A side menu as in Posit Connect (operator, 2026-10-30), one section at a
//! time, each menu row showing that section's state. Every value is read from
//! what serves the app, never typed in:
//! - Info: its address, title, language, environment and folder.
//! - Runtime: what runs it (app.env: module or port, health) and whether it
//!   answers; Start / Stop / Rebuild run its up.sh / down.sh / build.sh.
//! - Access: who may open it (Query::AppAccess, the platform's own levels);
//!   people and groups through Project › Access (access.rs).
//! - Visibility: on the web or not (DomainInfo.target - what serves
//!   https://<host>/), and Publish when it is not.
//! - Variables: KEY=VALUE for an app with its own process, kept in the
//!   project's `app.vars` (out of its saved versions by .gitignore) and
//!   exported by Up (apps.rs `action`) before up.sh starts it.
//! - Container image: a versioned image of the project that ndexr builds,
//!   publishes to ECR and keeps (Query::Image, docs/container-images.md);
//!   what it needs to run, and how to pull and run it.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::api::{AppAccessView, AppStatus, DomainInfo, FileText, ImageLogin, ImageStatus, JobView, Query};
use crate::backend::Backend;
use crate::theme;

type Slot<T> = Arc<Mutex<Option<T>>>;

/// The variables file, beside app.env (in .cove/, dotcove.rs).
pub const VARS: &str = "app.vars";
/// Where it is from the project's root.
const VARS_AT: &str = ".cove/app.vars";

/// What the tab asks the IDE to do: things owned elsewhere.
pub enum Act {
    /// Open Project › Access for people and groups.
    Access(String),
    /// Put the domain on the web (Ide::publish_domain).
    Publish(String),
    /// Build, Up or Down the open project (Ide::app_action), as the app strip does.
    Run(String),
    /// Make the project another type (dotcove.rs TYPES): its APP_LANGUAGE.
    SetType(String),
}

#[derive(Default)]
pub struct SettingsPane {
    /// The project these were read for; a different one reads again.
    host: String,
    access: Slot<Result<AppAccessView, String>>,
    vars: Slot<Vec<(String, String)>>,
    /// The rows being edited, and whether each value shows.
    edit: Vec<(String, String, bool)>,
    loaded: bool,
    new_key: String,
    new_val: String,
    note: Slot<Result<String, String>>,
    /// The side menu's chosen section.
    section: Section,
    /// The container image: what ndexr reports, its build, and a pull login.
    image: Slot<Result<ImageStatus, String>>,
    image_job: Slot<Result<JobView, String>>,
    image_login: Slot<Result<ImageLogin, String>>,
    image_asked: f64,
}

impl SettingsPane {
    fn read(&mut self, d: &DomainInfo, backend: &Backend, ctx: &egui::Context) {
        self.host = d.host.clone();
        self.loaded = false;
        self.edit.clear();
        *self.vars.lock().unwrap() = None;
        *self.note.lock().unwrap() = None;
        // Who can view it, for the summary (an empty level reads it).
        let (slot, c) = (self.access.clone(), ctx.clone());
        backend.query(Query::AppAccess { host: self.host.clone(), level: String::new() }, ctx, move |r: Result<AppAccessView, String>| {
            *slot.lock().unwrap() = Some(r);
            c.request_repaint();
        });
        *self.image.lock().unwrap() = None;
        *self.image_job.lock().unwrap() = None;
        *self.image_login.lock().unwrap() = None;
        self.image_asked = f64::NEG_INFINITY;
        let (slot, c) = (self.vars.clone(), ctx.clone());
        backend.query(Query::ReadFile { path: format!("{}/{VARS_AT}", d.path) }, ctx, move |r: Result<FileText, String>| {
            // No file yet is no variables, not a failure.
            *slot.lock().unwrap() = Some(r.map(|f| parse_vars(&f.text)).unwrap_or_default());
            c.request_repaint();
        });
    }

    fn save(&mut self, d: &DomainInfo, backend: &Backend, ctx: &egui::Context) {
        let rows: Vec<(String, String)> = self.edit.iter().map(|(k, v, _)| (k.clone(), v.clone())).collect();
        let (note, saved, c) = (self.note.clone(), self.vars.clone(), ctx.clone());
        let done_rows = rows.clone();
        backend.query(Query::WriteFile { path: format!("{}/{VARS_AT}", d.path), text: write_vars(&rows) }, ctx, move |r: Result<FileText, String>| {
            *note.lock().unwrap() = Some(r.map(|_| "Saved. The app has them from its next Up.".to_owned()));
            if note.lock().unwrap().as_ref().is_some_and(|r| r.is_ok()) {
                *saved.lock().unwrap() = Some(done_rows);
            }
            c.request_repaint();
        });
        // The values never go into the project's saved versions.
        let (gi, b, c) = (format!("{}/.gitignore", d.path), backend.clone(), ctx.clone());
        backend.query(Query::ReadFile { path: gi.clone() }, ctx, move |r: Result<FileText, String>| {
            let text = r.map(|f| f.text).unwrap_or_default();
            if !text.lines().any(|l| l.trim() == VARS_AT) {
                let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
                b.query(Query::WriteFile { path: gi, text: format!("{text}{sep}{VARS_AT}\n") }, &c, |_: Result<FileText, String>| {});
            }
        });
    }

    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, domain: Option<&DomainInfo>, app: Option<&AppStatus>) -> Option<Act> {
        let Some(d) = domain else {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Open a project to see its settings.").size(14.0).color(theme::text())));
            return None;
        };
        if self.host != d.host {
            self.read(d, backend, ctx);
        }
        if !self.loaded {
            if let Some(v) = self.vars.lock().unwrap().clone() {
                self.edit = v.into_iter().map(|(k, v)| (k, v, false)).collect();
                self.loaded = true;
            }
        }
        let mut act = None;
        let mut refresh = false;
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(&d.host).monospace().size(12.5).color(theme::text_strong()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                refresh = theme::tool_if(ui, true, "⟳", "read the settings again").clicked();
            });
        });
        if refresh {
            self.read(d, backend, ctx);
        }

        // THE SIDE MENU, AS IN POSIT CONNECT (operator, 2026-10-30: "settings
        // management similar to posit connect in settings in their side menu";
        // "we do not like poorly wasted space"). One section at a time, and
        // each menu row says that section's state, so the menu is the summary.
        let states = [
            (Section::Info, self.info_state(d, app)),
            (Section::Access, self.access_state()),
            (Section::Runtime, runtime_state(app)),
            (Section::Visibility, if d.target.is_empty() { "Not on the web".to_owned() } else { "On the web".to_owned() }),
            (Section::Variables, self.vars_state(app)),
            (Section::Image, self.image_state()),
        ];
        let nav_w = if ui.available_width() < 560.0 { 136.0 } else { 172.0 };
        egui::Panel::left(egui::Id::new(("settings-nav", &d.host)))
            .resizable(false)
            .default_size(nav_w)
            .size_range(nav_w..=nav_w)
            .show_separator_line(true)
            .frame(egui::Frame::new().fill(theme::surface()).inner_margin(egui::Margin::symmetric(6, 10)))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                for (s, state) in &states {
                    if nav_item(ui, s.title(), state, self.section == *s).clicked() {
                        self.section = *s;
                    }
                }
            });
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::bg())).show(ui, |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin { left: 20, right: 20, top: 16, bottom: 20 }).show(ui, |ui| {
                    ui.set_width(ui.available_width().min(720.0));
                    let a = match self.section {
                        Section::Info => info_section(ui, d, app),
                        Section::Access => self.access_section(ui, ctx, backend, d),
                        Section::Runtime => runtime_section(ui, app),
                        Section::Visibility => visibility_section(ui, d),
                        Section::Variables => {
                            self.vars_section(ui, ctx, backend, d, app);
                            None
                        }
                        Section::Image => {
                            self.image_section(ui, ctx, backend);
                            None
                        }
                    };
                    if a.is_some() {
                        act = a;
                    }
                });
            });
        });
        act
    }

    fn info_state(&self, d: &DomainInfo, app: Option<&AppStatus>) -> String {
        match app {
            Some(a) if !a.language.is_empty() => a.language.clone(),
            _ if !d.title.is_empty() => d.title.clone(),
            _ => "Address and folder".to_owned(),
        }
    }

    fn access_state(&self) -> String {
        match &*self.access.lock().unwrap() {
            Some(Ok(v)) => LEVELS.iter().find(|(l, _, _)| *l == v.level).map(|(_, w, _)| (*w).to_owned()).unwrap_or_else(|| "Admins only".to_owned()),
            Some(Err(_)) => "Could not read".to_owned(),
            None => "Reading…".to_owned(),
        }
    }

    fn vars_state(&self, app: Option<&AppStatus>) -> String {
        if app.is_some_and(|a| a.kind == "module") {
            return "None for a module".to_owned();
        }
        match self.edit.len() {
            0 => "None set".to_owned(),
            1 => "1 set".to_owned(),
            n => format!("{n} set"),
        }
    }

    /// Access is managed in one place, the Share window (access.rs): who
    /// can view it, and each person's level (referral #734, the owner,
    /// 2026-11-03: "manage it twice is confusing, should be once only, who
    /// can view is different level than who can access"). Here it is said
    /// once, with the way there.
    fn access_section(&mut self, ui: &mut egui::Ui, _ctx: &egui::Context, _backend: &Backend, d: &DomainInfo) -> Option<Act> {
        let view = self.access.lock().unwrap().clone();
        heading(ui, "Access", "Who can view this app, and who can do more with it.");
        match &view {
            Some(Ok(v)) => {
                let (word, meaning) = LEVELS.iter().find(|(l, _, _)| *l == v.level).map_or(("Admins only", "only ndexr admins - set by an admin"), |(_, w, m)| (*w, *m));
                ui.label(RichText::new(format!("Who can view it: {word}")).size(14.5).strong().color(theme::text_strong()));
                ui.label(RichText::new(meaning).size(13.5).color(theme::text()));
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).size(13.5).color(theme::warn()));
            }
            None => {
                ui.spinner();
            }
        }
        ui.add_space(12.0);
        if ui.add(button("Share…")).on_hover_text("who can view it, and who can see the code, work on it or manage it").clicked() {
            return Some(Act::Access(d.host.clone()));
        }
        None
    }

    fn vars_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, d: &DomainInfo, app: Option<&AppStatus>) {
        heading(ui, "Variables", "Environment variables the app starts with.");
        if app.is_some_and(|a| a.kind == "module") {
            ui.label(
                RichText::new("This app is a Shiny module, run by the platform's shared R sessions, which carry no per-app variables. An app with its own process (APP_KIND=port in .cove/app.env) has them.")
                    .size(13.5)
                    .color(theme::text()),
            );
            return;
        }
        if !self.loaded {
            ui.spinner();
            return;
        }
        let saved: Vec<(String, String)> = self.vars.lock().unwrap().clone().unwrap_or_default();
        let note = self.note.lock().unwrap().clone();
        let mut remove = None;
        let w = ui.available_width();
        egui::Frame::new().stroke(egui::Stroke::new(1.0, theme::moss())).corner_radius(8.0).show(ui, |ui| {
            ui.set_width(w);
            egui::Frame::new().fill(theme::surface()).corner_radius(egui::CornerRadius { nw: 8, ne: 8, sw: 0, se: 0 }).inner_margin(egui::Margin::symmetric(12, 7)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.add_sized([w * 0.32, 16.0], egui::Label::new(RichText::new("NAME").size(11.5).strong().color(theme::text())));
                    ui.label(RichText::new("VALUE").size(11.5).strong().color(theme::text()));
                });
            });
            egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.edit.is_empty() {
                    ui.label(RichText::new("No variables yet. Add the first below.").size(13.5).color(theme::text()));
                }
                for (i, (k, v, shown)) in self.edit.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.add_sized([w * 0.32, 22.0], egui::Label::new(RichText::new(k.as_str()).monospace().size(13.0).color(theme::text_strong())).truncate());
                        ui.add(egui::TextEdit::singleline(v).password(!*shown).desired_width((w * 0.42).max(120.0)).font(egui::TextStyle::Monospace));
                        if ui.small_button(if *shown { "Hide" } else { "Show" }).clicked() {
                            *shown = !*shown;
                        }
                        if ui.small_button("Remove").clicked() {
                            remove = Some(i);
                        }
                    });
                }
            });
        });
        if let Some(i) = remove {
            self.edit.remove(i);
        }
        ui.add_space(10.0);
        subheading(ui, "Add a variable");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.new_key).hint_text("NAME").desired_width((w * 0.3).max(110.0)).font(egui::TextStyle::Monospace));
            ui.add(egui::TextEdit::singleline(&mut self.new_val).hint_text("value").password(true).desired_width((w * 0.42).max(120.0)).font(egui::TextStyle::Monospace));
            let key = self.new_key.trim().to_owned();
            let why = key_problem(&key, &self.edit);
            if ui.add_enabled(why.is_none() && !key.is_empty(), button("Add")).on_disabled_hover_text(why.clone().unwrap_or_default()).clicked() {
                self.edit.push((key, std::mem::take(&mut self.new_val), false));
                self.new_key.clear();
            }
        });
        if let Some(w) = key_problem(self.new_key.trim(), &self.edit) {
            ui.label(RichText::new(w).size(13.0).color(theme::warn()));
        }
        let changed = self.edit.iter().map(|(k, v, _)| (k.clone(), v.clone())).collect::<Vec<_>>() != saved;
        ui.add_space(14.0);
        let mut save = false;
        ui.horizontal(|ui| {
            if ui.add_enabled(changed, theme::primary_button("Save variables").min_size(egui::vec2(0.0, 30.0))).clicked() {
                save = true;
            }
            match (&note, changed) {
                (_, true) => {
                    ui.label(RichText::new("Changes not saved yet").size(13.0).color(theme::amber()));
                }
                (Some(Ok(t)), false) => {
                    ui.label(RichText::new(format!("✓ {t}")).size(13.0).color(theme::ok()));
                }
                (Some(Err(e)), false) => {
                    ui.label(RichText::new(e).size(13.0).color(theme::warn()));
                }
                _ => {}
            }
        });
        ui.add_space(10.0);
        ui.label(RichText::new(format!("Kept in {VARS_AT}, which is out of the project's saved versions (.gitignore). Up gives them to the app.")).size(13.0).color(theme::text()));
        if save {
            self.save(d, backend, ctx);
        }
    }
}

/// The sections of the side menu, in Connect's order.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Section {
    #[default]
    Info,
    Access,
    Runtime,
    Visibility,
    Variables,
    Image,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Info => "Info",
            Section::Access => "Access",
            Section::Runtime => "Runtime",
            Section::Visibility => "Visibility",
            Section::Variables => "Variables",
            Section::Image => "Container image",
        }
    }
}

const LEVELS: [(&str, &str, &str); 3] = [
    ("public", "Everyone", "Anyone on the web, signed in or not."),
    ("login", "Anyone signed in", "Anyone with an ndexr account."),
    ("owner", "People you choose", "You, and the people and groups you share it with."),
];

impl SettingsPane {
    fn ask_image(&mut self, verb: &str, backend: &Backend, ctx: &egui::Context) {
        let c = ctx.clone();
        match verb {
            "status" => {
                let slot = self.image.clone();
                backend.query(Query::Image { host: self.host.clone(), verb: "status".into() }, ctx, move |r: Result<ImageStatus, String>| {
                    *slot.lock().unwrap() = Some(r);
                    c.request_repaint();
                });
            }
            "pull" => {
                let slot = self.image_login.clone();
                backend.query(Query::Image { host: self.host.clone(), verb: "pull".into() }, ctx, move |r: Result<ImageLogin, String>| {
                    *slot.lock().unwrap() = Some(r);
                    c.request_repaint();
                });
            }
            verb => {
                let slot = self.image_job.clone();
                backend.query(Query::Image { host: self.host.clone(), verb: verb.into() }, ctx, move |r: Result<JobView, String>| {
                    *slot.lock().unwrap() = Some(r);
                    c.request_repaint();
                });
            }
        }
    }

    fn image_state(&self) -> String {
        match &*self.image.lock().unwrap() {
            None => "Not asked yet".to_owned(),
            Some(Err(_)) => "Not available".to_owned(),
            Some(Ok(s)) if s.building => "Building".to_owned(),
            Some(Ok(s)) if s.versions.is_empty() => "No image yet".to_owned(),
            Some(Ok(s)) => format!("{} version{}", s.versions.len(), if s.versions.len() == 1 { "" } else { "s" }),
        }
    }

    /// Settings › Container image (operator, 2026-11-01: every project
    /// "capable of producing a runnable container image", built and kept by
    /// ndexr). What is published, Build / Rebuild, what it needs to run -
    /// as measured, never "portable" on its own - and how to pull and run it.
    fn image_section(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend) {
        let now = ctx.input(|i| i.time);
        // Asked on arriving, and every 3 s while a build runs.
        let job = self.image_job.lock().unwrap().clone();
        let running = matches!(&job, Some(Ok(j)) if j.running);
        if now - self.image_asked > if running { 3.0 } else { 60.0 } {
            self.image_asked = now;
            self.ask_image("status", backend, ctx);
            if running {
                self.ask_image("job", backend, ctx);
            }
        }
        if running {
            ctx.request_repaint_after(std::time::Duration::from_secs(3));
        }
        heading(ui, "Container image", "A versioned image of this project that you can pull and run - built, published to Amazon ECR and kept by ndexr. Every version stays; a new build never replaces an old one.");
        let status = self.image.lock().unwrap().clone();
        let s = match status {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("asking ndexr…").size(13.5).color(theme::text()));
                });
                return;
            }
            Some(Err(e)) => {
                badge(ui, "Not available", theme::warn());
                ui.add_space(8.0);
                ui.label(RichText::new(e).size(13.5).color(theme::text_strong()));
                return;
            }
            Some(Ok(s)) => s,
        };
        let busy = running || s.building;
        ui.horizontal_wrapped(|ui| {
            if busy {
                badge(ui, "Building", theme::amber());
            } else if s.versions.is_empty() {
                badge(ui, "No image yet", theme::text());
            } else {
                badge(ui, &format!("{} version{}", s.versions.len(), if s.versions.len() == 1 { "" } else { "s" }), theme::ok());
            }
            let word = if s.versions.is_empty() { "Build image" } else { "Rebuild image" };
            if ui.add_enabled(!busy, button(word)).on_hover_text("ndexr builds this project as it is now, checks that it starts, and publishes it as a new version").clicked() {
                self.ask_image("build", backend, ctx);
                self.image_asked = now;
            }
        });
        if s.changed_since && !s.versions.is_empty() && !busy {
            ui.add_space(6.0);
            ui.label(RichText::new("The project changed since the newest image: Rebuild publishes it as a new version.").size(13.5).color(theme::text_strong()));
        }
        // The build as far as it got.
        if let Some(Ok(j)) = &job {
            ui.add_space(10.0);
            let (word, c) = match (j.running, j.ok) {
                (true, _) => ("Building…", theme::amber()),
                (false, Some(true)) => ("Built and published", theme::ok()),
                _ => ("The build failed - its log is below", theme::alt()),
            };
            ui.label(RichText::new(word).size(13.5).strong().color(c));
            let tail: Vec<&String> = j.lines.iter().rev().take(14).collect::<Vec<_>>().into_iter().rev().collect();
            egui::Frame::new().fill(theme::surface()).corner_radius(6.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for l in tail {
                    ui.add(egui::Label::new(RichText::new(l).monospace().size(12.0).color(theme::text())).truncate());
                }
            });
        } else if let Some(Err(e)) = &job {
            ui.add_space(8.0);
            ui.label(RichText::new(format!("Not started: {e}")).size(13.5).color(theme::alt()));
        }
        let Some(v) = s.versions.first() else {
            ui.add_space(10.0);
            ui.label(RichText::new("No image has been published for this project yet.").size(13.5).color(theme::text()));
            return;
        };
        ui.add_space(16.0);
        subheading(ui, "Newest version");
        let reference = format!("{}@{}", s.repository, v.digest);
        facts(ui, "image-facts", &[
            ("Repository", s.repository.clone(), true),
            ("Digest", v.digest.clone(), true),
            ("Tag", v.tag.clone(), true),
            ("Built", v.built_at.clone(), false),
            ("From", v.source.clone(), true),
            ("Size", format!("{:.0} MB", v.size_bytes as f64 / 1e6), false),
            ("Checked", if v.validated { "it started and answered".to_owned() } else { "not checked".to_owned() }, false),
        ]);
        ui.horizontal(|ui| {
            if ui.add(button("Copy the image reference")).on_hover_text(reference.clone()).clicked() {
                ctx.copy_text(reference.clone());
            }
        });
        if s.versions.len() > 1 {
            ui.add_space(12.0);
            subheading(ui, &format!("Earlier versions · {}", s.versions.len() - 1));
            for e in s.versions.iter().skip(1).take(20) {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&e.tag).monospace().size(12.5).color(theme::text_strong()));
                    let short = e.digest.trim_start_matches("sha256:").chars().take(12).collect::<String>();
                    if ui.add(egui::Label::new(RichText::new(short).monospace().size(12.5).color(theme::text())).sense(egui::Sense::click())).on_hover_text("copy its reference").clicked() {
                        ctx.copy_text(format!("{}@{}", s.repository, e.digest));
                    }
                });
            }
        }
        ui.add_space(16.0);
        subheading(ui, "What it needs to run");
        for need in needs_words(&s.needs) {
            ui.label(RichText::new(format!("• {need}")).size(13.5).color(theme::text_strong()));
        }
        ui.add_space(16.0);
        subheading(ui, "Pull and run it");
        let login = self.image_login.lock().unwrap().clone();
        match &login {
            None => {
                if ui.add(button("Get a pull login")).on_hover_text("a login that can pull only your images, for a few hours").clicked() {
                    self.ask_image("pull", backend, ctx);
                }
            }
            Some(Err(e)) => {
                ui.label(RichText::new(format!("No login: {e}")).size(13.5).color(theme::alt()));
            }
            Some(Ok(l)) => {
                ui.label(RichText::new(format!("This login pulls only your images, until {}.", l.expires_at)).size(13.5).color(theme::text_strong()));
            }
        }
        let password = match &login {
            Some(Ok(l)) => l.password.clone(),
            _ => "<the password from Get a pull login>".to_owned(),
        };
        let registry = s.repository.split('/').next().unwrap_or("").to_owned();
        let (apptainer, podman) = run_commands(&s.needs, &s.port, &reference);
        ui.add_space(8.0);
        // Anywhere the ndexr CLI has the person's API key, it fetches the login
        // itself (api.ndexr.io/v1/images): no password to copy.
        let project = s.repository.rsplit('/').next().unwrap_or("");
        command(ui, ctx, "With the ndexr CLI and your API key", &format!("export NDEXR_API_KEY='<your key from api.ndexr.io>'\nndexr image pull {project} --tag {}\nndexr image pull {project} --tag {} --podman", v.tag, v.tag));
        command(ui, ctx, "Apptainer", &format!("export APPTAINER_DOCKER_USERNAME=AWS APPTAINER_DOCKER_PASSWORD='{password}'\n{apptainer}"));
        command(ui, ctx, "Podman or Docker", &format!("echo '{password}' | podman login --username AWS --password-stdin {registry}\n{podman}"));
    }
}

/// The run commands, as ndexr checked the image before publishing it: the
/// host's network (apps listen on 127.0.0.1, so a podman `-p` publish never
/// reaches them; Apptainer shares the network anyway), PORT for the port, and
/// read-only binds of exactly the folders it needed.
fn run_commands(needs: &crate::api::ImageNeeds, port: &str, reference: &str) -> (String, String) {
    let binds: Vec<String> = if needs.binds.is_empty() && needs.stack { vec!["/opt/ndexr/stack".to_owned()] } else { needs.binds.clone() };
    let env = if port.is_empty() { String::new() } else { format!("PORT={port} ") };
    let bind = if binds.is_empty() { String::new() } else { format!(" --bind {}", binds.join(",")) };
    let vols: String = binds.iter().map(|b| format!(" -v {b}:{b}:ro")).collect();
    let penv = if port.is_empty() { String::new() } else { format!(" -e PORT={port}") };
    (
        format!("{env}apptainer run --writable-tmpfs{bind} docker://{reference}"),
        format!("podman run --rm --network host{penv}{vols} {reference}"),
    )
}

/// What an image needs, each said plainly: nothing claims it runs on its own
/// unless ndexr measured that it does.
fn needs_words(n: &crate::api::ImageNeeds) -> Vec<String> {
    let mut v = Vec::new();
    if n.stack {
        let rev = if n.stack_revision.is_empty() { String::new() } else { format!(", revision {} or newer", n.stack_revision) };
        v.push(format!("The ndexr software stack mounted at /opt/ndexr/stack (CVMFS stack.ndexr.io{rev}) - on ndexr, in an account cell, or on Linux with the CVMFS client. The image does not carry it."));
        if !n.modules.is_empty() {
            v.push(format!("Modules it loads from the stack: {}", n.modules.join(", ")));
        }
    }
    if n.platform_r {
        v.push("ndexr's own R code (its shell and inputs modules), carried in the image at the revision it was built from".to_owned());
    }
    if n.gpu {
        v.push("A GPU and its host driver".to_owned());
    }
    for s in &n.services {
        v.push(format!("An outside service: {s}"));
    }
    if !n.arch.is_empty() {
        v.push(format!("A {} machine: it was built for that architecture only", n.arch));
    }
    if !n.stack && !n.platform_r && !n.gpu && n.services.is_empty() {
        v.insert(0, "Nothing outside the image: it runs on its own".to_owned());
    }
    v
}

/// A command to copy: its runtime, the lines, and Copy.
fn command(ui: &mut egui::Ui, ctx: &egui::Context, title: &str, text: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).size(13.5).strong().color(theme::text_strong()));
        if ui.small_button("Copy").clicked() {
            ctx.copy_text(text.to_owned());
        }
    });
    egui::Frame::new().fill(theme::surface()).corner_radius(6.0).inner_margin(egui::Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(RichText::new(text).monospace().size(12.5).color(theme::text_strong())).wrap());
    });
    ui.add_space(8.0);
}

/// A side-menu row: the section and its state underneath; the chosen one
/// carries an accent bar.
fn nav_item(ui: &mut egui::Ui, title: &str, state: &str, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 46.0), egui::Sense::click());
    let p = ui.painter();
    if on {
        p.rect_filled(rect, 6.0, theme::primary().gamma_multiply(0.16));
        p.rect_filled(egui::Rect::from_min_size(rect.min + egui::vec2(0.0, 8.0), egui::vec2(3.0, rect.height() - 16.0)), 2.0, theme::primary());
    } else if resp.hovered() {
        p.rect_filled(rect, 6.0, theme::surface_hi());
    }
    let x = rect.left() + 14.0;
    p.text(egui::pos2(x, rect.top() + 8.0), egui::Align2::LEFT_TOP, title, theme::display(14.0), if on { theme::text_strong() } else { theme::text() });
    let st = egui::text::LayoutJob::simple_singleline(state.to_owned(), egui::FontId::proportional(12.0), if on { theme::text_strong() } else { theme::text() });
    let galley = p.layout_job(egui::text::LayoutJob { wrap: egui::text::TextWrapping::truncate_at_width(rect.width() - 20.0), ..st });
    p.galley(egui::pos2(x, rect.top() + 26.0), galley, theme::text());
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A full-width choice: a radio mark, the choice, and what it means.
pub(crate) fn option_row(ui: &mut egui::Ui, title: &str, meaning: &str, on: bool, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 54.0), if enabled { egui::Sense::click() } else { egui::Sense::hover() });
    let p = ui.painter();
    let fill = if on { theme::primary().gamma_multiply(0.14) } else if resp.hovered() { theme::surface_hi() } else { theme::surface() };
    p.rect(rect.shrink2(egui::vec2(0.0, 2.0)), 8.0, fill, egui::Stroke::new(1.0, if on { theme::primary() } else { theme::moss() }), egui::StrokeKind::Inside);
    let c = egui::pos2(rect.left() + 20.0, rect.center().y);
    p.circle_stroke(c, 8.0, egui::Stroke::new(1.5, if on { theme::primary() } else { theme::text() }));
    if on {
        p.circle_filled(c, 4.5, theme::primary());
    }
    p.text(egui::pos2(rect.left() + 40.0, rect.top() + 10.0), egui::Align2::LEFT_TOP, title, theme::display(14.0), theme::text_strong());
    p.text(egui::pos2(rect.left() + 40.0, rect.top() + 30.0), egui::Align2::LEFT_TOP, meaning, egui::FontId::proportional(12.5), theme::text());
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

fn heading(ui: &mut egui::Ui, title: &str, what: &str) {
    ui.label(RichText::new(title).font(theme::display(20.0)).color(theme::text_strong()));
    ui.add_space(2.0);
    ui.label(RichText::new(what).size(13.5).color(theme::text()));
    ui.add_space(14.0);
}

fn subheading(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).font(theme::display(14.5)).color(theme::text_strong()));
    ui.add_space(4.0);
}

fn button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_owned()).size(13.5).color(theme::text_strong())).min_size(egui::vec2(0.0, 30.0)).corner_radius(6.0)
}

/// A state badge: a dot and a word, in the state's colour.
fn badge(ui: &mut egui::Ui, text: &str, color: egui::Color32) {
    egui::Frame::new().fill(color.gamma_multiply(0.15)).stroke(egui::Stroke::new(1.0, color)).corner_radius(12.0).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
        ui.label(RichText::new(format!("● {text}")).size(13.0).strong().color(color));
    });
}

/// Facts in two aligned columns.
fn facts(ui: &mut egui::Ui, id: &str, rows: &[(&str, String, bool)]) {
    egui::Grid::new(id).num_columns(2).spacing([22.0, 10.0]).show(ui, |ui| {
        for (k, v, mono) in rows {
            ui.label(RichText::new(*k).size(13.0).color(theme::text()));
            let t = RichText::new(v).size(13.5).color(theme::text_strong());
            ui.label(if *mono { t.monospace() } else { t });
            ui.end_row();
        }
    });
}

fn runtime_state(app: Option<&AppStatus>) -> String {
    match app {
        None => "Reading…".to_owned(),
        Some(a) if a.kind == "module" => "Platform-run".to_owned(),
        Some(a) if a.compiling.is_some() => "Building".to_owned(),
        Some(a) if a.running => "Running".to_owned(),
        Some(_) => "Stopped".to_owned(),
    }
}

fn info_section(ui: &mut egui::Ui, d: &DomainInfo, app: Option<&AppStatus>) -> Option<Act> {
    heading(ui, "Info", "What this app is, and where it lives.");
    let url = format!("https://{}", d.host);
    if !d.target.is_empty() {
        ui.hyperlink_to(RichText::new(format!("Open {url}  ↗")).size(14.0).strong(), &url);
        ui.add_space(12.0);
    }
    let mut rows: Vec<(&str, String, bool)> = Vec::new();
    if !d.title.is_empty() {
        rows.push(("Title", d.title.clone(), false));
    }
    rows.push(("Address", url, true));
    rows.push(("Environment", if d.env.is_empty() { "the platform default".to_owned() } else { d.env.clone() }, true));
    rows.push(("Folder", d.path.clone(), true));
    facts(ui, "settings-info", &rows);
    // The project's type, one pick away (operator, 2026-11-01: "should be
    // able to swap out project types with a selectize in the cove"): every
    // type's scripts are in .cove/, so a pick changes one line of
    // .cove/app.env. The app's own code stays as it is.
    let Some(a) = app else { return None };
    let mut act = None;
    ui.add_space(14.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Type").size(13.0).color(theme::text()));
        ui.add_space(12.0);
        let now = crate::dotcove::TYPES.iter().find(|t| t.0 == a.language).map_or_else(|| if a.language.is_empty() { "not set".to_owned() } else { a.language.clone() }, |t| t.2.to_owned());
        egui::ComboBox::from_id_salt("settings-type").width(300.0).selected_text(RichText::new(now).size(13.5).color(theme::text_strong())).show_ui(ui, |ui| {
            for (lang, _, name) in crate::dotcove::TYPES {
                if ui.selectable_label(a.language == *lang, RichText::new(*name).size(13.0)).clicked() && a.language != *lang {
                    act = Some(Act::SetType((*lang).to_owned()));
                }
            }
        });
    });
    ui.add_space(4.0);
    ui.label(RichText::new("Builds, starts and stops it with that type's scripts in .cove/. Your code stays as it is: a new type needs its own app file (app.py, server.js, server.jl, src/main.rs or module.r).").size(13.0).color(theme::text()));
    act
}

fn runtime_section(ui: &mut egui::Ui, app: Option<&AppStatus>) -> Option<Act> {
    heading(ui, "Runtime", "What runs the app, and starting or stopping it.");
    let Some(a) = app else {
        ui.spinner();
        return None;
    };
    let mut act = None;
    if a.kind == "module" {
        badge(ui, "Run by the platform", theme::ok());
        ui.add_space(10.0);
        ui.label(RichText::new("A Shiny module: the platform's shared R sessions run it, so there is nothing to start or stop here.").size(13.5).color(theme::text()));
    } else {
        ui.horizontal(|ui| match (a.compiling, a.running) {
            (Some(s), _) => badge(ui, &format!("Building · {s} s"), theme::amber()),
            (None, true) => badge(ui, "Running", theme::ok()),
            (None, false) => badge(ui, "Stopped", theme::warn()),
        });
        ui.add_space(12.0);
        let has = |v: &str| a.verbs.iter().any(|x| x == v);
        ui.horizontal(|ui| {
            if has("up") && !a.running && ui.add(theme::primary_button("Start").min_size(egui::vec2(84.0, 30.0))).on_hover_text(".cove/up.sh").clicked() {
                act = Some(Act::Run("up".into()));
            }
            if has("down") && a.running && ui.add(button("Stop")).on_hover_text(".cove/down.sh").clicked() {
                act = Some(Act::Run("down".into()));
            }
            if has("build") && ui.add(button("Rebuild")).on_hover_text(".cove/build.sh").clicked() {
                act = Some(Act::Run("build".into()));
            }
        });
        if !a.built {
            ui.add_space(4.0);
            ui.label(RichText::new("Nothing built yet: Rebuild first, then Start.").size(13.0).color(theme::amber()));
        }
    }
    ui.add_space(16.0);
    let mut rows: Vec<(&str, String, bool)> = vec![
        ("Language", if a.language.is_empty() { "not set".to_owned() } else { a.language.clone() }, false),
        ("Runs as", match a.kind.as_str() {
            "module" => "Shiny module".to_owned(),
            "port" => format!("its own process, port {}", a.port),
            k => k.to_owned(),
        }, false),
    ];
    if a.kind != "module" && !a.health.is_empty() {
        rows.push(("Health check", a.health.clone(), true));
    }
    if !a.verbs.is_empty() {
        rows.push(("Scripts", a.verbs.iter().map(|v| format!("{v}.sh")).collect::<Vec<_>>().join("  "), true));
    }
    facts(ui, "settings-runtime", &rows);
    act
}

fn visibility_section(ui: &mut egui::Ui, d: &DomainInfo) -> Option<Act> {
    heading(ui, "Visibility", "Whether the app is on the web.");
    let mut act = None;
    if d.target.is_empty() {
        badge(ui, "Not on the web", theme::amber());
        ui.add_space(10.0);
        ui.label(RichText::new("Only you see it, here in Cove. Publishing gives it an address, a certificate and a site; it answers in a minute or two.").size(13.5).color(theme::text()));
        ui.add_space(10.0);
        if ui.add(theme::primary_button("Publish").min_size(egui::vec2(96.0, 30.0))).clicked() {
            act = Some(Act::Publish(d.host.clone()));
        }
    } else {
        badge(ui, "On the web", theme::ok());
        ui.add_space(12.0);
        let url = format!("https://{}", d.host);
        ui.hyperlink_to(RichText::new(format!("Open {url}  ↗")).size(14.0).strong(), &url);
        ui.add_space(12.0);
        facts(ui, "settings-visibility", &[("Address", url.clone(), true), ("Served by", d.target.clone(), true)]);
        ui.add_space(10.0);
        ui.label(RichText::new("Who may open it is set under Access.").size(13.0).color(theme::text()));
    }
    act
}

/// Why `key` cannot be added ("" never: None when it can).
fn key_problem(key: &str, rows: &[(String, String, bool)]) -> Option<String> {
    if key.is_empty() {
        return None;
    }
    let ok = key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !ok {
        return Some("letters, digits and _, not starting with a digit".to_owned());
    }
    if key.starts_with("APP_") {
        return Some("APP_ names belong to app.env".to_owned());
    }
    if rows.iter().any(|(k, _, _)| k == key) {
        return Some("already set above".to_owned());
    }
    None
}

/// `KEY='value'` lines (what write_vars writes; any sh-quoted value reads back).
pub fn parse_vars(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .filter_map(|l| l.split_once('='))
        .filter(|(k, _)| !k.trim().is_empty())
        .map(|(k, v)| {
            let v = v.trim();
            let v = if v.len() >= 2 && v.starts_with('\'') && v.ends_with('\'') { v[1..v.len() - 1].replace(r"'\''", "'") } else { v.trim_matches('"').to_owned() };
            (k.trim().to_owned(), v)
        })
        .collect()
}

/// Every row as `KEY='value'`, safe to `.` from sh whatever the value holds.
pub fn write_vars(rows: &[(String, String)]) -> String {
    let mut s = String::from("# This app's environment variables (Cove › Settings), exported by Up.\n");
    for (k, v) in rows {
        s.push_str(&format!("{k}={}\n", crate::api::shell_quote(v)));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The contract's own examples (docs/container-images.md) are what Cove
    /// reads: a change to one side that the other does not follow fails here.
    #[test]
    fn the_contracts_examples_parse() {
        let doc = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../docs/container-images.md")).unwrap();
        let blocks: Vec<&str> = doc.split("```json").skip(1).map(|b| b.split("```").next().unwrap()).collect();
        assert_eq!(blocks.len(), 2, "status and pull-login");
        let s: crate::api::ImageStatus = serde_json::from_str(blocks[0]).unwrap();
        assert!(s.repository.ends_with("/cove/<owner>/<host>") && s.versions[0].digest.starts_with("sha256:") && s.versions[0].validated);
        assert!(s.needs.stack && s.needs.platform_r && s.needs.modules == ["env/r4.6_py3.13-release"]);
        let l: crate::api::ImageLogin = serde_json::from_str(blocks[1]).unwrap();
        assert_eq!(l.username, "AWS");
        assert!(!l.expires_at.is_empty());
    }

    #[test]
    fn run_commands_bind_what_was_measured() {
        let mut n = crate::api::ImageNeeds { stack: true, ..Default::default() };
        let (a, p) = super::run_commands(&n, "8903", "r/x@sha256:1");
        assert_eq!(a, "PORT=8903 apptainer run --writable-tmpfs --bind /opt/ndexr/stack docker://r/x@sha256:1");
        assert_eq!(p, "podman run --rm --network host -e PORT=8903 -v /opt/ndexr/stack:/opt/ndexr/stack:ro r/x@sha256:1");
        n.binds = vec!["/opt/ndexr/stack".into(), "/opt/ndexr/site-library".into()];
        assert!(super::run_commands(&n, "", "r").0.contains("--bind /opt/ndexr/stack,/opt/ndexr/site-library"));
        n = crate::api::ImageNeeds::default();
        assert_eq!(super::run_commands(&n, "", "r").1, "podman run --rm --network host r");
    }

    #[test]
    fn an_image_never_claims_to_run_on_its_own_unless_measured() {
        let mut n = crate::api::ImageNeeds { stack: true, stack_revision: "37".into(), modules: vec!["env/r4.6_py3.13-release".into()], platform_r: true, arch: "x86_64".into(), ..Default::default() };
        let w = super::needs_words(&n);
        assert!(w[0].contains("/opt/ndexr/stack") && w[0].contains("revision 37") && w[0].contains("does not carry it"), "{w:?}");
        assert!(w.iter().any(|l| l.contains("ndexr's own R code")));
        assert!(!w.iter().any(|l| l.contains("on its own")), "{w:?}");
        n = crate::api::ImageNeeds { arch: "x86_64".into(), ..Default::default() };
        assert_eq!(super::needs_words(&n)[0], "Nothing outside the image: it runs on its own");
        n.services = vec!["PostgreSQL".into()];
        assert!(!super::needs_words(&n).iter().any(|l| l.contains("on its own")));
    }

    #[test]
    fn variables_round_trip_whatever_the_value_holds() {
        let rows = vec![("TOKEN".to_owned(), "a b'c$d\"e".to_owned()), ("URL".to_owned(), "https://x/?a=1&b=2".to_owned())];
        assert_eq!(parse_vars(&write_vars(&rows)), rows);
    }

    #[test]
    fn a_name_is_checked_before_it_is_added() {
        let rows = vec![("A".to_owned(), String::new(), false)];
        assert!(key_problem("GOOD_1", &rows).is_none());
        assert!(key_problem("1BAD", &rows).is_some());
        assert!(key_problem("APP_PORT", &rows).is_some());
        assert!(key_problem("A", &rows).is_some());
    }
}
