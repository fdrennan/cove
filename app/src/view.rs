//! Visualization mode: Cove for anyone, signed in or not (operator,
//! 2026-10-17: "want to make this possible for people without signing in,
//! they cant like do stuff but we should make sure the data about the builds
//! and such is all available to the public for viewing").
//!
//! cove.ndexr.io/view/ - and cove.ndexr.io/ for someone not signed in - is
//! this: the atlas with the front page's slides over it (showcase.rs), and
//! the Software pane's Packages, Builds (where every build machine is now,
//! then curves, branches, 3D, timeline, queue) and the machines' Logs,
//! searchable. ndexr.io's front page is this same view (its nginx.conf), and
//! ndexr.io's agent owns it (its CLAUDE.md). `?tab=packages|builds|logs`
//! opens on one, so a post can link straight to it. Its backend is the
//! public view (server.rs `public_response`): the catalog, the builds and
//! their logs, read-only. No session, no consoles, no agent, nothing that
//! runs; what would need one says so and offers the sign-in.

use eframe::egui::{self, Align, Layout, RichText};

use crate::backend::Backend;
use crate::software::{segment, SoftwarePane, View};
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Atlas,
    Packages,
    Builds,
    Logs,
    /// Live Development: requested changes and the publishing pipeline, as
    /// the backend records them (server.rs development/status.json).
    Development,
}

impl Tab {
    /// The page's ?tab=, if it names one.
    fn asked() -> Option<Tab> {
        let q = web_sys::window()?.location().search().ok()?;
        let t = web_sys::UrlSearchParams::new_with_str(&q).ok()?.get("tab")?;
        match t.as_str() {
            // The paper was the front page 2026-11-14 and was dropped the
            // same day (operator: "drop the paper in the visual mode"); a
            // link to it opens the atlas.
            "paper" => Some(Tab::Atlas),
            "atlas" => Some(Tab::Atlas),
            "packages" => Some(Tab::Packages),
            "builds" => Some(Tab::Builds),
            "logs" => Some(Tab::Logs),
            "development" | "dev" => Some(Tab::Development),
            _ => None,
        }
    }
}

pub struct ViewApp {
    backend: Backend,
    backdrop: crate::backdrop::Backdrop,
    atlas: crate::atlas::Atlas,
    software: SoftwarePane,
    showcase: crate::showcase::Showcase,
    tab: Tab,
    toast: Option<(String, f64)>,
    /// The public sites only - nobody here is signed in (/view/domains.json,
    /// host.rs visible_domains(None)),
    /// asked again every minute: a new one joins the canopy within it.
    domains: std::sync::Arc<std::sync::Mutex<Option<Result<Vec<crate::api::DomainInfo>, String>>>>,
    domains_at: f64,
    /// A request, no sign-in needed (server.rs anon_request_response): the
    /// window open, its words, a contact, and how the last one went.
    asking: bool,
    /// The Install window.
    installing: bool,
    ask_what: String,
    ask_contact: String,
    asked: std::sync::Arc<std::sync::Mutex<Option<Result<i64, String>>>>,
    sending: bool,
    dev: std::sync::Arc<std::sync::Mutex<Option<Result<serde_json::Value, String>>>>,
    dev_at: f64,
}

/// Cove on this computer, in one line: packaging/install.sh, which this
/// server answers at /view/install.sh (server.rs) and ndexr.io at /install.sh.
pub const INSTALL: &str = "curl -fsSL https://cove.ndexr.io/view/install.sh | sh";

/// Where Sign in goes: the platform's sign-in, back to Cove itself.
const SIGN_IN: &str = "https://console.ndexr.io/?next=https://cove.ndexr.io/";

impl ViewApp {
    pub fn new(cc: &eframe::CreationContext<'_>, backend: Backend) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut software = SoftwarePane::default();
        software.public = true;
        let tab = Tab::asked().unwrap_or(Tab::Atlas);
        ViewApp {
            backend,
            backdrop: crate::backdrop::Backdrop::start(&cc.egui_ctx),
            atlas: {
                let mut a = crate::atlas::Atlas::default();
                a.public = true;
                a
            },
            software,
            showcase: Default::default(),
            tab,
            toast: None,
            domains: Default::default(),
            domains_at: f64::NEG_INFINITY,
            asking: false,
            // ?install opens it, so a link can go straight there.
            installing: web_sys::window().and_then(|w| w.location().search().ok()).is_some_and(|q| q.trim_start_matches('?').split('&').any(|p| p == "install" || p.starts_with("install="))),
            ask_what: String::new(),
            ask_contact: String::new(),
            asked: Default::default(),
            sending: false,
            dev: Default::default(),
            dev_at: f64::NEG_INFINITY,
        }
    }

