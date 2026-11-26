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

use eframe::egui::{self, Align, Color32, Layout, RichText, Sense, UiBuilder};

use crate::api::{AccountView, Query};
use crate::backend::Backend;
use crate::ui::kit::icons::Glyph;
use crate::ui::kit::{bar, theme};

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
    /// Every domain listed, not the first page.
    all_domains: bool,
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
        if crate::host::account::find_key().is_some() {
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                done(crate::host::account::overview(&revoke));
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

        // The page's head, as a console's (operator, 2026-11-26: "needs to be
        // 100x more professional, think amazon, anthropic, openai, google"):
        // its name large, what it is about beside, one control at the right.
        let mut refresh = false;
        ui.add_space(6.0);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            refresh = bar::glyph(ui, Glyph::Refresh, "read your account again", !busy).clicked();
            if busy {
                ui.spinner();
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                ui.label(RichText::new("Account").font(theme::display(24.0)).color(theme::text_strong()));
            });
        });
        ui.add_space(14.0);
        if refresh {
            self.confirm = None;
            self.ask(backend, ctx, String::new());
        }

        let v = match got {
            None => {
                ui.add_space(24.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("Reading your account").color(theme::text()));
                });
                return;
            }
            Some(Err(e)) => {
                banner(ui, &e, theme::alt());
                return;
            }
            Some(Ok(v)) => v,
        };

        let mut revoke: Option<String> = None;
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // Every row fits the page's width or wraps: one row wider than a
            // phone widened the whole column once (IMG_8746, 2026-11-14).
            ui.set_width(ui.available_width());
            if let Some(n) = &note {
                banner(ui, n, theme::alt());
            }
            profile(ui, &v);
            ui.add_space(GAP);
            tiles(ui, &v);
            ui.add_space(GAP);
            if ui.available_width() > 820.0 {
                ui.columns(2, |c| {
                    cell_card(&mut c[0], &v);
                    claude_card(&mut c[1], &v);
                });
            } else {
                cell_card(ui, &v);
                ui.add_space(GAP);
                claude_card(ui, &v);
            }
            ui.add_space(GAP);
            self.domains(ui, &v);
            ui.add_space(GAP);
            if let Some(k) = self.keys(ui, &v) {
                revoke = Some(k);
            }
            ui.add_space(GAP);
            events(ui, &v);
            ui.add_space(24.0);
        });
        if let Some(k) = revoke {
            self.confirm = None;
            self.ask(backend, ctx, k);
        }
    }

    fn domains(&mut self, ui: &mut egui::Ui, v: &AccountView) {
        let published = v.domains.iter().filter(|d| d.published).count();
        let f = self.filter.trim().to_ascii_lowercase();
        let rows: Vec<_> = v.domains.iter().filter(|d| f.is_empty() || d.domain.contains(&f)).collect();
        let filter = &mut self.filter;
        let all = &mut self.all_domains;
        section(
            ui,
            "Domains",
            &format!("{} · {published} published", v.domains.len()),
            |ui| {
                if v.domains.len() > 8 {
                    ui.add(egui::TextEdit::singleline(filter).hint_text("Filter domains").desired_width(220.0).margin(egui::Margin::symmetric(8, 5)));
                }
            },
            |ui| {
                if v.domains.is_empty() {
                    empty(ui, "No domains yet. Make one from the Domain menu.");
                    return;
                }
                if rows.is_empty() {
                    empty(ui, "No domain matches that filter.");
                    return;
                }
                let shown = if *all || !f.is_empty() { rows.len() } else { rows.len().min(PAGE) };
                let cols = [
                    Col { title: "Domain", w: FLEX, from: 0.0, right: false },
                    Col { title: "Status", w: 130.0, from: 520.0, right: false },
                    Col { title: "Access", w: 90.0, from: 0.0, right: false },
                    Col { title: "Served by", w: 110.0, from: 680.0, right: false },
                ];
                table(ui, "acct-domains", &cols, shown, |ui, r, c| {
                    let d = rows[r];
                    match c {
                        0 => {
                            let url = format!("https://{}", d.domain);
                            let resp = ui.add(egui::Label::new(RichText::new(&d.domain).size(13.0).color(theme::primary())).truncate().sense(Sense::click()));
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(&url).clicked() {
                                crate::ui::kit::links::new_tab(ui.ctx(), url);
                            }
                        }
                        1 => status(ui, d.published, if d.published { "Published" } else { "Not published" }),
                        2 => badge(ui, &title_case(&d.access_level), access_color(&d.access_level)),
                        _ => {
                            ui.add(egui::Label::new(RichText::new(served_by(&d.kind)).size(12.5).color(theme::text())).truncate()).on_hover_text(&d.target);
                        }
                    }
                });
                if shown < rows.len() || (*all && f.is_empty() && rows.len() > PAGE) {
                    footer(ui, |ui| {
                        ui.label(RichText::new(format!("Showing {shown} of {}", rows.len())).size(12.5).color(theme::text()));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let word = if *all { "Show fewer" } else { "Show all" };
                            if link(ui, word).clicked() {
                                *all = !*all;
                            }
                        });
                    });
                }
            },
        );
    }

    /// The keys, newest first: live ones as rows, revoked ones folded away.
    /// Returns a key id to revoke once its Revoke is clicked twice.
    fn keys(&mut self, ui: &mut egui::Ui, v: &AccountView) -> Option<String> {
        let live: Vec<_> = v.keys.iter().filter(|k| k.revoked_at.is_none()).collect();
        let gone: Vec<_> = v.keys.iter().filter(|k| k.revoked_at.is_some()).collect();
        let mut out = None;
        let confirm = &mut self.confirm;
        section(
            ui,
            "API keys",
            &format!("{} active · {} revoked", live.len(), gone.len()),
            |ui| {
                ui.hyperlink_to(RichText::new("Create key ↗").size(12.5), "https://api.ndexr.io/");
            },
            |ui| {
                if live.is_empty() {
                    empty(ui, "No active keys. Create one at api.ndexr.io; it signs in the ndexr CLI and the desktop app.");
                } else {
                    let cols = [
                        Col { title: "Name", w: FLEX, from: 0.0, right: false },
                        Col { title: "Key", w: 150.0, from: 560.0, right: false },
                        Col { title: "Last used", w: 150.0, from: 720.0, right: false },
                        Col { title: "Uses", w: 56.0, from: 820.0, right: true },
                        Col { title: "Expires", w: 110.0, from: 900.0, right: false },
                        Col { title: "", w: 150.0, from: 0.0, right: true },
                    ];
                    table(ui, "acct-keys", &cols, live.len(), |ui, r, c| {
                        let k = live[r];
                        match c {
                            0 => {
                                ui.add(egui::Label::new(RichText::new(k.label.as_deref().unwrap_or("Unnamed key")).size(13.0).color(theme::text_strong())).truncate())
                                    .on_hover_text(format!("made {} · scopes {}", k.created_at.as_deref().unwrap_or("–"), k.scopes.as_deref().unwrap_or("*")));
                            }
                            1 => {
                                ui.add(egui::Label::new(RichText::new(&k.key_id).monospace().size(12.0).color(theme::text())).truncate());
                            }
                            2 => {
                                ui.add(egui::Label::new(RichText::new(when(k.last_used_at.as_deref(), "Never")).size(12.5).color(theme::text())).truncate());
                            }
                            3 => {
                                ui.label(RichText::new(k.use_count.to_string()).size(12.5).color(theme::text()));
                            }
                            4 => {
                                ui.add(egui::Label::new(RichText::new(when(k.expires_at.as_deref(), "Never")).size(12.5).color(theme::text())).truncate());
                            }
                            _ => {
                                if k.this {
                                    badge(ui, "This app", theme::primary());
                                } else if confirm.as_deref() == Some(k.key_id.as_str()) {
                                    if quiet_button(ui, "Revoke", theme::alt(), true).on_hover_text("anything signed in with this key stops working").clicked() {
                                        out = Some(k.key_id.clone());
                                    }
                                    if quiet_button(ui, "Cancel", theme::text(), false).clicked() {
                                        *confirm = None;
                                    }
                                } else if quiet_button(ui, "Revoke", theme::text(), false).clicked() {
                                    *confirm = Some(k.key_id.clone());
                                }
                            }
                        }
                    });
                }
                if !gone.is_empty() {
                    footer(ui, |ui| {
                        egui::CollapsingHeader::new(RichText::new(format!("{} revoked keys", gone.len())).size(12.5).color(theme::text())).id_salt("acct-revoked").show(ui, |ui| {
                            for k in &gone {
                                ui.label(
                                    RichText::new(format!("{} · {} · revoked {}", k.label.as_deref().unwrap_or("Unnamed"), k.key_id, when(k.revoked_at.as_deref(), "")))
                                        .size(12.0)
                                        .color(theme::text()),
                                );
                            }
                        });
                    });
                }
            },
        );
        out
    }
}

