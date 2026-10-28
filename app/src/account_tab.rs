//! The Account tab (2026-10-15, operator: port the tools every person uses
//! into Cove - credentials.ndexr.io, api.ndexr.io's keys, box/my/recent): one
//! page of the person's own account - who they are, their cell and its
//! lenses, where they stand with Claude, every domain they own and what
//! serves it, their API keys (revoke here), and what those keys did lately.
//!
//! One answer behind it, api.ndexr.io/account.r `overview`, reached two ways:
//! the desktop app asks api.ndexr.io with its key (account.rs `overview`); the
//! web UI asks its server (Query::Account), which runs that R as the session's
//! owner (host.rs `account_as`; `cove multiuser` answers it as root for them).
//! Everything is keyed by the person's own email there, never by what is sent.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, RichText};

use crate::api::{AccountView, Query};
use crate::backend::Backend;
use crate::theme;

type Slot<T> = Arc<Mutex<Option<T>>>;

#[derive(Default)]
pub struct AccountPane {
    got: Slot<Result<AccountView, String>>,
    /// A refused revoke's words, shown over the last good view.
    note: Slot<String>,
    busy: Arc<AtomicBool>,
    asked: bool,
    /// The key a Revoke is waiting on a second click for.
    confirm: Option<String>,
    filter: String,
}

