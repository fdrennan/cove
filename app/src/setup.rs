//! First start (operator, 2026-10-15: "users who use cove have a clear
//! end-to-end setup on startup to get their claude credentials working with
//! cove in all versions of it, linux and mac ... the startup settings of any
//! ide for the first time, light dark theme, etc - needs to look beautiful and
//! clear and obvious").
//!
//! Getting in is not a Claude setup (operator, 2026-10-19: "cove needs to be
//! easy to get into, do not ask questions about setting up claude, you give
//! the person their free commands and then the claude stuff will pop up ...
//! after they run out"). First start is Look (and Account on the desktop),
//! then Ready: the agent runs on the free weekly commands from the first
//! prompt. The Claude step comes up by itself when an agent turn is refused
//! because they ran out (`connect_button`), and any time from Help › Set up
//! Cove or the toolbar's Claude chip:
//!
//!   Look     Dark (ndexr Regolith), Light (ndexr Light) or the system's,
//!            applied as it is clicked.
//!   Account  desktop only: sign in to ndexr (account.rs). A browser on
//!            cove.ndexr.io is signed in already.
//!   Claude   where the person stands - the site's own verdict and words
//!            (credentials.ndexr.io/connect.r over claude_access.r) - and the
//!            ways to connect their own: a key already on this computer (sent
//!            only once they tick that they want it sent), their Pro/Max
//!            subscription (Cove runs `claude setup-token` in the Terminal and
//!            picks the token off the screen), or a pasted key. Or not now:
//!            the free weekly commands on ndexr's shared Claude.
//!   Ready    what was set, and where to start.
//!
//! The credential goes where every ndexr surface reads it: the person's
//! "<email>-claude" secret, through `cove multiuser` on cove.ndexr.io and
//! through api.ndexr.io from the desktop app.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Sense, Stroke};
use serde::{Deserialize, Serialize};

use crate::api::ClaudeView;
#[cfg(target_arch = "wasm32")]
use crate::api::Query;
use crate::backend::Backend;
use crate::theme;

type Slot<T> = Arc<Mutex<Option<T>>>;

/// The person's pick of look; saved with the window's state (app.rs `Saved`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
/// `Dark` and `Light` keep their names so a saved pick from before the other
/// looks existed still reads: they are ndexr Regolith and ndexr Light.
pub enum ThemePref {
    #[default]
    Dark,
    Light,
    System,
    TokyoNight,
    Nord,
    Mocha,
    Gruvbox,
    GitHubLight,
    Latte,
    Hawaiian,
}

impl ThemePref {
    /// In the order they are offered: the dark looks, the light ones, then the system's.
    pub const ALL: [ThemePref; 10] = [
        ThemePref::Dark,
        ThemePref::TokyoNight,
        ThemePref::Nord,
        ThemePref::Mocha,
        ThemePref::Gruvbox,
        ThemePref::Hawaiian,
        ThemePref::Light,
        ThemePref::GitHubLight,
        ThemePref::Latte,
        ThemePref::System,
    ];

    /// Its palette (theme.rs `PALETTES`); the system's is ndexr Regolith or ndexr Light.
    pub fn palette(self, ctx: &egui::Context) -> usize {
        match self {
            ThemePref::Dark => 0,
            ThemePref::TokyoNight => 1,
            ThemePref::Nord => 2,
            ThemePref::Mocha => 3,
            ThemePref::Gruvbox => 4,
            ThemePref::Light => 5,
            ThemePref::GitHubLight => 6,
            ThemePref::Latte => 7,
            ThemePref::Hawaiian => 8,
            ThemePref::System => if ctx.system_theme() == Some(egui::Theme::Light) { 5 } else { 0 },
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            ThemePref::System => "Match system",
            ThemePref::Dark => theme::PALETTES[0].name,
            ThemePref::TokyoNight => theme::PALETTES[1].name,
            ThemePref::Nord => theme::PALETTES[2].name,
            ThemePref::Mocha => theme::PALETTES[3].name,
            ThemePref::Gruvbox => theme::PALETTES[4].name,
            ThemePref::Light => theme::PALETTES[5].name,
            ThemePref::GitHubLight => theme::PALETTES[6].name,
            ThemePref::Latte => theme::PALETTES[7].name,
            ThemePref::Hawaiian => theme::PALETTES[8].name,
        }
    }
}

/// Keep the palette on the person's pick (the system's may change under us).
pub fn follow_theme(pref: ThemePref, ctx: &egui::Context) {
    let i = pref.palette(ctx);
    if i != theme::current() {
        theme::set_palette(ctx, i);
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Step {
    Look,
    Account,
    Claude,
    Ready,
}

impl Step {
    fn all() -> &'static [Step] {
        // No Claude step at first start: the free commands come first, and
        // the step opens on its own when they run out.
        if cfg!(target_arch = "wasm32") {
            &[Step::Look, Step::Ready]
        } else {
            &[Step::Look, Step::Account, Step::Ready]
        }
    }
    fn name(self) -> &'static str {
        match self {
            Step::Look => "Look",
            Step::Account => "Account",
            Step::Claude => "Claude",
            Step::Ready => "Ready",
        }
    }
}

/// Set from anywhere (an agent turn refused for want of a Claude): app.rs opens
/// setup at the Claude step on its next frame.
static WANT_CLAUDE: AtomicBool = AtomicBool::new(false);