/// Between cards.
const GAP: f32 = 16.0;
/// Domains listed before "Show all".
const PAGE: usize = 25;
const ROW_H: f32 = 40.0;
/// Inside a card, left and right.
const PAD: f32 = 20.0;

/// A section of the page: one bordered card, its head (name, count, the
/// section's own control at the right) ruled off from its body. The body
/// runs edge to edge, so a table's rules meet the border; `padded` insets
/// anything else.
fn section(ui: &mut egui::Ui, title: &str, count: &str, actions: impl FnOnce(&mut egui::Ui), body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(theme::bg()).stroke(egui::Stroke::new(1.0, theme::moss())).corner_radius(10.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::Frame::NONE.inner_margin(egui::Margin { left: PAD as i8, right: 14, top: 14, bottom: 14 }).show(ui, |ui| {
            ui.set_min_height(28.0);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                actions(ui);
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.label(RichText::new(title).font(theme::display(15.5)).color(theme::text_strong()));
                    if !count.is_empty() {
                        ui.add_space(4.0);
                        ui.add(egui::Label::new(RichText::new(count).size(12.5).color(theme::text())).truncate());
                    }
                });
            });
        });
        rule(ui);
        body(ui);
    });
}

fn padded(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::NONE.inner_margin(egui::Margin { left: PAD as i8, right: PAD as i8, top: 14, bottom: 16 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        add(ui);
    });
}

