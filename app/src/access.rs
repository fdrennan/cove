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
//! module, and the vhost's gate too for an app on its own port. Below them,
//! who can find the person themselves (inbox.rs findable_switch).

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::api::{AppAccessView, InboxAct, Query};
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
    /// The person or group picked to share with: "@name" or "#id".
    pick: String,
}

/// The levels a person chooses between, as they read them.
const LEVELS: [(&str, &str, &str); 3] = [
    ("public", "Everyone", "anyone, signed in or not"),
    ("login", "Anyone signed in", "anyone with an ndexr account"),
    ("owner", "People and groups you choose", "you, and the people and groups you share it with below"),
];

impl Access {
    pub fn open_for(&mut self, host: &str, backend: &Backend, ctx: &egui::Context) {
        self.host = Some(host.to_owned());
        self.pick.clear();
        self.ask(backend, ctx, String::new());
    }

    fn ask(&mut self, backend: &Backend, ctx: &egui::Context, level: String) {
        let Some(host) = self.host.clone() else { return };
        *self.busy.lock().unwrap() = true;
        if level.is_empty() {
            *self.view.lock().unwrap() = None;
        }
        let (slot, busy, c) = (self.view.clone(), self.busy.clone(), ctx.clone());
        backend.query(Query::AppAccess { host, level }, ctx, move |r: Result<AppAccessView, String>| {
            *slot.lock().unwrap() = Some(r);
            *busy.lock().unwrap() = false;
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
        egui::Window::new(format!("Access · {host}")).open(&mut open).collapsible(false).default_width(520.0).show(ctx, |ui| {
            ui.label(RichText::new("Who can open this app").size(15.0).strong().color(theme::text_strong()));
            ui.add_space(4.0);
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
            if now == "admin" {
                ui.label(RichText::new("Admins only - an ndexr admin set this; choose below to open it up.").size(13.0).color(theme::text()));
            }
            let items: Vec<(String, String, bool, bool)> = LEVELS.iter().map(|(l, word, tip)| ((*word).to_owned(), (*tip).to_owned(), now == *l, !busy)).collect();
            if let Some(i) = crate::agent::segmented(ui, true, &items) {
                if LEVELS[i].0 != now {
                    level_pick = Some(LEVELS[i].0.to_owned());
                }
            }
            if let Some((_, _, words)) = LEVELS.iter().find(|(l, _, _)| *l == now) {
                ui.label(RichText::new(format!("Now: {words}.")).size(13.0).color(theme::text()));
            }
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(RichText::new("changing it…").size(13.0).color(theme::text()));
                });
            }
            if let Some(Ok(v)) = &view {
                if !v.said.is_empty() {
                    ui.label(RichText::new(format!("✓ {}", v.said)).size(13.0).color(theme::ok()));
                }
                if v.port {
                    ui.label(RichText::new("It runs on its own port, so the gate is in front of it: the app must be Up for a change.").size(12.5).color(theme::text()));
                }
            }
            ui.add_space(8.0);
            ui.separator();
            ui.label(RichText::new("People and groups").size(15.0).strong().color(theme::text_strong()));
            let Some(p) = people else {
                ui.spinner();
                return;
            };
            if now != "owner" && !now.is_empty() {
                ui.label(RichText::new("Shares count when it is kept to the people and groups you choose; anyone you add here can open it then.").size(12.5).color(theme::text()));
            }
            let mine: Vec<_> = p.shares.iter().filter(|s| s.mine && s.domain == host && s.status != "declined").collect();
            if mine.is_empty() {
                ui.label(RichText::new("Not shared with anyone yet.").size(13.0).color(theme::text()));
            }
            for s in &mine {
                ui.horizontal(|ui| {
                    let what = if s.group != 0 { "group" } else { "person" };
                    ui.label(RichText::new(&s.to).size(13.5).strong().color(theme::text_strong()));
                    ui.label(RichText::new(format!("{what} · {}", if s.status == "accepted" { "can open it" } else { "offered - waiting for them to accept" })).size(13.0).color(theme::text()));
                    if ui.small_button("Take back").on_hover_text("they can no longer open it through this share").clicked() {
                        acts.push(InboxAct::ShareRevoke { id: s.id });
                    }
                });
            }
            // Who it can go to: people I am connected with, groups I am in -
            // the same as Messages' share, so nobody is reached who did not
            // agree to hear from me.
            let shared_to: Vec<&str> = mine.iter().map(|s| s.to.as_str()).collect();
            let mut choices: Vec<(String, String)> = p
                .connections
                .iter()
                .filter(|c| c.status == "accepted" && !shared_to.contains(&c.username.as_str()))
                .map(|c| (format!("@{}", c.username), c.username.clone()))
                .collect();
            choices.extend(p.groups.iter().filter(|g| g.member && !shared_to.contains(&g.name.as_str())).map(|g| (format!("#{}", g.id), format!("{} (group)", g.name))));
            ui.add_space(4.0);
            if choices.is_empty() {
                ui.label(RichText::new("Connect with people or make a group in Messages, then share with them here.").size(12.5).color(theme::text()));
            } else {
                ui.horizontal(|ui| {
                    let shown = choices.iter().find(|(k, _)| *k == self.pick).map_or("choose someone…", |(_, n)| n.as_str());
                    egui::ComboBox::from_id_salt("access-share-to").selected_text(shown).width(240.0).show_ui(ui, |ui| {
                        for (k, n) in &choices {
                            ui.selectable_value(&mut self.pick, k.clone(), n);
                        }
                    });
                    if ui.add_enabled(!self.pick.is_empty(), egui::Button::new("Share")).on_hover_text("they are offered it; once they accept, they can open it").clicked() {
                        let act = match self.pick.strip_prefix('#').and_then(|g| g.parse::<i64>().ok()) {
                            Some(g) => InboxAct::Share { domain: host.clone(), to: String::new(), group: g },
                            None => InboxAct::Share { domain: host.clone(), to: self.pick.trim_start_matches('@').to_owned(), group: 0 },
                        };
                        acts.push(act);
                        self.pick.clear();
                    }
                });
            }
            if let Some(t) = inbox.trouble() {
                ui.label(RichText::new(t).size(13.0).color(theme::warn()));
            }
            ui.add_space(8.0);
            ui.separator();
            if let Some(level) = crate::inbox::findable_switch(ui, &p) {
                acts.push(InboxAct::Findable { level });
            }
        });
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