impl AccountPane {
    fn ask(&mut self, backend: &Backend, ctx: &egui::Context, revoke: String) {
        self.asked = true;
        self.busy.store(true, Ordering::Relaxed);
        *self.note.lock().unwrap() = None;
        let (got, note, busy) = (self.got.clone(), self.note.clone(), self.busy.clone());
        let revoking = !revoke.is_empty();
        let done = move |r: Result<AccountView, String>| {
            match r {
                Ok(v) => *got.lock().unwrap() = Some(Ok(v)),
                Err(e) if revoking => *note.lock().unwrap() = Some(e),
                Err(e) => *got.lock().unwrap() = Some(Err(e)),
            }
            busy.store(false, Ordering::Relaxed);
        };
        // The desktop app signed in to ndexr asks api.ndexr.io by its key (a
        // Mac has no platform to ask); anything else asks its server.
        #[cfg(not(target_arch = "wasm32"))]
        if crate::account::find_key().is_some() {
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                done(crate::account::overview(&revoke));
                ctx.request_repaint();
            });
            return;
        }
        let q = if revoking { Query::RevokeKey { key_id: revoke } } else { Query::Account };
        backend.query(q, ctx, done);
    }

    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend) {
        if !self.asked {
            self.ask(backend, ctx, String::new());
        }
        let busy = self.busy.load(Ordering::Relaxed);
        let got = self.got.lock().unwrap().clone();
        let note = self.note.lock().unwrap().clone();

        let mut refresh = false;
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("◉").size(13.0).color(theme::primary()));
            if let Some(Ok(v)) = &got {
                ui.label(RichText::new(&v.email).size(12.0).color(theme::text_strong()));
                if v.is_admin {
                    ui.label(RichText::new("admin").size(11.0).color(theme::amber()));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                refresh = theme::tool_if(ui, !busy, "⟳", "ask again").clicked();
                if busy {
                    ui.spinner();
                }
            });
        });
        if refresh {
            self.confirm = None;
            self.ask(backend, ctx, String::new());
        }

        let v = match got {
            None => {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("reading your account…").color(theme::muted())));
                return;
            }
            Some(Err(e)) => {
                ui.add_space(24.0);
                ui.vertical_centered(|ui| ui.label(RichText::new(e).color(theme::alt())));
                return;
            }
            Some(Ok(v)) => v,
        };

        let mut revoke: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.set_max_width(ui.available_width() - 12.0);
                    if let Some(n) = &note {
                        ui.label(RichText::new(n).color(theme::alt()));
                        ui.add_space(6.0);
                    }
                    // One page, laid out as cards (operator, 2026-10-20: "Account
                    // section could look significantly more clean and filled
                    // out"): who you are, the four numbers that matter, then
                    // cell and Claude side by side, domains, keys, activity.
                    profile(ui, &v);
                    ui.add_space(10.0);
                    tiles(ui, &v);
                    ui.add_space(10.0);
                    if ui.available_width() > 820.0 {
                        ui.columns(2, |c| {
                            cell_card(&mut c[0], &v);
                            claude_card(&mut c[1], &v);
                        });
                    } else {
                        cell_card(ui, &v);
                        ui.add_space(10.0);
                        claude_card(ui, &v);
                    }
                    ui.add_space(10.0);
                    self.domains(ui, &v);
                    ui.add_space(10.0);
                    if let Some(k) = self.keys(ui, &v) {
                        revoke = Some(k);
                    }
                    ui.add_space(10.0);
                    events(ui, &v);
                    ui.add_space(16.0);
                });
            });
        });
        if let Some(k) = revoke {
            self.confirm = None;
            self.ask(backend, ctx, k);
        }
    }

    fn domains(&mut self, ui: &mut egui::Ui, v: &AccountView) {
        let published = v.domains.iter().filter(|d| d.published).count();
        card(ui, &format!("Domains · {}", v.domains.len()), &format!("{published} published · what serves each, and who may open it"), |ui| {
            if v.domains.is_empty() {
                ui.label(RichText::new("No domains yet - make one from the Domain menu.").color(theme::muted()));
                return;
            }
            if v.domains.len() > 8 {
                ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("filter domains").desired_width(240.0));
                ui.add_space(6.0);
            }
            let f = self.filter.trim().to_ascii_lowercase();
            for d in v.domains.iter().filter(|d| f.is_empty() || d.domain.contains(&f)) {
                ui.horizontal(|ui| {
                    light(ui, if d.published { theme::ok() } else { theme::hush() });
                    ui.hyperlink_to(RichText::new(&d.domain).monospace().color(theme::text_strong()), format!("https://{}", d.domain))
                        .on_hover_text(if d.published { "published" } else { "not published" });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if !d.kind.is_empty() {
                            ui.label(RichText::new(&d.kind).size(11.0).color(theme::muted())).on_hover_text(&d.target);
                        }
                        pill(ui, &d.access_level, access_color(&d.access_level));
                    });
                });
                ui.add_space(2.0);
            }
        });
    }

    /// The keys, newest first: live ones as rows, revoked ones folded away.
    /// Returns a key id to revoke once its Revoke is clicked twice.
    fn keys(&mut self, ui: &mut egui::Ui, v: &AccountView) -> Option<String> {
        let live: Vec<_> = v.keys.iter().filter(|k| k.revoked_at.is_none()).collect();
        let gone: Vec<_> = v.keys.iter().filter(|k| k.revoked_at.is_some()).collect();
        let mut out = None;
        card(ui, &format!("API keys · {} live", live.len()), "A key signs the ndexr CLI and the desktop app in. New keys are made at api.ndexr.io, which shows the secret once.", |ui| {
            if live.is_empty() {
                ui.label(RichText::new("No live keys.").color(theme::muted()));
            }
            for k in &live {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(k.label.as_deref().unwrap_or("unnamed key")).color(theme::text_strong()));
                            ui.label(RichText::new(&k.key_id).monospace().size(11.0).color(theme::hush())).on_hover_text(format!("scopes: {}", k.scopes.as_deref().unwrap_or("*")));
                        });
                        ui.label(
                            RichText::new(format!(
                                "made {} · last used {} · {} uses · expires {}",
                                k.created_at.as_deref().unwrap_or("–"),
                                k.last_used_at.as_deref().unwrap_or("never"),
                                k.use_count,
                                k.expires_at.as_deref().unwrap_or("never")
                            ))
                            .size(11.0)
                            .color(theme::muted()),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if k.this {
                            pill(ui, "this app", theme::primary());
                        } else if self.confirm.as_deref() == Some(k.key_id.as_str()) {
                            if ui.small_button("keep").clicked() {
                                self.confirm = None;
                            }
                            if ui.button(RichText::new("Revoke it").color(theme::alt())).clicked() {
                                out = Some(k.key_id.clone());
                            }
                        } else if ui.small_button("Revoke").on_hover_text("anything signed in with this key stops working").clicked() {
                            self.confirm = Some(k.key_id.clone());
                        }
                    });
                });
                ui.add_space(4.0);
            }
            if !gone.is_empty() {
                egui::CollapsingHeader::new(RichText::new(format!("{} revoked", gone.len())).size(11.5).color(theme::muted())).id_salt("acct-revoked").show(ui, |ui| {
                    for k in &gone {
                        ui.label(
                            RichText::new(format!("{} · {} · revoked {}", k.label.as_deref().unwrap_or("unnamed"), k.key_id, k.revoked_at.as_deref().unwrap_or("")))
                                .size(11.0)
                                .color(theme::hush()),
                        );
                    }
                });
            }
        });
        out
    }
}

/// A titled card on the page.
fn card(ui: &mut egui::Ui, title: &str, sub: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::surface())
        .stroke(egui::Stroke::new(1.0, theme::moss()))
        .corner_radius(8.0)
        .inner_margin(14.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(title).font(theme::display(15.0)).color(theme::text_strong()));
            if !sub.is_empty() {
                ui.label(RichText::new(sub).size(11.5).color(theme::muted()));
            }
            ui.add_space(8.0);
            add(ui);
        });
}

