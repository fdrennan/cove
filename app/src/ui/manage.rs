//! Connections as the place to run your projects (operator, 2026-11-17: "a
//! full service like the ide space for quick management of process when you
//! are the owner of them, in row like fashion ... similar to what we have in
//! route.ndexr.io for that domain ... update its access permissions, remove
//! the project, archive ... while we also focus on multi project management
//! for quick multi project choices").
//!
//! Your own projects get working rows: a tick to select it, whether it is
//! running, who can view it (changed from the row), Start/Stop, and a More menu of
//! the rest - share, publish, route, details, archive, delete. Ticked rows get
//! one bar of actions for all of them at once. Every action is a query the IDE
//! already uses for the open project (AppAction, AppStatus, AppAccess,
//! AppViewer, AppRoute, SiteGate, SiteRequests), asked here by host - one
//! implementation of each, not two.

use std::collections::{BTreeSet, HashMap};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::api::{AppAccessView, AppStatus, DomainInfo, JobView, Query};
use crate::backend::Backend;
use crate::ui::kit::theme::{self, State};

type Slot<T> = Arc<Mutex<Option<T>>>;

/// What a row or the bar asks app.rs to do with the windows it owns.
pub enum Act {
    OpenIde(DomainInfo),
    /// The Share window (access.rs): who can view it, people and groups.
    Share(String),
    /// Put it on the web (provision: DNS, certificate, vhost).
    Publish(String),
    /// Archive (false) or Delete (true), through the IDE's confirm dialog.
    Remove(String, bool),
}

/// The levels, as the Share window words them.
pub const LEVELS: [(&str, &str); 4] = [("public", "Anyone"), ("login", "Signed in"), ("owner", "Only you and who you share with"), ("admin", "Admins")];

#[derive(Default)]
pub struct Manage {
    pub selected: BTreeSet<String>,
    /// Each project's run state, asked when its row is on screen (every 15 s).
    status: HashMap<String, (f64, Slot<Result<AppStatus, String>>)>,
    /// Who can view each, once, and again after a change.
    access: HashMap<String, Slot<Result<AppAccessView, String>>>,
    /// A level just chosen, shown at once while it is saved.
    pending: HashMap<String, String>,
    /// A Start / Stop / Build going, by host: its verb.
    running: HashMap<String, Slot<Result<JobView, String>>>,
    /// The last thing that went wrong or right, said once above the list.
    note: Slot<Result<String, String>>,
    /// The project whose details window is open.
    details: Option<DomainInfo>,
    gate: Slot<(String, Result<String, String>)>,
    requests: Slot<(String, Result<serde_json::Value, String>)>,
    /// The page password being typed in Details, and what setting it said.
    pw: String,
    pw_note: Slot<Result<String, String>>,
    pw_busy: bool,
    /// Projects ticked for Archive or Delete, taken one confirm at a time
    /// (app.rs hands the next to the IDE's dialog when it is free).
    queue: Vec<(String, bool)>,
}

impl Manage {
    /// Its state, from the last answer; asks again every 15 s while drawn.
    fn status(&mut self, host: &str, backend: &Backend, ctx: &egui::Context) -> Option<Result<AppStatus, String>> {
        let now = ctx.input(|i| i.time);
        let e = self.status.entry(host.to_owned()).or_insert_with(|| (f64::NEG_INFINITY, Default::default()));
        if now - e.0 > 15.0 {
            e.0 = now;
            let (slot, c) = (e.1.clone(), ctx.clone());
            backend.query(Query::AppStatus { host: host.to_owned() }, ctx, move |r: Result<AppStatus, String>| {
                *slot.lock().unwrap() = Some(r);
                c.request_repaint();
            });
        }
        let v = e.1.lock().unwrap().clone();
        v
    }

