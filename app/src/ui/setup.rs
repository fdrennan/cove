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
//!   Fit      how a site's parts belong together - you ask its agent, the
//!            agent writes its app, the app answers at its address - and
//!            which tab holds which (operator, 2026-11-26: "when cove starts
//!            up, we have to explain the link between all of those").
//!   Account  desktop only: sign in to ndexr (account.rs). A browser on
//!            cove.ndexr.io is signed in already.
//!   Claude   where the person stands - the site's own verdict and words
//!            (credentials.ndexr.io/connect.r over claude_access.r) - and the
//!            ways to connect their own: a key already on this computer (sent
//!            only once they tick that they want it sent), their Pro/Max
//!            subscription (Cove runs `claude setup-token` in the Terminal and
//!            picks the token off the screen), or a pasted key. Or not now:
//!            the free weekly commands on ndexr's shared Claude.
//!   Stack    where R, Python and every module come from (operator,
//!            2026-11-13: "cove.ndexr.io desktop version and server version
//!            should go through inital setup of installing the stack
//!            themselves or consuming it through cvmfs"): CernVM-FS
//!            (`ndexr install cvmfs`), a build here from source (`ndexr stack
//!            up` into ~/.ndexr/stack), or airgapped - a copy brought to this
//!            machine, and Cove asks nothing of the network (operator, the
//!            same day: "cove.ndexr.io should have airgap mode"). The server
//!            version answers for its own machine, through the same queries.
//!   Ready    what was set, and where to start.
//!
//! The credential goes where every ndexr surface reads it: the person's
//! "<email>-claude" secret, through `cove multiuser` on cove.ndexr.io and
//! through api.ndexr.io from the desktop app.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align, Color32, CornerRadius, Layout, RichText, Sense, Stroke};
use serde::{Deserialize, Serialize};

use crate::api::{ClaudeView, JobView, Query, SettingsView};
use crate::backend::Backend;
use crate::ui::kit::theme;

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
    Fit,
    Stack,
    Account,
    Claude,
    Ready,
}

impl Step {
    fn all() -> &'static [Step] {
        // No Claude step at first start: the free commands come first, and
        // the step opens on its own when they run out.
        if cfg!(target_arch = "wasm32") {
            &[Step::Look, Step::Fit, Step::Stack, Step::Ready]
        } else {
            &[Step::Look, Step::Fit, Step::Stack, Step::Account, Step::Ready]
        }
    }
    fn name(self) -> &'static str {
        match self {
            Step::Look => "Look",
            Step::Fit => "How it fits",
            Step::Stack => "Stack",
            Step::Account => "Account",
            Step::Claude => "Claude",
            Step::Ready => "Ready",
        }
    }
}

/// Set from anywhere (an agent turn refused for want of a Claude): app.rs opens
/// setup at the Claude step on its next frame.
static WANT_CLAUDE: AtomicBool = AtomicBool::new(false);
/// The same for signing in to ndexr (backend.rs SIGNED_OUT).
static WANT_SIGN_IN: AtomicBool = AtomicBool::new(false);

/// Whether an agent's refusal is about the person's Claude - then the place to
/// fix it is setup, not a page elsewhere.
pub fn about_claude(text: &str) -> bool {
    ["Connect your own Claude", "Credentials → Claude", "Claude is no longer connected", "rejected your Claude", "shared Claude Max", "which is for subscribers", "Connect your Claude"]
        .iter()
        .any(|k| text.contains(k))
}

/// When this Cove started (Unix seconds): what happened before is history.
pub fn session_start() -> i64 {
    static AT: std::sync::OnceLock<i64> = std::sync::OnceLock::new();
    *AT.get_or_init(crate::ui::software::now_secs)
}

/// Under an agent's error: a way straight to connecting a Claude, when the
/// error is about one - and the first time such an error shows (the free
/// commands ran out), the Claude step opens by itself. Only `fresh` errors -
/// a turn sent since this Cove started - open it: an old refusal in the
/// thread's history, or the Account tab's own button, opened Connect Claude
/// at every start with Claude connected (operator, 2026-11-19).
pub fn connect_button(ui: &mut egui::Ui, text: &str, fresh: bool) {
    if text.contains(crate::backend::SIGNED_OUT) {
        if ui.add(theme::primary_button("Sign in to ndexr")).clicked() {
            WANT_SIGN_IN.store(true, Ordering::Relaxed);
        }
        return;
    }
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
    if (first && fresh) || ui.add(theme::primary_button("Connect your Claude")).clicked() {
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
    found: Option<Vec<crate::host::account::FoundClaude>>,
    helper: Helper,
    /// The Stack step: its setup or build job as the machine last told it,
    /// and when it and the settings were last asked.
    job: Arc<Mutex<JobView>>,
    job_busy: Arc<AtomicBool>,
    job_asked: f64,
    stack_asked: f64,
    /// The airgapped copy's folder, as typed; and what saving it said.
    airgap_dir: Option<String>,
    airgap_note: Slot<Result<String, String>>,
    /// The stack as the Stack step saw it this frame, for the mount dialog,
    /// drawn after the setup dialog rather than inside it: a modal opened
    /// inside another took turns with it on top every frame, and the two
    /// flickered together too fast to read (operator, 2026-11-19, a new
    /// system).
    consent_for: Option<crate::api::StackView>,
}

impl Default for Setup {
    fn default() -> Self {
        session_start();
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
            job: Default::default(),
            job_busy: Default::default(),
            job_asked: f64::NEG_INFINITY,
            stack_asked: f64::NEG_INFINITY,
            airgap_dir: None,
            airgap_note: Default::default(),
            consent_for: None,
        }
    }
}

/// What setup needs of the rest of the app for one frame.
pub struct Env<'a> {
    pub backend: &'a Backend,
    pub ide: &'a mut crate::ui::ide::Ide,
    pub theme: &'a mut ThemePref,
    /// The app's own copy of the machine's settings and stack: what the Stack
    /// step reads, and where its saves land, so Global Options sees them.
    pub settings: &'a Slot<SettingsView>,
    #[cfg(not(target_arch = "wasm32"))]
    pub account: &'a crate::host::account::Shared,
}