/// A row under a table: its count, "Show all", the folded revoked keys.
fn footer(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    rule(ui);
    egui::Frame::NONE.inner_margin(egui::Margin { left: PAD as i8, right: PAD as i8, top: 10, bottom: 10 }).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(add);
    });
}

fn rule(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(r.x_range(), r.center().y, egui::Stroke::new(1.0, theme::moss()));
}

fn empty(ui: &mut egui::Ui, words: &str) {
    padded(ui, |ui| {
        ui.add(egui::Label::new(RichText::new(words).size(13.0).color(theme::text())).wrap());
    });
}

fn banner(ui: &mut egui::Ui, words: &str, c: Color32) {
    egui::Frame::new().fill(c.gamma_multiply(0.10)).stroke(egui::Stroke::new(1.0, c.gamma_multiply(0.5))).corner_radius(8.0).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add(egui::Label::new(RichText::new(words).size(13.0).color(theme::text_strong())).wrap());
    });
    ui.add_space(GAP);
}

/// A table's column: a fixed width (or `FLEX`, a share of what is left), and
/// the table width below which it is left out - a phone keeps the columns
/// that name and act, and drops the rest.
struct Col {
    title: &'static str,
    w: f32,
    from: f32,
    right: bool,
}
const FLEX: f32 = 0.0;

/// Rows under a header row, hairlines between, a wash on the row under the
/// pointer. `cell(ui, row, column)` draws each cell in its own clipped space.
fn table(ui: &mut egui::Ui, id: &str, cols: &[Col], rows: usize, mut cell: impl FnMut(&mut egui::Ui, usize, usize)) {
    let width = ui.available_width();
    let shown: Vec<usize> = (0..cols.len()).filter(|&i| width >= cols[i].from).collect();
    let gap = 16.0;
    let fixed: f32 = shown.iter().map(|&i| cols[i].w).sum::<f32>() + gap * shown.len().saturating_sub(1) as f32;
    let flex_n = shown.iter().filter(|&&i| cols[i].w == FLEX).count().max(1) as f32;
    let flex_w = ((width - 2.0 * PAD - fixed) / flex_n).max(80.0);
    let widths: Vec<f32> = shown.iter().map(|&i| if cols[i].w == FLEX { flex_w } else { cols[i].w }).collect();
    let spans = |r: egui::Rect| {
        let mut x = r.left() + PAD;
        widths
            .iter()
            .map(|w| {
                let c = egui::Rect::from_min_max(egui::pos2(x, r.top()), egui::pos2((x + w).min(r.right() - PAD), r.bottom()));
                x += w + gap;
                c
            })
            .collect::<Vec<_>>()
    };
    let (head, _) = ui.allocate_exact_size(egui::vec2(width, 34.0), Sense::hover());
    ui.painter().rect_filled(head, 0.0, theme::surface());
    ui.painter().hline(head.x_range(), head.bottom() - 0.5, egui::Stroke::new(1.0, theme::moss()));
    for (k, span) in spans(head).into_iter().enumerate() {
        let col = &cols[shown[k]];
        let g = ui.painter().layout_no_wrap(col.title.to_owned(), theme::display(12.0), theme::text());
        let x = if col.right { span.right() - g.size().x } else { span.left() };
        ui.painter().with_clip_rect(span).galley(egui::pos2(x, span.center().y - g.size().y / 2.0), g, theme::text());
    }
    for row in 0..rows {
        let (r, _) = ui.allocate_exact_size(egui::vec2(width, ROW_H), Sense::hover());
        if ui.rect_contains_pointer(r) {
            ui.painter().rect_filled(r, 0.0, theme::surface().gamma_multiply(0.7));
        }
        if row + 1 < rows {
            ui.painter().hline(r.x_range(), r.bottom() - 0.5, egui::Stroke::new(1.0, theme::moss().gamma_multiply(0.7)));
        }
        for (k, span) in spans(r).into_iter().enumerate() {
            let i = shown[k];
            let layout = if cols[i].right { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
            let mut c = ui.new_child(UiBuilder::new().max_rect(span).layout(layout).id_salt((id, row, i)));
            c.set_clip_rect(span.intersect(ui.clip_rect()));
            cell(&mut c, row, i);
        }
    }
}

/// A status word in its colour on a tint of it, square-cornered as a
/// console's - never colour alone.
fn badge(ui: &mut egui::Ui, text: &str, c: Color32) {
    if text.is_empty() {
        return;
    }
    egui::Frame::new()
        .fill(c.gamma_multiply(0.12))
        .stroke(egui::Stroke::new(1.0, c.gamma_multiply(0.35)))
        .corner_radius(4.0)
        .inner_margin(egui::Margin::symmetric(7, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(11.5).color(c));
        });
}