    fn level(&mut self, host: &str, backend: &Backend, ctx: &egui::Context) -> Option<String> {
        if let Some(p) = self.pending.get(host) {
            return Some(p.clone());
        }
        let slot = self.access.entry(host.to_owned()).or_insert_with(|| {
            let s: Slot<Result<AppAccessView, String>> = Default::default();
            let (slot, c) = (s.clone(), ctx.clone());
            backend.query(Query::AppAccess { host: host.to_owned(), level: String::new() }, ctx, move |r: Result<AppAccessView, String>| {
                *slot.lock().unwrap() = Some(r);
                c.request_repaint();
            });
            s
        });
        let v = slot.lock().unwrap().clone();
        v.and_then(|r| r.ok()).map(|v| v.level)
    }

    /// Who can view `host`, changed: shown at once, saved behind it.
    pub fn set_level(&mut self, host: &str, level: &str, backend: &Backend, ctx: &egui::Context) {
        self.pending.insert(host.to_owned(), level.to_owned());
        let slot = self.access.entry(host.to_owned()).or_default().clone();
        let (note, c, h) = (self.note.clone(), ctx.clone(), host.to_owned());
        backend.query(Query::AppAccess { host: host.to_owned(), level: level.to_owned() }, ctx, move |r: Result<AppAccessView, String>| {
            match r {
                Ok(v) => {
                    *note.lock().unwrap() = Some(Ok(if v.said.is_empty() { format!("{h}: access changed") } else { v.said.clone() }));
                    *slot.lock().unwrap() = Some(Ok(v));
                }
                Err(e) => *note.lock().unwrap() = Some(Err(format!("{h}: not changed - {e}"))),
            }
            c.request_repaint();
        });
    }

    /// Start, Stop or Build one project.
    pub fn run(&mut self, host: &str, verb: &str, backend: &Backend, ctx: &egui::Context) {
        let slot: Slot<Result<JobView, String>> = Default::default();
        self.running.insert(host.to_owned(), slot.clone());
        let c = ctx.clone();
        backend.query(Query::AppAction { host: host.to_owned(), verb: verb.to_owned() }, ctx, move |r: Result<JobView, String>| {
            *slot.lock().unwrap() = Some(r);
            c.request_repaint();
        });
        // Its state asked again soon, whatever the job says.
        if let Some(e) = self.status.get_mut(host) {
            e.0 = f64::NEG_INFINITY;
        }
    }

    /// Settle what came back: finished jobs and saved levels.
    pub fn settle(&mut self) {
        let done: Vec<String> = self.running.iter().filter(|(_, s)| s.lock().unwrap().as_ref().is_some_and(|r| r.as_ref().map_or(true, |j| !j.running))).map(|(h, _)| h.clone()).collect();
        for h in done {
            if let Some(Some(Err(e))) = self.running.remove(&h).map(|s| s.lock().unwrap().clone()) {
                *self.note.lock().unwrap() = Some(Err(format!("{h}: {e}")));
            }
            if let Some(e) = self.status.get_mut(&h) {
                e.0 = f64::NEG_INFINITY;
            }
        }
        // A saved level's own answer replaces what was shown.
        let saved: Vec<String> = self.pending.keys().filter(|h| self.access.get(*h).is_some_and(|s| s.lock().unwrap().as_ref().is_some_and(|r| r.as_ref().is_ok_and(|v| !v.said.is_empty())))).cloned().collect();
        for h in saved {
            self.pending.remove(&h);
        }
        // A refused change: back to what it was.
        if let Some(Err(e)) = self.note.lock().unwrap().as_ref() {
            if e.contains("not changed") {
                let h = e.split(':').next().unwrap_or("").to_owned();
                self.pending.remove(&h);
            }
        }
    }