    /// The development snapshot, every 15 s while the tab is open.
    fn ask_dev(&mut self, now: f64, ctx: &egui::Context) {
        ctx.request_repaint_after(std::time::Duration::from_secs(15));
        if now - self.dev_at < 15.0 {
            return;
        }
        self.dev_at = now;
        let Backend::Http { base } = &self.backend;
        let (slot, ctx) = (self.dev.clone(), ctx.clone());
        ehttp::fetch(ehttp::Request::get(format!("{base}/development/status.json")), move |r| {
            let v = match r {
                Ok(r) if r.ok => serde_json::from_slice(&r.bytes).map_err(|e| e.to_string()),
                Ok(r) => Err(format!("the development status answered {}: {}", r.status, String::from_utf8_lossy(&r.bytes).chars().take(200).collect::<String>())),
                Err(e) => Err(e),
            };
            *slot.lock().unwrap() = Some(v);
            ctx.request_repaint();
        });
    }

    fn ask_domains(&mut self, now: f64, ctx: &egui::Context) {
        if now - self.domains_at < 60.0 {
            return;
        }
        self.domains_at = now;
        let Backend::Http { base } = &self.backend;
        let (slot, ctx) = (self.domains.clone(), ctx.clone());
        ehttp::fetch(ehttp::Request::get(format!("{base}/domains.json")), move |r| {
            let v = match r {
                Ok(r) if r.ok => serde_json::from_slice(&r.bytes).map_err(|e| e.to_string()),
                Ok(r) => Err(format!("domains.json: {}", r.status)),
                Err(e) => Err(e),
            };
            // A failed refresh keeps the canopy it had.
            let mut s = slot.lock().unwrap();
            if v.is_ok() || s.is_none() {
                *s = Some(v);
            }
            ctx.request_repaint();
        });
    }

