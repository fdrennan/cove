//! Project › Access: who may open a project's app, chosen where the project is
//! worked on (operator, 2026-10-28: "All users, Logged in users, specific users
//! or groups ... users need some way within the ide section ... as a drop down
//! ... in the project to set settings like that"). Connect is for finding
//! apps; this is for deciding who finds yours.
//!
//! Three choices, the platform's own levels (domain_registry.access_level, the
//! same ones route.ndexr.io sets): Everyone (public), Anyone signed in (login),
//! Only people and groups you choose (owner, plus the shares of
//! people_sharing.sql - Messages' people and groups). Set by `cove multiuser`
//! as root (Query::AppAccess, assets/set-access.sh): the registry for a Shiny
//! module, and the vhost's gate too for an app on its own port.
//!
//! A share is one of two roles (operator, 2026-11-02: "the ability for an end
//! user to be able to help collaborate on a site with me"): Can open it, or
//! Can work on it - its files at ~/shared/<host> in their Cove and its agent,
//! whose thread is the owner's too (share_editors; multiuser.rs
//! shares_reconcile). Archive, access and sharing stay the owner's.
//!
//! Anyone can be shared with, found by typing part of their name (operator,
//! 2026-11-02: "need to be able to easily select another user, all users
//! publicly searchable to all users"): the search is Messages' own
//! (InboxView.found, every ndexr account), and a share to a person is still an
//! offer they accept. The same three levels draw as Settings › Access does
//! (settings_tab::option_row), so the two places look like one.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::api::{AppAccessView, InboxAct, InboxView, Person, Query};
use crate::backend::Backend;
use crate::inbox::Inbox;
use crate::theme;

type Slot<T> = Arc<Mutex<Option<T>>>;

#[derive(Default)]
pub struct Access {
    /// The project whose access is open, if the window is.
    host: Option<String>,
    view: Slot<Result<AppAccessView, String>>,
    busy: Arc<Mutex<bool>>,
    /// The level just picked, shown chosen at once while it is saved
    /// (operator, 2026-11-17: the buttons were "painfully non-responsive"),
    /// and why the last change did not take, if it did not.
    pending: Slot<String>,
    failed: Slot<String>,
    /// What is typed in the people search.
    find: String,
    /// The search last sent, and what it found.
    asked: String,
    found: Slot<(String, Vec<Person>)>,
    /// The role a new share goes with: "use" or "edit".
    role: String,
}

/// A share's levels, lowest first, as they read them (people_sharing.sql;
/// operator, 2026-11-02: repo-style, "with their level of ownership given by
/// the user"). Each includes the ones before it.
pub const ROLES: [(&str, &str, &str); 4] = [
    ("use", "Open it", "they can open the app"),
    ("read", "See the code", "they can open it and read its files in their Cove, without changing them"),
    ("edit", "Work on it", "they can change its files in their Cove and run its agent - the same thread as yours"),
    ("manage", "Manage it", "they can also share it with others and set who may open it; only you can archive it"),
];

/// What a level lets someone do, as a sentence ends: "they can open it".
pub fn role_words(role: &str) -> &'static str {
    match role {
        "read" => "can see the code",
        "edit" => "can work on it",
        "manage" => "can manage it",
        _ => "can open it",
    }
}

/// The levels a person chooses between, as they read them (Settings › Access
/// says the same).
const LEVELS: [(&str, &str, &str); 3] = [
    ("public", "Everyone", "Anyone on the web, signed in or not."),
    ("login", "Anyone signed in", "Anyone with an ndexr account."),
    ("owner", "People you choose", "You, and the people and groups you share it with below."),
];

impl Access {
    pub fn open_for(&mut self, host: &str, backend: &Backend, ctx: &egui::Context) {
        self.host = Some(host.to_owned());
        self.find.clear();
        self.asked.clear();
        *self.found.lock().unwrap() = None;
        self.role = "use".into();
        self.ask(backend, ctx, String::new());
    }

    fn ask(&mut self, backend: &Backend, ctx: &egui::Context, level: String) {
        let Some(host) = self.host.clone() else { return };
        *self.busy.lock().unwrap() = true;
        if level.is_empty() {
            *self.view.lock().unwrap() = None;
        }
        let reading = level.is_empty();
        *self.failed.lock().unwrap() = None;
        *self.pending.lock().unwrap() = (!reading).then(|| level.clone());
        let (slot, busy, pending, failed, c) = (self.view.clone(), self.busy.clone(), self.pending.clone(), self.failed.clone(), ctx.clone());
        backend.query(Query::AppAccess { host, level }, ctx, move |r: Result<AppAccessView, String>| {
            match r {
                Ok(v) => *slot.lock().unwrap() = Some(Ok(v)),
                // A change that did not take: the level shown goes back to
                // what it was, and says why - the view itself stays.
                Err(e) if !reading => *failed.lock().unwrap() = Some(e),
                Err(e) => *slot.lock().unwrap() = Some(Err(e)),
            }
            *pending.lock().unwrap() = None;
            *busy.lock().unwrap() = false;
            c.request_repaint();
        });
    }