    /// The note above the list, once.
    pub fn note_ui(&mut self, ui: &mut egui::Ui) {
        let n = self.note.lock().unwrap().clone();
        if let Some(n) = n {
            ui.horizontal_wrapped(|ui| {
                match &n {
                    Ok(t) => theme::state_mark(ui, State::Good, t),
                    Err(e) => theme::state_mark(ui, State::Failed, e),
                };
                if ui.small_button("✕").clicked() {
                    *self.note.lock().unwrap() = None;
                }
            });
        }
    }

    /// The next Archive or Delete waiting for the confirm dialog.
    pub fn next_remove(&mut self) -> Option<(String, bool)> {
        if self.queue.is_empty() { None } else { Some(self.queue.remove(0)) }
    }

    /// What is still waiting to be archived or deleted, and a way to stop.
    pub fn queue_ui(&mut self, ui: &mut egui::Ui) {
        if self.queue.is_empty() {
            return;
        }
        ui.horizontal_wrapped(|ui| {
            let verb = if self.queue.iter().any(|(_, d)| *d) { "delete" } else { "archive" };
            theme::state_mark(ui, State::Attention, &format!("{} more to {verb} after this one", self.queue.len()));
            if ui.button("Stop after this one").clicked() {
                self.queue.clear();
            }
        });
    }

