//! Messages: requests and their answers, and people to talk to - one window,
//! opened from the toolbar (operator, 2026-10-18; the server's half and the
//! why are in messages.rs).
//!
//!   Requests   everyone's own, each a thread: what they asked, the team's
//!              answers, the outcome. Its sender can mark it resolved or open
//!              it again. An admin sees everyone's, who sent it, answers and
//!              moves it (Working · Resolved · Won't fix · Reopen). Hand to
//!              Exec gives the request to the admin's executive agent to fix;
//!              its answer ends with a reply to them, which lands in the box
//!              to read over and send with Reply & resolve.
//!   Chat       person to person, laid out like a messenger (operator,
//!              2026-10-18: "almost feels something like using WhatsApp or
//!              Discord"): the conversations down the left - who I talk with,
//!              newest first, and anyone public by search - and the thread on
//!              the right. A message is text, a file (saved into the domain
//!              open here with one click) or a site (opened on its own
//!              address). The private/public switch sits over the list.
//!              A conversation is one timeline (#35): with the team, it holds
//!              the person's requests too - each where it was sent, every
//!              answer and status at its time - and ↩ on one writes on it.
//!              Everyone but the team has "ndexr team" pinned at the top,
//!              where writing files a new request. ✓ sent, ✓✓ read.
//!
//! Polled every 4 s while open and every 30 s while closed: that keeps the
//! toolbar's badge, and the person's presence, current.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, RichText};

use crate::api::{InboxAct, InboxView, Query, RequestView};
use crate::backend::Backend;
use crate::theme;

type Slot<T> = Arc<Mutex<Option<T>>>;

#[derive(Clone, Copy, PartialEq, Default)]
enum Tab {
    #[default]
    Requests,
    Chat,
}

/// Chat's conversation with the ndexr team: a person's own requests, and new
/// ones written there. Not a username (those have no '@').
const TEAM: &str = "@team";