/// A light and its word.
fn status(ui: &mut egui::Ui, good: bool, words: &str) {
    light(ui, if good { theme::ok() } else { theme::hush() });
    ui.label(RichText::new(words).size(12.5).color(theme::text()));
}

/// A status light.
fn light(ui: &mut egui::Ui, c: Color32) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
    ui.painter().circle_filled(r.center(), 3.5, c);
}

/// An outlined button for a row's action; `strong` fills it in its colour's
/// tint (the second click of a Revoke).
fn quiet_button(ui: &mut egui::Ui, text: &str, c: Color32, strong: bool) -> egui::Response {
    let b = egui::Button::new(RichText::new(text).size(12.5).color(c))
        .fill(if strong { c.gamma_multiply(0.14) } else { Color32::TRANSPARENT })
        .stroke(egui::Stroke::new(1.0, if strong { c.gamma_multiply(0.6) } else { theme::moss() }))
        .corner_radius(6.0)
        .min_size(egui::vec2(0.0, 26.0));
    ui.add(b)
}

fn link(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Label::new(RichText::new(text).size(12.5).color(theme::primary())).sense(Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn title_case(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// What serves a domain, in words.
fn served_by(kind: &str) -> String {
    match kind {
        "platform" => "Platform".to_owned(),
        "port" => "Own server".to_owned(),
        "" => "–".to_owned(),
        k => title_case(k),
    }
}

/// A time as the API gives it, to the minute; `none` when there is none.
fn when(t: Option<&str>, none: &str) -> String {
    match t {
        Some(t) if !t.is_empty() => t.replace('T', " ").chars().take(16).collect(),
        _ => none.to_owned(),
    }
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
        ("Not ready", theme::warn())
    } else if v.cell_enabled {
        ("On", theme::ok())
    } else {
        ("Off", theme::hush())
    }
}

/// Who you are: a monogram, your name and email, your role and cell as
/// badges, and your two addresses.
fn profile(ui: &mut egui::Ui, v: &AccountView) {
    let name = v.username.clone().unwrap_or_else(|| v.email.split('@').next().unwrap_or("").to_owned());
    egui::Frame::new().fill(theme::bg()).stroke(egui::Stroke::new(1.0, theme::moss())).corner_radius(10.0).inner_margin(egui::Margin::symmetric(PAD as i8, 18)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 24.0, theme::primary());
            let initial = name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default();
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, initial, theme::display(21.0), theme::bg());
            ui.add_space(14.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&name).font(theme::display(19.0)).color(theme::text_strong()));
                    ui.add_space(4.0);
                    if v.is_admin {
                        badge(ui, "Admin", theme::amber());
                    }
                    let (s, c) = cell_state(v);
                    badge(ui, &format!("Cell {}", s.to_lowercase()), c);
                });
                ui.add(egui::Label::new(RichText::new(&v.email).size(13.0).color(theme::text())).wrap());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 16.0;
                    if let Some(pd) = &v.personal_domain {
                        ui.hyperlink_to(RichText::new(format!("{pd} ↗")).size(12.5), format!("https://{pd}"));
                    }
                    if let Some(c) = &v.cell {
                        ui.hyperlink_to(RichText::new(format!("{} ↗", c.trim_start_matches("https://"))).size(12.5), c);
                    }
                });
            });
        });
    });
}