/// A small rounded label in a status colour.
fn pill(ui: &mut egui::Ui, text: &str, c: Color32) {
    if text.is_empty() {
        return;
    }
    egui::Frame::new().fill(c.gamma_multiply(0.18)).corner_radius(9.0).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.label(RichText::new(text).size(11.0).color(c));
    });
}

/// A status light.
fn light(ui: &mut egui::Ui, c: Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
    ui.painter().circle_filled(r.center(), 4.0, c);
}

fn access_color(level: &str) -> Color32 {
    match level {
        "public" => theme::ok(),
        "login" => theme::primary(),
        "owner" => theme::net(),
        "admin" => theme::amber(),
        "password" => theme::warn(),
        _ => theme::muted(),
    }
}

fn cell_state(v: &AccountView) -> (&'static str, Color32) {
    if !v.ready {
        ("not ready", theme::warn())
    } else if v.cell_enabled {
        ("on", theme::ok())
    } else {
        ("off", theme::hush())
    }
}

/// Who you are: a monogram, your name and email, and where your site is.
fn profile(ui: &mut egui::Ui, v: &AccountView) {
    let name = v.username.clone().unwrap_or_else(|| v.email.split('@').next().unwrap_or("").to_owned());
    egui::Frame::new()
        .fill(theme::surface())
        .stroke(egui::Stroke::new(1.0, theme::moss()))
        .corner_radius(8.0)
        .inner_margin(16.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(52.0, 52.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 26.0, theme::primary().gamma_multiply(0.25));
                let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, initial, theme::display(24.0), theme::primary());
                ui.add_space(10.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&name).font(theme::display(20.0)).color(theme::text_strong()));
                        if v.is_admin {
                            pill(ui, "admin", theme::amber());
                        }
                        let (s, c) = cell_state(v);
                        pill(ui, &format!("cell {s}"), c);
                    });
                    ui.label(RichText::new(&v.email).color(theme::muted()));
                    ui.horizontal(|ui| {
                        if let Some(pd) = &v.personal_domain {
                            ui.hyperlink_to(RichText::new(pd).monospace().size(12.0), format!("https://{pd}"));
                        }
                        if let Some(c) = &v.cell {
                            ui.label(RichText::new("·").color(theme::hush()));
                            ui.hyperlink_to(RichText::new(c.trim_start_matches("https://")).monospace().size(12.0), c);
                        }
                    });
                });
            });
        });
}

/// Four numbers across the page.
fn tiles(ui: &mut egui::Ui, v: &AccountView) {
    let published = v.domains.iter().filter(|d| d.published).count();
    let live = v.keys.iter().filter(|k| k.revoked_at.is_none()).count();
    let (cs, cc) = cell_state(v);
    let claude = v.claude.as_ref().map(|c| (c.title.clone(), c.kind.clone())).unwrap_or_else(|| ("unknown".into(), String::new()));
    let items: [(String, &str, String, Color32); 4] = [
        (v.domains.len().to_string(), "domains", format!("{published} published"), theme::text_strong()),
        (live.to_string(), "live API keys", format!("{} in all", v.keys.len()), theme::text_strong()),
        (cs.to_owned(), "cell", v.cell_last_seen.clone().map(|t| format!("seen {t}")).unwrap_or_default(), cc),
        (claude.0, "Claude", claude.1, theme::text_strong()),
    ];
    let n = if ui.available_width() > 620.0 { 4 } else { 2 };
    ui.columns(n, |cols| {
        for (i, (val, what, sub, c)) in items.iter().enumerate() {
            let ui = &mut cols[i % n];
            egui::Frame::new().fill(theme::surface()).stroke(egui::Stroke::new(1.0, theme::moss())).corner_radius(8.0).inner_margin(12.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(RichText::new(val).font(theme::display(20.0)).color(*c)).truncate());
                ui.label(RichText::new(*what).size(11.5).color(theme::text()));
                ui.add(egui::Label::new(RichText::new(if sub.is_empty() { " " } else { sub.as_str() }).size(11.0).color(theme::muted())).truncate());
            });
            if n == 2 && i < 2 {
                ui.add_space(8.0);
            }
        }
    });
}

fn row(ui: &mut egui::Ui, k: &str, add: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(k).size(11.5).color(theme::hush()));
    add(ui);
    ui.end_row();
}