    /// The Install window: the one line, what it does on each system, and
    /// the way in without installing anything.
    fn install_window(&mut self, ctx: &egui::Context) {
        if !self.installing {
            return;
        }
        let mut open = true;
        let w = (ctx.content_rect().width() - 24.0).min(520.0);
        egui::Window::new("Install Cove").open(&mut open).collapsible(false).resizable(false).default_width(w).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 48.0)).show(ctx, |ui| {
            ui.set_width(w - 24.0);
            ui.label(RichText::new("In a terminal, on Linux (x86_64 or ARM64) or macOS:").size(13.0).color(theme::text()));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                let mut c = INSTALL.to_owned();
                ui.add(egui::TextEdit::singleline(&mut c).font(egui::TextStyle::Monospace).desired_width(ui.available_width() - 64.0));
                if ui.button("Copy").clicked() {
                    ui.ctx().copy_text(INSTALL.to_owned());
                    self.toast = Some(("Copied - paste it into a terminal.".into(), ui.ctx().input(|i| i.time) + 4.0));
                }
            });
            ui.add_space(6.0);
            for line in [
                "Linux: the build for your machine's architecture (x86_64 or ARM64), into ~/.local - no sudo, any distribution.",
                "macOS: Cove.app in Applications.",
                "Every download is checked against its published SHA-256 first. Cove updates itself after that.",
            ] {
                ui.add(egui::Label::new(RichText::new(format!("·  {line}")).size(12.5).color(theme::text())).wrap());
            }
            ui.add_space(6.0);
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("Then run").size(12.5).color(theme::text()));
                ui.label(RichText::new("cove").monospace().size(12.5).color(theme::text_strong()));
                ui.label(RichText::new("- its first start sets up the stack and your Claude. Or use it in this browser:").size(12.5).color(theme::text()));
                ui.hyperlink_to(RichText::new("Sign in →").size(12.5).strong().color(theme::primary()), SIGN_IN);
            });
            ui.horizontal_wrapped(|ui| {
                ui.hyperlink_to(RichText::new("Read the installer").size(12.0).color(theme::hush()), "https://cove.ndexr.io/view/install.sh");
                ui.add_space(8.0);
                ui.hyperlink_to(RichText::new("Source").size(12.0).color(theme::hush()), crate::app::SOURCE_URL);
            });
        });
        if !open {
            self.installing = false;
        }
    }

    /// The request window: what they need and, if they like, where to answer.
    fn request_window(&mut self, ctx: &egui::Context) {
        if let Some(r) = self.asked.lock().unwrap().take() {
            self.sending = false;
            match r {
                Ok(id) => {
                    self.ask_what.clear();
                    self.asking = false;
                    self.toast = Some((format!("Request #{id} sent - thank you."), ctx.input(|i| i.time) + 6.0));
                }
                Err(e) => self.toast = Some((format!("Not sent: {e}"), ctx.input(|i| i.time) + 8.0)),
            }
        }
        if !self.asking {
            return;
        }
        let mut open = true;
        let w = (ctx.content_rect().width() - 24.0).min(440.0);
        egui::Window::new("Send a request").open(&mut open).collapsible(false).resizable(false).default_width(w).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 48.0)).show(ctx, |ui| {
            ui.set_width(w - 24.0);
            ui.label(RichText::new("What you need, or what is broken. No sign-in needed: it reaches the people who build Cove, filed as anonymous.").size(13.0).color(theme::text()));
            ui.add(egui::TextEdit::multiline(&mut self.ask_what).hint_text("what you need…").desired_rows(5).desired_width(f32::INFINITY));
            ui.add(egui::TextEdit::singleline(&mut self.ask_contact).hint_text("an email for the answer (optional)").desired_width(f32::INFINITY));
            ui.add_space(4.0);
            let ready = !self.ask_what.trim().is_empty() && !self.sending;
            if ui.add_enabled(ready, theme::primary_button(if self.sending { "Sending…" } else { "Send request" })).clicked() {
                self.sending = true;
                let Backend::Http { base } = &self.backend;
                let page = web_sys::window().and_then(|w| w.location().href().ok()).unwrap_or_default();
                let body = serde_json::to_vec(&serde_json::json!({ "what": self.ask_what, "contact": self.ask_contact, "page": page })).unwrap_or_default();
                let mut req = ehttp::Request::post(format!("{base}/request"), body);
                // text/plain: a simple request, no CORS preflight.
                req.headers.insert("Content-Type", "text/plain");
                let (slot, c) = (self.asked.clone(), ctx.clone());
                ehttp::fetch(req, move |r| {
                    *slot.lock().unwrap() = Some(match r {
                        Ok(r) if r.ok => serde_json::from_slice::<serde_json::Value>(&r.bytes).ok().and_then(|v| v["id"].as_i64()).ok_or_else(|| "no request number came back".to_owned()),
                        Ok(r) => Err(crate::backend::anon_refused(r.status)),
                        Err(e) => Err(e),
                    });
                    c.request_repaint();
                });
            }
        });
        if !open {
            self.asking = false;
        }
    }

    fn bar(&mut self, ui: &mut egui::Ui) {
        // The right side first, so it is always on screen, and the tabs in
        // what is left, scrolling sideways when they do not fit: drawn the
        // other way round, on a phone "Source ↗" landed on top of "Logs"
        // (operator's screenshot IMG_8740, 2026-11-14: "Lsogsrce").
        let phone = ui.ctx().content_rect().width() < 640.0;
        theme::strip(ui, 34.0, theme::ground(), |ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(10.0);
                ui.hyperlink_to(RichText::new(if phone { "Sign in →" } else { "Sign in to build →" }).strong().color(theme::primary()), SIGN_IN);
                ui.add_space(8.0);
                if ui.add(egui::Button::selectable(self.installing, RichText::new("Install").size(12.5).strong())).on_hover_text("Cove on your computer: Linux and macOS, one line").clicked() {
                    self.installing = !self.installing;
                }
                ui.add_space(4.0);
                if ui.add(egui::Button::selectable(self.asking, RichText::new("Request").size(12.5))).on_hover_text("ask for something, or say what is broken - no sign-in needed").clicked() {
                    self.asking = !self.asking;
                }
                // The source, public (operator, 2026-11-13: "add the gitlab
                // repo link to cove and ndexr.io"); this view is ndexr.io's
                // front page, so this is that link too. Not on a phone: the
                // tabs need the room, and the paper links it.
                if !phone {
                    ui.add_space(8.0);
                    ui.hyperlink_to(RichText::new("Source ↗").size(12.5).color(theme::hush()), crate::app::SOURCE_URL)
                        .on_hover_text(crate::app::SOURCE_URL);
                }
                ui.add_space(8.0);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    egui::ScrollArea::horizontal().id_salt("view-tabs").max_width(ui.available_width()).show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.add_space(if phone { 6.0 } else { 10.0 });
                            if !phone {
                                ui.label(RichText::new("Cove").size(16.0).strong().color(theme::text_strong()));
                                ui.add_space(10.0);
                            }
                            ui.spacing_mut().item_spacing.x = 4.0;
                            for (t, name, tip) in [
                                (Tab::Atlas, "Atlas", "the whole stack from the kernel up, and the story of it"),
                                (Tab::Packages, "Packages", "search everything the stack builds: every Prefix package, EasyBuild module, R and Python package"),
                                (Tab::Builds, "Builds", "how it got built and what is building now: curves, branches, 3D, the timeline and the queue"),
                                (Tab::Logs, "Logs", "each build machine's log, live as it builds"),
                                (Tab::Development, "Development", "changes people asked for, the agents' work, review and installs, and the public source - live"),
                            ] {
                                if segment(ui, name, tip, self.tab == t).clicked() {
                                    self.tab = t;
                                }
                            }
                        });
                    });
                });
            });
        });
    }
}

