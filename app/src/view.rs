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
}

impl Tab {
    /// The page's ?tab=, if it names one.
    fn asked() -> Option<Tab> {
        let q = web_sys::window()?.location().search().ok()?;
        let t = web_sys::UrlSearchParams::new_with_str(&q).ok()?.get("tab")?;
        match t.as_str() {
            "atlas" => Some(Tab::Atlas),
            "packages" => Some(Tab::Packages),
            "builds" => Some(Tab::Builds),
            "logs" => Some(Tab::Logs),
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
}

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
        }
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

    fn bar(&mut self, ui: &mut egui::Ui) {
        theme::strip(ui, 34.0, theme::ground(), |ui| {
            ui.add_space(10.0);
            ui.label(RichText::new("Cove").size(16.0).strong().color(theme::text_strong()));
            ui.add_space(10.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            for (t, name, tip) in [
                (Tab::Atlas, "Atlas", "the whole stack from the kernel up, and the story of it"),
                (Tab::Packages, "Packages", "search everything the stack builds: every Prefix package, EasyBuild module, R and Python package"),
                (Tab::Builds, "Builds", "how it got built and what is building now: curves, branches, 3D, the timeline and the queue"),
                (Tab::Logs, "Logs", "each build machine's log, live as it builds"),
            ] {
                if segment(ui, name, tip, self.tab == t).clicked() {
                    self.tab = t;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(10.0);
                ui.hyperlink_to(RichText::new("Sign in to build →").strong().color(theme::primary()), SIGN_IN);
                if ui.available_width() > 260.0 {
                    ui.label(RichText::new("public view · read-only").size(11.5).color(theme::hush()));
                }
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
                    Some(crate::atlas::Act::Enter(d)) => ctx.open_url(egui::OpenUrl::new_tab(format!("https://{}/", d.host))),
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