    /// Everyone on ndexr whose name holds `find`, through Messages' search.
    fn search(&mut self, backend: &Backend, ctx: &egui::Context) {
        let find = self.find.trim().to_owned();
        if find == self.asked {
            return;
        }
        self.asked = find.clone();
        if find.is_empty() {
            *self.found.lock().unwrap() = None;
            return;
        }
        let (slot, c) = (self.found.clone(), ctx.clone());
        backend.query(Query::Inbox { all: false, chat: String::new(), find: find.clone() }, ctx, move |r: Result<InboxView, String>| {
            let mut s = slot.lock().unwrap();
            // A slower answer to an older search must not replace a newer one.
            if s.as_ref().map_or(true, |(f, _)| *f != find) {
                *s = Some((find, r.map(|v| v.found).unwrap_or_default()));
            }
            c.request_repaint();
        });
    }

    pub fn ui(&mut self, ctx: &egui::Context, backend: &Backend, inbox: &mut Inbox) {
        let Some(host) = self.host.clone() else { return };
        let mut open = true;
        let view = self.view.lock().unwrap().clone();
        let busy = *self.busy.lock().unwrap();
        let people = inbox.view();
        let mut level_pick = None;
        let mut acts: Vec<InboxAct> = Vec::new();
        let mut typed = false;
        egui::Window::new(RichText::new(format!("Share {host}")).font(theme::display(15.0)))
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(540.0)
            .min_width(380.0)
            .show(ctx, |ui| {
                ui.add_space(4.0);
                section(ui, "Who can view it");
                let now = match &view {
                    Some(Ok(v)) => v.level.clone(),
                    Some(Err(e)) => {
                        ui.label(RichText::new(e).size(13.5).color(theme::warn()));
                        String::new()
                    }
                    None => {
                        ui.spinner();
                        return;
                    }
                };
                // The pick shows at once; the server catches up behind it.
                let now = self.pending.lock().unwrap().clone().unwrap_or(now);
                if now == "admin" {
                    ui.label(RichText::new("Admins only: an ndexr admin set this. Choose below to open it up.").size(13.5).color(theme::amber()));
                }
                for (level, word, meaning) in LEVELS {
                    if crate::settings_tab::option_row(ui, word, meaning, now == level, !busy).clicked() && level != now {
                        level_pick = Some(level.to_owned());
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    if busy {
                        ui.spinner();
                        ui.label(RichText::new("Saving…").size(13.0).color(theme::text()));
                    } else if let Some(e) = self.failed.lock().unwrap().clone() {
                        ui.label(RichText::new(format!("Not changed: {e}")).size(13.0).color(theme::warn()));
                    } else if let Some(Ok(v)) = &view {
                        if !v.said.is_empty() {
                            ui.label(RichText::new(format!("✓ {}", v.said)).size(13.0).color(theme::ok()));
                        }
                    }
                });
                if let Some(Ok(v)) = &view {
                    if v.port {
                        ui.label(RichText::new("It runs on its own port: start it (Up) for a change here to take effect.").size(13.0).color(theme::text()));
                    }
                }

                ui.add_space(14.0);
                section(ui, "People");
                let Some(p) = people else {
                    ui.spinner();
                    return;
                };
                if now == "public" || now == "login" {
                    let who = if now == "public" { "Everyone" } else { "Anyone signed in" };
                    ui.label(RichText::new(format!("{who} can open it already. Share to let someone work on it, or for when you keep it to people you choose.")).size(13.0).color(theme::text()));
                    ui.add_space(4.0);
                }

                // Add: a search over everyone on ndexr and my groups, and
                // what they may do; one click on a name shares it.
                let mine: Vec<_> = p.shares.iter().filter(|s| s.mine && s.domain == host && s.status != "declined").collect();
                let shared_to: Vec<String> = mine.iter().map(|s| s.to.to_lowercase()).collect();
                ui.horizontal(|ui| {
                    ui.label(RichText::new("They can").size(13.5).color(theme::text()));
                    for (r, word, tip) in ROLES {
                        if ui.selectable_label(self.role == r, RichText::new(word).size(13.5)).on_hover_text(tip).clicked() {
                            self.role = r.to_owned();
                        }
                    }
                });
                ui.add_space(4.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut self.find)
                        .hint_text("⚲ Search everyone on ndexr, or your groups, by name…")
                        .desired_width(f32::INFINITY)
                        .margin(egui::Margin::symmetric(8, 6)),
                );
                typed = r.changed();
                let find = self.find.trim().to_lowercase();
                if !find.is_empty() {
                    let found = self.found.lock().unwrap().clone();
                    let groups: Vec<_> = p.groups.iter().filter(|g| g.member && g.name.to_lowercase().contains(&find) && !shared_to.contains(&g.name.to_lowercase())).collect();
                    let persons: Vec<Person> = found
                        .as_ref()
                        .filter(|(f, _)| f.to_lowercase() == find)
                        .map(|(_, v)| v.iter().filter(|x| !shared_to.contains(&x.username.to_lowercase())).cloned().collect())
                        .unwrap_or_default();
                    egui::Frame::new().fill(theme::surface()).corner_radius(8.0).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        egui::ScrollArea::vertical().id_salt("access-found").max_height(220.0).show(ui, |ui| {
                            for g in &groups {
                                if pick_row(ui, &g.name, &format!("group · {} member{}", g.members.len(), if g.members.len() == 1 { "" } else { "s" }), false).clicked() {
                                    acts.push(InboxAct::Share { domain: host.clone(), to: String::new(), group: g.id, role: self.role.clone() });
                                    self.find.clear();
                                }
                            }
                            for x in &persons {
                                if pick_row(ui, &x.username, if x.live { "here now" } else { "ndexr account" }, x.live).clicked() {
                                    acts.push(InboxAct::Share { domain: host.clone(), to: x.username.clone(), group: 0, role: self.role.clone() });
                                    self.find.clear();
                                }
                            }
                            if groups.is_empty() && persons.is_empty() {
                                let words = if found.as_ref().is_some_and(|(f, _)| f.to_lowercase() == find) { "No one by that name." } else { "Searching…" };
                                ui.label(RichText::new(words).size(13.0).color(theme::text()));
                            }
                        });
                    });
                }
                if let Some(t) = inbox.trouble() {
                    ui.label(RichText::new(t).size(13.0).color(theme::warn()));
                }