impl eframe::App for ViewApp {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        self.backdrop.paint(ctx);
        self.software.load(&self.backend, ctx);
        // Immersed, the atlas has the whole screen (atlas.rs `immersive`).
        let immersed = self.tab == Tab::Atlas && self.atlas.immersive;
        if !immersed {
            egui::Panel::top("view-bar").resizable(false).show_separator_line(true).exact_size(34.0).frame(egui::Frame::new()).show(root, |ui| self.bar(ui));
        }
        let now = ctx.input(|i| i.time);
        match self.tab {
            Tab::Atlas => {
                let cat = self.software.catalog();
                self.ask_domains(now, ctx);
                let domains = self.domains.lock().unwrap().clone();
                // The panel's whole space, read inside it (app.rs does the same).
                let (act, within) = egui::CentralPanel::default()
                    .frame(egui::Frame::new())
                    .show(root, |ui| (self.atlas.ui(ui, ctx, cat.clone(), domains, "", None), ui.max_rect()))
                    .inner;
                self.showcase.within = Some(within);
                match act {
                    Some(crate::atlas::Act::Software(i)) => {
                        self.tab = Tab::Packages;
                        self.software.reveal(i);
                    }
                    // A domain is someone's site: go and look at it.
                    Some(crate::atlas::Act::Enter(d)) => crate::links::new_tab(ctx, format!("https://{}/", d.host)),
                    // Requests are signed: sign in, and ask from the atlas there.
                    Some(crate::atlas::Act::AddSoftware) => ctx.open_url(egui::OpenUrl::same_tab(SIGN_IN)),
                    Some(_) => self.toast = Some(("Opening source and building need an account - Sign in to build, top right.".into(), now + 5.0)),
                    None => {}
                }
                self.showcase.bare = self.atlas.immersive;
                if self.atlas.chosen() {
                    self.showcase.hold();
                }
                // On a phone, a picked star's details own the bottom: the
                // tour steps aside (app.rs does the same).
                let phone_pick = self.atlas.chosen() && ctx.content_rect().width() < 760.0;
                match if phone_pick { None } else { self.showcase.ui(ctx, cat.as_deref()) } {
                    Some(crate::showcase::Point::At(i)) => {
                        if !self.atlas.show_item(i) {
                            self.showcase.retry();
                        }
                    }
                    Some(crate::showcase::Point::Kernel(j)) => {
                        if !self.atlas.show_kernel(j) {
                            self.showcase.retry();
                        }
                    }
                    Some(crate::showcase::Point::Release) => self.atlas.release(),
                    None => {}
                }
            }
            Tab::Development => {
                self.ask_dev(now, ctx);
                let data = self.dev.lock().unwrap().clone();
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme::bg().gamma_multiply(theme::win_alpha())).inner_margin(12.0))
                    .show(root, |ui| {
                        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| development(ui, data.as_ref()));
                    });
            }
            Tab::Packages | Tab::Builds | Tab::Logs => {
                // Logs and Builds are one Software tab's views: the top bar
                // picks which, and the pane's own switch still moves between them.
                match self.tab {
                    Tab::Logs if self.software.view != View::Logs => self.software.view = View::Logs,
                    Tab::Builds if self.software.view == View::Logs => self.software.view = View::Curves,
                    _ => {}
                }
                let group = if self.tab == Tab::Packages { 0 } else { 1 };
                egui::CentralPanel::default()
                    .frame(egui::Frame::new().fill(theme::bg().gamma_multiply(theme::win_alpha())).inner_margin(4.0))
                    .show(root, |ui| {
                        // Builds opens on where every build is now, the same
                        // machine cards signed-in Cove's Builds mode leads
                        // with (operator, 2026-11-14: "builds section and also
                        // ndexr.io is VIEWER mode of cove.ndexr.io"). They read
                        // the public builds.json and carry no buttons: starting
                        // and stopping machines stays signed-in Cove's. Capped
                        // at a third of the height, scrolling past it, so the
                        // views below keep their room on a phone.
                        if self.tab == Tab::Builds {
                            let cap = (ui.available_height() * 0.34).max(140.0);
                            egui::ScrollArea::vertical().id_salt("view-builds-now").max_height(cap).auto_shrink([false, true]).show(ui, |ui| {
                                self.software.where_builds_are(ui, ctx, &self.backend);
                            });
                            ui.separator();
                        }
                        // Code for a console has nowhere to run here.
                        if self.software.ui(ui, ctx, &self.backend, group).is_some() {
                            self.toast = Some(("Running it needs a session - Sign in to build, top right.".into(), now + 5.0));
                        }
                        // Picked from the pane's own switch: the top bar follows.
                        if group == 1 {
                            self.tab = if self.software.view == View::Logs { Tab::Logs } else { Tab::Builds };
                        }
                    });
            }
        }
        self.install_window(ctx);
        self.request_window(ctx);
        if let Some((msg, until)) = self.toast.clone() {
            if now > until {
                self.toast = None;
            } else {
                egui::Area::new(egui::Id::new("view-toast")).anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 48.0)).show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| ui.label(msg));
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
        }
    }
}