/// Four numbers across the page: what it is, the number, one line under it.
fn tiles(ui: &mut egui::Ui, v: &AccountView) {
    let published = v.domains.iter().filter(|d| d.published).count();
    let live = v.keys.iter().filter(|k| k.revoked_at.is_none()).count();
    let (cs, cc) = cell_state(v);
    let (claude, claude_sub) = match &v.claude {
        Some(c) => (
            match c.kind.as_str() {
                "own" => "Your account".to_owned(),
                "operator" => "Operator".to_owned(),
                "plan" => "ndexr plan".to_owned(),
                "none" => "Not connected".to_owned(),
                _ => c.title.clone(),
            },
            if c.label.is_empty() { c.title.clone() } else { c.label.clone() },
        ),
        None => ("Unknown".to_owned(), String::new()),
    };
    let items: [(&str, String, String, Color32); 4] = [
        ("Domains", v.domains.len().to_string(), format!("{published} published"), theme::text_strong()),
        ("API keys", live.to_string(), format!("{} in all", v.keys.len()), theme::text_strong()),
        ("Cell", cs.to_owned(), v.cell_last_seen.as_deref().map(|t| format!("Seen {}", when(Some(t), ""))).unwrap_or_default(), cc),
        ("Claude", claude, claude_sub, theme::text_strong()),
    ];
    let n = if ui.available_width() > 620.0 { 4 } else { 2 };
    ui.columns(n, |cols| {
        for (i, (what, val, sub, c)) in items.iter().enumerate() {
            let ui = &mut cols[i % n];
            egui::Frame::new().fill(theme::bg()).stroke(egui::Stroke::new(1.0, theme::moss())).corner_radius(10.0).inner_margin(egui::Margin::symmetric(18, 16)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(RichText::new(*what).font(theme::display(12.5)).color(theme::text()));
                ui.add(egui::Label::new(RichText::new(val).font(theme::display(24.0)).color(*c)).truncate());
                ui.add(egui::Label::new(RichText::new(if sub.is_empty() { " " } else { sub.as_str() }).size(12.0).color(theme::text())).truncate());
            });
            if n == 2 && i < 2 {
                ui.add_space(GAP);
            }
        }
    });
}

/// A key-value row of a description list, ruled under.
fn row(ui: &mut egui::Ui, k: &str, add: impl FnOnce(&mut egui::Ui)) {
    let w = ui.available_width();
    ui.horizontal(|ui| {
        ui.set_min_height(36.0);
        ui.add_sized([120.0, 20.0], egui::Label::new(RichText::new(k).size(12.5).color(theme::muted())).truncate());
        ui.vertical(|ui| {
            ui.set_max_width((w - 130.0).max(120.0));
            add(ui);
        });
    });
}

fn cell_card(ui: &mut egui::Ui, v: &AccountView) {
    section(ui, "Your cell", "", |_| {}, |ui| {
        padded(ui, |ui| {
            row(ui, "State", |ui| {
                let words = if !v.ready {
                    format!("Not ready ({})", v.provision_status.as_deref().unwrap_or("pending"))
                } else if v.cell_enabled {
                    "On".to_owned()
                } else {
                    "Off - ask for it to be turned on".to_owned()
                };
                ui.horizontal(|ui| {
                    light(ui, cell_state(v).1);
                    ui.label(RichText::new(words).size(13.0).color(theme::text_strong()));
                });
            });
            if v.ready {
                row(ui, "Services", |ui| {
                    ui.horizontal_wrapped(|ui| {
                        for (up, what) in [(v.up_app, "App"), (v.up_code, "Code")] {
                            let (c, w) = match up {
                                Some(true) => (theme::ok(), "running"),
                                Some(false) => (theme::hush(), "stopped"),
                                None => (theme::warn(), "unknown"),
                            };
                            light(ui, c);
                            ui.label(RichText::new(format!("{what} {w}")).size(13.0).color(theme::text()));
                            ui.add_space(10.0);
                        }
                    });
                });
            }
            row(ui, "Username", |ui| {
                ui.label(RichText::new(v.username.as_deref().unwrap_or("Not picked yet")).monospace().size(12.5).color(theme::text_strong()));
            });
            if let Some(t) = &v.cell_last_seen {
                row(ui, "Last seen", |ui| {
                    ui.label(RichText::new(when(Some(t), "")).size(13.0).color(theme::text()));
                });
            }
            // Your sites rest when nobody visits and wake on the next visit
            // (the cell agent; operator, 2026-11-17); a subscription keeps
            // them awake.
            row(ui, "Idle sites", |ui| {
                if v.always_awake {
                    ui.label(RichText::new("Always awake on your subscription").size(13.0).color(theme::text()));
                } else {
                    let m = v.idle_timeout_min.unwrap_or(30);
                    ui.add(egui::Label::new(RichText::new(format!("Sleep after {m} min with no visitors; wake on the next visit")).size(13.0).color(theme::text())).wrap());
                    ui.hyperlink_to(RichText::new("Keep them awake ↗").size(12.5), "https://billing.ndexr.io/");
                }
            });
        });
    });
}