/// Whether an agent's refusal is about the person's Claude - then the place to
/// fix it is setup, not a page elsewhere.
pub fn about_claude(text: &str) -> bool {
    ["Connect your own Claude", "Credentials → Claude", "Claude is no longer connected", "rejected your Claude", "shared Claude Max", "which is for subscribers", "Connect your Claude"]
        .iter()
        .any(|k| text.contains(k))
}

/// Under an agent's error: a way straight to connecting a Claude, when the
/// error is about one - and the first time such an error shows (the free
/// commands ran out), the Claude step opens by itself.
pub fn connect_button(ui: &mut egui::Ui, text: &str) {
    if !about_claude(text) {
        return;
    }
    static OPENED: Mutex<Vec<u64>> = Mutex::new(Vec::new());
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        text.hash(&mut h);
        h.finish()
    };
    let first = {
        let mut seen = OPENED.lock().unwrap();
        let new = !seen.contains(&key);
        if new {
            seen.push(key);
        }
        new
    };
    if first || ui.add(theme::primary_button("Connect your Claude")).clicked() {
        WANT_CLAUDE.store(true, Ordering::Relaxed);
    }
}

/// What a frame of setup asks app.rs to do.
pub enum Done {
    Nothing,
    /// Closed; with a toast to show.
    Closed(Option<String>),
}

/// Watching the Terminal for `claude setup-token`'s token.
#[derive(Default)]
struct Helper {
    on: bool,
    found: Option<String>,
    /// The sign-in link claude printed, once it is on the screen.
    link: Option<String>,
    started: f64,
}

pub struct Setup {
    pub open: bool,
    step: Step,
    /// Where the person stands (None: still asking). Err: could not be asked.
    status: Slot<Result<ClaudeView, String>>,
    /// An ask is out (connect, forget or status).
    busy: Arc<AtomicBool>,
    /// ... and it is a connect or a forget: the buttons wait for it.
    changing: Arc<AtomicBool>,
    /// A connect or forget pressed while a status check was out: sent when it lands.
    pending: Option<(&'static str, String)>,
    /// What the last connect / forget said, good or bad.
    note: Slot<Result<String, String>>,
    asked_at: f64,
    paste: String,
    /// They ticked "send it to my ndexr account" for a key found here.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    consent: bool,
    #[cfg(not(target_arch = "wasm32"))]
    found: Option<Vec<crate::account::FoundClaude>>,
    helper: Helper,
}

impl Default for Setup {
    fn default() -> Self {
        Setup {
            open: false,
            step: Step::Look,
            status: Default::default(),
            busy: Default::default(),
            changing: Default::default(),
            pending: None,
            note: Default::default(),
            asked_at: f64::NEG_INFINITY,
            paste: String::new(),
            consent: false,
            #[cfg(not(target_arch = "wasm32"))]
            found: None,
            helper: Helper::default(),
        }
    }
}

/// What setup needs of the rest of the app for one frame.
pub struct Env<'a> {
    pub backend: &'a Backend,
    pub ide: &'a mut crate::ide::Ide,
    pub theme: &'a mut ThemePref,
    #[cfg(not(target_arch = "wasm32"))]
    pub account: &'a crate::account::Shared,
}

impl Setup {
    /// From the start (first launch, Help › Set up Cove).
    pub fn start(&mut self) {
        self.open = true;
        self.step = Step::Look;
        self.helper = Helper::default();
    }

    /// Straight to the Claude step (the toolbar chip, an agent's refusal).
    pub fn start_claude(&mut self, ctx: &egui::Context) {
        self.open = true;
        self.step = Step::Claude;
        self.helper = Helper::default();
        self.asked_at = f64::NEG_INFINITY;
        let _ = ctx;
    }

    /// The last answer about the person's Claude (the toolbar chip).
    pub fn status(&self) -> Option<Result<ClaudeView, String>> {
        self.status.lock().unwrap().clone()
    }

    /// Ask where the person stands - at start, then every ten minutes, and
    /// at once when setup opens on the Claude step.
    pub fn refresh(&mut self, backend: &Backend, ctx: &egui::Context, every: f64) {
        let now = ctx.input(|i| i.time);
        if now - self.asked_at < every || self.busy.load(Ordering::Relaxed) {
            return;
        }
        self.asked_at = now;
        self.ask("status", String::new(), backend, ctx);
    }