// ── Live Development ────────────────────────────────────────────────────────

/// A stage's word and colour, from the status `ndexr change` recorded.
fn stage(status: &str) -> (&'static str, egui::Color32) {
    match status {
        "requested" => ("Requested", theme::primary()),
        "running" => ("Agent working", theme::amber()),
        "proposed" => ("Under review", theme::primary()),
        "merged" => ("Installed", theme::ok()),
        "rejected" => ("Rejected", theme::alt()),
        "conflict" => ("Blocked", theme::alt()),
        "failed" => ("Failed", theme::alt()),
        "no_change" => ("Nothing to change", theme::text()),
        _ => ("Unknown", theme::text()),
    }
}

/// "5 min ago" from an RFC 3339 time or Unix seconds.
fn ago(v: &serde_json::Value) -> String {
    let then = match v {
        serde_json::Value::Number(n) => n.as_f64().map(|s| s * 1000.0),
        serde_json::Value::String(s) => Some(js_sys::Date::parse(s)).filter(|t| t.is_finite()),
        _ => None,
    };
    let Some(then) = then else { return "—".into() };
    let s = ((js_sys::Date::now() - then) / 1000.0).max(0.0) as i64;
    match s {
        0..=59 => format!("{s} s ago"),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=86_399 => format!("{} h ago", s / 3600),
        _ => format!("{} d ago", s / 86_400),
    }
}