/// One thing in a conversation, in time order.
enum Item<'a> {
    Dm(&'a crate::api::DmView),
    /// A request, where it was sent.
    Opened(&'a RequestView),
    /// Something said on a request, or its status moving.
    Said(&'a RequestView, &'a crate::api::ThreadMsg),
    /// A domain shared here.
    Share(&'a crate::api::ShareView),
}

/// Which requests the list shows.
#[derive(Clone, Copy, PartialEq, Default)]
enum Show {
    #[default]
    Active,
    Done,
    Every,
}

#[derive(Default)]
pub struct Inbox {
    pub open: bool,
    tab: Tab,
    /// On a phone the list and what is picked in it are one pane at a time:
    /// whether the picked request or conversation is the one showing.
    phone_detail: bool,
    view: Slot<Result<InboxView, String>>,
    asked: f64,
    in_flight: Arc<Mutex<bool>>,
    /// An admin looking at only their own requests. Off by default: an admin
    /// opened on "mine" never saw anyone else's (operator, 2026-10-18: "I
    /// still can't see if other users have requested anything of me").
    mine_only: bool,
    show: Show,
    picked: Option<i64>,
    chat_with: String,
    /// The request Chat's box writes on (↩ on one), instead of a message.
    reply_to: Option<i64>,
    /// The name typed for a new group.
    new_group: String,
    /// Chat's search box.
    find: String,
    /// The domain open in the IDE (host, folder): where a file sent in Chat
    /// is saved. Set by app.rs each frame, with every domain I can share.
    pub here: Option<(String, String)>,
    pub domains: Vec<String>,
    draft: String,
    chat_draft: String,
    /// What came of the last thing done: Ok(a word to show, or ""), or why not.
    sent: Slot<Result<String, String>>,
    note: Option<String>,
    /// Requests whose thread this session has told the server it read.
    seen: std::collections::HashSet<(i64, usize)>,
    /// A request to hand to Exec: app.rs takes it, asks Exec, and gives the
    /// turn's key back through `handed`.
    pub to_exec: Option<(i64, String)>,
    /// Requests handed to Exec this session: the turn, how far it is.
    exec: std::collections::HashMap<i64, ExecRun>,
}

/// One request's Exec turn.
struct ExecRun {
    key: String,
    reply: String,
    done: bool,
    /// Its reply has been put in the box once already.
    drafted: bool,
    /// Its own call on the request (`verdict`): what sending the reply sets.
    verdict: Option<&'static str>,
}

/// What Exec is asked: the request whole, and to end with what to tell them.
fn exec_prompt(r: &RequestView) -> String {
    let mut p = format!(
        "A request came in through Cove's Messages: #{} from {}{} about {}.\n\n{}\n",
        r.id,
        r.username,
        if r.email.is_empty() { String::new() } else { format!(" ({})", r.email) },
        r.domain,
        r.what.trim()
    );
    if !r.trying.trim().is_empty() {
        p.push_str(&format!("\nThey were trying to: {}\n", r.trying.trim()));
    }
    let said: Vec<String> = r.messages.iter().filter(|m| m.kind != "status").map(|m| format!("{}: {}", m.who, m.body.trim())).collect();
    if !said.is_empty() {
        p.push_str(&format!("\nThe thread so far:\n{}\n", said.join("\n")));
    }
    p.push_str(&format!(
        "\nFix it if you can, in whichever domain it belongs to. Then end your answer with a line `{STATUS_MARK} resolved` (sorted - nothing left for them or for us), `{STATUS_MARK} working` (still in progress, or waiting on a build or a release) or `{STATUS_MARK} wontfix`, and then a line `{REPLY_MARK}` and, after it, a short plain message to them: what was wrong, what you changed, and whether it is sorted. I read it over and send it from Messages, and sending it sets the request to your status - do not message them yourself."
    ));
    p
}

const REPLY_MARK: &str = "REPLY TO THEM:";
const STATUS_MARK: &str = "STATUS:";

/// Exec's own call on the request: the last `STATUS:` line before its reply,
/// as a status (operator, 2026-10-18: "a better process for making sure that
/// they get sent to resolved when they are"). None when it gave none.
fn verdict(answer: &str) -> Option<&'static str> {
    let head = answer.rsplit_once(REPLY_MARK).map_or(answer, |(h, _)| h);
    let line = head.lines().rev().map(|l| l.trim().trim_matches('*').trim().trim_matches('`')).find(|l| l.to_ascii_uppercase().starts_with(STATUS_MARK))?;
    let word = line[STATUS_MARK.len()..].trim().to_ascii_lowercase();
    if word.starts_with("resolved") || word.starts_with("sorted") || word.starts_with("done") || word.starts_with("fixed") {
        Some("resolved")
    } else if word.starts_with("wontfix") || word.starts_with("won't") || word.starts_with("wont") {
        Some("wontfix")
    } else if word.starts_with("working") || word.starts_with("in progress") || word.starts_with("waiting") {
        Some("working")
    } else {
        None
    }
}

/// The message to them at the end of Exec's answer, if it wrote one.
fn reply_part(answer: &str) -> Option<String> {
    let (_, after) = answer.rsplit_once(REPLY_MARK)?;
    let t = after.trim().trim_matches('*').trim();
    (!t.is_empty()).then(|| t.to_owned())
}

fn status_color(s: &str) -> Color32 {
    match s {
        "open" => theme::warn(),
        "working" => theme::primary(),
        "resolved" => theme::ok(),
        _ => theme::muted(),
    }
}

fn status_word(s: &str) -> &str {
    match s {
        "wontfix" => "won't fix",
        s => s,
    }
}

impl Inbox {
    /// Open on a tab: from the toolbar, or after a request is sent.
    pub fn show_requests(&mut self) {
        self.open = true;
        self.tab = Tab::Requests;
        self.asked = f64::NEG_INFINITY;
    }

    /// Exec's turn for request `id` started, as `key`.
    pub fn handed(&mut self, id: i64, key: String) {
        self.exec.insert(id, ExecRun { key, reply: String::new(), done: false, drafted: false, verdict: None });
    }

    /// The Exec turns still to follow: (request, turn key).
    pub fn exec_keys(&self) -> Vec<(i64, String)> {
        self.exec.iter().filter(|(_, r)| !r.done).map(|(id, r)| (*id, r.key.clone())).collect()
    }

    /// How far request `id`'s Exec turn is.
    pub fn exec_update(&mut self, id: i64, reply: String, done: bool) {
        if let Some(r) = self.exec.get_mut(&id) {
            r.reply = reply;
            r.done = done;
        }
    }

    /// The badge's count: new answers, and new messages from people.
    pub fn unread(&self) -> i64 {
        match self.view.lock().unwrap().as_ref() {
            Some(Ok(v)) => v.unread(),
            _ => 0,
        }
    }

    fn ask(&mut self, backend: &Backend, ctx: &egui::Context, force: bool) {
        let now = ctx.input(|i| i.time);
        let every = if self.open { 4.0 } else { 30.0 };
        if *self.in_flight.lock().unwrap() || (!force && now - self.asked < every) {
            return;
        }
        self.asked = now;
        *self.in_flight.lock().unwrap() = true;
        let (slot, busy, c) = (self.view.clone(), self.in_flight.clone(), ctx.clone());
        let (chat, find) = if self.tab == Tab::Chat { (if self.chat_with == TEAM { String::new() } else { self.chat_with.clone() }, self.find.trim().to_owned()) } else { Default::default() };
        backend.query(Query::Inbox { all: !self.mine_only, chat, find }, ctx, move |r: Result<InboxView, String>| {
            *slot.lock().unwrap() = Some(r);
            *busy.lock().unwrap() = false;
            c.request_repaint();
        });
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(every));
    }

    /// The last answer, for windows that work on the same people, groups and
    /// shares (access.rs).
    pub fn view(&self) -> Option<InboxView> {
        self.view.lock().unwrap().as_ref().and_then(|r| r.as_ref().ok()).cloned()
    }

    /// Do something from another window; the view is asked again after.
    pub fn send(&mut self, backend: &Backend, ctx: &egui::Context, act: InboxAct) {
        self.act(backend, ctx, act);
    }

    /// Why the last thing done did not happen, if it did not.
    pub fn trouble(&self) -> Option<&str> {
        self.note.as_deref()
    }

    fn act(&mut self, backend: &Backend, ctx: &egui::Context, act: InboxAct) {
        let (slot, c) = (self.sent.clone(), ctx.clone());
        backend.query(Query::InboxAct { act }, ctx, move |r: Result<serde_json::Value, String>| {
            *slot.lock().unwrap() = Some(r.map(|_| String::new()));
            c.request_repaint();
        });
    }

    /// Every frame: keep it current, and draw the window when open.
    pub fn ui(&mut self, ctx: &egui::Context, backend: &Backend) {
        self.ask(backend, ctx, false);
        if let Some(r) = self.sent.lock().unwrap().take() {
            self.note = match r {
                Ok(m) if m.is_empty() => None,
                Ok(m) => Some(m),
                Err(e) => Some(e),
            };
            self.asked = f64::NEG_INFINITY;
        }
        if !self.open {
            return;
        }
        let view = self.view.lock().unwrap().clone();
        let mut open = true;
        let screen = ctx.content_rect();
        // A phone: the screen, under the top bar, and not a window to drag -
        // capped at 600 it ended a third of the way up and an open request's
        // reply box drew past its edge (operator, 2026-11-01).
        let phone = screen.width() < 600.0;
        let w = (screen.width() - if phone { 12.0 } else { 40.0 }).min(860.0);
        let h = if phone { screen.height() - 76.0 } else { (screen.height() - 90.0).min(600.0) };
        // In front of the windows, always: Builds' charts paint on the
        // foreground layer and had covered it (operator, 2026-10-18).
        let id = egui::Id::new("messages-window");
        ctx.move_to_top(egui::LayerId::new(egui::Order::Foreground, id));
        let mut window = egui::Window::new("Messages").id(id).order(egui::Order::Foreground).open(&mut open).collapsible(false);
        window = if phone { window.resizable(false).fixed_size([w, h]).anchor(egui::Align2::CENTER_TOP, [0.0, 40.0]) } else { window.resizable(true).default_size([w, h]) };
        window.show(ctx, |ui| {
            let Some(view) = view else {
                ui.spinner();
                return;
            };
            let v = match view {
                Ok(v) => v,
                Err(e) => {
                    ui.label(RichText::new(e).color(theme::alt()));
                    return;
                }
            };
            ui.horizontal(|ui| {
                let unread_people: i64 = v.people.iter().map(|p| p.unread).sum();
                let tabs = [
                    (Tab::Requests, if v.admin && v.open > 0 { format!("Requests · {} open", v.open) } else { "Requests".to_owned() }),
                    (Tab::Chat, if unread_people > 0 { format!("Chat · {unread_people} new") } else { "Chat".to_owned() }),
                ];
                for (t, label) in tabs {
                    if ui.selectable_label(self.tab == t, label).clicked() && self.tab != t {
                        self.tab = t;
                        self.asked = f64::NEG_INFINITY;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("signed in as {}{}", v.me, if v.admin { " · admin" } else { "" })).size(11.0).color(theme::muted()));
                });
            });
            if let Some(n) = &self.note {
                ui.label(RichText::new(n).size(11.5).color(theme::alt()));
            }
            ui.separator();
            match self.tab {
                Tab::Requests => self.requests(ui, ctx, backend, &v),
                Tab::Chat => self.chat(ui, ctx, backend, &v),
            }
        });
        self.open &= open;
    }

    fn requests(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, v: &InboxView) {
        // A phone (operator, 2026-11-01, screenshots: the hint drawn over
        // "done" and "all", the list squeezed into half the width): one pane.
        let narrow = ui.available_width() < 600.0;
        if narrow && self.phone_detail {
            if let Some(r) = self.picked.and_then(|id| v.requests.iter().find(|r| r.id == id)).cloned() {
                if back(ui, "← requests") {
                    self.phone_detail = false;
                }
                self.request(ui, ctx, backend, v, &r);
                return;
            }
            self.phone_detail = false;
        }
        ui.horizontal_wrapped(|ui| {
            if v.admin {
                let before = self.mine_only;
                ui.selectable_value(&mut self.mine_only, false, "everyone's");
                ui.selectable_value(&mut self.mine_only, true, "mine");
                if before != self.mine_only {
                    self.asked = f64::NEG_INFINITY;
                    self.picked = None;
                }
                ui.separator();
            }
            ui.selectable_value(&mut self.show, Show::Active, "open");
            ui.selectable_value(&mut self.show, Show::Done, "done");
            ui.selectable_value(&mut self.show, Show::Every, "all");
            if !narrow {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("new requests: ? Help in the toolbar").size(11.0).color(theme::hush()));
                });
            }
        });
        let list: Vec<&RequestView> = v
            .requests
            .iter()
            .filter(|r| match self.show {
                Show::Active => matches!(r.status.as_str(), "open" | "working"),
                Show::Done => matches!(r.status.as_str(), "resolved" | "wontfix"),
                Show::Every => true,
            })
            .collect();
        if self.picked.is_none() && !narrow {
            self.picked = list.first().map(|r| r.id);
        }
        let h = ui.available_height();
        if narrow {
            if self.request_list(ui, v, &list, h) {
                self.phone_detail = true;
            }
            return;
        }
        ui.columns(2, |c| {
            self.request_list(&mut c[0], v, &list, h);
            let picked = self.picked.and_then(|id| v.requests.iter().find(|r| r.id == id)).cloned();
            match picked {
                Some(r) => self.request(&mut c[1], ctx, backend, v, &r),
                None => {
                    c[1].label(RichText::new("Pick a request.").color(theme::muted()));
                }
            }
        });
    }

    /// The requests the filters leave, newest first; whether one was picked.
    fn request_list(&mut self, ui: &mut egui::Ui, v: &InboxView, list: &[&RequestView], h: f32) -> bool {
        let mut chose = false;
            egui::ScrollArea::vertical().id_salt("inbox-reqs").max_height(h).auto_shrink([false, false]).show(ui, |ui| {
                // "Nothing here" with every request resolved read as "no one
                // has asked you anything" (operator, 2026-10-18, request #32):
                // say what the other filters hold, and offer them.
                if list.is_empty() {
                    let done = v.requests.iter().filter(|r| matches!(r.status.as_str(), "resolved" | "wontfix")).count();
                    let others = v.requests.iter().filter(|r| !r.mine).count();
                    let words = match (self.show, v.requests.len()) {
                        (_, 0) if self.mine_only || !v.admin => "You have not sent any requests yet.".to_owned(),
                        (_, 0) => "No one has sent a request yet.".to_owned(),
                        (Show::Active, _) => format!(
                            "Nothing open - all {} {} answered{}.",
                            done,
                            if done == 1 { "request is" } else { "requests are" },
                            if v.admin && !self.mine_only { format!(" ({others} from other people)") } else { String::new() }
                        ),
                        _ => "Nothing here.".to_owned(),
                    };
                    ui.label(RichText::new(words).color(theme::muted()));
                    if !v.requests.is_empty() && self.show != Show::Every && ui.button("Show all requests").clicked() {
                        self.show = Show::Every;
                        self.picked = None;
                    }
                }
                for r in list {
                    let on = self.picked == Some(r.id);
                    let resp = egui::Frame::new()
                        .fill(if on { theme::surface_hi() } else { Color32::TRANSPARENT })
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                if r.unread {
                                    ui.label(RichText::new("●").color(theme::primary()));
                                }
                                ui.label(RichText::new(format!("#{}", r.id)).monospace().size(11.0).color(theme::muted()));
                                ui.label(RichText::new(status_word(&r.status)).size(11.0).strong().color(status_color(&r.status)));
                                if r.urgency == "blocked" {
                                    ui.label(RichText::new("blocked").size(11.0).color(theme::alt()));
                                }
                                if !r.mine {
                                    ui.label(RichText::new(&r.username).size(11.0).color(theme::text()));
                                }
                            });
                            let first = r.what.lines().next().unwrap_or("");
                            ui.add(egui::Label::new(RichText::new(first).size(12.5).color(theme::text_strong())).truncate());
                            ui.label(RichText::new(format!("{} · {}", r.domain, r.created)).size(10.5).color(theme::hush()));
                        })
                        .response
                        .interact(egui::Sense::click());
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        chose = true;
                        if self.picked != Some(r.id) {
                            self.picked = Some(r.id);
                            self.draft.clear();
                        }
                    }
                }
            });
        chose
    }

    fn request(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, v: &InboxView, r: &RequestView) {
        // Read: the sender's unread answers stop being new.
        if r.mine && r.unread && self.seen.insert((r.id, r.messages.len())) {
            self.act(backend, ctx, InboxAct::Seen { id: r.id });
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("#{}", r.id)).monospace().color(theme::muted()));
            ui.label(RichText::new(status_word(&r.status)).strong().color(status_color(&r.status)));
            ui.label(RichText::new(&r.domain).size(11.5).color(theme::text()));
        });
        if !r.mine {
            let who = if r.email.is_empty() { r.username.clone() } else { format!("{} · {}", r.username, r.email) };
            ui.label(RichText::new(format!("from {who}")).size(11.5).color(theme::text()));
        }
        // What can be done with it, above the thread so it is never scrolled
        // away. For an admin, one obvious first move: hand it to Exec.
        let mut to: Option<&str> = None;
        if v.admin {
            let run = self.exec.get(&r.id).map(|x| (x.done, reply_part(&x.reply)));
            ui.horizontal_wrapped(|ui| {
                match &run {
                    None => {
                        if ui.add(theme::primary_button("➤ Hand to Exec")).on_hover_text("your executive agent fixes it, then drafts the reply to them here").clicked() {
                            self.to_exec = Some((r.id, exec_prompt(r)));
                            // In hand now: the list shows it, before Exec answers.
                            if r.status == "open" {
                                to = Some("working");
                            }
                        }
                    }
                    Some((false, _)) => {
                        ui.add(egui::Spinner::new().size(13.0).color(theme::primary()));
                        ui.label(RichText::new("Exec is on it - follow it in Agents · Exec").size(12.0).color(theme::text()));
                    }
                    Some((true, Some(_))) => {
                        let call = match self.exec.get(&r.id).and_then(|x| verdict(&x.reply)) {
                            Some("resolved") => " - it says sorted",
                            Some("working") => " - it says still in progress",
                            Some("wontfix") => " - it says won't fix",
                            _ => "",
                        };
                        ui.label(RichText::new(format!("✓ Exec finished{call} - its reply is in the box below")).size(12.0).color(theme::ok()));
                    }
                    Some((true, None)) => {
                        ui.label(RichText::new("Exec finished without a reply for them - see Agents · Exec").size(12.0).color(theme::warn()));
                    }
                }
                ui.separator();
                for (s, label) in [("working", "Working"), ("resolved", "Resolved"), ("wontfix", "Won't fix"), ("open", "Reopen")] {
                    if r.status != s && ui.small_button(label).clicked() {
                        to = Some(s);
                    }
                }
            });
            // Exec's reply into the box, once, to read over before sending.
            if let Some(x) = self.exec.get_mut(&r.id) {
                if x.done && !x.drafted {
                    x.drafted = true;
                    if let Some(reply) = reply_part(&x.reply) {
                        self.draft = reply;
                    }
                    x.verdict = verdict(&x.reply);
                }
            }
        } else if r.mine {
            ui.horizontal_wrapped(|ui| {
                if matches!(r.status.as_str(), "open" | "working") {
                    if ui.small_button("✓ Mark resolved").on_hover_text("it is sorted - close it").clicked() {
                        to = Some("resolved");
                    }
                } else if ui.small_button("Open it again").clicked() {
                    to = Some("open");
                }
            });
        }
        if let Some(s) = to {
            // Closing it with an answer still in the box sends the answer too:
            // a request marked resolved with nothing said to its sender (#33,
            // 2026-10-18) leaves them guessing.
            if v.admin && matches!(s, "resolved" | "wontfix") && !self.draft.trim().is_empty() {
                let body = std::mem::take(&mut self.draft);
                self.act(backend, ctx, InboxAct::Post { id: r.id, body });
            }
            self.act(backend, ctx, InboxAct::Status { id: r.id, status: s.to_owned() });
        }
        ui.separator();
        let composer_h = 92.0;
        // The thread takes what the actions above left: they wrap to more
        // rows on a phone than a fixed allowance for them assumed.
        egui::ScrollArea::vertical().id_salt(("inbox-thread", r.id)).max_height((ui.available_height() - composer_h - 12.0).max(80.0)).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            bubble(ui, if r.mine { "you" } else { &r.username }, &r.created, &r.what, r.mine, false);
            if !r.trying.trim().is_empty() {
                ui.label(RichText::new(format!("trying to: {}", r.trying)).size(11.5).color(theme::muted()));
            }
            if r.n_uploads > 0 {
                ui.label(RichText::new(format!("{} file(s) sent with it", r.n_uploads)).size(11.0).color(theme::hush()));
            }
            for m in &r.messages {
                if m.kind == "status" {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(format!("{} marked it {} · {}", if m.mine { "you" } else { &m.who }, status_word(&m.body), m.at)).size(11.0).color(status_color(&m.body)));
                    });
                } else {
                    let who = if m.mine && m.team { "you · ndexr team".to_owned() } else if m.mine { "you".to_owned() } else if m.team { format!("{} · ndexr team", m.who) } else { m.who.clone() };
                    bubble(ui, &who, &m.at, &m.body, m.mine, m.team);
                }
            }
        });
        if r.mine || v.admin {
            ui.add(egui::TextEdit::multiline(&mut self.draft).hint_text(if v.admin { "answer…" } else { "add to your request…" }).desired_rows(3).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                // An admin's first button resolves with the reply - on their own
                // request too, so asking themselves shows what a sender sees.
                // The first button follows Exec's call when it made one.
                let call = self.exec.get(&r.id).and_then(|x| x.verdict).unwrap_or("resolved");
                let label = match call {
                    "working" => "Reply & keep working",
                    "wontfix" => "Reply & close (won't fix)",
                    _ => "Reply & resolve",
                };
                if v.admin && ui.add_enabled(!self.draft.trim().is_empty(), theme::primary_button(label)).clicked() {
                    let body = std::mem::take(&mut self.draft);
                    self.act(backend, ctx, InboxAct::Post { id: r.id, body });
                    if r.status != call {
                        self.act(backend, ctx, InboxAct::Status { id: r.id, status: call.into() });
                    }
                }
                let send = ui.add_enabled(!self.draft.trim().is_empty(), if v.admin { egui::Button::new("Reply") } else { theme::primary_button("Send") }).clicked();
                if send {
                    let body = std::mem::take(&mut self.draft);
                    self.act(backend, ctx, InboxAct::Post { id: r.id, body });
                }
            });
        }
    }

    /// Chat: conversations on the left, the thread on the right.
    fn chat(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, v: &InboxView) {
        let w = ui.available_width();
        // A phone: the conversations, or the one open, the whole width.
        if w < 600.0 {
            if self.phone_detail && !self.chat_with.trim().is_empty() {
                if back(ui, "← conversations") {
                    self.phone_detail = false;
                }
                self.thread(ui, ctx, backend, v);
            } else {
                self.phone_detail = self.conversations(ui, ctx, backend, v);
            }
            return;
        }
        egui::Panel::left("chat-list")
            .resizable(true)
            .show_separator_line(true)
            .default_size((w * 0.32).clamp(180.0, 280.0))
            .size_range(150.0..=(w - 260.0).max(151.0))
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.conversations(ui, ctx, backend, v));
        egui::CentralPanel::no_frame().show(ui, |ui| self.thread(ui, ctx, backend, v));
    }

    /// The list: what waits on me (asks to connect, group places, domains
    /// offered - one click each), the team, my groups, then people.
    /// Whether one was opened.
    fn conversations(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, v: &InboxView) -> bool {
        // Everyone on ndexr is findable by everyone (operator, 2026-11-02), so
        // there is no "who can find you" switch: the search is the whole list.
        ui.add_space(4.0);
        let r = ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("⚲ find anyone on ndexr by name…").desired_width(f32::INFINITY));
        if r.changed() {
            self.asked = f64::NEG_INFINITY;
        }
        ui.separator();
        let find = self.find.trim().to_owned();
        let searching = !find.is_empty();
        let connected = |name: &str| v.connections.iter().find(|c| c.username.eq_ignore_ascii_case(name));
        let mut acts: Vec<InboxAct> = Vec::new();
        let mut open: Option<String> = None;
        egui::ScrollArea::vertical().id_salt("chat-people").auto_shrink([false, false]).show(ui, |ui| {
            if searching {
                for p in &v.found {
                    if conversation_row(ui, false, p.live, &p.username, if p.live { "here now" } else { "away" }, 0, "").clicked() {
                        open = Some(p.username.clone());
                    }
                    connect_button(ui, connected(&p.username), &p.username, &mut acts);
                }
                if v.found.is_empty() {
                    ui.label(RichText::new("No one on ndexr by that name.").size(12.5).color(theme::text()));
                }
                return;
            }
            // Waiting on me, each a click.
            let asks: Vec<_> = v.connections.iter().filter(|c| c.incoming).collect();
            let invites: Vec<_> = v.groups.iter().filter(|g| !g.member).collect();
            let offers: Vec<_> = v.shares.iter().filter(|s| !s.mine && s.status == "offered").collect();
            if !asks.is_empty() || !invites.is_empty() || !offers.is_empty() {
                ui.label(RichText::new("waiting on you").size(10.5).strong().color(theme::warn()));
                for c in asks {
                    if let Some(accept) = pending_row(ui, &format!("☍ {} asks to connect", c.username), "Accept") {
                        acts.push(InboxAct::Connection { with: c.username.clone(), accept });
                    }
                }
                for g in invites {
                    if let Some(accept) = pending_row(ui, &format!("⊞ join {}", g.name), "Join") {
                        acts.push(InboxAct::GroupJoin { id: g.id, accept });
                    }
                }
                for s in offers {
                    if let Some(accept) = pending_row(ui, &format!("⛓ {} shares {} with you", s.owner, s.domain), "Accept") {
                        acts.push(InboxAct::ShareDecide { id: s.id, accept });
                    }
                }
                ui.separator();
            }
            // Everyone but the team has the team at the top: their requests
            // and the answers, as one conversation (#35).
            if !v.admin {
                let mine: Vec<&RequestView> = v.requests.iter().filter(|r| r.mine).collect();
                let open_n = mine.iter().filter(|r| matches!(r.status.as_str(), "open" | "working")).count();
                let unread = mine.iter().filter(|r| r.unread).count();
                let sub = match (open_n, mine.len()) {
                    (_, 0) => "ask for anything here".to_owned(),
                    (0, n) => format!("{n} request{} - all answered", if n == 1 { "" } else { "s" }),
                    (o, _) => format!("{o} open request{}", if o == 1 { "" } else { "s" }),
                };
                if conversation_row(ui, self.chat_with == TEAM, true, "ndexr team", &sub, unread as i64, "").clicked() {
                    open = Some(TEAM.to_owned());
                }
            }
            for g in v.groups.iter().filter(|g| g.member) {
                let key = format!("#{}", g.id);
                let last = if g.last.is_empty() { format!("{} member{}", g.members.len(), if g.members.len() == 1 { "" } else { "s" }) } else { g.last.lines().next().unwrap_or("").to_owned() };
                if conversation_row(ui, self.chat_with == key, true, &format!("⊞ {}", g.name), &last, 0, &g.last_at).clicked() {
                    open = Some(key);
                }
            }
            // People: those I talk with, and everyone I am connected with.
            let mut people: Vec<crate::api::Person> = v.people.clone();
            for c in v.connections.iter().filter(|c| c.status == "accepted") {
                if !people.iter().any(|p| p.username.eq_ignore_ascii_case(&c.username)) {
                    people.push(crate::api::Person { username: c.username.clone(), live: c.live, ..Default::default() });
                }
            }
            if people.is_empty() && v.groups.is_empty() {
                ui.label(RichText::new("No conversations yet - find someone above.").size(11.5).color(theme::muted()));
            }
            for p in &people {
                let on = self.chat_with.eq_ignore_ascii_case(&p.username);
                // An admin's badge for someone counts their unanswered requests too.
                let waiting = if v.admin { v.requests.iter().filter(|r| !r.mine && r.unread && r.username.eq_ignore_ascii_case(&p.username)).count() as i64 } else { 0 };
                let last = if p.last.is_empty() { if p.live { "here now" } else { "away" } } else { p.last.lines().next().unwrap_or("") };
                let tag = match (p.admin, connected(&p.username).is_some_and(|c| c.status == "accepted")) {
                    (true, _) => " · ndexr team",
                    (false, true) => " · connected",
                    _ => "",
                };
                if conversation_row(ui, on, p.live, &format!("{}{tag}", p.username), last, p.unread + waiting, &p.last_at).clicked() {
                    open = Some(p.username.clone());
                }
            }
            // Shared with me: one click to open.
            let have: Vec<_> = v.shares.iter().filter(|s| !s.mine && s.status == "accepted").collect();
            if !have.is_empty() {
                ui.separator();
                ui.label(RichText::new("shared with you").size(10.5).strong().color(theme::muted()));
                for s in have {
                    ui.horizontal(|ui| {
                        if ui.small_button("Open ↗").on_hover_text(format!("https://{}/", s.domain)).clicked() {
                            ctx.open_url(egui::OpenUrl::new_tab(format!("https://{}/", s.domain)));
                        }
                        let from = if s.group > 0 { format!("{} · {}", s.owner, s.to) } else { s.owner.clone() };
                        ui.add(egui::Label::new(RichText::new(format!("{} · from {from}", s.domain)).size(11.0).color(theme::text())).truncate());
                    });
                }
            }
            ui.separator();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.new_group).hint_text("new group…").desired_width(ui.available_width() - 34.0));
                if ui.add_enabled(!self.new_group.trim().is_empty(), egui::Button::new("✚")).on_hover_text("make a group - then invite people you are connected with").clicked() {
                    acts.push(InboxAct::GroupNew { name: std::mem::take(&mut self.new_group) });
                }
            });
        });
        let opened = open.is_some();
        if let Some(c) = open {
            self.chat_with = c;
            self.reply_to = None;
            self.find.clear();
            self.asked = f64::NEG_INFINITY;
        }
        for a in acts {
            self.act(backend, ctx, a);
        }
        opened
    }

    /// One conversation, Discord-like (a person's request, 2026-10-18:
    /// "user chat and request is combined into one thread history where admin
    /// can approve and chat along with the user and solve together"): the
    /// messages between us and, with the team, the requests too - each where
    /// it was sent, every answer and status change at its time, tagged with
    /// its number. ↩ on a request points the box at it; otherwise the box
    /// sends a message (with the team: files a new request).
    fn thread(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, v: &InboxView) {
        if self.chat_with.trim().is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.label(RichText::new("Pick a conversation, or find anyone on ndexr.").color(theme::muted()));
                ui.label(RichText::new("Send messages, files, and the sites you're building.").size(11.5).color(theme::hush()));
            });
            return;
        }
        let who = self.chat_with.clone();
        let team = who == TEAM;
        let group = who.strip_prefix('#').and_then(|g| g.parse::<i64>().ok()).and_then(|id| v.groups.iter().find(|g| g.id == id && g.member));
        if who.starts_with('#') && group.is_none() {
            ui.label(RichText::new("You are no longer in that group.").color(theme::muted()));
            return;
        }
        let them = v.people.iter().chain(&v.found).find(|p| p.username.eq_ignore_ascii_case(&who));
        let live = them.is_some_and(|p| p.live);
        // Whose requests belong here: mine, with the team (or one of them);
        // theirs, when I am the team.
        let reqs: Vec<&RequestView> = if team || (!v.admin && them.is_some_and(|p| p.admin)) {
            v.requests.iter().filter(|r| r.mine).collect()
        } else if v.admin {
            v.requests.iter().filter(|r| !r.mine && r.username.eq_ignore_ascii_case(&who)).collect()
        } else {
            Vec::new()
        };
        if self.reply_to.is_some_and(|id| !reqs.iter().any(|r| r.id == id)) {
            self.reply_to = None;
        }
        // Read: answers on my requests stop being new once shown here.
        for r in &reqs {
            if r.mine && r.unread && self.seen.insert((r.id, r.messages.len())) {
                self.act(backend, ctx, InboxAct::Seen { id: r.id });
            }
            // Exec's reply, once, into the box - pointed at its request.
            if let Some(x) = self.exec.get_mut(&r.id) {
                if x.done && !x.drafted {
                    x.drafted = true;
                    x.verdict = verdict(&x.reply);
                    if let Some(reply) = reply_part(&x.reply) {
                        self.chat_draft = reply;
                        self.reply_to = Some(r.id);
                    }
                }
            }
        }
        ui.horizontal(|ui| {
            if team {
                ui.label(RichText::new("ndexr team").size(15.0).strong().color(theme::text_strong()));
                ui.label(RichText::new("your requests and the answers - write to ask for anything").size(11.0).color(theme::muted()));
            } else if let Some(g) = group {
                ui.label(RichText::new(format!("⊞ {}", g.name)).size(15.0).strong().color(theme::text_strong()));
                let mut who_in = g.members.join(", ");
                if !g.invited.is_empty() {
                    who_in.push_str(&format!(" · invited {}", g.invited.join(", ")));
                }
                ui.add(egui::Label::new(RichText::new(who_in).size(11.0).color(theme::muted())).truncate());
            } else {
                ui.label(RichText::new("●").color(if live { theme::ok() } else { theme::hush() }));
                ui.label(RichText::new(&who).size(15.0).strong().color(theme::text_strong()));
                ui.label(RichText::new(if live { "here now" } else { "away" }).size(11.0).color(theme::muted()));
                if !reqs.is_empty() {
                    let open = reqs.iter().filter(|r| matches!(r.status.as_str(), "open" | "working")).count();
                    ui.label(RichText::new(format!("· {} request{}{}", reqs.len(), if reqs.len() == 1 { "" } else { "s" }, if open > 0 { format!(", {open} open") } else { String::new() })).size(11.0).color(theme::muted()));
                }
            }
        });
        if !team {
            self.thread_actions(ui, ctx, backend, v, &who, group);
        }
        ui.separator();
        // Everything, in the order it happened (both stamps are UTC
        // "YYYY-MM-DD HH:MM", so they sort as text; the sort is stable, so a
        // request still opens before what was said on it the same minute).
        let mut items: Vec<(&str, Item)> = if team { Vec::new() } else { v.chat.iter().map(|m| (m.at.as_str(), Item::Dm(m))).collect() };
        // Domains shared in this conversation: between us, or with this group.
        items.extend(
            v.shares
                .iter()
                .filter(|s| match group {
                    Some(g) => s.group == g.id,
                    None => !team && s.group == 0 && (if s.mine { s.to.eq_ignore_ascii_case(&who) } else { s.owner.eq_ignore_ascii_case(&who) }),
                })
                .map(|s| (s.at.as_str(), Item::Share(s))),
        );
        for r in &reqs {
            items.push((r.created.as_str(), Item::Opened(r)));
            items.extend(r.messages.iter().map(|m| (m.at.as_str(), Item::Said(r, m))));
        }
        items.sort_by(|a, b| a.0.cmp(b.0));
        let h = ui.available_height() - if self.reply_to.is_some() { 88.0 } else { 64.0 };
        let mut save: Option<(i64, String)> = None;
        let mut moves: Vec<(i64, &'static str)> = Vec::new();
        let mut reply: Option<i64> = None;
        let mut hand: Option<i64> = None;
        let mut share_acts: Vec<InboxAct> = Vec::new();
        egui::ScrollArea::vertical().id_salt(("inbox-chat", who.clone())).max_height(h.max(80.0)).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            if items.is_empty() {
                let hello = if team { "Nothing asked yet - say what you need below, attach a file with ⊕ File." } else { "No messages yet - say hello." };
                ui.label(RichText::new(hello).color(theme::muted()));
            }
            for (_, it) in &items {
                match it {
                    Item::Dm(m) => {
                        let name = if m.mine { "you" } else if group.is_some() { m.who.as_str() } else { who.as_str() };
                        // ✓ sent, ✓✓ read - on mine, between two people.
                        let at = if m.mine && group.is_none() { format!("{} {}", m.at, if m.read { "✓✓" } else { "✓" }) } else { m.at.clone() };
                        match m.kind.as_str() {
                            "file" => card(ui, name, &at, m.mine, |ui| {
                                ui.label(RichText::new(format!("⎙ {}", m.body)).strong().color(theme::text_strong()));
                                ui.label(RichText::new(human(m.size)).size(10.5).color(theme::muted()));
                                let to = self.here.as_ref().map(|(h, _)| h.clone());
                                let tip = to.as_ref().map_or("open a domain to save files into it".to_owned(), |h| format!("save it into {h}"));
                                if ui.add_enabled(to.is_some(), egui::Button::new(format!("Save to {}", to.as_deref().unwrap_or("a domain")))).on_hover_text(tip).clicked() {
                                    save = Some((m.id, m.body.clone()));
                                }
                            }),
                            "site" => card(ui, name, &at, m.mine, |ui| {
                                ui.label(RichText::new(format!("⇗ {}", m.body)).strong().color(theme::text_strong()));
                                ui.label(RichText::new("a site, shared").size(10.5).color(theme::muted()));
                                if ui.button("Open ↗").on_hover_text(format!("https://{}/ in a new tab - its own sign-in decides who sees it", m.body)).clicked() {
                                    ctx.open_url(egui::OpenUrl::new_tab(format!("https://{}/", m.body)));
                                }
                            }),
                            _ => bubble(ui, name, &at, &m.body, m.mine, false),
                        }
                    }
                    Item::Opened(r) => {
                        let name = if r.mine { "you" } else { r.username.as_str() };
                        card(ui, name, &r.created, r.mine, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new(format!("request #{}", r.id)).monospace().size(11.0).color(theme::muted()));
                                ui.label(RichText::new(status_word(&r.status)).size(11.0).strong().color(status_color(&r.status)));
                                ui.label(RichText::new(&r.domain).size(11.0).color(theme::text()));
                                if r.urgency == "blocked" {
                                    ui.label(RichText::new("blocked").size(11.0).color(theme::alt()));
                                }
                            });
                            ui.add(egui::Label::new(RichText::new(r.what.trim()).color(theme::text())).wrap());
                            if !r.trying.trim().is_empty() {
                                ui.label(RichText::new(format!("trying to: {}", r.trying.trim())).size(11.0).color(theme::muted()));
                            }
                            if r.n_uploads > 0 {
                                ui.label(RichText::new(format!("{} file(s) sent with it", r.n_uploads)).size(11.0).color(theme::hush()));
                            }
                            ui.horizontal_wrapped(|ui| {
                                let on = self.reply_to == Some(r.id);
                                if ui.selectable_label(on, "↩ reply").on_hover_text("write on this request").clicked() {
                                    reply = Some(r.id);
                                }
                                if v.admin {
                                    match self.exec.get(&r.id) {
                                        None => {
                                            if ui.small_button("➤ Exec").on_hover_text("your executive agent fixes it, then drafts the reply here").clicked() {
                                                hand = Some(r.id);
                                            }
                                        }
                                        Some(x) if !x.done => {
                                            ui.add(egui::Spinner::new().size(12.0).color(theme::primary()));
                                            ui.label(RichText::new("Exec is on it").size(11.0).color(theme::text()));
                                        }
                                        Some(_) => {}
                                    }
                                    for (s, label) in [("working", "Approve · working"), ("resolved", "Resolved"), ("wontfix", "Won't fix"), ("open", "Reopen")] {
                                        if r.status != s && ui.small_button(label).clicked() {
                                            moves.push((r.id, s));
                                        }
                                    }
                                } else if r.mine {
                                    if matches!(r.status.as_str(), "open" | "working") {
                                        if ui.small_button("✓ Mark resolved").clicked() {
                                            moves.push((r.id, "resolved"));
                                        }
                                    } else if ui.small_button("Open it again").clicked() {
                                        moves.push((r.id, "open"));
                                    }
                                }
                            });
                        });
                    }
                    Item::Share(s) => {
                        let name = if s.mine { "you" } else { s.owner.as_str() };
                        card(ui, name, &s.at, s.mine, |ui| {
                            ui.label(RichText::new(format!("⛓ {}", s.domain)).strong().color(theme::text_strong()));
                            let can = crate::access::role_words(&s.role);
                            let open = can.trim_start_matches("can ");
                            let words = match (s.mine, s.status.as_str(), s.group > 0) {
                                (true, _, true) => format!("shared with {} - everyone in it {can}", s.to),
                                (true, "offered", _) => format!("shared with {} - waiting for them to accept", s.to),
                                (true, "declined", _) => format!("{} turned it down", s.to),
                                (true, _, _) => format!("shared with {} - they {can}", s.to),
                                (false, "offered", _) => format!("shared with you - accept to {open}"),
                                (false, _, _) if s.role != "use" => format!("shared with you - you {can}, and it is among your projects"),
                                _ => "shared with you".to_owned(),
                            };
                            ui.label(RichText::new(words).size(10.5).color(theme::muted()));
                            ui.horizontal(|ui| {
                                if !s.mine && s.status == "offered" {
                                    if ui.add(theme::primary_button("Accept")).clicked() {
                                        share_acts.push(InboxAct::ShareDecide { id: s.id, accept: true });
                                    }
                                    if ui.small_button("No thanks").clicked() {
                                        share_acts.push(InboxAct::ShareDecide { id: s.id, accept: false });
                                    }
                                } else if s.mine || s.status == "accepted" {
                                    if ui.button("Open ↗").on_hover_text(format!("https://{}/ in a new tab", s.domain)).clicked() {
                                        ctx.open_url(egui::OpenUrl::new_tab(format!("https://{}/", s.domain)));
                                    }
                                }
                                if s.mine && ui.small_button("Take back").on_hover_text("they can no longer open it").clicked() {
                                    share_acts.push(InboxAct::ShareRevoke { id: s.id });
                                }
                            });
                        });
                    }
                    Item::Said(r, m) if m.kind == "status" => {
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(format!("{} marked #{} {} · {}", if m.mine { "you" } else { &m.who }, r.id, status_word(&m.body), m.at)).size(11.0).color(status_color(&m.body)));
                        });
                    }
                    Item::Said(r, m) => {
                        let name = if m.mine { "you".to_owned() } else if m.team { format!("{} · ndexr team", m.who) } else { m.who.clone() };
                        bubble(ui, &format!("{name} · on #{}", r.id), &m.at, &m.body, m.mine, m.team);
                    }
                }
            }
        });
        if let Some((id, name)) = save {
            self.save_file(backend, ctx, id, name);
        }
        for a in share_acts {
            self.act(backend, ctx, a);
        }
        if let Some(id) = reply {
            self.reply_to = if self.reply_to == Some(id) { None } else { Some(id) };
        }
        if let Some(r) = hand.and_then(|id| reqs.iter().find(|r| r.id == id)) {
            self.to_exec = Some((r.id, exec_prompt(r)));
            if r.status == "open" {
                moves.push((r.id, "working"));
            }
        }
        for (id, s) in moves {
            // Closing the one the box points at sends what is in it too.
            if v.admin && self.reply_to == Some(id) && matches!(s, "resolved" | "wontfix") && !self.chat_draft.trim().is_empty() {
                let body = std::mem::take(&mut self.chat_draft);
                self.act(backend, ctx, InboxAct::Post { id, body });
            }
            self.act(backend, ctx, InboxAct::Status { id, status: s.to_owned() });
        }
        if let Some(id) = self.reply_to {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("↩ writing on request #{id}")).size(11.5).color(theme::primary()));
                if ui.small_button("✕").on_hover_text("back to a plain message").clicked() {
                    self.reply_to = None;
                }
            });
        }
        let target = self.reply_to;
        ui.horizontal(|ui| {
            let clip_tip = if team { "attach a file - it goes with a new request".to_owned() } else { format!("send a file (up to {} MB)", crate::api::DM_FILE_MAX >> 20) };
            if ui.button("⊕ File").on_hover_text(clip_tip).clicked() {
                if team {
                    self.send_request_file(backend, ctx);
                } else {
                    self.send_file(backend, ctx, who.clone());
                }
            }
            if !team {
                ui.menu_button("⇗ Site", |ui| {
                    ui.label(RichText::new("Share a site").size(11.0).color(theme::muted()));
                    if self.domains.is_empty() {
                        ui.label(RichText::new("no domains yet").size(11.0).color(theme::hush()));
                    }
                    egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                        for h in self.domains.clone() {
                            if ui.button(&h).clicked() {
                                self.act(backend, ctx, InboxAct::Dm { to: who.clone(), body: h, kind: "site".into(), file: None });
                                ui.close();
                            }
                        }
                    });
                })
                .response
                .on_hover_text("share one of your sites");
            }
            // An admin answering a request can send and close in one go,
            // following Exec's call when it made one.
            let call = target.and_then(|id| self.exec.get(&id)).and_then(|x| x.verdict).unwrap_or("resolved");
            let closing = v.admin && target.is_some();
            let hint = match (target, team) {
                (Some(_), _) => "answer on the request…",
                (None, true) => "what do you need? - sends a new request",
                (None, false) => "message…",
            };
            let room = if closing { 190.0 } else { 70.0 };
            let r = ui.add(egui::TextEdit::singleline(&mut self.chat_draft).hint_text(hint).desired_width(ui.available_width() - room));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let close = closing
                && ui
                    .add_enabled(!self.chat_draft.trim().is_empty(), egui::Button::new(match call {
                        "working" => "Send · working",
                        "wontfix" => "Send · won't fix",
                        _ => "Send & resolve",
                    }))
                    .clicked();
            if (close || ui.add(theme::primary_button("Send")).clicked() || enter) && !self.chat_draft.trim().is_empty() {
                let body = std::mem::take(&mut self.chat_draft);
                match target {
                    Some(id) => {
                        self.act(backend, ctx, InboxAct::Post { id, body });
                        let status = reqs.iter().find(|r| r.id == id).map(|r| r.status.clone()).unwrap_or_default();
                        if close && status != call {
                            self.act(backend, ctx, InboxAct::Status { id, status: call.into() });
                        }
                    }
                    None if team => self.new_request(backend, ctx, body, Vec::new()),
                    None => self.act(backend, ctx, InboxAct::Dm { to: who.clone(), body, kind: String::new(), file: None }),
                }
                r.request_focus();
            }
        });
    }

    /// Under a conversation's name: connect, share a domain, and for a group
    /// invite and leave.
    fn thread_actions(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, v: &InboxView, who: &str, group: Option<&crate::api::GroupView>) {
        let mut acts: Vec<InboxAct> = Vec::new();
        ui.horizontal_wrapped(|ui| {
            match group {
                Some(g) => {
                    let can: Vec<&str> = v
                        .connections
                        .iter()
                        .filter(|c| c.status == "accepted" && !g.members.iter().chain(&g.invited).any(|m| m.eq_ignore_ascii_case(&c.username)))
                        .map(|c| c.username.as_str())
                        .collect();
                    ui.menu_button("✚ Invite", |ui| {
                        if can.is_empty() {
                            ui.label(RichText::new("Everyone you are connected with is in it - connect with more people first.").size(11.0).color(theme::hush()));
                        }
                        for c in &can {
                            if ui.button(*c).clicked() {
                                acts.push(InboxAct::GroupInvite { id: g.id, who: (*c).to_owned() });
                                ui.close();
                            }
                        }
                    })
                    .response
                    .on_hover_text("people you are connected with; they accept to join");
                    if let Some(d) = share_menu(ui, &self.domains, &format!("everyone in {} can open it", g.name)) {
                        acts.push(InboxAct::Share { domain: d, to: String::new(), group: g.id, role: String::new() });
                    }
                    if ui.small_button("Leave").clicked() {
                        acts.push(InboxAct::GroupLeave { id: g.id });
                    }
                }
                None => {
                    let c = v.connections.iter().find(|c| c.username.eq_ignore_ascii_case(who));
                    connect_button(ui, c, who, &mut acts);
                    if c.is_some_and(|c| c.status == "accepted") || v.admin {
                        if let Some(d) = share_menu(ui, &self.domains, &format!("{who} accepts, then can open it")) {
                            acts.push(InboxAct::Share { domain: d, to: who.to_owned(), group: 0, role: String::new() });
                        }
                    }
                }
            }
        });
        let leaving = acts.iter().any(|a| matches!(a, InboxAct::GroupLeave { .. }));
        for a in acts {
            self.act(backend, ctx, a);
        }
        if leaving {
            self.chat_with.clear();
        }
    }

    /// A new request from the team conversation, about the domain open here.
    fn new_request(&self, backend: &Backend, ctx: &egui::Context, what: String, files: Vec<crate::api::Attachment>) {
        let domain = self.here.as_ref().map_or_else(|| "cove.ndexr.io".to_owned(), |(h, _)| h.clone());
        let (slot, c) = (self.sent.clone(), ctx.clone());
        let q = Query::SendRequest { domain, what, trying: String::new(), blocked: false, files };
        backend.query(q, ctx, move |r: Result<serde_json::Value, String>| {
            *slot.lock().unwrap() = Some(r.map(|id| format!("sent - request #{id}")));
            c.request_repaint();
        });
    }

    /// 📎 with the team: a file, and what is in the box, as a new request.
    fn send_request_file(&mut self, backend: &Backend, ctx: &egui::Context) {
        let what = std::mem::take(&mut self.chat_draft);
        let (backend, slot, ctx, here) = (backend.clone(), self.sent.clone(), ctx.clone(), self.here.clone());
        let pick = async move {
            let Some(f) = crate::pick::files("Attach to a request", false).await.and_then(|v| v.into_iter().next()) else { return };
            let name = f.file_name();
            let bytes = f.read().await;
            if bytes.len() > crate::api::REQUEST_FILES_MAX {
                *slot.lock().unwrap() = Some(Err(format!("{name}: a request carries up to {} MB", crate::api::REQUEST_FILES_MAX >> 20)));
                ctx.request_repaint();
                return;
            }
            let file = crate::api::Attachment { name: name.clone(), data: crate::api::b64_encode(&bytes) };
            let what = if what.trim().is_empty() { format!("(sent {name})") } else { what };
            let domain = here.map_or_else(|| "cove.ndexr.io".to_owned(), |(h, _)| h);
            let c = ctx.clone();
            backend.query(Query::SendRequest { domain, what, trying: String::new(), blocked: false, files: vec![file] }, &ctx, move |r: Result<serde_json::Value, String>| {
                *slot.lock().unwrap() = Some(r.map(|id| format!("sent - request #{id}")));
                c.request_repaint();
            });
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || pollster::block_on(pick));
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(pick);
    }

    /// 📎: pick a file on this computer and send it to `to`.
    fn send_file(&self, backend: &Backend, ctx: &egui::Context, to: String) {
        let (backend, slot, ctx) = (backend.clone(), self.sent.clone(), ctx.clone());
        let pick = async move {
            let Some(f) = crate::pick::files("Send in Chat", false).await.and_then(|v| v.into_iter().next()) else { return };
            let name = f.file_name();
            let bytes = f.read().await;
            if bytes.len() > crate::api::DM_FILE_MAX {
                *slot.lock().unwrap() = Some(Err(format!("{name}: files in Chat are up to {} MB", crate::api::DM_FILE_MAX >> 20)));
                ctx.request_repaint();
                return;
            }
            let file = crate::api::Attachment { name: name.clone(), data: crate::api::b64_encode(&bytes) };
            let c = ctx.clone();
            backend.query(Query::InboxAct { act: InboxAct::Dm { to, body: name, kind: "file".into(), file: Some(file) } }, &ctx, move |r: Result<serde_json::Value, String>| {
                *slot.lock().unwrap() = Some(r.map(|_| String::new()));
                c.request_repaint();
            });
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || pollster::block_on(pick));
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(pick);
    }

    /// Save to <domain>: fetch the file sent in Chat and put it in the domain
    /// open here (never over a file already there - host.rs `upload`).
    fn save_file(&self, backend: &Backend, ctx: &egui::Context, id: i64, name: String) {
        let Some((_, dir)) = self.here.clone() else { return };
        let (b, slot, c) = (backend.clone(), self.sent.clone(), ctx.clone());
        backend.query(Query::DmFile { id }, ctx, move |r: Result<crate::api::Attachment, String>| {
            let bytes = r.and_then(|a| a.bytes().ok_or_else(|| format!("{name} did not arrive whole")));
            match bytes {
                Ok(bytes) => {
                    let s2 = slot.clone();
                    b.upload(&dir, &name, bytes, &c, move |r| {
                        *s2.lock().unwrap() = Some(r.map(|to| format!("saved {to}")));
                    });
                }
                Err(e) => *slot.lock().unwrap() = Some(Err(e)),
            }
            c.request_repaint();
        });
    }
}