                // Who has it: each a row, with what they may do and Remove.
                ui.add_space(10.0);
                if mine.is_empty() {
                    ui.label(RichText::new("Not shared with anyone yet.").size(13.5).color(theme::text()));
                } else {
                    ui.label(RichText::new(format!("Shared with {}", mine.len())).size(13.5).strong().color(theme::text_strong()));
                }
                for s in &mine {
                    egui::Frame::new().fill(theme::surface()).corner_radius(8.0).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&s.to).size(14.0).strong().color(theme::text_strong()));
                                let what = if s.group != 0 { "group" } else { "person" };
                                let can = role_words(&s.role);
                                let state = if s.status == "accepted" { can.to_owned() } else { format!("{can} once they accept") };
                                ui.label(RichText::new(format!("{what} · {state}")).size(12.5).color(theme::text()));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Remove").on_hover_text("they can no longer open it, or work on it, through this share").clicked() {
                                    acts.push(InboxAct::ShareRevoke { id: s.id });
                                }
                                // Their level, changed in place: a share is
                                // re-offered at the new one (messages.rs Share).
                                let now = ROLES.iter().find(|(r, ..)| *r == s.role).map_or("Open it", |(_, w, _)| *w);
                                egui::ComboBox::from_id_salt(("share-level", s.id)).selected_text(now).show_ui(ui, |ui| {
                                    for (r, word, tip) in ROLES {
                                        if ui.selectable_label(s.role == r, word).on_hover_text(tip).clicked() && s.role != r {
                                            let to = if s.group != 0 { String::new() } else { s.to.clone() };
                                            acts.push(InboxAct::Share { domain: host.clone(), to, group: s.group, role: r.into() });
                                        }
                                    }
                                });
                            });
                        });
                    });
                    ui.add_space(4.0);
                }
            });
        if typed || (!self.find.trim().is_empty() && self.find.trim() != self.asked) {
            self.search(backend, ctx);
        }
        for a in acts {
            inbox.send(backend, ctx, a);
        }
        if let Some(l) = level_pick {
            self.ask(backend, ctx, l);
        }
        if !open {
            self.host = None;
        }
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.label(RichText::new(title).font(theme::display(14.5)).color(theme::text_strong()));
    ui.add_space(4.0);
}

/// A search result: the name, a line under it, and the whole row is the
/// button that shares.
fn pick_row(ui: &mut egui::Ui, name: &str, under: &str, live: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 44.0), egui::Sense::click());
    let p = ui.painter();
    if resp.hovered() {
        p.rect_filled(rect, 6.0, theme::surface_hi());
    }
    let dot = egui::pos2(rect.left() + 14.0, rect.center().y);
    p.circle_filled(dot, 4.0, if live { theme::ok() } else { theme::moss() });
    p.text(egui::pos2(rect.left() + 28.0, rect.top() + 6.0), egui::Align2::LEFT_TOP, name, theme::display(14.0), theme::text_strong());
    p.text(egui::pos2(rect.left() + 28.0, rect.top() + 25.0), egui::Align2::LEFT_TOP, under, egui::FontId::proportional(12.0), theme::text());
    if resp.hovered() {
        p.text(egui::pos2(rect.right() - 12.0, rect.center().y), egui::Align2::RIGHT_CENTER, "Share", egui::FontId::proportional(13.5), theme::primary());
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}