fn development(ui: &mut egui::Ui, data: Option<&Result<serde_json::Value, String>>) {
    ui.label(RichText::new("Live Development").size(22.0).strong().color(theme::text_strong()));
    ui.label(
        RichText::new("Anyone signed in can ask for a change to a public project at api.ndexr.io. An agent makes it away from the live site, code checks what it changed, and it is installed only when a person approves. This is what the backend recorded, failures included; what an agent says it did is not shown.")
            .size(14.0)
            .color(theme::text()),
    );
    ui.add_space(10.0);
    let v = match data {
        None => {
            ui.spinner();
            return;
        }
        Some(Err(e)) => {
            ui.label(RichText::new(format!("Not available: {e}")).size(14.0).color(theme::alt()));
            return;
        }
        Some(Ok(v)) => v,
    };
    // The stages, counted.
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 18.0;
        for st in ["requested", "running", "proposed", "merged", "conflict", "failed", "rejected", "no_change"] {
            let n = v["counts"][st].as_i64().unwrap_or(0);
            let (word, color) = stage(st);
            ui.label(RichText::new(format!("{word} {n}")).size(15.0).strong().color(if n > 0 { color } else { theme::text() }));
        }
    });
    ui.add_space(12.0);
    // The publishing pipeline.
    let p = &v["pipeline"];
    egui::Frame::new().fill(theme::surface()).corner_radius(8).inner_margin(12.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("Publishing").size(16.0).strong().color(theme::text_strong()));
        // Lines that wrap, not a grid: a release id does not fit beside its
        // label on a phone, and a grid that cannot wrap widened everything
        // below it past the screen's edge.
        let row = |ui: &mut egui::Ui, name: &str, value: egui::RichText| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(format!("{name}:")).size(14.0).color(theme::text()));
                ui.add(egui::Label::new(value).wrap());
            });
        };
        row(ui, "Cove release", RichText::new(format!("{}  ·  {}", p["release"].as_str().unwrap_or("—"), ago(&p["release_at"]))).monospace().size(13.5).color(theme::text_strong()));
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Public source:").size(14.0).color(theme::text()));
            let src = p["source"].as_str().unwrap_or("https://github.com/fdrennan/cove").to_owned();
            ui.hyperlink_to(RichText::new(format!("{}  ·  {}", p["source_commit"].as_str().unwrap_or("—"), ago(&p["source_at"]))).monospace().size(13.5), src);
        });
        let (word, color) = if p["push_held"].as_bool() == Some(true) {
            (format!("held: {}", p["last_push"].as_str().unwrap_or("")), theme::alt())
        } else if p["release"] == p["source_release"] {
            ("the public source is this release".to_owned(), theme::ok())
        } else {
            (format!("behind: the public source is release {}; it follows within the hour", p["source_release"].as_str().unwrap_or("—")), theme::amber())
        };
        row(ui, "In step", RichText::new(word).size(14.0).color(color));
    });
    ui.add_space(14.0);
    ui.label(RichText::new("Changes").size(16.0).strong().color(theme::text_strong()));
    let changes = v["changes"].as_array().cloned().unwrap_or_default();
    if changes.is_empty() {
        ui.label(RichText::new("None yet.").size(14.0).color(theme::text()));
    }
    for c in &changes {
        egui::Frame::new().stroke(egui::Stroke::new(1.0, theme::pane_brd())).corner_radius(8).inner_margin(10.0).outer_margin(egui::Margin::symmetric(0, 4)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                let (word, color) = stage(c["status"].as_str().unwrap_or(""));
                ui.label(RichText::new(format!("#{}", c["id"])).monospace().size(14.0).color(theme::text_strong()));
                ui.label(RichText::new(word).size(14.0).strong().color(color));
                ui.label(RichText::new(c["project"].as_str().unwrap_or("")).size(14.0).color(theme::text_strong()));
                if let Some(l) = c["level"].as_i64() {
                    let lw = ["", "copy and style", "code", "privileged", "refused"].get(l as usize).copied().unwrap_or("");
                    ui.label(RichText::new(format!("level {l} · {lw}")).size(13.0).color(if l >= 4 { theme::alt() } else if l == 3 { theme::amber() } else { theme::text() }));
                }
                ui.label(RichText::new(format!("{} file(s)", c["files"])).size(13.0).color(theme::text()));
            });
            if let Some(t) = c["title"].as_str() {
                ui.label(RichText::new(t).size(14.0).color(theme::text()));
            }
            ui.horizontal_wrapped(|ui| {
                for k in c["checks"].as_array().into_iter().flatten() {
                    let ok = k["ok"].as_bool() == Some(true);
                    ui.label(RichText::new(format!("{} {}", if ok { "✓" } else { "✘" }, k["name"].as_str().unwrap_or(""))).size(13.0).color(if ok { theme::ok() } else { theme::alt() }));
                }
                ui.label(RichText::new(format!("asked {}", ago(&c["created_at"]))).size(13.0).color(theme::text()));
                if !c["decided_at"].is_null() {
                    ui.label(RichText::new(format!("decided {}", ago(&c["decided_at"]))).size(13.0).color(theme::text()));
                }
                if let Some(h) = c["installed"].as_str() {
                    ui.label(RichText::new(format!("installed as {h}")).monospace().size(13.0).color(theme::ok()));
                }
            });
        });
    }
}