    fn ask(&mut self, op: &'static str, token: String, backend: &Backend, ctx: &egui::Context) {
        if self.busy.swap(true, Ordering::Relaxed) {
            if op != "status" {
                self.pending = Some((op, token));
                self.changing.store(true, Ordering::Relaxed);
            }
            return;
        }
        if op != "status" {
            *self.note.lock().unwrap() = None;
            self.changing.store(true, Ordering::Relaxed);
        }
        let (status, busy, note, changing) = (self.status.clone(), self.busy.clone(), self.note.clone(), self.changing.clone());
        let done = move |r: Result<ClaudeView, String>| {
            // The backend puts the HTTP status before a server's words ("500 Anthropic
            // rejected this key"); the person needs only the words.
            let r = r.map_err(|e| e.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start().to_owned());
            match (&r, op) {
                (Ok(v), "connect") => *note.lock().unwrap() = Some(Ok(format!("Connected - {}", v.label))),
                (Ok(_), "forget") => *note.lock().unwrap() = Some(Ok("Removed. Your agent runs on ndexr's Claude until you connect another.".into())),
                (Err(e), "connect" | "forget") => *note.lock().unwrap() = Some(Err(e.clone())),
                _ => {}
            }
            // A failed connect leaves where they stand as it was.
            if r.is_ok() || op == "status" {
                *status.lock().unwrap() = Some(r);
            }
            busy.store(false, Ordering::Relaxed);
            changing.store(false, Ordering::Relaxed);
        };
        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = backend;
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                done(crate::account::claude(op, &token));
                ctx.request_repaint();
            });
        }
        #[cfg(target_arch = "wasm32")]
        {
            let q = match op {
                "connect" => Query::ClaudeConnect { token },
                "forget" => Query::ClaudeForget,
                _ => Query::ClaudeStatus,
            };
            backend.query(q, ctx, done);
        }
    }

    /// One frame: the dialog while open, the Terminal helper while it runs,
    /// and requests to open from elsewhere.
    pub fn show(&mut self, ctx: &egui::Context, env: Env<'_>) -> Done {
        if WANT_CLAUDE.swap(false, Ordering::Relaxed) {
            self.start_claude(ctx);
        }
        if !self.busy.load(Ordering::Relaxed) {
            if let Some((op, token)) = self.pending.take() {
                self.ask(op, token, env.backend, ctx);
            }
        } else if self.pending.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if self.helper.on {
            return self.helper_window(ctx, env);
        }
        if !self.open {
            return Done::Nothing;
        }
        if self.step == Step::Claude {
            self.refresh(env.backend, ctx, 30.0);
        }
        let w = (ctx.content_rect().width() - 32.0).clamp(300.0, 640.0);
        let frame = egui::Frame::new()
            .fill(theme::surface_hi())
            .stroke(Stroke::new(1.0, theme::pane_brd()))
            .corner_radius(14)
            .inner_margin(egui::Margin { left: 28, right: 28, top: 22, bottom: 22 })
            .shadow(egui::epaint::Shadow { offset: [0, 14], blur: 48, spread: 0, color: Color32::from_black_alpha(if theme::is_light() { 50 } else { 150 }) });
        let mut close: Option<Option<String>> = None;
        let mut env = env;
        egui::Modal::new("cove-setup".into()).frame(frame).backdrop_color(Color32::from_black_alpha(if theme::is_light() { 70 } else { 150 })).show(ctx, |ui| {
            ui.set_width(w - 56.0);
            ui.spacing_mut().item_spacing.y = 8.0;
            self.progress(ui);
            ui.add_space(6.0);
            // The step scrolls, the steps row and Back / Continue stay: zoomed
            // in on a laptop (Ctrl +) the dialog was taller than the window
            // and Continue was below it (operator, 2026-10-18).
            let room = (ctx.content_rect().height() - 190.0).max(120.0);
            egui::ScrollArea::vertical().id_salt("setup-step").max_height(room).auto_shrink([false, true]).show(ui, |ui| match self.step {
                Step::Look => self.look(ui, ctx, env.theme),
                Step::Account => self.account(ui, ctx, &mut env),
                Step::Claude => self.claude(ui, ctx, &mut env),
                Step::Ready => self.ready(ui, &env),
            });
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(4.0);
            close = self.nav(ui);
        });
        match close {
            Some(toast) => {
                self.open = false;
                Done::Closed(toast)
            }
            None => Done::Nothing,
        }
    }

    /// The steps as a row of dots and names, the current one lit.
    fn progress(&self, ui: &mut egui::Ui) {
        // The Claude step stands alone: it opens when the free commands run
        // out (or from the chip), not as a step of first start.
        let steps: &[Step] = if self.step == Step::Claude { &[Step::Claude] } else { Step::all() };
        let at = steps.iter().position(|s| *s == self.step).unwrap_or(0);
        // Narrow (a phone, or zoomed in): every name ran off the right; the
        // current one only, the others a numbered dot.
        let names = ui.available_width() >= 440.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (i, s) in steps.iter().enumerate() {
                let (c, txt) = if i < at {
                    (theme::ok(), theme::muted())
                } else if i == at {
                    (theme::primary(), theme::text_strong())
                } else {
                    (theme::hush(), theme::hush())
                };
                let (r, _) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), Sense::hover());
                if i < at {
                    ui.painter().circle_filled(r.center(), 8.0, c);
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "✓", egui::FontId::proportional(11.0), theme::surface_hi());
                } else {
                    ui.painter().circle_stroke(r.center(), 7.5, Stroke::new(1.5, c));
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, format!("{}", i + 1), egui::FontId::proportional(10.5), c);
                }
                if names || i == at {
                    ui.label(RichText::new(s.name()).size(12.0).color(txt));
                }
                if i + 1 < steps.len() {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(if names { 22.0 } else { 10.0 }, 18.0), Sense::hover());
                    ui.painter().hline(r.x_range(), r.center().y, Stroke::new(1.0, theme::pane_brd()));
                }
            }
        });
    }

    /// Back / Skip / Continue. Some(toast) closes.
    fn nav(&mut self, ui: &mut egui::Ui) -> Option<Option<String>> {
        // The Claude step stands alone: it opens when the free commands run
        // out (or from the chip), not as a step of first start.
        let steps: &[Step] = if self.step == Step::Claude { &[Step::Claude] } else { Step::all() };
        let at = steps.iter().position(|s| *s == self.step).unwrap_or(0);
        let mut out = None;
        ui.horizontal(|ui| {
            if at > 0 && ui.button("Back").clicked() {
                self.step = steps[at - 1];
            }
            if !matches!(self.step, Step::Ready | Step::Claude) && ui.add(egui::Button::new(RichText::new("Skip setup").color(theme::muted())).frame(false)).on_hover_text("Help › Set up Cove brings this back").clicked() {
                out = Some(Some("Setup skipped - Help › Set up Cove brings it back".to_owned()));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = match self.step {
                    Step::Ready => "Start using Cove",
                    Step::Claude if !self.status().is_some_and(|s| s.is_ok_and(|v| v.own())) => "Continue without",
                    _ => "Continue",
                };
                if ui.add(theme::primary_button(label).min_size(egui::vec2(120.0, 30.0))).clicked() {
                    if matches!(self.step, Step::Ready | Step::Claude) {
                        out = Some(None);
                    } else {
                        self.step = steps[at + 1];
                    }
                }
            });
        });
        out
    }

    fn title(ui: &mut egui::Ui, title: &str, lead: &str) {
        ui.label(RichText::new(title).font(theme::display(24.0)).color(theme::text_strong()));
        ui.label(RichText::new(lead).size(13.5).color(theme::muted()));
        ui.add_space(6.0);
    }

    // ── Look ────────────────────────────────────────────────────────────────
    fn look(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, pref: &mut ThemePref) {
        Self::title(ui, "Welcome to Cove", "Build and manage scientific applications - kernel to user - with an agent beside you. A minute here and it is ready for you.");
        ui.label(RichText::new("CHOOSE A LOOK").size(10.5).color(theme::hush()));
        // Three across where the names fit beside "dark", else two.
        let n = if ui.available_width() >= 520.0 { 3.0 } else { 2.0 };
        let gap = 12.0;
        let card_w = ((ui.available_width() - gap * (n - 1.0)) / n).floor();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            for p in ThemePref::ALL {
                if theme_card(ui, card_w, p, *pref == p, ctx) {
                    *pref = p;
                    follow_theme(p, ctx);
                }
            }
        });
        ui.add_space(4.0);
        ui.label(RichText::new("Change it any time under View › Theme. The terminal stays dark in every look, so programs like claude and vim draw their colours right.").size(12.0).color(theme::hush()));
    }

    // ── Account (desktop) ───────────────────────────────────────────────────
    #[cfg(not(target_arch = "wasm32"))]
    fn account(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, env: &mut Env<'_>) {
        let a = env.account.lock().unwrap().clone();
        Self::title(ui, "Your ndexr account", "Signing in connects this app to your cell - every environment in the ndexr stack - and keeps your Claude with your account, so it works the same on every machine.");
        match (&a.email, &a.waiting) {
            (Some(email), _) if !a.unverified => {
                status_card(ui, theme::ok(), "✓", &format!("Signed in as {email}"), &match &a.on_cell {
                    Some(_) => "This window runs on your cell.".to_owned(),
                    None => "This window runs on this computer; the account menu (top right) moves it to your cell.".to_owned(),
                });
            }
            (Some(_), _) => status_card(ui, theme::warn(), "!", "A sign-in is saved here, but ndexr could not be reached", "Check your connection; Cove keeps trying."),
            (None, Some(url)) => {
                status_card(ui, theme::primary(), "…", "Finish signing in in your browser", "Approve Cove on the page that opened. This window follows as soon as you do.");
                if ui.link("Open the sign-in page again").clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
            (None, None) => {
                status_card(ui, theme::primary(), "→", "Sign in with your browser", "Your browser opens ndexr's sign-in; approve Cove there and you come straight back. No password is typed into this app.");
                ui.add_space(4.0);
                if ui.add(theme::primary_button("Sign in to ndexr").min_size(egui::vec2(180.0, 34.0))).clicked() {
                    match crate::account::sign_in(env.account.clone(), ctx.clone()) {
                        Ok(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                        Err(e) => *self.note.lock().unwrap() = Some(Err(e)),
                    }
                }
                ui.label(RichText::new("No account yet? The same page signs you up - free.").size(12.0).color(theme::hush()));
            }
        }
        self.note_line(ui);
    }

    #[cfg(target_arch = "wasm32")]
    fn account(&mut self, _: &mut egui::Ui, _: &egui::Context, _: &mut Env<'_>) {
        self.step = Step::Claude;
    }

    // ── Claude ──────────────────────────────────────────────────────────────
    fn claude(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, env: &mut Env<'_>) {
        Self::title(ui, "Connect Claude", "Cove's agent runs on Claude. Use your own - a Claude Pro or Max subscription, or an Anthropic API key - and it is billed to you, with no limit from ndexr.");
        let busy = self.changing.load(Ordering::Relaxed);
        let status = self.status();
        match &status {
            None => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(theme::primary()));
                    ui.label(RichText::new("Checking your Claude…").color(theme::muted()));
                });
            }
            Some(Err(e)) => {
                #[cfg(not(target_arch = "wasm32"))]
                let signed_out = env.account.lock().unwrap().email.is_none();
                #[cfg(target_arch = "wasm32")]
                let signed_out = false;
                if signed_out {
                    status_card(ui, theme::warn(), "!", "Sign in first", "Your Claude is kept with your ndexr account. Go Back a step to sign in - or connect it later from the Claude chip in the toolbar.");
                } else {
                    status_card(ui, theme::warn(), "!", "Couldn't check your Claude here", e);
                    if ui.link("Connect it at console.ndexr.io instead").clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab("https://console.ndexr.io/#claude"));
                    }
                }
            }
            Some(Ok(v)) => self.verdict(ui, v, busy, env.backend, ctx),
        }
        let own = status.as_ref().is_some_and(|s| s.as_ref().is_ok_and(ClaudeView::own));
        if status.as_ref().is_some_and(|s| s.is_ok()) && !own {
            ui.add_space(4.0);
            self.ways(ui, ctx, env, busy);
        }
        self.note_line(ui);
    }

    /// Where they stand now, in the site's words.
    fn verdict(&mut self, ui: &mut egui::Ui, v: &ClaudeView, busy: bool, backend: &Backend, ctx: &egui::Context) {
        match v.kind.as_str() {
            "own" => {
                status_card(ui, theme::ok(), "✓", &v.label, "Your agent runs on your own Claude. ndexr doesn't meter or limit it.");
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy, egui::Button::new("Use a different one")).clicked() {
                        // Show the ways again by pretending nothing is connected.
                        if let Some(Ok(s)) = self.status.lock().unwrap().as_mut() {
                            s.kind = "none".into();
                        }
                    }
                    if ui.add_enabled(!busy, egui::Button::new(RichText::new("Remove").color(theme::alt()))).on_hover_text("forget it on ndexr - every ndexr surface stops using it").clicked() {
                        self.ask("forget", String::new(), backend, ctx);
                    }
                });
            }
            "operator" => status_card(ui, theme::ok(), "✓", &v.title, &v.detail),
            "plan" => status_card(ui, theme::ok(), "✓", &v.title, &v.detail),
            _ => {
                let (c, head, body) = trial_words(v);
                status_card(ui, c, if c == theme::ok() { "✓" } else { "!" }, &head, &body);
            }
        }
    }

    /// The ways to connect one.
    fn ways(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, env: &mut Env<'_>, busy: bool) {
        // 1. A key this computer already has (desktop): offered, never sent unasked.
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.found.is_none() {
                self.found = Some(crate::account::find_local_claude());
            }
            let found = self.found.clone().unwrap_or_default();
            for f in found.iter().take(1) {
                choice(ui, "Found on this computer", &format!("{} {} - {}", capitalise(f.what), f.masked(), f.from), |ui| {
                    ui.checkbox(&mut self.consent, RichText::new("Send it to my ndexr account so Cove's agent can use it. It is checked with Anthropic first, kept encrypted, and you can remove it any time.").size(12.0));
                    if ui.add_enabled(self.consent && !busy, theme::primary_button("Use this one")).clicked() {
                        self.ask("connect", f.value.clone(), env.backend, ctx);
                    }
                });
            }
        }
        // 2. Their subscription, through claude setup-token in Cove's terminal.
        choice(ui, "Claude Pro or Max subscription", "Cove runs claude setup-token in its Terminal. You approve in the browser, and Cove picks up the token it prints - nothing to copy.", |ui| {
            if ui.add_enabled(!busy, theme::primary_button("Start in the Terminal")).clicked() {
                env.ide.run_in_terminal("claude setup-token");
                self.helper = Helper { on: true, started: ctx.input(|i| i.time), ..Default::default() };
            }
        });
        // 3. Paste a key or token.
        choice(ui, "Paste an API key or token", "An Anthropic API key (sk-ant-api…) from console.anthropic.com, or a token from claude setup-token (sk-ant-oat…). Checked with Anthropic before it is kept.", |ui| {
            ui.horizontal(|ui| {
                let w = (ui.available_width() - 110.0).max(140.0);
                ui.add(egui::TextEdit::singleline(&mut self.paste).password(true).hint_text("sk-ant-…").desired_width(w));
                let ok = self.paste.trim().starts_with("sk-ant-");
                if ui.add_enabled(ok && !busy, theme::primary_button(if busy { "Checking…" } else { "Connect" })).clicked() {
                    let t = std::mem::take(&mut self.paste);
                    self.ask("connect", t, env.backend, ctx);
                }
            });
            if ui.link("Get an API key at console.anthropic.com").clicked() {
                ctx.open_url(egui::OpenUrl::new_tab("https://console.anthropic.com/settings/keys"));
            }
        });
        if busy {
            ui.horizontal(|ui| {
                ui.add(egui::Spinner::new().size(13.0).color(theme::primary()));
                ui.label(RichText::new("Checking with Anthropic…").size(12.0).color(theme::muted()));
            });
        }
    }

    fn note_line(&self, ui: &mut egui::Ui) {
        if let Some(n) = self.note.lock().unwrap().as_ref() {
            let (c, t) = match n {
                Ok(t) => (theme::ok(), t.as_str()),
                Err(t) => (theme::alt(), t.as_str()),
            };
            ui.label(RichText::new(t).size(12.5).color(c));
        }
    }

    // ── Ready ───────────────────────────────────────────────────────────────
    fn ready(&mut self, ui: &mut egui::Ui, env: &Env<'_>) {
        Self::title(ui, "You're set", "Here is what Cove is using. All of it can be changed later.");
        let claude = match self.status() {
            Some(Ok(v)) if v.own() => (theme::ok(), v.label.clone()),
            Some(Ok(v)) if matches!(v.kind.as_str(), "plan" | "operator") => (theme::ok(), v.title.clone()),
            Some(Ok(v)) => {
                let (c, head, _) = trial_words(&v);
                (c, head)
            }
            _ => (theme::primary(), "Free agent commands on ndexr's Claude - connect your own when they run out".to_owned()),
        };
        summary_row(ui, theme::ok(), "Look", env.theme.label());
        #[cfg(not(target_arch = "wasm32"))]
        {
            let a = env.account.lock().unwrap().clone();
            match a.email {
                Some(e) => summary_row(ui, theme::ok(), "Account", &e),
                None => summary_row(ui, theme::warn(), "Account", "not signed in - the toolbar's Sign in"),
            }
        }
        summary_row(ui, claude.0, "Claude", &claude.1);
        ui.add_space(8.0);
        ui.label(RichText::new("WHERE TO START").size(10.5).color(theme::hush()));
        for (k, v) in [
            ("Agents", "Say what you want built in your domain; the agent works beside the code and you watch it happen. Your first commands are free."),
            ("Software", "The ndexr stack's environments and tools - pick one from Environment in the toolbar."),
            ("Terminal", "A real shell in your environment; claude, vim and ranger run there too."),
        ] {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(k).strong().color(theme::text_strong()));
                ui.label(RichText::new(v).size(12.5).color(theme::muted()));
            });
        }
    }

    // ── the Terminal helper ─────────────────────────────────────────────────
    /// A small window over the Terminal while `claude setup-token` runs: what
    /// to do, and the token once it is on the screen - connected only on the
    /// person's click.
    fn helper_window(&mut self, ctx: &egui::Context, env: Env<'_>) -> Done {
        if self.helper.found.is_none() {
            self.helper.found = env.ide.terminal_oauth_token();
            // The link on screen now - a new run prints a new one.
            self.helper.link = env.ide.terminal_signin_link();
            ctx.request_repaint_after(std::time::Duration::from_millis(400));
        }
        let busy = self.changing.load(Ordering::Relaxed);
        let connected = matches!(self.note.lock().unwrap().as_ref(), Some(Ok(_)));
        let mut close = false;
        let mut back = false;
        let w = 360.0f32.min(ctx.content_rect().width() - 24.0);
        egui::Window::new(RichText::new("Connect your subscription").font(theme::display(14.0)))
            .anchor(egui::Align2::RIGHT_BOTTOM, [-18.0, -18.0])
            .collapsible(false)
            .resizable(false)
            .default_width(w)
            .frame(egui::Frame::window(&ctx.global_style()).fill(theme::surface_hi()).corner_radius(10).inner_margin(14))
            .show(ctx, |ui| {
                ui.set_width(w);
                ui.spacing_mut().item_spacing.y = 6.0;
                let step = |ui: &mut egui::Ui, n: &str, done: bool, text: &str| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(if done { "✓" } else { n }).strong().color(if done { theme::ok() } else { theme::primary() }));
                        ui.label(RichText::new(text).size(12.5).color(if done { theme::muted() } else { theme::text() }));
                    });
                };
                let got = self.helper.found.is_some();
                step(ui, "1", got || self.helper.link.is_some(), "In the Terminal, claude shows a sign-in link. Open it in your browser.");
                if let (Some(link), false) = (&self.helper.link, got) {
                    if ui.add(theme::primary_button("Open the sign-in page")).on_hover_text(link.as_str()).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(link));
                    }
                }
                step(ui, "2", got, "Approve in the browser. If it gives you a code, paste it back into the Terminal and press Enter.");
                step(ui, "3", connected, "Cove spots the token claude prints, and connects it when you say so.");
                ui.add_space(4.0);
                let found = self.helper.found.clone();
                match (&found, connected) {
                    (_, true) => {
                        ui.label(RichText::new("Connected - your agent now runs on your subscription.").color(theme::ok()));
                        if ui.add(theme::primary_button("Done")).clicked() {
                            back = true;
                        }
                    }
                    (Some(t), false) => {
                        ui.label(RichText::new(format!("Found your token {}…{}", &t[..t.len().min(14)], &t[t.len().saturating_sub(4)..])).color(theme::text_strong()));
                        ui.horizontal(|ui| {
                            if ui.add_enabled(!busy, theme::primary_button(if busy { "Checking…" } else { "Connect it" })).clicked() {
                                self.ask("connect", t.clone(), env.backend, ctx);
                            }
                            if ui.button("Not this one").clicked() {
                                self.helper.found = None;
                            }
                        });
                    }
                    (None, false) => {
                        ui.horizontal(|ui| {
                            ui.add(egui::Spinner::new().size(12.0).color(theme::primary()));
                            ui.label(RichText::new("Watching the Terminal for the token…").size(12.0).color(theme::muted()));
                        });
                        if self.helper.link.is_none() && ctx.input(|i| i.time) - self.helper.started > 25.0 {
                            ui.label(RichText::new("No sign-in link? claude may not be installed here - Paste a key instead.").size(11.5).color(theme::hush()));
                        }
                    }
                }
                if let Some(Err(e)) = self.note.lock().unwrap().as_ref() {
                    ui.label(RichText::new(e).size(12.0).color(theme::alt()));
                }
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    if !connected && ui.button("Back to setup").clicked() {
                        back = true;
                    }
                    if ui.add(egui::Button::new(RichText::new("Close").color(theme::muted())).frame(false)).clicked() {
                        close = true;
                    }
                });
            });
        if back {
            self.helper.on = false;
            self.open = true;
            self.step = if connected { Step::Ready } else { Step::Claude };
        } else if close {
            self.helper.on = false;
            self.open = false;
            return Done::Closed(connected.then(|| "Claude connected".to_owned()));
        }
        Done::Nothing
    }

    /// Setup or its helper is on screen (app.rs hides the App tab's frame,
    /// which would swallow the pointer).
    pub fn covering(&self) -> bool {
        self.open || self.helper.on
    }
}

