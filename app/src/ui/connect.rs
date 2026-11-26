//! Use Cove from Claude (operator, 2026-11-26): the address Claude's custom
//! connector takes, the steps to add it in Claude.ai, Claude Desktop and
//! Claude Code, and the connectors this person has allowed, each with what
//! it may do and Revoke. The server half is server/mcp.rs.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};
use serde_json::Value;

use crate::api::Query;
use crate::backend::Backend;
use crate::ui::kit::theme;

type Slot<T> = Arc<Mutex<Option<T>>>;

#[derive(Default)]
pub struct Connect {
    pub open: bool,
    grants: Slot<Result<Value, String>>,
    asked: bool,
    note: Slot<Result<String, String>>,
}

/// Where the connector points: this page's own server in a browser, else
/// cove.ndexr.io (the desktop app reaches Claude's connector through it).
fn url() -> String {
    #[cfg(target_arch = "wasm32")]
    if let Some(o) = web_sys::window().and_then(|w| w.location().origin().ok()) {
        if o.starts_with("https://") {
            return format!("{o}/mcp");
        }
    }
    "https://cove.ndexr.io/mcp".to_owned()
}

impl Connect {
    pub fn show(&mut self, ctx: &egui::Context, backend: &Backend) {
        if !self.open {
            self.asked = false;
            return;
        }
        if !self.asked {
            self.asked = true;
            let slot = self.grants.clone();
            backend.query(Query::McpGrants, ctx, move |r: Result<Value, String>| {
                *slot.lock().unwrap() = Some(r);
            });
        }
        let addr = url();
        let mut open = true;
        let mut revoke: Option<String> = None;
        egui::Window::new("Use Cove from Claude")
            .id(egui::Id::new("connect-claude"))
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("Claude works in your Cove: it lists your projects, reads and searches their files and changes, and - if you allow it - changes files, runs commands and deploys, as you.").size(13.0).color(theme::text()));
                ui.add_space(8.0);
                ui.label(RichText::new("CONNECTOR ADDRESS").size(10.5).color(theme::hush()));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&addr).monospace().size(13.5).color(theme::text_strong()));
                    if ui.button("Copy").clicked() {
                        ctx.copy_text(addr.clone());
                    }
                });
                ui.add_space(8.0);
                ui.label(RichText::new("CLAUDE.AI AND CLAUDE DESKTOP").size(10.5).color(theme::hush()));
                for (n, step) in [
                    "Customize › Connectors › Add custom connector.",
                    "Name it Cove, paste the address above, and Add.",
                    "Connect: sign in to Cove if asked, tick what Claude may do beyond reading, and Allow.",
                ]
                .iter()
                .enumerate()
                {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(format!("{}.", n + 1)).strong().color(theme::primary()));
                        ui.label(RichText::new(*step).size(13.0).color(theme::text()));
                    });
                }
                ui.add_space(6.0);
                ui.label(RichText::new("CLAUDE CODE").size(10.5).color(theme::hush()));
                let cmd = format!("claude mcp add --transport http cove {addr}");
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&cmd).monospace().size(12.5).color(theme::text_strong()));
                    if ui.button("Copy").clicked() {
                        ctx.copy_text(cmd.clone());
                    }
                });
                ui.label(RichText::new("then /mcp in Claude Code to sign in.").size(12.5).color(theme::text()));
                ui.add_space(10.0);
                ui.label(RichText::new("CONNECTED").size(10.5).color(theme::hush()));
                match self.grants.lock().unwrap().clone() {
                    None => {
                        ui.spinner();
                    }
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).size(12.5).color(theme::warn()));
                    }
                    Some(Ok(v)) => {
                        let list = v.as_array().cloned().unwrap_or_default();
                        if list.is_empty() {
                            ui.label(RichText::new("Nothing yet - connectors you allow show here, with Revoke.").size(12.5).color(theme::text()));
                        }
                        let now = crate::ui::software::now_secs();
                        for g in list {
                            ui.horizontal(|ui| {
                                let scopes: Vec<String> = g["scopes"].as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_owned)).collect()).unwrap_or_default();
                                ui.label(RichText::new(g["client"].as_str().unwrap_or("a connector")).strong().color(theme::text_strong()));
                                ui.label(RichText::new(scopes.join(" · ")).size(12.0).color(theme::text()));
                                let used = g["used"].as_i64().unwrap_or(0);
                                ui.label(RichText::new(format!("used {} ago", crate::ui::software::fmt_dur((now - used).max(0)))).size(12.0).color(theme::text()));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("Revoke").on_hover_text("it can no longer act in your Cove; Claude asks to connect again").clicked() {
                                        revoke = g["id"].as_str().map(str::to_owned);
                                    }
                                });
                            });
                        }
                    }
                }
                if let Some(n) = self.note.lock().unwrap().clone() {
                    let (t, c) = match n {
                        Ok(t) => (t, theme::ok()),
                        Err(e) => (e, theme::warn()),
                    };
                    ui.label(RichText::new(t).size(12.5).color(c));
                }
            });
        if let Some(id) = revoke {
            let (note, grants, b2, c2) = (self.note.clone(), self.grants.clone(), backend.clone(), ctx.clone());
            backend.query(Query::McpRevoke { id }, ctx, move |r: Result<Value, String>| {
                *note.lock().unwrap() = Some(r.map(|_| "Revoked.".to_owned()));
                b2.query(Query::McpGrants, &c2, move |r: Result<Value, String>| {
                    *grants.lock().unwrap() = Some(r);
                });
            });
        }
        if !open {
            self.open = false;
        }
    }
}
