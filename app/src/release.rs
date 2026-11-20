//! A newer Cove, offered rather than imposed (operator, 2026-10-18: "we can't
//! be kicking people out of their sessions whenever we restart ... they can be
//! asked if they would like to update or to wait a certain amount of time and
//! then for us to ask again ... or if the session closes and they restart it,
//! then it will be updated").
//!
//! A session runs the build it started on (multiuser.rs `current_release`).
//! This asks the server, every minute, whether a newer one is published
//! (`/s/<id>/release.json`) and, when there is, shows a strip across the top:
//! Update now - the session starts again on the new build (`/s/<id>/update`)
//! and the page reloads into it - or ask again in 5 or 30 minutes, or not for
//! this build at all (the next session starts on it anyway). Only in a browser
//! on the multi-user server; the desktop app updates itself (update.rs).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::backend::Backend;
use crate::theme;

#[derive(Default, Clone, serde::Deserialize)]
struct Answer {
    #[serde(default)]
    running: String,
    #[serde(default)]
    current: String,
    #[serde(default)]
    newer: bool,
    /// What the newer build brings (app/WHATSNEW.md), newest first.
    #[serde(default)]
    notes: Vec<String>,
    /// What changed since the build this browser last saw (asked ?since=).
    #[serde(default)]
    since_notes: Vec<String>,
}

/// The changelog's lines shipped after release `after` and by `upto`
/// (release ids start "YYYYMMDDTHHMMSSZ"; lines "- 2026-10-19T23:20Z · …"),
/// newest first (operator, 2026-10-19: "each time an update happens, the app
/// needs to tell the user what is new"). An empty `upto` is no bound.
pub fn notes(text: &str, after: &str, upto: &str) -> Vec<String> {
    let stamp = |id: &str| -> String { id.chars().filter(|c| c.is_ascii_digit()).take(12).collect() };
    let (lo, hi) = (stamp(after), stamp(upto));
    if lo.len() < 12 {
        return Vec::new();
    }
    text.lines()
        .filter_map(|l| {
            let (when, what) = l.strip_prefix("- ")?.split_once(" · ")?;
            let at = stamp(when);
            (at.len() == 12 && at > lo && (hi.len() < 12 || at <= hi)).then(|| what.trim().to_owned())
        })
        .collect()
}

/// The release this browser last saw (localStorage), so a new build - taken
/// with Update now, or found on opening Cove - says what changed since.
fn seen() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        return web_sys::window().and_then(|w| w.local_storage().ok().flatten()).and_then(|s| s.get_item("cove-seen-release").ok().flatten()).unwrap_or_default();
    }
    #[cfg(not(target_arch = "wasm32"))]
    String::new()
}

fn set_seen(id: &str) {
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item("cove-seen-release", id);
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = id;
}

/// "Update without asking" (operator, 2026-10-26: "allow user to select
/// interrupt my session if release available so they dont have to click
/// update now"): kept per browser, like the release it last saw.
fn auto_stored() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        return web_sys::window().and_then(|w| w.local_storage().ok().flatten()).and_then(|s| s.get_item("cove-auto-update").ok().flatten()).as_deref() == Some("1");
    }
    #[cfg(not(target_arch = "wasm32"))]
    false
}

fn set_auto_stored(on: bool) {
    #[cfg(target_arch = "wasm32")]
    if let Some(s) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) {
        let _ = s.set_item("cove-auto-update", if on { "1" } else { "0" });
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = on;
}

#[derive(Default)]
pub struct ReleaseBar {
    /// Update without asking: a newer build is taken as soon as it is seen.
    /// None until read from the browser.
    auto: Option<bool>,
    answer: Arc<Mutex<Option<Answer>>>,
    asked: f64,
    /// Not before this (page time, seconds): "ask me again in …".
    quiet_until: f64,
    /// "Not for this one": the build it was said for.
    skipped: String,
    /// Update now was pressed: (when, its answer once there).
    updating: Option<(f64, Arc<Mutex<Option<Result<(), String>>>>)>,
    error: Option<String>,
    /// Help › Check for Updates (operator, 2026-10-19: "all of the ides need
    /// a visual when we click on update and if there is one, it should do
    /// it"): when it was chosen, and how many answers had come by then - the
    /// strip says "Checking…" until a newer one arrives, and a newer build is
    /// taken at once.
    manual: Option<(f64, u64)>,
    answers: Arc<AtomicU64>,
    /// A passing word in the strip, until (page time): "the newest", why not.
    note: Option<(f64, String, bool)>,
    /// What is new since the build this browser last saw: shown once, in a
    /// card, after an update; None once read.
    news: Option<Vec<String>>,
    news_checked: bool,
}