/// Something waiting on me, with its yes and no: Some(true) for yes.
fn pending_row(ui: &mut egui::Ui, what: &str, yes: &str) -> Option<bool> {
    let mut out = None;
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(what).size(11.5).color(theme::text_strong()));
        if ui.add(theme::primary_button(yes)).clicked() {
            out = Some(true);
        }
        if ui.small_button("No").clicked() {
            out = Some(false);
        }
    });
    out
}

/// Where I stand with `name`, and the one thing to do about it.
fn connect_button(ui: &mut egui::Ui, c: Option<&crate::api::Connection>, name: &str, acts: &mut Vec<InboxAct>) {
    match c {
        Some(c) if c.status == "accepted" => {
            ui.label(RichText::new("✓ connected").size(11.0).color(theme::ok()));
            if ui.small_button("Disconnect").clicked() {
                acts.push(InboxAct::Connection { with: name.to_owned(), accept: false });
            }
        }
        Some(c) if c.incoming => {
            if ui.add(theme::primary_button("Accept connection")).clicked() {
                acts.push(InboxAct::Connection { with: name.to_owned(), accept: true });
            }
        }
        Some(_) => {
            ui.label(RichText::new("asked to connect").size(11.0).color(theme::muted()));
            if ui.small_button("Withdraw").clicked() {
                acts.push(InboxAct::Connection { with: name.to_owned(), accept: false });
            }
        }
        None => {
            if ui.small_button("☍ Connect").on_hover_text("once they accept you can message, invite and share with each other").clicked() {
                acts.push(InboxAct::Connect { to: name.to_owned() });
            }
        }
    }
}