    /// The bar for ticked projects: the same actions, on all of them.
    pub fn bulk_bar(&mut self, ui: &mut egui::Ui, backend: &Backend, ctx: &egui::Context) -> Option<Act> {
        if self.selected.is_empty() {
            return None;
        }
        let hosts: Vec<String> = self.selected.iter().cloned().collect();
        let mut act = None;
        egui::Frame::new().fill(theme::primary().gamma_multiply(0.12)).corner_radius(10).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{} selected", hosts.len())).size(14.0).strong().color(theme::text_strong()));
                ui.add_space(8.0);
                let b = |ui: &mut egui::Ui, w: &str| ui.add(egui::Button::new(RichText::new(w).size(13.5)).min_size(egui::vec2(0.0, 30.0))).clicked();
                if b(ui, "▶ Start") {
                    for h in &hosts {
                        self.run(h, "up", backend, ctx);
                    }
                }
                if b(ui, "■ Stop") {
                    for h in &hosts {
                        self.run(h, "down", backend, ctx);
                    }
                }
                if b(ui, "Build") {
                    for h in &hosts {
                        self.run(h, "build", backend, ctx);
                    }
                }
                ui.menu_button(RichText::new("Who can view ▾").size(13.5), |ui| {
                    theme::chrome_menu(ui, 260.0);
                    for (k, w) in LEVELS.iter().take(3) {
                        if ui.button(*w).clicked() {
                            for h in &hosts {
                                self.set_level(h, k, backend, ctx);
                            }
                            ui.close();
                        }
                    }
                });
                // Each through the IDE's confirm dialog, one after another: each
                // says what it removes.
                if b(ui, "Archive…") {
                    self.queue = hosts.iter().map(|h| (h.clone(), false)).collect();
                    self.selected.clear();
                }
                if ui.add(egui::Button::new(RichText::new("Delete…").size(13.5).color(theme::alt())).min_size(egui::vec2(0.0, 30.0))).clicked() {
                    self.queue = hosts.iter().map(|h| (h.clone(), true)).collect();
                    self.selected.clear();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Clear").clicked() {
                        self.selected.clear();
                    }
                });
            });
        });
        ui.add_space(6.0);
        act
    }

    /// One of the person's own projects, one row: tick, mark, name and
    /// address, its state, who can view it, Start/Stop, Open, IDE and More.
    pub fn row(&mut self, ui: &mut egui::Ui, d: &DomainInfo, w: f32, h: f32, backend: &Backend, ctx: &egui::Context) -> Option<Act> {
        let mut act = None;
        let host = d.host.clone();
        let st = self.status(&host, backend, ctx);
        let level = self.level(&host, backend, ctx);
        let job = self.running.get(&host).and_then(|s| s.lock().unwrap().clone());
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        let ticked = self.selected.contains(&host);
        if ticked {
            ui.painter().rect_filled(rect, 8.0, theme::primary().gamma_multiply(0.10));
        } else if ui.rect_contains_pointer(rect) {
            ui.painter().rect_filled(rect, 8.0, theme::surface().gamma_multiply(0.6));
        }
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, theme::pane_brd()));
        let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(10.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let ui = &mut row;
        ui.spacing_mut().item_spacing.x = 12.0;
        let mut t = ticked;
        if ui.checkbox(&mut t, "").on_hover_text("select it - the bar above acts on every selected project").changed() {
            if t {
                self.selected.insert(host.clone());
            } else {
                self.selected.remove(&host);
            }
        }
        if crate::ui::kit::frameworks::icon(ui, &d.framework, 40.0).is_none() {
            ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
        }
        let actions_w = 380.0f32.min(ui.available_width() * 0.55);
        let text_w = (ui.available_width() - actions_w).max(120.0);
        ui.allocate_ui_with_layout(egui::vec2(text_w, h), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(text_w);
            ui.add_space(h / 2.0 - 20.0);
            let name = if d.title.is_empty() { crate::ui::ide::tabs::Tabs::short(&host).to_owned() } else { d.title.clone() };
            ui.add(egui::Label::new(RichText::new(name).size(16.0).strong().color(theme::text_strong())).truncate());
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // Running, stopped, served by the platform, or still being asked.
                let (s, words) = match (&job, &st) {
                    (Some(Ok(j)), _) if j.running => (State::Active, "working…".to_owned()),
                    (_, Some(Ok(s))) if s.kind == "port" && s.running => (State::Good, format!("running · :{}", s.port)),
                    (_, Some(Ok(s))) if s.kind == "port" => (State::Idle, "stopped".to_owned()),
                    (_, Some(Ok(_))) => (State::Good, "served by ndexr".to_owned()),
                    (_, Some(Err(_))) => (State::Idle, "state unknown".to_owned()),
                    _ => (State::Idle, "…".to_owned()),
                };
                theme::state_mark(ui, s, &words);
                ui.add(egui::Label::new(RichText::new(&host).monospace().size(12.0).color(theme::text())).truncate());
            });
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let big = |w: &str| egui::Button::new(RichText::new(w).size(13.5)).min_size(egui::vec2(0.0, 32.0));
            ui.menu_button(RichText::new("More ▾").size(13.5), |ui| {
                theme::chrome_menu(ui, 260.0);
                if ui.button("Share and access…").clicked() {
                    act = Some(Act::Share(host.clone()));
                    ui.close();
                }
                if ui.button("Details").clicked() {
                    self.open_details(d, backend, ctx);
                    ui.close();
                }
                if ui.button("Build").clicked() {
                    self.run(&host, "build", backend, ctx);
                    ui.close();
                }
                if ui.button("Publish to the web…").on_hover_text("its address: DNS, certificate and site (the provisioner route.ndexr.io runs)").clicked() {
                    act = Some(Act::Publish(host.clone()));
                    ui.close();
                }
                theme::menu_sep(ui);
                if ui.button("Archive…").on_hover_text("save its history and remove its folder; Restore brings it back").clicked() {
                    act = Some(Act::Remove(host.clone(), false));
                    ui.close();
                }
                if ui.button(RichText::new("Delete…").color(theme::alt())).on_hover_text("archive it, then give its name up").clicked() {
                    act = Some(Act::Remove(host.clone(), true));
                    ui.close();
                }
            });
            if ui.add(big("IDE")).on_hover_text("open it in the workspace").clicked() {
                act = Some(Act::OpenIde(d.clone()));
            }
            if ui.add(big("Open ↗")).on_hover_text(format!("https://{host}/")).clicked() {
                crate::ui::kit::links::new_tab(ctx, format!("https://{host}/"));
            }
            let port = matches!(&st, Some(Ok(s)) if s.kind == "port");
            let up = matches!(&st, Some(Ok(s)) if s.running);
            let busy = matches!(&job, Some(Ok(j)) if j.running) || (self.running.contains_key(&host) && job.is_none());
            if port {
                let (word, verb) = if up { ("■ Stop", "down") } else { ("▶ Start", "up") };
                if ui.add_enabled(!busy, big(word)).clicked() {
                    self.run(&host, verb, backend, ctx);
                }
            }
            // Who can view it, changed right here.
            let shown = level.as_deref().map(|l| LEVELS.iter().find(|(k, _)| *k == l).map_or(l, |(_, w)| *w)).unwrap_or("…");
            ui.menu_button(RichText::new(format!("{shown} ▾")).size(13.0), |ui| {
                theme::chrome_menu(ui, 280.0);
                ui.label(RichText::new("Who can view it").size(12.5).strong().color(theme::text_strong()));
                for (k, w) in LEVELS.iter().take(3) {
                    if ui.selectable_label(level.as_deref() == Some(*k), *w).clicked() {
                        self.set_level(&host, k, backend, ctx);
                        ui.close();
                    }
                }
                theme::menu_sep(ui);
                if ui.button("People and groups…").clicked() {
                    act = Some(Act::Share(host.clone()));
                    ui.close();
                }
            })
            .response
            .on_hover_text("who can view it - changes at once");
        });
        act
    }

    fn set_password(&mut self, host: &str, password: String, backend: &Backend, ctx: &egui::Context) {
        self.pw_busy = true;
        *self.pw_note.lock().unwrap() = None;
        let (slot, gate, c, h, b) = (self.pw_note.clone(), self.gate.clone(), ctx.clone(), host.to_owned(), backend.clone());
        backend.query(Query::SitePassword { host: host.to_owned(), password }, ctx, move |r: Result<String, String>| {
            *slot.lock().unwrap() = Some(r);
            // What is in front of it, asked again.
            b.query(Query::SiteGate { host: h.clone() }, &c, move |g: Result<String, String>| {
                *gate.lock().unwrap() = Some((h, g));
            });
            c.request_repaint();
        });
    }

    fn open_details(&mut self, d: &DomainInfo, backend: &Backend, ctx: &egui::Context) {
        self.pw.clear();
        *self.pw_note.lock().unwrap() = None;
        self.pw_busy = false;
        self.details = Some(d.clone());
        *self.gate.lock().unwrap() = None;
        *self.requests.lock().unwrap() = None;
        let (slot, c, h) = (self.gate.clone(), ctx.clone(), d.host.clone());
        backend.query(Query::SiteGate { host: d.host.clone() }, ctx, move |r: Result<String, String>| {
            *slot.lock().unwrap() = Some((h, r));
            c.request_repaint();
        });
        let (slot, c, h) = (self.requests.clone(), ctx.clone(), d.host.clone());
        backend.query(Query::SiteRequests { host: d.host.clone() }, ctx, move |r: Result<serde_json::Value, String>| {
            *slot.lock().unwrap() = Some((h, r));
            c.request_repaint();
        });
    }

    /// A project's details, as route.ndexr.io shows a domain: how it is
    /// served, who can view it, what is in front of it, its traffic, its dates.
    pub fn details_ui(&mut self, ctx: &egui::Context, backend: &Backend) -> Option<Act> {
        let d = self.details.clone()?;
        let mut open = true;
        let mut act = None;
        let st = self.status(&d.host, backend, ctx);
        let level = self.level(&d.host, backend, ctx);
        egui::Window::new(RichText::new(&d.host).font(theme::display(15.0))).id(egui::Id::new("manage-details")).open(&mut open).collapsible(false).resizable(true).default_width(460.0).show(ctx, |ui| {
            let row = |ui: &mut egui::Ui, k: &str, v: &str| {
                ui.horizontal(|ui| {
                    ui.add_sized([130.0, 20.0], egui::Label::new(RichText::new(k).size(13.0).color(theme::text())));
                    ui.add(egui::Label::new(RichText::new(v).size(13.0).color(theme::text_strong())).wrap());
                });
            };
            row(ui, "Address", &format!("https://{}/", d.host));
            row(ui, "Built with", crate::ui::kit::frameworks::mark(&d.framework).map_or("Not detected", |m| m.name));
            let served = match d.target.as_str() {
                "" | "module" => "the ndexr platform (a Shiny module)".to_owned(),
                t if t.starts_with("port:") => format!("its own server on port {}", &t[5..]),
                t => t.to_owned(),
            };
            row(ui, "Served by", &served);
            match &st {
                Some(Ok(s)) if s.kind == "port" => row(ui, "Running", if s.running { "yes" } else { "no" }),
                _ => {}
            }
            row(ui, "Who can view", level.as_deref().map(|l| LEVELS.iter().find(|(k, _)| *k == l).map_or(l, |(_, w)| *w)).unwrap_or("…"));
            let gate = self.gate.lock().unwrap().clone().filter(|(h, _)| *h == d.host).map(|(_, g)| g);
            row(ui, "In front of it", &match gate {
                Some(Ok(g)) => match g.as_str() {
                    "open" => "nothing - it answers anyone allowed".to_owned(),
                    "login" => "the ndexr sign-in".to_owned(),
                    "password" => "a page password".to_owned(),
                    "down" => "nothing answers - not published, or stopped".to_owned(),
                    other => other.to_owned(),
                },
                Some(Err(e)) => e,
                None => "…".to_owned(),
            });
            let reqs = self.requests.lock().unwrap().clone().filter(|(h, _)| *h == d.host).map(|(_, r)| r);
            if let Some(Ok(v)) = reqs {
                let n = v["requests"].as_array().map_or(0, |a| a.len());
                row(ui, "Recent requests", &n.to_string());
            }
            let now = crate::ui::software::now_secs();
            if d.created > 0 {
                row(ui, "Created", &format!("{} ago", crate::ui::app::age_words(now - d.created as i64)));
            }
            if d.modified > 0 {
                row(ui, "Last changed", &format!("{} ago", crate::ui::app::age_words(now - d.modified as i64)));
            }
            row(ui, "Folder", &d.path);
            // A page password, as route.ndexr.io sets one: asked before the
            // site shows, on top of who can view it.
            ui.add_space(8.0);
            ui.label(RichText::new("Page password").size(13.5).strong().color(theme::text_strong()));
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.pw).password(true).hint_text("4 characters or more").desired_width(180.0));
                if ui.add_enabled(!self.pw_busy && self.pw.chars().count() >= 4, egui::Button::new("Set")).clicked() {
                    let pw = std::mem::take(&mut self.pw);
                    self.set_password(&d.host, pw, backend, ctx);
                }
                if ui.add_enabled(!self.pw_busy, egui::Button::new("Remove")).on_hover_text("no page password: who can view it alone decides").clicked() {
                    self.set_password(&d.host, String::new(), backend, ctx);
                }
                if self.pw_busy {
                    ui.spinner();
                }
            });
            let pn = self.pw_note.lock().unwrap().clone();
            if let Some(n) = pn {
                self.pw_busy = false;
                match &n {
                    Ok(t) => theme::state_mark(ui, State::Good, t),
                    Err(e) => theme::state_mark(ui, State::Failed, e),
                };
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button("Share and access…").clicked() {
                    act = Some(Act::Share(d.host.clone()));
                }
                if ui.button("Publish to the web…").clicked() {
                    act = Some(Act::Publish(d.host.clone()));
                }
                if ui.button("Open in the IDE").clicked() {
                    act = Some(Act::OpenIde(d.clone()));
                }
            });
        });
        if !open {
            self.details = None;
        }
        act
    }
}