const EVERY: f64 = 60.0;
/// Seconds Update now waits for its answer before reloading anyway.
const UPDATE_GIVE_UP: f64 = 60.0;

impl ReleaseBar {
    /// A session of the multi-user server, which has a release to ask about.
    pub fn applies(backend: &Backend) -> bool {
        matches!(backend, Backend::Http { base } if base.contains("/s/"))
    }

    /// Update now is in flight. Its session is stopped and started again on
    /// purpose, so the "session ended" card (app.rs) must not take the page
    /// away meanwhile: it navigated to / after 6 s, which cancelled this
    /// request (nginx 499 at 6.7-6.9 s) and could land on another session
    /// still on the old build (operator, 2026-10-26: "the update now button
    /// stopped doing things").
    pub fn updating(&self) -> bool {
        self.updating.is_some()
    }

    /// Update without asking is on (Help menu, and the strip's checkbox).
    pub fn auto(&mut self) -> bool {
        *self.auto.get_or_insert_with(auto_stored)
    }

    pub fn set_auto(&mut self, on: bool) {
        self.auto = Some(on);
        set_auto_stored(on);
    }

    /// Check for Updates: ask now, and take a newer build if there is one.
    pub fn check_now(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.manual = Some((now, self.answers.load(Ordering::Relaxed)));
        self.asked = f64::NEG_INFINITY;
        self.quiet_until = 0.0;
        self.skipped.clear();
        self.note = None;
        ctx.request_repaint();
    }