fn cell_card(ui: &mut egui::Ui, v: &AccountView) {
    card(ui, "Your cell", "Your own machine on ndexr: your home, your domains' code, R, Python and a shell.", |ui| {
        egui::Grid::new("acct-cell").num_columns(2).spacing([18.0, 6.0]).show(ui, |ui| {
            let (s, c) = cell_state(v);
            row(ui, "state", |ui| {
                let words = if !v.ready {
                    format!("not ready ({})", v.provision_status.as_deref().unwrap_or("pending"))
                } else if v.cell_enabled {
                    "on".to_owned()
                } else {
                    "off - ask for it to be turned on".to_owned()
                };
                ui.horizontal(|ui| {
                    light(ui, c);
                    ui.label(RichText::new(words).color(if s == "on" { theme::text() } else { theme::muted() }));
                });
            });
            if v.ready {
                row(ui, "services", |ui| {
                    ui.horizontal(|ui| {
                        for (up, what) in [(v.up_app, "app"), (v.up_code, "code")] {
                            let (c, w) = match up {
                                Some(true) => (theme::ok(), "up"),
                                Some(false) => (theme::hush(), "stopped"),
                                None => (theme::warn(), "unknown"),
                            };
                            light(ui, c);
                            ui.label(RichText::new(format!("{what} {w}")).color(theme::muted()));
                            ui.add_space(8.0);
                        }
                    });
                });
            }
            row(ui, "username", |ui| {
                ui.label(RichText::new(v.username.as_deref().unwrap_or("not picked yet")).monospace());
            });
            if let Some(t) = &v.cell_last_seen {
                row(ui, "last seen", |ui| {
                    ui.label(RichText::new(t).color(theme::muted()));
                });
            }
            if let Some(m) = v.idle_timeout_min {
                row(ui, "sleeps after", |ui| {
                    ui.label(RichText::new(format!("{m} min idle")).color(theme::muted()));
                });
            }
        });
    });
}

fn claude_card(ui: &mut egui::Ui, v: &AccountView) {
    card(ui, "Claude", "The model your agents run on, and whose account pays for it.", |ui| {
        let Some(c) = &v.claude else {
            ui.label(RichText::new("Could not be asked - ⟳ above tries again.").color(theme::muted()));
            return;
        };
        let col: Color32 = match c.kind.as_str() {
            "own" | "operator" => theme::ok(),
            "plan" => theme::net(),
            "none" => theme::warn(),
            _ => theme::muted(),
        };
        ui.horizontal(|ui| {
            light(ui, col);
            ui.label(RichText::new(&c.title).color(theme::text_strong()));
        });
        if !c.label.is_empty() {
            ui.label(RichText::new(&c.label).size(11.5).color(theme::muted()));
        }
        if !c.detail.is_empty() {
            ui.label(RichText::new(&c.detail).size(11.5).color(theme::muted()));
        }
        if let (Some(left), Some(limit)) = (c.seat_left, c.seat_limit) {
            ui.add_space(6.0);
            let reset = c.seat_resets.as_deref().map(|r| format!(" · resets {r}")).unwrap_or_default();
            ui.label(RichText::new(format!("Shared seat: {left} of {limit} left this week{reset}")).size(11.5).color(theme::text()));
            let frac = if limit > 0 { left as f32 / limit as f32 } else { 0.0 };
            ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).desired_height(8.0).fill(if frac < 0.2 { theme::warn() } else { theme::primary() }));
        }
        if !matches!(c.kind.as_str(), "own" | "operator") {
            ui.add_space(8.0);
            crate::setup::connect_button(ui, "Connect your Claude");
        }
    });
}

fn events(ui: &mut egui::Ui, v: &AccountView) {
    card(ui, "Activity", "What your keys did lately, newest first.", |ui| {
        if v.events.is_empty() {
            ui.label(RichText::new("Nothing yet.").color(theme::muted()));
            return;
        }
        let row = |ui: &mut egui::Ui, e: &crate::api::AccountEvent| {
            let s = |x: &Option<String>| x.clone().unwrap_or_default();
            ui.horizontal(|ui| {
                ui.add_sized([150.0, 16.0], egui::Label::new(RichText::new(s(&e.at)).size(11.0).color(theme::muted())).truncate());
                ui.label(RichText::new(s(&e.event)).size(11.5).color(theme::text_strong()));
                let more = [s(&e.key_id), s(&e.audience), s(&e.detail)].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
                ui.add(egui::Label::new(RichText::new(more).size(11.0).color(theme::hush())).truncate());
            });
        };
        for e in v.events.iter().take(12) {
            row(ui, e);
        }
        if v.events.len() > 12 {
            egui::CollapsingHeader::new(RichText::new(format!("{} more", v.events.len() - 12)).size(11.5).color(theme::muted())).id_salt("acct-events-more").show(ui, |ui| {
                for e in v.events.iter().skip(12) {
                    row(ui, e);
                }
            });
        }
    });
}