/// The toolbar's chip: which Claude the agent runs on, at a glance; a click
/// opens setup on the Claude step.
pub fn chip(ui: &mut egui::Ui, status: Option<Result<ClaudeView, String>>) -> bool {
    let (c, text, tip) = match &status {
        None => (theme::hush(), "Claude …".to_owned(), "checking which Claude your agent runs on".to_owned()),
        Some(Err(e)) => (theme::warn(), "Claude ?".to_owned(), e.clone()),
        Some(Ok(v)) => match v.kind.as_str() {
            "own" => (theme::ok(), "Claude · yours".to_owned(), v.label.clone()),
            "plan" => (theme::ok(), "Claude · plan".to_owned(), format!("{} - {}", v.title, v.detail)),
            "operator" => (theme::ok(), "Claude · operator".to_owned(), v.detail.clone()),
            _ => match (v.seat_allowed, v.seat_left) {
                // Free use is how everyone starts: not a warning, not a question.
                (Some(true), Some(n)) if n > 0 => (theme::primary(), format!("Claude · {n} free left"), "free commands this week on ndexr's Claude - connect your own any time for no limit".to_owned()),
                (None, _) | (_, None) => (theme::primary(), "Claude · free".to_owned(), "your agent runs on ndexr's Claude - connect your own any time".to_owned()),
                _ => (theme::alt(), "Connect Claude".to_owned(), "your agent needs a Claude to run on".to_owned()),
            },
        },
    };
    let galley = ui.painter().layout_no_wrap(text, egui::FontId::proportional(11.5), theme::text());
    let size = egui::vec2(galley.size().x + 26.0, 20.0);
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let fill = if resp.hovered() { theme::surface_hi() } else { theme::surface() };
    ui.painter().rect(r, 99.0, fill, Stroke::new(1.0, c.gamma_multiply(0.6)), egui::StrokeKind::Inside);
    ui.painter().circle_filled(egui::pos2(r.left() + 10.0, r.center().y), 3.5, c);
    ui.painter().galley(egui::pos2(r.left() + 18.0, r.center().y - galley.size().y / 2.0), galley, theme::text());
    resp.on_hover_text(tip).clicked()
}