impl Setup {
    /// From the start (first launch, Help › Set up Cove).
    pub fn start(&mut self) {
        self.open = true;
        self.step = Step::Look;
        self.helper = Helper::default();
        self.stack_asked = f64::NEG_INFINITY;
        self.job_asked = f64::NEG_INFINITY;
    }

    /// Straight to the Stack step: a browser's first visit to a server with
    /// no stack (app.rs), where the rest of first start is skipped.
    pub fn start_stack(&mut self) {
        self.start();
        self.step = Step::Stack;
    }

    /// Straight to the Stack step with a stack job that ran elsewhere (the
    /// banner's background setup): its whole output and why it failed show
    /// there, beside the ways to try again. The banner's Details opened
    /// Global Options, which says nothing about a failure (operator's
    /// screenshot, 2026-11-14: "details sends us to this which is confusing").
    pub fn show_job(&mut self, job: JobView) {
        self.start_stack();
        *self.job.lock().unwrap() = job;
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
                done(crate::host::account::claude(op, &token));
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
        if WANT_SIGN_IN.swap(false, Ordering::Relaxed) {
            #[cfg(not(target_arch = "wasm32"))]
            match crate::host::account::sign_in(env.account.clone(), ctx.clone()) {
                Ok(url) => crate::ui::kit::links::new_tab(ctx, url),
                Err(e) => *self.airgap_note.lock().unwrap() = Some(Err(e)),
            }
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
        let mut build_now = false;
        let asking = *ASK_CVMFS.lock().unwrap() == Some("setup");
        let mut mount = false;
        if self.step != Step::Stack {
            self.consent_for = None;
        }
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
                Step::Fit => fit(ui),
                // The mount ask, when it is open, is this step's content (one dialog).
                Step::Stack if asking && self.consent_for.is_some() => {
                    let s = self.consent_for.clone().unwrap_or_default();
                    mount = cvmfs_consent_inline(ui, ctx, "setup", &s);
                }
                Step::Stack => self.stack(ui, ctx, &mut env),
                Step::Account => self.account(ui, ctx, &mut env),
                Step::Claude => self.claude(ui, ctx, &mut env),
                Step::Ready => {
                    if self.ready(ui, &mut env) {
                        build_now = true;
                    }
                }
            });
            ui.add_space(10.0);
            ui.separator();
            ui.add_space(4.0);
            close = self.nav(ui);
        });
        if build_now {
            // The New project dialog, then the starters: what the welcome
            // tour opens too.
            env.ide.start_new_project();
            close = Some(None);
        }
        if mount {
            #[cfg(not(target_arch = "wasm32"))]
            crate::host::account::set_stack_mode(crate::api::StackMode::Cvmfs);
            self.stack_job(Query::StackSetup, "Setting up the ndexr stack", ctx, &mut env);
        }
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

    // ── Stack ───────────────────────────────────────────────────────────────
    fn stack(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, env: &mut Env<'_>) {
        self.stack_refresh(ctx, env);
        Self::title(ui, "The ndexr stack", "R, Python and every tool Cove runs come from the ndexr stack, built kernel to user and the same on every machine. Bring it to this one.");
        let Some(v) = env.settings.lock().unwrap().clone() else {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Looking for the stack on this machine");
            });
            return;
        };
        let s = &v.stack;
        let job = self.job.lock().unwrap().clone();
        if !s.unsupported.is_empty() {
            status_card(ui, theme::alt(), "i", "The stack runs on Linux", &s.unsupported);
            return;
        }
        let air = v.settings.airgap;
        // The three ways, chosen here on the desktop (api.rs StackMode).
        // Below them: the options for the one chosen.
        self.consent_for = Some(s.clone());
        #[cfg(not(target_arch = "wasm32"))]
        let mode = self.mode_cards(ui, ctx, env, &v);
        #[cfg(target_arch = "wasm32")]
        let mode: Option<crate::api::StackMode> = None;
        let show = |m: crate::api::StackMode| mode.is_none() || mode == Some(m);
        if s.ok {
            let rev = if s.revision.is_empty() { String::new() } else { format!(", revision {}", s.revision) };
            status_card(ui, theme::ok(), "✓", "The stack is on this machine", &format!("{}{rev}", s.root));
        } else if air {
            status_card(ui, theme::warn(), "!", "Airgap mode is on, but no stack is here yet", &format!("{} has no use.sh with this machine's modules ({}). Copy the stack there, then Check again.", s.root, s.mine));
        } else if s.building {
            status_card(ui, theme::primary(), "…", "A build of the stack is under way here", &format!("Cove uses it once it is there. Follow it in a terminal: ndexr stack logs --dir {} -f", s.build_dir));
        }
        ui.add_space(4.0);
        let running = job.running;
        if !s.ok && !air && show(crate::api::StackMode::Cvmfs) {
            choice(ui, "Set up CernVM-FS", "Mounts stack.ndexr.io read-only at /opt/ndexr/stack. Needs root once.", |ui| {
                if s.root_by.is_empty() {
                    ui.label(RichText::new("No root here without a prompt Cove can show. Run this once in a terminal, then Check again:").size(12.5).color(theme::text_strong()));
                    code_line(ui, &s.setup_cmd);
                } else if ui.add_enabled(!running, theme::primary_button("Set up CernVM-FS…")).clicked() {
                    ask_cvmfs("setup");
                }
            });
        }
        if !s.ok && !air && show(crate::api::StackMode::Installed) {
            // The one answer (api.rs Settings::report), shown and changed here.
            let mut report = v.settings.report == Some(true);
            let mut changed = false;
            let mut start = false;
            choice(ui, "Experimental: let Cove build it for you", &format!("Cove runs the whole build here from upstream sources into {}: no root, no ndexr servers. Plan for 150-250 GB of disk and most of a day; Cove uses it as soon as it is built. This build is experimental and some steps may fail.", s.build_dir), |ui| {
                changed = ui.checkbox(&mut report, RichText::new("Send build errors to ndexr.io. We fix them in Cove and the stack, and your build reruns on the release that has the fix.").size(12.5).color(theme::text_strong()))
                    .on_hover_text("Only failures, and only once each: the step and recipe that failed, its exit code and error excerpt, and the machine's shape (OS, arch, kernel, glibc, cores, memory). User, host and domain names, paths, emails and IP addresses are scrubbed first. Untick to keep everything on this machine.")
                    .changed();
                if ui.add_enabled(!running && !s.building, egui::Button::new("Allow the experimental build")).clicked() {
                    start = true;
                }
            });
            if changed {
                let mut st = v.settings.clone();
                st.report = Some(report);
                let slot = env.settings.clone();
                if let Some(view) = slot.lock().unwrap().as_mut() {
                    view.settings.report = Some(report);
                }
                env.backend.query(Query::SaveSettings { settings: st }, ctx, move |r: Result<SettingsView, String>| {
                    if let Ok(v) = r {
                        *slot.lock().unwrap() = Some(v);
                    }
                });
            }
            if start {
                self.stack_job(Query::StackBuild { report }, if report { "Building the ndexr stack here (errors go to ndexr.io)" } else { "Building the ndexr stack here" }, ctx, env);
            }
        }
        let dir = self.airgap_dir.get_or_insert_with(|| if v.settings.stack_dir.trim().is_empty() { s.build_dir.clone() } else { v.settings.stack_dir.clone() });
        let mut save: Option<(bool, Option<String>)> = None;
        // With the stack here, airgap is an advanced choice, not a question
        // for a first start: Global Options › Environment keeps it.
        if show(crate::api::StackMode::Installed) && (!s.ok || air) {
        choice(
            ui,
            "Airgapped",
            "No network at all: no update checks, no software catalog, no ndexr sign-in. Cove uses a copy of the stack you bring to this machine - a folder with use.sh, from `ndexr stack clone` on a connected machine of the same platform. The agent still needs a Claude it can reach.",
            |ui| {
                if air {
                    ui.label(RichText::new(format!("On. The stack is read from {}.", s.root)).size(12.5).color(theme::text_strong()));
                    if ui.add(egui::Button::new("Turn airgap off")).clicked() {
                        save = Some((false, None));
                    }
                } else {
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(dir).font(egui::TextStyle::Monospace).hint_text("/path/with/use.sh").desired_width((ui.available_width() - 190.0).max(120.0)));
                        if ui.add_enabled(dir.trim().starts_with('/'), egui::Button::new("Use this copy, airgapped")).clicked() {
                            save = Some((true, Some(dir.trim().to_owned())));
                        }
                    });
                }
            },
        );
        }
        if let Some((on, d)) = save {
            self.save_airgap(on, d, ctx, env);
        }
        if let Some(n) = self.airgap_note.lock().unwrap().clone() {
            let (c, t) = match n {
                Ok(t) => (theme::ok(), t),
                Err(e) => (theme::warn(), e),
            };
            ui.label(RichText::new(t).size(12.5).color(c));
        }
        if !job.what.is_empty() {
            ui.add_space(4.0);
            egui::Frame::new().fill(theme::surface()).stroke(Stroke::new(1.0, theme::pane_brd())).corner_radius(10).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    match job.ok {
                        _ if job.running => {
                            ui.spinner();
                        }
                        Some(true) => {
                            ui.label(RichText::new("✓").color(theme::ok()));
                        }
                        _ => {
                            ui.label(RichText::new("✗").color(theme::warn()));
                        }
                    }
                    ui.label(RichText::new(&job.what).strong().color(theme::text_strong()));
                });
                // Failed: the reason first, in words, above the log - it was
                // scrolled past by the diagnostics that follow it (operator,
                // 2026-11-19, a fresh Gentoo: "still issues").
                if !job.running && job.ok == Some(false) {
                    let why = job.why();
                    if !why.is_empty() {
                        ui.add(egui::Label::new(RichText::new(why).size(13.0).strong().color(theme::warn())).wrap());
                        ui.add_space(2.0);
                    }
                }
                // The whole output, scrolled to its end: the reason a setup
                // failed is often well above its last line.
                theme::log_scroll(egui::ScrollArea::vertical().id_salt("setup-job-log").max_height(260.0).stick_to_bottom(true), ui, |ui| {
                    theme::selectable_lines(ui, &job.lines, egui::FontId::monospace(11.5), theme::text_strong(), true);
                });
            });
        }
        ui.horizontal(|ui| {
            if !s.ok && ui.add_enabled(!running, egui::Button::new("Check again")).clicked() {
                self.stack_asked = f64::NEG_INFINITY;
            }
            if !s.ok {
                ui.label(RichText::new("Or skip: the consoles wait for the stack, and Global Options › Environment brings it later. The Terminal works without it.").size(12.5).color(theme::text_strong()));
            }
        });
    }

    /// The three ways this computer can get the stack, each saying whether
    /// it is the one chosen (operator, 2026-11-14: "all three must be allowed
    /// and for the user to choose which mode (installed local, cvmfs local, or
    /// remote)"). Returns the mode chosen, if any.
    #[cfg(not(target_arch = "wasm32"))]
    fn mode_cards(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, env: &mut Env<'_>, v: &SettingsView) -> Option<crate::api::StackMode> {
        use crate::api::StackMode;
        let now = crate::host::account::stack_mode();
        let s = &v.stack;
        let cvmfs_here = s.ok && s.root.starts_with("/opt/ndexr/stack");
        let installed_here = s.ok && !cvmfs_here;
        let signed_in = env.account.lock().unwrap().email.is_some();
        let mut pick: Option<StackMode> = None;
        // CernVM-FS is the default (operator, 2026-11-14: "the default should
        // be to mount cvmfs, advanced is building it locally"): it leads,
        // carries the primary button, and the build sits under Advanced.
        let mut card = |ui: &mut egui::Ui, m: StackMode| {
            let (title, body, button) = match m {
                StackMode::Installed => (
                    "Build it on this computer",
                    if installed_here { format!("The stack in {} - built here or copied here. Works with no connection to ndexr.io; Builds and Software follow the install.", s.root) } else { "Build the stack here from source (150-250 GB, most of a day), or use a copy you bring. Works with no connection to ndexr.io; Builds and Software follow the install.".to_owned() },
                    "Build or bring it here",
                ),
                StackMode::Cvmfs => (
                    "Mount it with CernVM-FS",
                    if cvmfs_here { format!("stack.ndexr.io is mounted at {}, fetched as it is used.", s.root) } else { "Our prebuilt stack.ndexr.io, mounted read-only at /opt/ndexr/stack and fetched as it is used. Needs root once, and the network.".to_owned() },
                    "Mount CernVM-FS",
                ),
                StackMode::Remote => (
                    "ndexr's stack, remotely",
                    if signed_in { "Cove runs on your cell, with the stack there; this computer needs none of its own.".to_owned() } else { "Sign in to ndexr: Cove runs on your cell, with the stack there; this computer needs none of its own.".to_owned() },
                    if signed_in { "Use my cell" } else { "Sign in and use my cell" },
                ),
            };
            let on = now == Some(m);
            egui::Frame::new()
                .fill(if on { theme::surface_hi() } else { theme::surface() })
                .stroke(Stroke::new(if on { 2.0 } else { 1.0 }, if on { theme::primary() } else { theme::pane_brd() }))
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(title).size(14.5).strong().color(theme::text_strong()));
                        if on {
                            theme::state_mark(ui, theme::State::Good, "chosen");
                        } else if m == StackMode::Cvmfs {
                            theme::state_mark(ui, theme::State::Active, "default");
                        }
                    });
                    ui.add(egui::Label::new(RichText::new(body).size(12.5).color(theme::text())).wrap());
                    let b = if m == StackMode::Cvmfs { theme::primary_button(button) } else { egui::Button::new(button) };
                    if !on && ui.add(b.min_size(egui::vec2(0.0, 30.0))).clicked() {
                        pick = Some(m);
                    }
                });
            ui.add_space(6.0);
        };
        card(ui, StackMode::Cvmfs);
        card(ui, StackMode::Remote);
        egui::CollapsingHeader::new(RichText::new("Advanced: build the stack on this computer").size(13.0).color(theme::text_strong()))
            .id_salt("setup-advanced-installed")
            .default_open(now == Some(StackMode::Installed))
            .show(ui, |ui| card(ui, StackMode::Installed));
        // CernVM-FS still to set up: chosen once they agree to what it does
        // to this computer (cvmfs_consent, in the step below).
        if pick == Some(StackMode::Cvmfs) && !cvmfs_here && !s.root_by.is_empty() {
            ask_cvmfs("setup");
            pick = None;
        }
        if let Some(m) = pick {
            crate::host::account::set_stack_mode(m);
            if m == StackMode::Remote && !signed_in {
                match crate::host::account::sign_in(env.account.clone(), ctx.clone()) {
                    Ok(url) => crate::ui::kit::links::new_tab(ctx, url),
                    Err(e) => *self.airgap_note.lock().unwrap() = Some(Err(e)),
                }
            }
            self.stack_asked = f64::NEG_INFINITY;
        }
        if now.is_none() && pick.is_none() {
            ui.label(RichText::new("Mount CernVM-FS to go on, or choose another way. Cove asks again at every start until you do.").size(12.5).color(theme::State::Attention.color()));
        }
        // Nothing chosen yet: the options shown below are the default's.
        pick.or(now).or(Some(StackMode::Cvmfs))
    }

    /// The settings (and the stack in them) when the step opens or Check
    /// again is pressed; the job at once, then every second while it runs,
    /// and the settings again when it ends.
    fn stack_refresh(&mut self, ctx: &egui::Context, env: &Env<'_>) {
        let now = ctx.input(|i| i.time);
        if self.stack_asked == f64::NEG_INFINITY {
            self.stack_asked = now;
            let slot = env.settings.clone();
            env.backend.query(Query::Settings, ctx, move |r: Result<SettingsView, String>| {
                if let Ok(v) = r {
                    *slot.lock().unwrap() = Some(v);
                }
            });
        }
        let running = self.job.lock().unwrap().running;
        if running {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        if !(running || self.job_asked == f64::NEG_INFINITY) || now - self.job_asked < 1.0 || self.job_busy.swap(true, Ordering::Relaxed) {
            return;
        }
        self.job_asked = now;
        let (job, busy, slot, backend, ctx2) = (self.job.clone(), self.job_busy.clone(), env.settings.clone(), env.backend.clone(), ctx.clone());
        env.backend.query(Query::StackJob, ctx, move |r: Result<JobView, String>| {
            if let Ok(v) = r {
                let ended = !v.running && job.lock().unwrap().running;
                *job.lock().unwrap() = v;
                if ended {
                    backend.query(Query::Settings, &ctx2, move |r: Result<SettingsView, String>| {
                        if let Ok(v) = r {
                            *slot.lock().unwrap() = Some(v);
                        }
                    });
                }
            }
            busy.store(false, Ordering::Relaxed);
        });
    }

    /// Start a stack setup or build on the machine; its progress is polled.
    fn stack_job(&mut self, q: Query, what: &str, ctx: &egui::Context, env: &Env<'_>) {
        *self.job.lock().unwrap() = JobView { what: what.into(), running: true, ..Default::default() };
        // A poll already out may answer from before the start: let it land first.
        self.job_asked = ctx.input(|i| i.time);
        let job = self.job.clone();
        env.backend.query(q, ctx, move |r: Result<JobView, String>| match r {
            Ok(v) => *job.lock().unwrap() = v,
            Err(e) => {
                let mut j = job.lock().unwrap();
                j.running = false;
                j.ok = Some(false);
                j.lines.push(e);
            }
        });
    }

    /// Turn airgap mode on with the stack copy at `dir`, or off. The machine
    /// says afterwards whether a stack is really there.
    fn save_airgap(&mut self, on: bool, dir: Option<String>, ctx: &egui::Context, env: &Env<'_>) {
        let Some(mut s) = env.settings.lock().unwrap().as_ref().map(|v| v.settings.clone()) else {
            *self.airgap_note.lock().unwrap() = Some(Err("still reading this machine's settings - try again in a moment".into()));
            return;
        };
        s.airgap = on;
        if let Some(d) = dir {
            s.stack_dir = d;
        }
        let (slot, note) = (env.settings.clone(), self.airgap_note.clone());
        *note.lock().unwrap() = None;
        env.backend.query(Query::SaveSettings { settings: s }, ctx, move |r: Result<SettingsView, String>| {
            let n = match &r {
                Ok(v) if !v.settings.airgap => Ok("Airgap mode is off.".to_owned()),
                Ok(v) if v.stack.ok => Ok(format!("Airgapped, with the stack at {}.", v.stack.root)),
                Ok(v) => Err(format!("Airgap mode is on, but {} has no use.sh with {}'s modules yet.", v.stack.root, v.stack.mine)),
                Err(e) => Err(format!("not saved: {e}")),
            };
            *note.lock().unwrap() = Some(n);
            if let Ok(v) = r {
                *slot.lock().unwrap() = Some(v);
            }
        });
    }

    // ── Account (desktop) ───────────────────────────────────────────────────
    #[cfg(not(target_arch = "wasm32"))]
    fn account(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, env: &mut Env<'_>) {
        if airgapped(env) {
            Self::title(ui, "Your ndexr account", "Signing in connects this app to your cell and keeps your Claude with your account.");
            status_card(ui, theme::alt(), "i", "Airgap mode: no sign-in", "Cove talks to no ndexr server. Turn airgap off on the Stack step (Back) to sign in.");
            return;
        }
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
                    crate::ui::kit::links::new_tab(ctx, url);
                }
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            }
            (None, None) => {
                status_card(ui, theme::primary(), "→", "Sign in with your browser", "Your browser opens ndexr's sign-in; approve Cove there and you come straight back. No password is typed into this app.");
                ui.add_space(4.0);
                if ui.add(theme::primary_button("Sign in to ndexr").min_size(egui::vec2(180.0, 34.0))).clicked() {
                    match crate::host::account::sign_in(env.account.clone(), ctx.clone()) {
                        Ok(url) => crate::ui::kit::links::new_tab(ctx, url),
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
                    // A Claude signed in on this computer is used as it is
                    // (localagent.rs) - the status then says so. Here there is none.
                    status_card(ui, theme::warn(), "!", "No Claude on this computer yet", "Install Claude Code and sign in to it - run `claude` in the Terminal - and Cove's agent uses it here, with no ndexr account. Or Go Back a step and sign in to ndexr to keep your Claude with your account.");
                } else {
                    status_card(ui, theme::warn(), "!", "Couldn't check your Claude here", e);
                    if ui.link("Connect it at console.ndexr.io instead").clicked() {
                        crate::ui::kit::links::new_tab(ctx, "https://console.ndexr.io/#claude");
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
                self.found = Some(crate::host::account::find_local_claude());
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
                crate::ui::kit::links::new_tab(ctx, "https://console.anthropic.com/settings/keys");
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
    /// True when the person picks Build something: setup closes onto a new
    /// project and its starters (ide start_new_project), the first decision a
    /// new box asks.
    fn ready(&mut self, ui: &mut egui::Ui, env: &mut Env<'_>) -> bool {
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
        let stack = match env.settings.lock().unwrap().as_ref() {
            Some(v) if v.settings.airgap && v.stack.ok => (theme::ok(), format!("airgapped - {}", v.stack.root)),
            Some(v) if v.settings.airgap => (theme::warn(), format!("airgapped - no stack at {}", v.stack.root)),
            Some(v) if v.stack.ok => (theme::ok(), v.stack.root.clone()),
            Some(v) if !v.stack.unsupported.is_empty() => (theme::alt(), "Linux only - this computer's own R, Python and shell".to_owned()),
            Some(v) if v.stack.building || self.job.lock().unwrap().running => (theme::primary(), "on its way - Cove uses it once it is there".to_owned()),
            _ => (theme::warn(), "not set up - the consoles wait for it".to_owned()),
        };
        summary_row(ui, stack.0, "Stack", &stack.1);
        #[cfg(not(target_arch = "wasm32"))]
        {
            let a = env.account.lock().unwrap().clone();
            match a.email {
                Some(e) => summary_row(ui, theme::ok(), "Account", &e),
                None => summary_row(ui, theme::warn(), "Account", "not signed in - the toolbar's Sign in"),
            }
        }
        summary_row(ui, claude.0, "Claude", &claude.1);
        ui.add_space(10.0);
        // One next step, not a list of places (the same day: "there
        // shouldnt be ambiguity once its started"): build something, or
        // look around first.
        let mut build = false;
        let w = ui.available_width();
        if next_card(ui, w, true, "Build something", "Pick a starter - a dashboard, a model, an API, a site - in R, Python, Julia, Rust or JavaScript. It opens running, with its agent beside it.") {
            build = true;
        }
        ui.add_space(6.0);
        ui.label(RichText::new("Or Start using Cove to look around first: the Agents tab, the consoles and the Terminal are ready.").size(12.5).color(theme::text()));
        build
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
                        crate::ui::kit::links::new_tab(ctx, link);
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

/// A wide, inviting card for the next step: its title strong, what it does
/// under it, the whole card the button. `lead` paints it in the accent.
fn next_card(ui: &mut egui::Ui, w: f32, lead: bool, title: &str, what: &str) -> bool {
    let h = if crate::ui::kit::device::coarse() { 86.0 } else { 74.0 };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), Sense::click());
    let p = ui.painter();
    let hot = resp.hovered();
    let fill = if lead { theme::primary().gamma_multiply(if hot { 0.26 } else { 0.18 }) } else { theme::surface() };
    p.rect(rect, 12.0, fill, Stroke::new(1.0, if lead { theme::primary().gamma_multiply(0.7) } else { theme::pane_brd() }), egui::StrokeKind::Inside);
    // A small mark: the starters' spark, drawn.
    let c = rect.left_center() + egui::vec2(30.0, 0.0);
    for k in 0..8 {
        let a = std::f32::consts::TAU * k as f32 / 8.0;
        let (r0, r1) = if k % 2 == 0 { (5.0, 13.0) } else { (5.0, 8.5) };
        p.line_segment([c + egui::vec2(a.cos(), a.sin()) * r0, c + egui::vec2(a.cos(), a.sin()) * r1], Stroke::new(2.0, theme::primary()));
    }
    let x = rect.left() + 58.0;
    p.text(egui::pos2(x, rect.top() + 16.0), egui::Align2::LEFT_TOP, title, theme::display(17.0), theme::text_strong());
    let job = egui::text::LayoutJob::simple(what.to_owned(), egui::FontId::proportional(12.5), theme::text(), rect.right() - x - 40.0);
    let g = ui.fonts_mut(|f| f.layout_job(job));
    p.galley(egui::pos2(x, rect.top() + 40.0), g, theme::text());
    p.text(rect.right_center() - egui::vec2(18.0, 0.0), egui::Align2::RIGHT_CENTER, "→", egui::FontId::proportional(20.0), theme::primary());
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// The toolbar's chip: which Claude the agent runs on, at a glance; a click
/// opens setup on the Claude step.
pub fn chip(ui: &mut egui::Ui, status: Option<Result<ClaudeView, String>>) -> bool {
    let (c, text, tip) = match &status {
        None => (theme::hush(), "Claude …".to_owned(), "checking which Claude your agent runs on".to_owned()),
        Some(Err(e)) => (theme::warn(), "Claude ?".to_owned(), e.clone()),
        // Cove credit, when the agent runs on ndexr's Claude: the balance, and
        // a click goes to buy more (billing.ndexr.io), then back.
        Some(Ok(v)) if !matches!(v.kind.as_str(), "own" | "operator") && v.credit_usd.is_some_and(|c| c > 0.0) => {
            let usd = v.credit_usd.unwrap_or(0.0);
            let hours = v.credit_hours.clone().unwrap_or_default();
            let c = if usd < 1.0 { theme::warn() } else { theme::ok() };
            let r = credit_chip(ui, c, &format!("Cove credit · ${usd:.2}"), &format!("{hours} of building on Sonnet - click to add credit"));
            if r {
                crate::ui::kit::links::new_tab(ui.ctx(), crate::api::CREDIT_URL);
            }
            return false;
        }
        Some(Ok(v)) if v.kind == "none" && !v.seat_allowed.unwrap_or(false) => {
            if credit_chip(ui, theme::alt(), "Add Cove credit", "pay ahead and build at once on Sonnet - or connect your own Claude in setup") {
                crate::ui::kit::links::new_tab(ui.ctx(), crate::api::CREDIT_URL);
            }
            return false;
        }
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

/// A chip like `chip`'s, drawn the same way; true when clicked.
fn credit_chip(ui: &mut egui::Ui, c: Color32, text: &str, tip: &str) -> bool {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), egui::FontId::proportional(11.5), theme::text());
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

/// Whether the machine is in airgap mode (as the app last heard).
#[cfg(not(target_arch = "wasm32"))]
fn airgapped(env: &Env<'_>) -> bool {
    env.settings.lock().unwrap().as_ref().is_some_and(|v| v.settings.airgap)
}

/// Who asked to set the stack up and is waiting on the person's yes: "setup"
/// (the first-start dialog) or "app" (the banner and Global Options). Each
/// draws the dialog only for its own ask, and acts on its own yes.
static ASK_CVMFS: Mutex<Option<&'static str>> = Mutex::new(None);

/// Ask before mounting the stack (operator, 2026-11-14: "when installing the
/// stack, the users need more of a modal explaining what they are doing, why
/// root is used, etc"). Nothing starts until `cvmfs_consent` says yes.
pub fn ask_cvmfs(who: &'static str) {
    *ASK_CVMFS.lock().unwrap() = Some(who);
}

/// The dialog for `who`'s ask, if it is open: what setting up CernVM-FS
/// does to this computer, why it needs root once, how root is asked for,
/// what it leaves alone and how to undo it - each true of stages/cvmfs.sh,
/// the script `ndexr install cvmfs` runs. True on the frame they agree.
pub fn cvmfs_consent(ctx: &egui::Context, who: &'static str, s: &crate::api::StackView) -> bool {
    if *ASK_CVMFS.lock().unwrap() != Some(who) {
        return false;
    }
    let (mut yes, mut no) = (false, false);
    let w = (ctx.content_rect().width() - 32.0).clamp(300.0, 600.0);
    egui::Modal::new(egui::Id::new("cvmfs-consent")).show(ctx, |ui| {
        ui.set_width(w);
        (yes, no) = consent_body(ui, ctx, s);
    });
    if yes || no {
        *ASK_CVMFS.lock().unwrap() = None;
    }
    yes
}

/// The same ask inside a dialog already open - setup's own, in place of its
/// Stack step. Two dialogs at once took turns on top of each other every
/// frame, too fast to read (operator, 2026-11-19, and again 2026-11-21 in a
/// browser): there is one now. True on the frame they agree.
pub fn cvmfs_consent_inline(ui: &mut egui::Ui, ctx: &egui::Context, who: &'static str, s: &crate::api::StackView) -> bool {
    if *ASK_CVMFS.lock().unwrap() != Some(who) {
        return false;
    }
    let (yes, no) = consent_body(ui, ctx, s);
    if yes || no {
        *ASK_CVMFS.lock().unwrap() = None;
    }
    yes
}

/// The mount ask itself: what it does, why root, what it leaves, how to
/// undo it, the command, and the two answers - (yes, no).
fn consent_body(ui: &mut egui::Ui, ctx: &egui::Context, s: &crate::api::StackView) -> (bool, bool) {
    let (mut yes, mut no) = (false, false);
    let how = match s.root_by.as_str() {
        "root" => "Cove is running as root here, so nothing is asked.",
        "sudo" => "sudo runs it: this account may use sudo without a password, so nothing is asked.",
        "pkexec" => "Your system asks for an administrator's password once, in its own window. Cove never sees or keeps it.",
        _ => "Cove cannot ask for root on this machine: run the command below in a terminal yourself, then press Check again.",
    };
    let sudo = if s.root_by == "root" { "" } else { "sudo " };
    {
        ui.label(RichText::new("Mount the ndexr stack on this computer").size(19.0).strong().color(theme::text_strong()));
        ui.add_space(4.0);
        ui.add(egui::Label::new(RichText::new("The stack is R, Python, Julia, compilers and thousands of scientific libraries, built once by ndexr. CernVM-FS - the file system research computing centres use to share software - shows all of it at /opt/ndexr/stack and downloads each file the first time something uses it. Nothing is copied up front.").size(13.0).color(theme::text())).wrap());
        egui::ScrollArea::vertical().max_height((ctx.content_rect().height() * 0.55).max(200.0)).show(ui, |ui| {
            let head = |ui: &mut egui::Ui, t: &str| {
                ui.add_space(8.0);
                ui.label(RichText::new(t).size(14.0).strong().color(theme::text_strong()));
            };
            let item = |ui: &mut egui::Ui, t: &str| {
                ui.add(egui::Label::new(RichText::new(format!("·  {t}")).size(12.5).color(theme::text())).wrap());
            };
            head(ui, "What it does");
            item(ui, "Installs the CernVM-FS client through this computer's package manager: from CERN's repository with apt or dnf, or from Gentoo's own tree with emerge.");
            item(ui, "Fetches ndexr's public signing key: the client accepts only files ndexr signed, and checks every one.");
            item(ui, "Writes the stack's settings to /etc/cvmfs/config.d/stack.ndexr.io.conf and adds one line to /etc/fstab, so the stack mounts read-only at /opt/ndexr/stack, now and after a restart.");
            item(ui, "Adds /etc/profile.d/ndexr-modules.sh and a line in /etc/bash.bashrc, so a new terminal can load software with `ml R/...`.");
            head(ui, "Why it needs root, once");
            item(ui, "Installing a system package, mounting a file system and writing under /etc are things only an administrator may do on Linux. This is the only step that needs it.");
            item(ui, how);
            item(ui, "After it, nothing runs as root: R, Python, your projects and Cove itself run as you.");
            head(ui, "What it leaves alone");
            item(ui, "The software already on this computer: the stack sits beside it in /opt/ndexr/stack, read-only, and nothing in it can be changed from here.");
            item(ui, "Your files: nothing in your home folder is read or sent anywhere.");
            item(ui, "Disk: files are cached as they are used, up to 20 GB, and the oldest go first.");
            head(ui, "Undo it");
            item(ui, &format!("{sudo}umount /opt/ndexr/stack, delete the stack.ndexr.io line from /etc/fstab, then remove the cvmfs package ({sudo}apt remove cvmfs, or dnf)."));
            head(ui, "The command it runs");
            // The CLI asks for sudo itself where it needs it.
            code_line(ui, &s.setup_cmd);
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if s.root_by.is_empty() {
                no = ui.add(theme::primary_button("I will run it myself")).clicked();
            } else {
                yes = ui.add(theme::primary_button("Mount the stack")).clicked();
                no = ui.button("Not now").clicked();
            }
        });
    }
    (yes, no)
}

/// A command to run by hand, selectable.
fn code_line(ui: &mut egui::Ui, cmd: &str) {
    let mut c = cmd.to_owned();
    ui.add(egui::TextEdit::singleline(&mut c).font(egui::TextStyle::Monospace).desired_width(ui.available_width()));
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

// ── How it fits ─────────────────────────────────────────────────────────────

/// The four on the chain, in the order work moves along it: a name, what it
/// is in a few words, and the verb that carries it to the next.
const CHAIN: [(&str, &str, &str); 4] = [
    ("You", "say what you want, typed or spoken", "ask"),
    ("Agent", "Claude, working in the site's folder", "writes"),
    ("App", "the program: Shiny, FastAPI, React, Rust…", "answers at"),
    ("Address", "name.ndexr.io - where people open it", ""),
];

/// One site, drawn (operator, 2026-11-26: "explain the link between all of
/// those to the user"): you ask its agent, the agent writes its app, the app
/// answers at its address, a pulse running along the chain; the last three
/// sit in one frame, because they are one site and one folder. Under it,
/// which tab holds which.
fn fit(ui: &mut egui::Ui) {
    Setup::title(ui, "How Cove fits together", "Everything you make here is a site. A site is three things that belong together, and you talk to it.");
    let w = ui.available_width();
    let wide = w >= 520.0;
    let t = ui.input(|i| i.time) as f32;
    ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
    // Nodes: across when there is room, else down.
    let (node, gap) = if wide { (egui::vec2(((w - 3.0 * 34.0) / 4.0).floor(), 112.0), 34.0) } else { (egui::vec2(w - 24.0, 56.0), 26.0) };
    let size = if wide { egui::vec2(w, node.y + 40.0) } else { egui::vec2(w, 4.0 * node.y + 3.0 * gap + 34.0) };
    let (area, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(area);
    let top = area.top() + 26.0;
    let rect_of = |i: usize| {
        if wide {
            egui::Rect::from_min_size(egui::pos2(area.left() + i as f32 * (node.x + gap), top), node)
        } else {
            egui::Rect::from_min_size(egui::pos2(area.left() + 24.0, top + i as f32 * (node.y + gap)), node)
        }
    };
    // The site's frame round Agent, App and Address.
    let site = rect_of(1).union(rect_of(3)).expand(8.0);
    p.rect(site, 14.0, theme::primary().gamma_multiply(0.06), Stroke::new(1.0, theme::primary().gamma_multiply(0.45)), egui::StrokeKind::Inside);
    p.text(egui::pos2(site.left() + 12.0, site.top() - 4.0), egui::Align2::LEFT_BOTTOM, "ONE SITE · ONE FOLDER", egui::FontId::proportional(10.5), theme::primary());
    // The links, each with its verb, and the pulse: one dot moving down the
    // chain every 3 seconds, lighting each node as it passes.
    let phase = (t / 3.0).fract() * 3.0;
    for i in 0..3 {
        let (a, b) = (rect_of(i), rect_of(i + 1));
        let (from, to) = if wide { (a.right_center(), b.left_center()) } else { (a.center_bottom(), b.center_top()) };
        p.line_segment([from, to], Stroke::new(1.5, theme::moss()));
        let tip = to;
        let back = if wide { egui::vec2(-6.0, 0.0) } else { egui::vec2(0.0, -6.0) };
        let side = egui::vec2(back.y, -back.x) * 0.6;
        p.add(egui::Shape::convex_polygon(vec![tip, tip + back + side, tip + back - side], theme::moss(), Stroke::NONE));
        let mid = from + (to - from) / 2.0;
        let verb = CHAIN[i].2;
        if wide {
            p.text(mid - egui::vec2(0.0, 9.0), egui::Align2::CENTER_BOTTOM, verb, egui::FontId::proportional(11.0), theme::text());
        } else {
            p.text(mid + egui::vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, verb, egui::FontId::proportional(11.5), theme::text());
        }
        let k = phase - i as f32;
        if (0.0..1.0).contains(&k) {
            p.circle_filled(from + (to - from) * k, 3.5, theme::amber());
        }
    }
    for (i, (name, what, _)) in CHAIN.iter().enumerate() {
        let r = rect_of(i);
        // Lit while the pulse is at it.
        let lit = (phase - i as f32).abs() < 0.25 || (i == 3 && phase > 2.75);
        p.rect(r, 10.0, theme::surface_hi(), Stroke::new(if lit { 1.5 } else { 1.0 }, if lit { theme::amber() } else { theme::pane_brd() }), egui::StrokeKind::Inside);
        let c = if lit { theme::amber() } else { theme::primary() };
        if wide {
            let ic = egui::pos2(r.center().x, r.top() + 24.0);
            mark(&p, i, ic, 11.0, c);
            p.text(egui::pos2(r.center().x, r.top() + 44.0), egui::Align2::CENTER_TOP, *name, theme::display(15.0), theme::text_strong());
            let job = egui::text::LayoutJob { halign: Align::Center, ..egui::text::LayoutJob::simple((*what).to_owned(), egui::FontId::proportional(11.5), theme::text(), r.width() - 14.0) };
            let g = ui.fonts_mut(|f| f.layout_job(job));
            p.galley(egui::pos2(r.center().x, r.top() + 66.0), g, theme::text());
        } else {
            let ic = egui::pos2(r.left() + 26.0, r.center().y);
            mark(&p, i, ic, 10.0, c);
            p.text(egui::pos2(r.left() + 50.0, r.top() + 10.0), egui::Align2::LEFT_TOP, *name, theme::display(14.5), theme::text_strong());
            let job = egui::text::LayoutJob::simple((*what).to_owned(), egui::FontId::proportional(11.5), theme::text(), r.width() - 60.0);
            let g = ui.fonts_mut(|f| f.layout_job(job));
            p.galley(egui::pos2(r.left() + 50.0, r.top() + 30.0), g, theme::text());
        }
    }
    ui.add_space(10.0);
    ui.label(RichText::new("WHERE EACH LIVES IN COVE").size(10.5).color(theme::hush()));
    for (tab, what) in [
        ("Workspace", "one site open: talk to its agent, read its files, run its consoles, and see its app live in App"),
        ("Sites", "every site you may open - yours, shared with you, and public - to open in Workspace or in the browser"),
        ("Account", "your addresses, your Claude and your keys"),
    ] {
        ui.horizontal_top(|ui| {
            let g = ui.painter().layout_no_wrap(tab.to_owned(), egui::FontId::proportional(12.5), theme::primary());
            let (r, _) = ui.allocate_exact_size(egui::vec2(92.0, g.size().y + 8.0), Sense::hover());
            let pill = egui::Rect::from_min_size(r.min, g.size() + egui::vec2(16.0, 8.0));
            ui.painter().rect_filled(pill, 9.0, theme::primary().gamma_multiply(0.14));
            ui.painter().galley(pill.center() - g.size() / 2.0, g, theme::primary());
            ui.add(egui::Label::new(RichText::new(what).size(12.5).color(theme::text())).wrap());
        });
    }
}

/// The chain's marks, drawn at `c` with radius `r`: a person, the agent's
/// spark, the app's play triangle, the address's globe.
fn mark(p: &egui::Painter, i: usize, c: egui::Pos2, r: f32, col: Color32) {
    let st = Stroke::new(1.8, col);
    match i {
        0 => {
            p.circle_stroke(c - egui::vec2(0.0, r * 0.45), r * 0.42, st);
            let pts: Vec<egui::Pos2> = (0..=12).map(|k| {
                let a = std::f32::consts::PI * (1.0 + k as f32 / 12.0);
                c + egui::vec2(a.cos() * r * 0.85, r * 0.95 + a.sin() * r * 0.75)
            }).collect();
            p.add(egui::Shape::line(pts, st));
        }
        1 => {
            for k in 0..8 {
                let a = std::f32::consts::TAU * k as f32 / 8.0;
                let (r0, r1) = if k % 2 == 0 { (r * 0.35, r) } else { (r * 0.35, r * 0.62) };
                p.line_segment([c + egui::vec2(a.cos(), a.sin()) * r0, c + egui::vec2(a.cos(), a.sin()) * r1], st);
            }
        }
        2 => crate::ui::kit::icons::paint(p, egui::Rect::from_center_size(c, egui::vec2(r * 2.2, r * 2.2)), crate::ui::kit::icons::Glyph::Play, col),
        _ => {
            p.circle_stroke(c, r, st);
            p.line_segment([c - egui::vec2(r, 0.0), c + egui::vec2(r, 0.0)], st);
            let pts: Vec<egui::Pos2> = (0..=16).map(|k| {
                let a = std::f32::consts::PI * (k as f32 / 16.0 - 0.5);
                c + egui::vec2(a.cos() * r * 0.45, a.sin() * r)
            }).collect();
            p.add(egui::Shape::line(pts.clone(), st));
            p.add(egui::Shape::line(pts.iter().map(|q| egui::pos2(2.0 * c.x - q.x, q.y)).collect(), st));
        }
    }
}
