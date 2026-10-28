//! Recent: the person's other Cove sessions, one click from the toolbar
//! (operator, 2026-10-18: "cove.ndexr.io/sessions moves into cove.ndexr.io as
//! simply a recents panel ... you can already select the environment there
//! and we dont need multiple selectors for the same thing in different
//! places"). So there is no New here and no environment picker: the toolbar's
//! Env is the one. What is left of the old list is Open and End.
//!
//! The list is the multi-user server's (`/s/<id>/sessions.json`, most recently
//! active first), asked for when the menu opens. Only in a browser on that
//! server; the desktop app is one session.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::backend::Backend;
use crate::theme;

#[derive(Default, Clone, serde::Deserialize)]
struct Recent {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    module: String,
    #[serde(default)]
    project: String,
    /// Seconds since it was last used.
    #[serde(default)]
    active: u64,
    #[serde(default)]
    current: bool,
}

#[derive(Default)]
pub struct Recents {
    list: Arc<Mutex<Option<Result<Vec<Recent>, String>>>>,
    asked: f64,
}

/// Asked again on opening when the last answer is older than this.
const FRESH: f64 = 10.0;

impl Recents {
    /// The person's Cove sessions on the multi-user server, to open or end,
    /// as rows under the project menu's projects (ide.rs domain_bar;
    /// operator, 2026-10-26: Recent's menu is the project button's). Nothing
    /// outside a multi-user session.
    pub fn sessions_list(&mut self, ui: &mut egui::Ui, backend: &Backend) {
        #[cfg(not(target_arch = "wasm32"))]
        let Backend::Http { base } = backend else { return };
        #[cfg(target_arch = "wasm32")]
        let Backend::Http { base } = backend;
        let Some((root, _)) = base.rsplit_once("/s/") else { return };
        let (root, base) = (root.to_owned(), base.clone());
        let ctx = ui.ctx().clone();
        let mut end: Option<String> = None;
        {
            ui.separator();
            ui.label(RichText::new("Sessions").size(11.5).strong().color(theme::text()));
            let now = ctx.input(|i| i.time);
            if now - self.asked > FRESH {
                self.asked = now;
                self.fetch(&base, &ctx);
            }
            let list = self.list.lock().unwrap().clone();
            match list {
                None => {
                    ui.spinner();
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e).size(11.0).color(theme::warn()));
                }
                Some(Ok(v)) => {
                    for r in &v {
                        ui.horizontal(|ui| {
                            let env = if r.module.is_empty() { "on PATH" } else { r.module.as_str() };
                            // A project it was started in, when there was one; "home" said nothing.
                            let place = r.project.rsplit('/').next().filter(|p| !p.is_empty()).map(|p| format!("  ·  {p}")).unwrap_or_default();
                            let text = format!("{}{place}  ·  {}", r.name, ago(r.active));
                            if r.current {
                                ui.label(RichText::new(format!("{text}  (this one)")).size(12.0).color(theme::text())).on_hover_text(env);
                                return;
                            }
                            if ui.add(egui::Button::new(RichText::new(text).size(12.0)).frame(false)).on_hover_text(format!("open it · {env}")).clicked() {
                                ctx.open_url(egui::OpenUrl::same_tab(format!("{root}/s/{}/", r.id)));
                                ui.close();
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("End").on_hover_text("end its R, Python and shell; its files stay").clicked() {
                                    end = Some(r.id.clone());
                                }
                            });
                        });
                    }
                }
            }
        }
        if let Some(id) = end {
            self.end(&base, &id, &ctx);
        }
    }

    fn fetch(&self, base: &str, ctx: &egui::Context) {
        let (slot, c) = (self.list.clone(), ctx.clone());
        let mut req = ehttp::Request::get(format!("{base}/sessions.json"));
        #[cfg(target_arch = "wasm32")]
        {
            req.credentials = ehttp::Credentials::SameOrigin;
        }
        req.headers.insert("Accept", "application/json");
        ehttp::fetch(req, move |r| {
            let got = match r {
                Ok(r) if r.ok => serde_json::from_slice::<Vec<Recent>>(&r.bytes).map_err(|e| e.to_string()),
                Ok(r) => Err(format!("{} {}", r.status, r.status_text)),
                Err(e) => Err(e),
            };
            // A failed ask (the server restarting) keeps what was known.
            let mut s = slot.lock().unwrap();
            if got.is_ok() || s.is_none() {
                *s = Some(got);
            }
            c.request_repaint();
        });
    }

    /// End one, then ask again so it leaves the list.
    fn end(&mut self, base: &str, id: &str, ctx: &egui::Context) {
        let (slot, c) = (self.list.clone(), ctx.clone());
        let mut req = ehttp::Request::post(format!("{base}/end"), format!("id={id}").into_bytes());
        req.headers.insert("X-Cove", "1");
        req.headers.insert("Content-Type", "application/x-www-form-urlencoded");
        #[cfg(target_arch = "wasm32")]
        {
            req.credentials = ehttp::Credentials::SameOrigin;
        }
        let gone = id.to_owned();
        ehttp::fetch(req, move |r| {
            if r.is_ok_and(|r| r.ok) {
                if let Some(Ok(v)) = slot.lock().unwrap().as_mut() {
                    v.retain(|s| s.id != gone);
                }
            }
            c.request_repaint();
        });
        self.asked = 0.0;
    }
}

fn ago(secs: u64) -> String {
    match secs {
        0..=59 => "just now".to_owned(),
        60..=3599 => format!("{} min ago", secs / 60),
        3600..=86399 => format!("{} h ago", secs / 3600),
        _ => format!("{} d ago", secs / 86400),
    }
}