    /// The desktop app's own updater (update.rs), told as the session's is:
    /// what it is doing while it does it, and - when asked - a newer version
    /// installed and Cove started again on it.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn desktop(&mut self, root: &mut egui::Ui, ctx: &egui::Context, u: &crate::update::Updater) {
        use crate::update::Status;
        let now = ctx.input(|i| i.time);
        let status = u.status();
        let asked = self.manual.is_some();
        let busy = match &status {
            Status::Checking if asked => Some("Checking for a newer Cove…".to_owned()),
            Status::Downloading(v) => Some(format!("Downloading Cove v{v}…")),
            Status::Ready(v) if asked => {
                self.manual = None;
                u.restart(ctx);
                Some(format!("Cove v{v} is installed - starting it…"))
            }
            _ => None,
        };
        if let Some(msg) = busy {
            strip(root, |ui| {
                ui.add(egui::Spinner::new().size(12.0));
                ui.label(RichText::new(msg).color(theme::text()));
            });
            ctx.request_repaint_after(std::time::Duration::from_millis(400));
            return;
        }
        if asked {
            self.manual = None;
            let (msg, ok) = match &status {
                Status::Current => (format!("This is the newest Cove (v{}).", crate::update::VERSION), true),
                Status::Ready(v) => (format!("Cove v{v} is installed - it starts next time."), true),
                Status::Managed(why) => (format!("This Cove is not updated from here: {why}"), false),
                Status::Unpublished => ("No Cove is published for this computer yet.".to_owned(), false),
                Status::Failed(e) => (format!("The update did not go: {e}"), false),
                _ => (String::new(), true),
            };
            if !msg.is_empty() {
                self.note = Some((now + 6.0, msg, ok));
            }
        }
        self.show_note(root, ctx, now);
    }

    /// The passing word, while it lasts. True when it was shown.
    fn show_note(&mut self, root: &mut egui::Ui, ctx: &egui::Context, now: f64) -> bool {
        let Some((until, msg, ok)) = self.note.clone() else { return false };
        if now >= until {
            self.note = None;
            return false;
        }
        strip(root, |ui| {
            ui.label(RichText::new(if ok { "✓" } else { "!" }).strong().color(if ok { theme::ok() } else { theme::warn() }));
            ui.label(RichText::new(msg).color(theme::text()));
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        true
    }

    /// After an update - Update now, or a new build found on opening Cove - a
    /// card says what is new since the build this browser last saw, once. A
    /// first visit only remembers the build: nothing to compare it with.
    fn whats_new(&mut self, ctx: &egui::Context) {
        if !self.news_checked {
            let Some(a) = self.answer.lock().unwrap().clone() else { return };
            if a.running.is_empty() {
                return;
            }
            self.news_checked = true;
            let was = seen();
            if !was.is_empty() && was != a.running && !a.since_notes.is_empty() {
                self.news = Some(a.since_notes.clone());
            }
            set_seen(&a.running);
        }
        let Some(lines) = self.news.clone() else { return };
        // A modal over the whole page, most of the screen: a dozen changes at
        // 13 px in a 520-px box at the top was too small to read (operator,
        // 2026-10-23: "what's new in cove should be a larger modal").
        let screen = ctx.content_rect();
        let w = (screen.width() - 32.0).clamp(280.0, 860.0);
        let h = (screen.height() * 0.78).max(240.0);
        let mut done = false;
        let r = egui::Modal::new(egui::Id::new("whats-new")).show(ctx, |ui| {
            ui.set_width(w);
            ui.label(RichText::new("What's new in Cove").size(26.0).strong().color(theme::text_strong()));
            ui.label(RichText::new(format!("{} since you last used it", if lines.len() == 1 { "1 change".to_owned() } else { format!("{} changes", lines.len()) })).size(15.0).color(theme::text()));
            ui.add_space(10.0);
            egui::ScrollArea::vertical().max_height(h - 140.0).auto_shrink([false, true]).show(ui, |ui| {
                for l in &lines {
                    ui.horizontal_top(|ui| {
                        ui.label(RichText::new("•").size(18.0).color(theme::primary()));
                        ui.add(egui::Label::new(RichText::new(l).size(16.0).color(theme::text())).wrap());
                    });
                    ui.add_space(8.0);
                }
            });
            ui.add_space(10.0);
            done = ui.add(theme::primary_button("Got it").min_size(egui::vec2(140.0, 40.0))).clicked();
        });
        if done || r.should_close() {
            self.news = None;
        }
    }

    /// The strip, when a newer build waits and the person has not put it off.
    pub fn ui(&mut self, root: &mut egui::Ui, ctx: &egui::Context, backend: &Backend) {
        #[cfg(not(target_arch = "wasm32"))]
        let Backend::Http { base } = backend else { return };
        #[cfg(target_arch = "wasm32")]
        let Backend::Http { base } = backend;
        // Only a session of the multi-user server has a release to ask about;
        // and a Cove inside another Cove (its App tab) leaves the asking to
        // the one around it (operator, 2026-10-21: "when cove is run in cove
        // the update bar should still only show in the main cove").
        if !base.contains("/s/") || embedded() {
            return;
        }
        let now = ctx.input(|i| i.time);
        if self.updating.is_none() && now - self.asked > EVERY {
            self.asked = now;
            let (slot, c, answers) = (self.answer.clone(), ctx.clone(), self.answers.clone());
            let seen = seen();
            let url = if seen.is_empty() { format!("{base}/release.json") } else { format!("{base}/release.json?since={seen}") };
            let mut req = ehttp::Request::get(url);
            #[cfg(target_arch = "wasm32")]
            {
                req.credentials = ehttp::Credentials::SameOrigin;
            }
            req.headers.insert("Accept", "application/json");
            ehttp::fetch(crate::backend::with_token(req), move |r| {
                // A failed ask (the server restarting) keeps what was known.
                if let Some(a) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice::<Answer>(&r.bytes).ok()) {
                    *slot.lock().unwrap() = Some(a);
                }
                answers.fetch_add(1, Ordering::Relaxed);
                c.request_repaint();
            });
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(EVERY as u64));
        self.whats_new(ctx);

        if let Some((since, done)) = &self.updating {
            let since = *since;
            let got = done.lock().unwrap().take();
            match got {
                Some(Ok(())) => reload(),
                Some(Err(e)) => {
                    self.error = Some(e);
                    self.updating = None;
                }
                // No answer in a minute: the restart takes ~6 s, so the
                // session is on the new build by now whatever became of the
                // request - reload onto it rather than spin.
                None if now - since > UPDATE_GIVE_UP => reload(),
                None => {}
            }
            if self.updating.is_some() {
                strip(root, |ui| {
                    ui.add(egui::Spinner::new().size(12.0));
                    ui.label(RichText::new(format!("Starting your session on the new Cove… {}s", (now - since) as u32)).color(theme::text()));
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
                return;
            }
        }

        // Check for Updates: "Checking…" until the answer, then a newer build
        // is taken, or the strip says this one is the newest.
        let mut act = 0;
        if let Some((since, before)) = self.manual {
            if self.answers.load(Ordering::Relaxed) <= before {
                if now - since > 20.0 {
                    self.manual = None;
                    self.note = Some((now + 6.0, "Cove's server did not answer - try again in a moment.".into(), false));
                } else {
                    strip(root, |ui| {
                        ui.add(egui::Spinner::new().size(12.0));
                        ui.label(RichText::new("Checking for a newer Cove…").color(theme::text()));
                    });
                    ctx.request_repaint_after(std::time::Duration::from_millis(300));
                    return;
                }
            } else {
                self.manual = None;
                match self.answer.lock().unwrap().clone() {
                    Some(a) if a.newer => act = 1,
                    Some(a) => self.note = Some((now + 6.0, format!("This is the newest Cove ({}).", a.current), true)),
                    None => self.note = Some((now + 6.0, "Cove's server did not answer - try again in a moment.".into(), false)),
                }
            }
        }
        if act == 0 && self.show_note(root, ctx, now) {
            return;
        }
        let a = self.answer.lock().unwrap().clone().unwrap_or_default();
        if act == 0 && (!a.newer || now < self.quiet_until || self.skipped == a.current) {
            return;
        }
        // On a phone the sentence and four buttons ran off the right edge and
        // cut Update in half (operator screenshot, 2026-10-18): there, the
        // headline, Update and Later only. The full sentence needs about 980
        // points, which a laptop zoomed to 150% no longer has (operator,
        // 2026-10-18: Ctrl + on Windows hid the right of the screen).
        // Update without asking: a newer build is taken when seen, unless
        // the person said "Not this one" for it. Not again after a failed
        // try - that would ask every frame; the strip shows the error and
        // its Update now instead.
        if act == 0 && self.error.is_none() && self.auto() {
            act = 1;
        }
        if act == 1 {
            // Taken at once (Check for Updates found one, or update without
            // asking is on): no question first.
        } else if root.available_width() < 980.0 {
            strip(root, |ui| {
                let r = ui.label(RichText::new("A newer Cove is ready").strong().color(theme::text_strong()));
                if !a.notes.is_empty() {
                    r.on_hover_text(a.notes.iter().map(|n| format!("• {n}")).collect::<Vec<_>>().join("\n"));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if ui.button("Later").on_hover_text("ask again in five minutes").clicked() {
                        act = 2;
                    }
                    if ui.button("Always").on_hover_text("update now, and from now on whenever a newer Cove is ready - no question").clicked() {
                        act = 5;
                    }
                    if ui.add(theme::primary_button("Update")).on_hover_text("restarts this session's R, Python and terminal; your files, settings and agents stay").clicked() {
                        act = 1;
                    }
                });
            });
        } else {
        strip(root, |ui| {
            ui.label(RichText::new("A newer Cove is ready.").strong().color(theme::text_strong()));
            // What it brings, the newest first; all of it on hover.
            if let Some(first) = a.notes.first() {
                let more = if a.notes.len() > 1 { format!("  +{} more", a.notes.len() - 1) } else { String::new() };
                ui.add(egui::Label::new(RichText::new(format!("New: {first}{more}")).color(theme::text())).truncate())
                    .on_hover_text(a.notes.iter().map(|n| format!("• {n}")).collect::<Vec<_>>().join("\n"));
            } else {
                ui.label(
                    RichText::new("Updating restarts this session's R, Python and terminal; your files, settings and agents stay.")
                        .color(theme::muted()),
                );
            }
            if let Some(e) = &self.error {
                ui.label(RichText::new(format!("The last try failed: {e}")).color(theme::warn()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if ui.button("Not this one").on_hover_text("keep this session as it is; your next session starts on the new Cove").clicked() {
                    act = 4;
                }
                if ui.button("Always update").on_hover_text("update now, and from now on whenever a newer Cove is ready - no question. Turn it off in Help").clicked() {
                    act = 5;
                }
                if ui.button("In 30 min").on_hover_text("ask again in half an hour").clicked() {
                    act = 3;
                }
                if ui.button("In 5 min").on_hover_text("ask again in five minutes").clicked() {
                    act = 2;
                }
                if ui.add(theme::primary_button("Update now")).clicked() {
                    act = 1;
                }
            });
        });
        }
        if act == 5 {
            self.set_auto(true);
            act = 1;
        }
        match act {
            1 => {
                let done = Arc::new(Mutex::new(None));
                let (slot, c) = (done.clone(), ctx.clone());
                let mut req = ehttp::Request::post(format!("{base}/update"), Vec::new());
                req.headers.insert("X-Cove", "1");
                #[cfg(target_arch = "wasm32")]
                {
                    req.credentials = ehttp::Credentials::SameOrigin;
                }
                ehttp::fetch(crate::backend::with_token(req), move |r| {
                    *slot.lock().unwrap() = Some(match r {
                        Ok(r) if r.ok => Ok(()),
                        Ok(r) => Err(format!("{} {}", r.status, String::from_utf8_lossy(&r.bytes).chars().take(160).collect::<String>())),
                        Err(e) => Err(e),
                    });
                    c.request_repaint();
                });
                self.error = None;
                self.updating = Some((now, done));
            }
            2 => self.quiet_until = now + 5.0 * 60.0,
            3 => self.quiet_until = now + 30.0 * 60.0,
            4 => self.skipped = a.current.clone(),
            _ => {}
        }
    }
}

fn strip(root: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    egui::Panel::top("rs-release")
        .resizable(false)
        .show_separator_line(true)
        .exact_size(30.0)
        .frame(egui::Frame::new().fill(theme::surface_hi()).inner_margin(egui::Margin::symmetric(10, 4)))
        .show(root, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                add(ui)
            });
        });
}