/// The words for someone with no Claude of their own: the free weekly
/// commands on the shared seat while they last, else what to do.
fn trial_words(v: &ClaudeView) -> (Color32, String, String) {
    match (v.seat_allowed, v.seat_left, v.seat_limit) {
        (Some(true), Some(left), Some(limit)) if left > 0 => (
            theme::primary(),
            format!("Free to start: {left} of {limit} agent commands left this week"),
            format!(
                "Until you connect your own, your agent runs on ndexr's shared Claude with the subscription models (Opus, Sonnet){}. Connecting your own removes the limit.",
                v.seat_resets.as_deref().map(|r| format!(" - the count resets {}", short_date(r))).unwrap_or_default()
            ),
        ),
        // Not known just now: the site's own words, never a claim about the allowance.
        (None, _, _) | (_, None, _) => (theme::warn(), v.title.clone(), v.detail.clone()),
        _ => (
            theme::alt(),
            "No free commands left this week".to_owned(),
            v.seat_message.clone().filter(|m| !m.is_empty()).unwrap_or_else(|| v.detail.clone()),
        ),
    }
}

/// "2026-10-18T05:59:59Z" → "Oct 18".
fn short_date(iso: &str) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let m: usize = iso.get(5..7).and_then(|m| m.parse().ok()).unwrap_or(0);
    let d: u32 = iso.get(8..10).and_then(|d| d.parse().ok()).unwrap_or(0);
    match M.get(m.wrapping_sub(1)) {
        Some(name) if d > 0 => format!("{name} {d}"),
        _ => iso.chars().take(10).collect(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default()
}

/// A card saying where something stands: a coloured mark, a line, a detail.
fn status_card(ui: &mut egui::Ui, c: Color32, mark: &str, head: &str, body: &str) {
    egui::Frame::new().fill(c.gamma_multiply(0.10)).stroke(Stroke::new(1.0, c.gamma_multiply(0.45))).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_top(|ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), Sense::hover());
            ui.painter().circle_filled(r.center(), 11.0, c);
            ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, mark, egui::FontId::proportional(12.5), theme::surface_hi());
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(head).strong().size(13.5).color(theme::text_strong()));
                ui.label(RichText::new(body).size(12.5).color(theme::muted()));
            });
        });
    });
}