fn claude_card(ui: &mut egui::Ui, v: &AccountView) {
    section(ui, "Claude", "", |_| {}, |ui| {
        padded(ui, |ui| {
            let Some(c) = &v.claude else {
                ui.label(RichText::new("Could not be read. Refresh at the top of the page.").size(13.0).color(theme::text()));
                return;
            };
            let col: Color32 = match c.kind.as_str() {
                "own" | "operator" => theme::ok(),
                "plan" => theme::net(),
                "none" => theme::warn(),
                _ => theme::muted(),
            };
            row(ui, "Model access", |ui| {
                ui.horizontal(|ui| {
                    light(ui, col);
                    ui.add(egui::Label::new(RichText::new(&c.title).size(13.0).color(theme::text_strong())).wrap());
                });
            });
            if !c.label.is_empty() {
                row(ui, "Account", |ui| {
                    ui.add(egui::Label::new(RichText::new(&c.label).size(13.0).color(theme::text())).wrap());
                });
            }
            if !c.detail.is_empty() {
                row(ui, "Billing", |ui| {
                    ui.add(egui::Label::new(RichText::new(&c.detail).size(13.0).color(theme::text())).wrap());
                });
            }
            if let (Some(left), Some(limit)) = (c.seat_left, c.seat_limit) {
                row(ui, "This week", |ui| {
                    let reset = c.seat_resets.as_deref().map(|r| format!(" · resets {r}")).unwrap_or_default();
                    ui.label(RichText::new(format!("{left} of {limit} left{reset}")).size(13.0).color(theme::text()));
                    let frac = if limit > 0 { left as f32 / limit as f32 } else { 0.0 };
                    ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).desired_height(6.0).fill(if frac < 0.2 { theme::warn() } else { theme::primary() }));
                });
            }
            if !matches!(c.kind.as_str(), "own" | "operator") {
                ui.add_space(8.0);
                crate::ui::setup::connect_button(ui, "Connect your Claude", false);
            }
        });
    });
}

fn events(ui: &mut egui::Ui, v: &AccountView) {
    let n = v.events.len();
    section(ui, "Activity", &if n > 0 { format!("{n} recent") } else { String::new() }, |_| {}, |ui| {
        if v.events.is_empty() {
            empty(ui, "Nothing yet. What your keys do shows here.");
            return;
        }
        let s = |x: &Option<String>| x.clone().unwrap_or_default();
        let cols = [
            Col { title: "Time", w: 140.0, from: 0.0, right: false },
            Col { title: "Event", w: 170.0, from: 0.0, right: false },
            Col { title: "Details", w: FLEX, from: 560.0, right: false },
        ];
        let list: Vec<_> = v.events.iter().take(20).collect();
        table(ui, "acct-events", &cols, list.len(), |ui, r, c| {
            let e = list[r];
            match c {
                0 => {
                    ui.add(egui::Label::new(RichText::new(when(e.at.as_deref(), "")).size(12.5).color(theme::text())).truncate());
                }
                1 => {
                    ui.add(egui::Label::new(RichText::new(s(&e.event)).size(13.0).color(theme::text_strong())).truncate());
                }
                _ => {
                    let more = [s(&e.key_id), s(&e.audience), s(&e.detail)].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
                    ui.add(egui::Label::new(RichText::new(more).size(12.5).color(theme::text())).truncate());
                }
            }
        });
    });
}