/// Into the session, now on the new build: the same address.
/// This page is inside another: an iframe (the browser's App tab), or the
/// desktop app's own webview, which marks the pages it opens
/// (webview.rs `__coveEmbedded`).
#[cfg(target_arch = "wasm32")]
pub fn embedded() -> bool {
    let Some(w) = web_sys::window() else { return false };
    let framed = w.top().ok().flatten().is_some_and(|top| wasm_bindgen::JsValue::from(top) != wasm_bindgen::JsValue::from(w.clone()));
    framed || js_sys::Reflect::get(&w, &"__coveEmbedded".into()).is_ok_and(|v| v.is_truthy())
}

#[cfg(not(target_arch = "wasm32"))]
pub fn embedded() -> bool {
    false
}

fn reload() {
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = web_sys::window() {
        let _ = w.location().reload();
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn whats_new_is_what_shipped_between_two_builds() {
        let text = "# What's new\n\n- 2026-10-19T23:20Z · c\n- 2026-10-19T22:40Z · b\n- 2026-10-19T19:10Z · a\nnot a line\n";
        assert_eq!(super::notes(text, "20261019T191500Z-2aa612f", "20261019T230652Z-2aa612f"), vec!["b"]);
        assert_eq!(super::notes(text, "20261019T191500Z-2aa612f", ""), vec!["c", "b"]);
        assert!(super::notes(text, "", "20261019T230652Z-x").is_empty(), "no build to compare with");
        // The real changelog parses: every entry is a stamped line.
        let real = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/WHATSNEW.md")).unwrap();
        assert!(super::notes(&real, "20200101T000000Z", "").len() >= 10);
    }
}