/// 🔗 Share…: one of my domains - a site, an app, a software project.
fn share_menu(ui: &mut egui::Ui, domains: &[String], tip: &str) -> Option<String> {
    let mut out = None;
    ui.menu_button("⛓ Share…", |ui| {
        ui.label(RichText::new("Share one of your domains").size(11.0).color(theme::muted()));
        if domains.is_empty() {
            ui.label(RichText::new("no domains yet").size(11.0).color(theme::hush()));
        }
        egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
            for h in domains {
                if ui.button(h).clicked() {
                    out = Some(h.clone());
                    ui.close();
                }
            }
        });
    })
    .response
    .on_hover_text(tip);
    out
}

/// A conversation in Chat's list: a dot for here or away, the name, the last
/// thing said, and a badge or the time.
fn conversation_row(ui: &mut egui::Ui, on: bool, live: bool, name: &str, last: &str, unread: i64, at: &str) -> egui::Response {
    egui::Frame::new()
        .fill(if on { theme::surface_hi() } else { Color32::TRANSPARENT })
        .corner_radius(6)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new("●").color(if live { theme::ok() } else { theme::hush() }));
                ui.label(RichText::new(name).strong().color(theme::text_strong()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if unread > 0 {
                        ui.label(RichText::new(format!(" {unread} ")).size(11.0).strong().color(theme::bg()).background_color(theme::primary()));
                    } else if !at.is_empty() {
                        ui.label(RichText::new(at.get(11..).unwrap_or(at)).size(10.0).color(theme::hush()));
                    }
                });
            });
            ui.add(egui::Label::new(RichText::new(last).size(11.0).color(theme::muted())).truncate());
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A file or a site in the thread: a card on my side or theirs.
fn card(ui: &mut egui::Ui, who: &str, at: &str, mine: bool, body: impl FnOnce(&mut egui::Ui)) {
    let layout = if mine { egui::Layout::right_to_left(egui::Align::Min) } else { egui::Layout::left_to_right(egui::Align::Min) };
    ui.with_layout(layout, |ui| {
        egui::Frame::new()
            .fill(if mine { theme::surface_hi() } else { theme::bg() })
            .stroke(egui::Stroke::new(1.0, theme::primary().gamma_multiply(0.5)))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_max_width(320.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("{who} · {at}")).size(10.5).color(theme::muted()));
                    body(ui);
                });
            });
    });
    ui.add_space(4.0);
}