/// One way to do it: a titled, bordered block with its own controls.
fn choice(ui: &mut egui::Ui, head: &str, body: &str, add: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new().fill(theme::surface()).stroke(Stroke::new(1.0, theme::pane_brd())).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(head).strong().size(13.5).color(theme::text_strong()));
        ui.label(RichText::new(body).size(12.5).color(theme::muted()));
        add(ui);
    });
}

fn summary_row(ui: &mut egui::Ui, c: Color32, k: &str, v: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(egui::vec2(12.0, 16.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 4.0, c);
        ui.add_sized([70.0, 16.0], egui::Label::new(RichText::new(k).color(theme::muted())));
        ui.label(RichText::new(v).color(theme::text_strong()));
    });
}

/// A look to pick: a small drawing of Cove in it - the ground, a tab strip
/// with its amber mark, two windows, a line of code - and its name.
fn theme_card(ui: &mut egui::Ui, w: f32, p: ThemePref, picked: bool, ctx: &egui::Context) -> bool {
    let h = (w * 0.5).round() + 34.0;
    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
    let painter = ui.painter();
    let ring = if picked { theme::primary() } else if resp.hovered() { theme::hush() } else { theme::pane_brd() };
    painter.rect(r, 10.0, theme::surface(), Stroke::new(if picked { 2.0 } else { 1.0 }, ring), egui::StrokeKind::Inside);
    let art = egui::Rect::from_min_size(r.min + egui::vec2(8.0, 8.0), egui::vec2(w - 16.0, h - 42.0));
    // Drawn in the look's own palette: its ground, panes, accent and three
    // lines of its code colours.
    let draw = |painter: &egui::Painter, art: egui::Rect, pl: &theme::Palette| {
        let c = theme::rgb;
        let (ground, pane, strip, line, accent, amber) = (c(pl.ground), c(pl.code), c(pl.surface), c(pl.moss), c(pl.keyword), c(pl.amber));
        let code = [c(pl.keyword), c(pl.string), c(pl.function)];
        painter.rect_filled(art, 6.0, ground);
        let g = 4.0;
        let left = egui::Rect::from_min_max(art.min + egui::vec2(g, g), egui::pos2(art.center().x - g / 2.0, art.max.y - g));
        let right = egui::Rect::from_min_max(egui::pos2(art.center().x + g / 2.0, art.min.y + g), art.max - egui::vec2(g, g));
        for (k, win) in [left, right].into_iter().enumerate() {
            painter.rect(win, 4.0, pane, Stroke::new(1.0, line), egui::StrokeKind::Inside);
            let tabs = egui::Rect::from_min_size(win.min, egui::vec2(win.width(), 8.0));
            painter.rect_filled(tabs, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, strip);
            painter.hline(tabs.min.x + 3.0..=tabs.min.x + 15.0, tabs.min.y + 1.0, Stroke::new(1.5, amber));
            for i in 0..3 {
                let y = tabs.max.y + 6.0 + i as f32 * 6.0;
                if y > win.max.y - 3.0 {
                    break;
                }
                let len = win.width() * [0.7, 0.45, 0.6][i] - 8.0;
                let c = if k == 0 { code[i] } else if i == 0 { accent } else { c(pl.muted) };
                painter.hline(win.min.x + 5.0..=win.min.x + 5.0 + len.max(4.0), y, Stroke::new(2.0, c));
            }
        }
    };
    match p {
        ThemePref::System => {
            // Half and half, split on the diagonal's simplest cousin: down the middle.
            let (l, rr) = (egui::Rect::from_min_max(art.min, egui::pos2(art.center().x, art.max.y)), egui::Rect::from_min_max(egui::pos2(art.center().x, art.min.y), art.max));
            draw(&painter.with_clip_rect(l), art, &theme::PALETTES[0]);
            draw(&painter.with_clip_rect(rr), art, &theme::PALETTES[5]);
        }
        p => draw(painter, art, &theme::PALETTES[p.palette(ctx)]),
    }
    let label_y = art.max.y + 17.0;
    let sub = match p {
        ThemePref::System => format!("now {}", if ctx.system_theme() == Some(egui::Theme::Light) { "light" } else { "dark" }),
        p if theme::PALETTES[p.palette(ctx)].light => "light".to_owned(),
        _ => "dark".to_owned(),
    };
    // The name, clipped to the card; "dark"/"light" only where it fits
    // beside it - narrow, the two were drawn over each other.
    let name = painter.layout_no_wrap(p.label().to_owned(), theme::display(13.0), theme::text_strong());
    let subg = painter.layout_no_wrap(sub, egui::FontId::proportional(11.0), theme::hush());
    let room = w - 24.0;
    let clip = painter.with_clip_rect(r.shrink(6.0));
    clip.galley(egui::pos2(r.left() + 12.0, label_y - name.size().y / 2.0), name.clone(), theme::text_strong());
    if name.size().x + 8.0 + subg.size().x <= room {
        clip.galley(egui::pos2(r.right() - 12.0 - subg.size().x, label_y - subg.size().y / 2.0), subg, theme::hush());
    }
    if picked {
        let c = egui::pos2(r.right() - 16.0, r.top() + 16.0);
        painter.circle_filled(c, 9.0, theme::primary());
        painter.text(c, egui::Align2::CENTER_CENTER, "✓", egui::FontId::proportional(11.0), theme::surface_hi());
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_refusals_about_claude_are_spotted() {
        assert!(about_claude("You're on ndexr's Claude, which is for subscribers. Connect your own Claude in console.ndexr.io (Credentials → Claude), subscribe, or add funds in billing.ndexr.io to keep building."));
        assert!(about_claude("You've used your 25 shared Claude Max commands this week. They reset Sun Oct 18 05:59 UTC."));
        assert!(about_claude("Anthropic rejected your Claude credential. Reconnect it in console.ndexr.io (Credentials → Claude)."));
        assert!(!about_claude("cove.ndexr.io is not one of your domains"));
    }

    #[test]
    fn dates_are_short() {
        assert_eq!(short_date("2026-10-18T05:59:59Z"), "Oct 18");
        assert_eq!(short_date("nonsense"), "nonsense");
    }
}