fn human(n: i64) -> String {
    match n {
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.0} KB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

/// One message: who and when over it, mine to the right.
fn bubble(ui: &mut egui::Ui, who: &str, at: &str, body: &str, mine: bool, team: bool) {
    let layout = if mine { egui::Layout::right_to_left(egui::Align::Min) } else { egui::Layout::left_to_right(egui::Align::Min) };
    ui.with_layout(layout, |ui| {
        let w = (ui.available_width() * 0.82).max(160.0);
        egui::Frame::new()
            .fill(if mine { theme::surface_hi() } else { theme::bg() })
            .stroke(egui::Stroke::new(1.0, if team { theme::primary().gamma_multiply(0.6) } else { theme::pane_brd() }))
            .corner_radius(6)
            .inner_margin(egui::Margin::symmetric(10, 6))
            .show(ui, |ui| {
                ui.set_max_width(w);
                ui.vertical(|ui| {
                    ui.label(RichText::new(format!("{who} · {at}")).size(10.5).color(if team { theme::primary() } else { theme::muted() }));
                    ui.add(egui::Label::new(RichText::new(body).color(theme::text())).wrap());
                });
            });
    });
    ui.add_space(4.0);
}

#[cfg(test)]
mod tests {
    use super::{reply_part, verdict};

    #[test]
    fn exec_says_what_sending_sets() {
        let a = "Fixed the vhost.\n\n**STATUS: resolved**\nREPLY TO THEM:\nAll sorted - reload.";
        assert_eq!(verdict(a), Some("resolved"));
        assert_eq!(reply_part(a).as_deref(), Some("All sorted - reload."));
        assert_eq!(verdict("STATUS: working (waits on the next release)\nREPLY TO THEM: soon"), Some("working"));
        assert_eq!(verdict("`STATUS: wontfix`\nREPLY TO THEM: no"), Some("wontfix"));
        assert_eq!(verdict("no verdict\nREPLY TO THEM: hi"), None);
        // A STATUS: inside the reply to them is theirs to read, not a verdict.
        assert_eq!(verdict("REPLY TO THEM: STATUS: resolved"), None);
    }
}

/// A phone's way back from a picked request or conversation to its list.
fn back(ui: &mut egui::Ui, word: &str) -> bool {
    let b = ui.add(egui::Button::new(RichText::new(word).size(14.0)).min_size(egui::vec2(0.0, 34.0))).clicked();
    ui.add_space(2.0);
    b
}
