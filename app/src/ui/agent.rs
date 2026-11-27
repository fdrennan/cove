//! The Agent tab beside the consoles: the open domain's agent, exec.ndexr.io's
//! workflow built into the IDE (operator, 2026-10-14). Each turn goes through
//! the platform's own host_run_claude (host.rs agent_run) into the domain's
//! thread - the same thread the in-app chat shows, resuming its last session -
//! and its stream-json is followed live (AgentPoll): what it says, every tool
//! it runs, its cost. Nothing here talks to a model itself.

// What a message is made of before it is sent: voice, its provenance, drafts.
pub(crate) mod drafts;
pub(crate) mod spoken;
pub(crate) mod voice;

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Key, RichText};

use crate::api::{AgentChunk, AgentEvent, AgentTurn, DomainInfo, FileText, Lang, Listing, Query};
use crate::backend::Backend;
use crate::ui::kit::{bar, theme};

/// How a turn may act - exec.ndexr.io's modes (operator, 2026-10-16: "exec and
/// agents need auto, plan, etc modes"): (mode, label, what it means).
const MODES: [(&str, &str, &str); 5] = [
    ("build", "Auto", "does the work: edits files and runs commands"),
    // Auto for a live talk (operator, 2026-11-09: "presentation mode ... should
    // go under the auto ... we want Sonnet to be able to quickly modify files"):
    // runs as Auto on Sonnet, keeps changes small and replies in one line; and
    // Listen, acting for this box, tells it each piece is from a live talk.
    ("present", "Presenting", "Auto for a live talk: Sonnet, small fast changes, one-line replies - Listen keeps the page in step with the talk"),
    // Setup's Supervised (operator, 2026-11-27): edits files in the project,
    // runs no commands - enforced by the agent's own permissions (Claude Code
    // acceptEdits, Codex workspace-write), never only asked for in words.
    ("supervised", "Supervised", "edits files in the project - runs no commands"),
    ("plan", "Plan", "thinks it through and writes a plan - changes nothing"),
    ("ask", "Ask", "answers questions - reads, never changes"),
];

/// The Presenting mode's instruction, ahead of what was said or typed.
const PRESENTING: &str = "(Presenting: a live talk is going on. Keep what is on screen in step with it - move to the slide or section being spoken about, or make the one small change asked for. Change nothing when it already matches. Reply in one line; never stop to explain.)";

/// What a turn runs on: every Claude model the CLI knows (`claude --model`
/// aliases), each on a subscription (`sub-`: the person's own connected
/// Claude, or the shared seat) or on the API (operator, 2026-10-26: "all model
/// options for claude for users to pick, subscription or api too"). The
/// wrapper strips `sub-` and runs the alias on OAuth (ndexr-claude-run).
/// The first two stay sub-opus and sub-sonnet: the agent's and file-ask's
/// defaults are indexes into this list.
const MODELS: [(&str, &str); 19] = [
    ("sub-opus", "Opus 5.5 · subscription"),
    ("sub-sonnet", "Sonnet 5.5 · subscription"),
    ("sub-fable", "Fable 5.1 · subscription"),
    ("sub-haiku", "Haiku 5.5 · subscription"),
    ("sub-opus[1m]", "Opus 5.5, 1M context · subscription"),
    ("sub-sonnet[1m]", "Sonnet 5.5, 1M context · subscription"),
    ("sub-fable[1m]", "Fable 5.1, 1M context · subscription"),
    ("sub-opusplan", "Opus plans, Sonnet builds · subscription"),
    ("opus", "Opus 5.5 · API"),
    ("sonnet", "Sonnet 5.5 · API"),
    ("fable", "Fable 5.1 · API"),
    ("haiku", "Haiku 5.5 · API"),
    ("opus[1m]", "Opus 5.5, 1M context · API"),
    ("sonnet[1m]", "Sonnet 5.5, 1M context · API"),
    ("fable[1m]", "Fable 5.1, 1M context · API"),
    ("opusplan", "Opus plans, Sonnet builds · API"),
    // OpenAI, through the platform's LiteLLM proxy (src/litellm/config.yaml):
    // ndexr-claude-run points the same agent - tools, files, thread - at it
    // for any gpt-* model (operator, 2026-11-09: "openai as an option through
    // the api ... anthropic and open support through chatbot dropdown").
    ("gpt-5.5", "GPT-5.5 · OpenAI"),
    ("gpt-5.4-mini", "GPT-5.4 mini · OpenAI"),
    ("gpt-5.3-codex", "GPT-5.3 Codex · OpenAI"),
];

/// The model picker's list: subscription models, then API ones, a heading
/// over each.
fn model_menu(ui: &mut egui::Ui, model: &mut String) {
    // Anthropic on a subscription, Anthropic on the API, then OpenAI.
    let group = |m: &str| if m.starts_with("sub-") { 0 } else if m.starts_with("gpt-") { 2 } else { 1 };
    for (g, head) in ["Anthropic · subscription", "Anthropic · API", "OpenAI · API"].into_iter().enumerate() {
        if g > 0 {
            ui.separator();
        }
        ui.label(RichText::new(head).size(10.5).strong().color(theme::hush()));
        for (m, l) in MODELS.iter().filter(|(m, _)| group(m) == g) {
            ui.selectable_value(model, (*m).to_owned(), l.split(" · ").next().unwrap_or(l));
        }
    }
}

/// The thread, with every run in it that is still going. Several go at once
/// (operator, 2026-10-15: "submitting multiple chats in parallel like
/// exec.ndexr.io does so we can add in thoughts instead of being blocked"):
/// each Send starts its own run, as exec's composer does, and each streams
/// into its own turn.
#[derive(Default)]
struct State {
    /// The domain whose thread is loaded.
    loaded_for: String,
    loading: bool,
    turns: Vec<AgentTurn>,
    /// Runs being followed, and how far into each one's stream.
    live: HashMap<String, u64>,
    /// Runs with a poll out.
    polling: HashSet<String>,
    /// Turns sent and not yet given a run id ("+N" keys stand in until then).
    starting: usize,
    costs: HashMap<String, f64>,
    /// Runs paid by the person's own plan: their cost is what the API would
    /// have charged, shown as such and never added to money spent (#736).
    subs: HashSet<String>,
    /// Code the agent gave while auto-run was on, for the consoles to take.
    outbox: Vec<(Lang, String)>,
    /// A turn's stand-in key ("+N") to the run id it was given, so the
    /// Inputs page can follow a turn it sent across the swap.
    aliases: HashMap<String, String>,
    /// The images a turn was sent with: (path, thumbnail), by turn id - its
    /// stand-in key until the run has an id.
    shots: HashMap<String, Vec<(String, egui::TextureHandle)>>,
    /// Runs that finished while Speak was on, to be said aloud (`tick`).
    to_speak: Vec<String>,
    /// The reply whose Listen was clicked, and how far it got: Ok(false)
    /// preparing, Ok(true) playing, Err why it could not be read. One at a
    /// time - a second Listen takes over.
    reading: Option<(String, Result<bool, String>)>,
    /// A thread that could not load while Cove restarted: which, and when to
    /// ask again - the thread on screen stays, no red line in its place.
    retry_for: String,
    retry_at: f64,
    /// Messages sent while Cove restarted: (turn key, the run asked for, when
    /// to send it again). Each goes once Cove answers (operator, 2026-11-22:
    /// "a restart is no longer scary with red text").
    resend: Vec<(String, Query, f64)>,
}

/// A failure that is Cove restarting (a release) or the network blinking, not
/// an answer: the request is asked again, nothing is said in red.
fn restarting(e: &str) -> bool {
    let l = e.to_ascii_lowercase();
    ["restarting", "session has ended", "failed to fetch", "networkerror", "error sending request", "connection refused", "connection reset", "timed out", "502", "503", "504"].iter().any(|k| l.contains(k))
}

/// The fenced blocks of a reply that a console can run: ```r and ```python go
/// to the R session (Python through reticulate), ```bash / ```sh to the
/// Terminal. Untagged or other languages are shown, never run.
pub fn runnable(text: &str) -> Vec<(Option<Lang>, String)> {
    let mut out = Vec::new();
    let mut open: Option<(Option<Lang>, String)> = None;
    for line in text.lines() {
        let t = line.trim_start();
        match &mut open {
            None if t.starts_with("```") => {
                open = Some((crate::ui::kit::md::fence_lang(t.trim_start_matches('`')), String::new()));
            }
            None => {}
            Some(_) if t.starts_with("```") => {
                let (l, code) = open.take().unwrap();
                out.push((l, code.trim_end().to_owned()));
            }
            Some((_, code)) => {
                code.push_str(line);
                code.push('\n');
            }
        }
    }
    out
}

/// When a turn was sent, as the person reads a clock: "14:32" today,
/// "Nov 7 · 14:32" before, and on hover the whole date and how long ago. The
/// browser's own time zone; the desktop build says UTC.
fn sent_at(at: i64, now: i64) -> (String, String) {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    // (year, month 0-11, day, hour, minute) of a Unix time.
    #[cfg(target_arch = "wasm32")]
    let parts = |t: i64| {
        let d = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(t as f64 * 1000.0));
        (d.get_full_year() as i64, d.get_month() as usize, d.get_date() as i64, d.get_hours() as i64, d.get_minutes() as i64)
    };
    #[cfg(not(target_arch = "wasm32"))]
    let parts = |t: i64| {
        // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
        let (days, secs) = (t.div_euclid(86400), t.rem_euclid(86400));
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = if mp < 10 { mp + 3 } else { mp - 9 };
        let year = yoe + era * 400 + if month <= 2 { 1 } else { 0 };
        (year, (month - 1) as usize, day, secs / 3600, secs % 3600 / 60)
    };
    let zone = if cfg!(target_arch = "wasm32") { "" } else { " UTC" };
    let (y, mo, d, h, mi) = parts(at);
    let (ny, nmo, nd, _, _) = parts(now);
    let clock = format!("{h:02}:{mi:02}{zone}");
    let short = if (y, mo, d) == (ny, nmo, nd) {
        clock.clone()
    } else if y == ny {
        format!("{} {d} · {clock}", MONTHS[mo])
    } else {
        format!("{} {d}, {y} · {clock}", MONTHS[mo])
    };
    let s = (now - at).max(0);
    let ago = match s {
        s if s < 60 => "just now".to_owned(),
        s if s < 3600 => format!("{} min ago", s / 60),
        s if s < 86400 => format!("{} h {} min ago", s / 3600, s % 3600 / 60),
        s => format!("{} days ago", s / 86400),
    };
    (short, format!("sent {} {d}, {y} at {clock} - {ago}", MONTHS[mo]))
}

/// The tail of the R console handed to the agent with a turn, and how to send
/// code back (operator, 2026-10-14: "the agent send things to the console in
/// real time ... like a single command").
fn with_console(prompt: &str, tail: &str) -> String {
    format!(
        "{prompt}\n\n---\nI am working in Cove, weaving the stack's scientific software into this domain's app. The end of my live R console (R, with Python through reticulate) is below. \
         To run something in it for me, reply with ONE fenced ```r block (```python for reticulate, ```bash for my terminal) - \
         the IDE runs it in that console with one click. Keep it to a single command or short block. \
         This project builds, starts and stops with .cove/build.sh, .cove/up.sh and .cove/down.sh, run from its root (.cove/app.env names its language and port; \
         .cove/<language>/ holds each type's build.sh, env.sh and run.sh; APP-CONTRACT.md) - run them when it makes sense; Cove's Build / Up / Down buttons run the same files. \
         Keep Cove's scripts in .cove/, never at the project's root: the root is the person's app. \
         Log through the project's own helper (r/log.r, log.py, log.js, log.jl or src/log.rs; one line format in every language) rather than print: \
         a start line, one line per request or session, each error, and log_fn() first in every R ui_*/server_*. The log is .cove/run/app.log, Cove's Logs tab. \
         Cove's App pane reloads when the project's files change, and nothing under .cove/ counts: for a change that takes several saves, build it in .cove/stage/ and move it into place when it is ready, \
         or create .cove/reload and App reloads only when you touch that file.\n\n```text\n{tail}\n```"
    )
}

/// What the switch beside the mic shows: the open site, if any, and whether
/// Exec (the agent of all the person's sites) is the one chosen.
#[derive(Clone, Default)]
pub struct Scope {
    pub site: Option<String>,
    pub exec_on: bool,
    pub exec_busy: bool,
}

/// The site's first label, so the switch fits beside the mic on a phone; the
/// whole host is in the hover.
fn site_word(host: &str) -> String {
    let w = host.split('.').next().unwrap_or(host);
    if w.chars().count() > 14 { format!("{}…", w.chars().take(13).collect::<String>()) } else { w.to_owned() }
}

/// One choice of several, the chosen one filled - the switch beside the mic
/// and Go's Listen (ide.rs listen_bar) draw the same way. Each item is (word,
/// hover, chosen, enabled); returns the index clicked.
pub fn segmented(ui: &mut egui::Ui, big: bool, items: &[(String, String, bool, bool)]) -> Option<usize> {
    let (size, h) = if big { (14.0, 36.0) } else { (12.0, 24.0) };
    let mut pick = None;
    // Outlined in the primary colour, the chosen word strong: in a pane-border
    // grey the switch sank into the composer row and who you were talking to
    // was hard to read in every theme (request #179, 2026-11-01).
    egui::Frame::new().fill(theme::bg()).stroke(egui::Stroke::new(1.5, theme::primary())).corner_radius(10).inner_margin(egui::Margin::same(2)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, (word, tip, on, enabled)) in items.iter().enumerate() {
            let text = RichText::new(word).size(size).color(if *on { theme::bg() } else { theme::text() });
            let text = if *on { text.strong() } else { text };
            let b = egui::Button::new(text).fill(if *on { theme::primary() } else { egui::Color32::TRANSPARENT }).stroke(egui::Stroke::NONE).corner_radius(8).min_size(egui::vec2(0.0, h));
            if ui.add_enabled(*enabled, b).on_hover_text(tip.as_str()).clicked() {
                pick = Some(i);
            }
        }
    });
    pick
}

/// Site | Exec, the chosen one filled. Some(true) when Exec was clicked,
/// Some(false) for the site.
pub fn scope_toggle(ui: &mut egui::Ui, sc: &Scope, big: bool) -> Option<bool> {
    let (word, tip) = match &sc.site {
        Some(host) => (site_word(host), format!("{host}'s agent - it works in this site's folder")),
        None => ("Site".to_owned(), "open a project first".to_owned()),
    };
    let dot = if sc.exec_busy { " ●" } else { "" };
    // Exec first, then the site (operator, 2026-11-22: "move exec to left of
    // site name").
    let items = [
        (format!("Exec{dot}"), "the executive agent of all your sites - one thread across every domain".to_owned(), sc.exec_on, true),
        (word, tip, !sc.exec_on, sc.site.is_some()),
    ];
    segmented(ui, big, &items).map(|i| i == 0)
}

pub struct AgentPane {
    st: Arc<Mutex<State>>,
    input: String,
    model: String,
    /// How the next turn may act (`MODES`).
    mode: &'static str,
    asked: f64,
    /// Turns sent from this pane, for their stand-in keys.
    sent: u64,
    /// Hand the console's tail to the agent with each turn.
    pub share_console: bool,
    /// Run the code the agent replies with as it arrives, without a click.
    auto_run: Arc<AtomicBool>,
    /// The R console under the thread (Ide draws it).
    pub split: bool,
    /// The composer's height last frame. The thread takes what is left above
    /// it: a fixed 64 px guess left the + / mic / send row under the window's
    /// bottom edge (operator, 2026-10-15: "i cant see buttons at the bottom").
    composer_h: f32,
    /// How much of each streaming turn's text is on screen: a reply types in
    /// at a pace that keeps up with it, rather than landing in 700 ms lumps
    /// (operator, 2026-10-15: "chat section should feel smooth and fluid").
    reveal: HashMap<String, f64>,
    /// The tool steps a person opened, by (turn, group).
    open_tools: HashSet<(String, usize)>,
    /// Speak instead of typing (voice.rs).
    mic: crate::ui::agent::voice::Mic,
    /// Whose draft the box holds, and the text last kept for it (drafts.rs).
    draft_for: String,
    draft_saved: String,
    /// What the mic dictated into the box, as it came: shown in its own ink
    /// and sent tagged, so the agent knows what was said and what was typed
    /// (spoken.rs).
    spoken: Vec<String>,
    /// Shown in Go: the composer's mic and Send drawn big (ide.rs go_agent).
    pub big: bool,
    /// Go's Listen button beside the mic (mode, enabled, hover), set by ide.rs
    /// for the frame; None draws none. A click comes back as `listen_clicked`.
    pub listen: Option<(crate::ui::agent::voice::ListenMode, bool, String)>,
    pub listen_clicked: bool,
    /// Seconds between Listen's pieces (voice.rs PIECES), picked beside the
    /// mode (ide.rs keeps the one value for every composer), and the seconds
    /// until the next piece while Listen is on.
    pub listen_secs: u32,
    pub listen_left: Option<u32>,
    /// Why the last Paste (paste.rs) got nothing, until the next one works.
    paste_err: Option<String>,
    /// Which agent the composer talks to, drawn beside the mic (ide.rs sets
    /// it when one agent is shown at a time; None side by side, where both
    /// are on screen).
    pub scope: Option<Scope>,
    /// The execution bar folded to its one-line summary (exec_bar).
    exec_folded: bool,
    /// Only the composer: the thread, its status rows and what it works from
    /// put away, the domain/Exec switch at the top of the pane (operator,
    /// 2026-11-17: "collapse the output of the agents section so we only see
    /// the panel for submissions").
    pub thread_folded: bool,
    /// The pane was too short for a thread last frame (it folds by itself).
    short_pane: bool,
    /// The scope a click beside the mic chose: true = Exec (ide.rs takes it).
    pub scope_pick: Option<bool>,
    /// Where the thread showed last frame and how far it was scrolled: a
    /// selection dragged past its edge scrolls it on.
    thread_rect: egui::Rect,
    thread_off: f32,
    /// What the agent works from in this domain (commons' layers): its docs
    /// and the recipes they hold.
    know: Arc<Mutex<Know>>,
    /// The "works from" panel is open.
    show_know: Option<bool>,
    /// Files to open in Files (a doc, a file a turn changed); Ide takes them.
    pub open: Vec<String>,
    /// Files attached to the next message (operator, 2026-10-15: "need to be
    /// able to upload files and supporting docs through cove agent"):
    /// uploaded at once into the domain's supporting_docs/, named in the
    /// prompt when it is sent.
    attached: Arc<Mutex<Vec<Attached>>>,
    /// Set on the Exec tab's pane: what its strip says instead of
    /// "this site", and the namespace its widgets' ids live in - so the
    /// Agents and Exec panes can both be on screen without sharing an input box.
    pub scope_label: Option<String>,
    /// What a message is about (operator, 2026-11-17: Files' own agent
    /// folded in here - "consolidating all the options in agents and files
    /// ... in a single interface"): 0 the whole site, 1 the file open in
    /// Files, 2 the folder its tree is in. ide.rs sets the file and folder
    /// every frame; Exec ignores it.
    pub focus: u8,
    pub focus_file: Option<String>,
    pub focus_folder: String,
    /// A file-focused message was sent: ide.rs saves the open file first.
    pub wants_save: bool,
    /// The host whose empty thread took the keyboard, so it takes it once.
    focused_for: String,
}

/// An attachment's name as its chip shows it: at most 18 characters, the
/// extension kept ("IMG_8117.png", "a-very-long-na….pdf").
fn chip_name(name: &str) -> String {
    const MAX: usize = 18;
    if name.chars().count() <= MAX {
        return name.to_owned();
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() && e.chars().count() <= 5 => (s, format!(".{e}")),
        _ => (name, String::new()),
    };
    let keep = MAX.saturating_sub(ext.chars().count() + 1).max(4);
    format!("{}…{ext}", stem.chars().take(keep).collect::<String>())
}

/// A file attached to the message being written.
#[derive(Clone, Default)]
struct Attached {
    name: String,
    /// Where it landed, once uploaded.
    path: Option<String>,
    error: Option<String>,
    /// An image's thumbnail (a screenshot, a paste): shown in its chip and,
    /// once sent, under the person's bubble.
    thumb: Option<egui::TextureHandle>,
}

/// What the agent is told about the files attached to a message, after what
/// the person wrote (and before the console's tail, which shown_prompt cuts).
fn with_attachments(prompt: &str, paths: &[String]) -> String {
    if paths.is_empty() {
        return prompt.to_owned();
    }
    let list: String = paths.iter().map(|p| format!("\n- {p}")).collect();
    format!("{prompt}\n\nAttached to this message (uploaded into the domain's supporting_docs/) - read them before you answer; an image is a screenshot of what I mean, so look at it (Read shows images):{list}")
}

/// The model and mode a new agent session starts on, as setup chose them
/// (setup.rs Prefs::apply); "" leaves the built-in default.
static DEFAULTS: std::sync::Mutex<(String, String)> = std::sync::Mutex::new((String::new(), String::new()));

pub fn set_defaults(model: &str, mode: &str) {
    let mut d = DEFAULTS.lock().unwrap();
    if model.is_empty() || MODELS.iter().any(|(m, _)| *m == model) {
        d.0 = model.to_owned();
    }
    if mode.is_empty() || MODES.iter().any(|(m, _, _)| *m == mode) {
        d.1 = mode.to_owned();
    }
}

pub(crate) fn default_model() -> String {
    let d = DEFAULTS.lock().unwrap();
    if d.0.is_empty() { MODELS[0].0.into() } else { d.0.clone() }
}

fn default_mode() -> &'static str {
    let d = DEFAULTS.lock().unwrap();
    MODES.iter().find(|(m, _, _)| *m == d.1).map_or(MODES[0].0, |(m, _, _)| *m)
}

impl Default for AgentPane {
    fn default() -> Self {
        AgentPane {
            st: Default::default(),
            input: String::new(),
            model: default_model(),
            mode: default_mode(),
            asked: f64::NEG_INFINITY,
            sent: 0,
            share_console: true,
            auto_run: Default::default(),
            split: false,
            composer_h: 120.0,
            reveal: HashMap::new(),
            open_tools: HashSet::new(),
            mic: Default::default(),
            draft_for: String::new(),
            draft_saved: String::new(),
            spoken: Vec::new(),
            big: false,
            listen: None,
            listen_clicked: false,
            listen_secs: 60,
            listen_left: None,
            paste_err: None,
            scope: None,
            scope_pick: None,
            exec_folded: false,
            thread_folded: false,
            short_pane: false,
            thread_rect: egui::Rect::NOTHING,
            thread_off: 0.0,
            know: Default::default(),
            show_know: None,
            open: Vec::new(),
            attached: Default::default(),
            scope_label: None,
            focus: 0,
            focus_file: None,
            focus_folder: String::new(),
            wants_save: false,
            focused_for: String::new(),
        }
    }
}

impl AgentPane {
    /// The Exec tab's pane: the person's executive agent over every domain
    /// they own (operator, 2026-10-15), kept apart from the Agents tab's.
    pub fn exec() -> Self {
        AgentPane { scope_label: Some("all sites".to_owned()), ..Default::default() }
    }

    /// What the running turns have written or edited so far, oldest first,
    /// and the file the newest step is on when that step is an edit: Files
    /// marks them as they happen (request #165).
    pub fn changing(&self) -> (Vec<String>, Option<String>) {
        let s = self.st.lock().unwrap();
        let (mut all, mut now) = (Vec::new(), None);
        for t in s.turns.iter().filter(|t| !t.done && s.live.contains_key(&t.id)) {
            for f in provenance(&t.events).changed {
                if !all.contains(&f) {
                    all.push(f);
                }
            }
            now = t.events.iter().rev().find(|e| e.kind == "tool").and_then(|e| edit_target(&e.text)).map(str::to_owned).or(now);
        }
        (all, now)
    }

    /// Which pane a widget id belongs to.
    fn ns(&self) -> &'static str {
        if self.scope_label.is_some() { "exec" } else { "domain" }
    }

    /// A run is starting or streaming (the tab's light).
    pub fn busy(&self) -> bool {
        let s = self.st.lock().unwrap();
        s.starting > 0 || !s.live.is_empty()
    }

    /// What a run going now is doing, in a few words, for the folded pane:
    /// "editing r/home.r", "working · Bash", "thinking…", "starting…".
    fn doing(&self) -> Option<String> {
        let s = self.st.lock().unwrap();
        if s.live.is_empty() {
            return (s.starting > 0).then(|| "starting…".to_owned());
        }
        let t = s.turns.iter().rev().find(|t| !t.done && s.live.contains_key(&t.id))?;
        Some(t.events.iter().rev().find(|e| e.kind == "tool").map_or("thinking…".to_owned(), |e| match edit_target(&e.text) {
            Some(f) => format!("editing {}", f.rsplitn(3, '/').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/")),
            None => format!("working · {}", e.text.split(" · ").next().unwrap_or("")),
        }))
    }

    /// Type `text` into the prompt for the person to read and send.
    pub fn prefill(&mut self, text: &str) {
        self.input = text.to_owned();
    }

    fn load(&self, backend: &Backend, ctx: &egui::Context, domain: &str) {
        let now = ctx.input(|i| i.time);
        {
            let mut s = self.st.lock().unwrap();
            // Asked again after a restart: not before its time, and what is on
            // screen - the thread, messages waiting to go - stays.
            let again = s.retry_for == domain;
            if again && now < s.retry_at {
                return;
            }
            let (turns, resend) = if again { (std::mem::take(&mut s.turns), std::mem::take(&mut s.resend)) } else { Default::default() };
            *s = State { loaded_for: domain.to_owned(), loading: true, ..Default::default() };
            if again {
                s.turns = turns;
                s.resend = resend;
            }
        }
        let st = self.st.clone();
        let d = domain.to_owned();
        backend.query(Query::AgentHistory { domain: d.clone() }, ctx, move |r: Result<Vec<AgentTurn>, String>| {
            let mut s = st.lock().unwrap();
            if s.loaded_for != d {
                return;
            }
            match r {
                Err(e) if restarting(&e) => {
                    s.loaded_for.clear();
                    s.retry_for = d.clone();
                    s.retry_at = now + 3.0;
                    return;
                }
                _ => {}
            }
            s.loading = false;
            s.retry_for.clear();
            match r {
                Ok(mut turns) => {
                    // Turns still running (here a moment ago, the in-app chat,
                    // exec): follow each from the start of its stream.
                    for t in turns.iter_mut().filter(|t| !t.done && !t.id.is_empty()) {
                        s.live.insert(t.id.clone(), 0);
                        t.events.clear();
                    }
                    // Sent here and not answered yet (a restart in between):
                    // they stay, after the thread.
                    let waiting: Vec<AgentTurn> = s.turns.iter().filter(|t| t.id.starts_with('+') && !t.done).cloned().collect();
                    s.turns = turns;
                    s.turns.extend(waiting);
                }
                Err(e) => s.turns = vec![AgentTurn { events: vec![AgentEvent { kind: "error".into(), text: e }], done: true, ..Default::default() }],
            }
        });
    }

    fn poll(&mut self, backend: &Backend, ctx: &egui::Context) {
        let due: Vec<(String, u64)> = {
            let s = self.st.lock().unwrap();
            if s.live.is_empty() {
                return;
            }
            s.live.iter().filter(|(id, _)| !s.polling.contains(*id)).map(|(id, o)| (id.clone(), *o)).collect()
        };
        let now = ctx.input(|i| i.time);
        ctx.request_repaint_after(std::time::Duration::from_millis(700));
        if now - self.asked < 0.7 {
            return;
        }
        self.asked = now;
        for (id, offset) in due {
            self.poll_one(backend, ctx, id, offset);
        }
    }

    fn poll_one(&self, backend: &Backend, ctx: &egui::Context, id: String, offset: u64) {
        self.st.lock().unwrap().polling.insert(id.clone());
        let (st, auto) = (self.st.clone(), self.auto_run.clone());
        backend.query(Query::AgentPoll { id: id.clone(), offset }, ctx, move |r: Result<AgentChunk, String>| {
            let mut s = st.lock().unwrap();
            s.polling.remove(&id);
            let Ok(c) = r else { return };
            match s.live.get_mut(&id) {
                Some(o) => *o = c.offset,
                None => return,
            }
            if let Some(cost) = c.cost {
                s.costs.insert(id.clone(), cost);
            }
            if c.subscription {
                s.subs.insert(id.clone());
            }
            if auto.load(Ordering::Relaxed) {
                for e in c.events.iter().filter(|e| e.kind == "text") {
                    for (l, code) in runnable(&e.text) {
                        if let Some(l) = l.filter(|_| !code.trim().is_empty()) {
                            s.outbox.push((l, code));
                        }
                    }
                }
            }
            if let Some(t) = s.turns.iter_mut().rev().find(|t| t.id == id) {
                t.events.extend(c.events);
                t.done = c.done;
            }
            if c.done {
                s.live.remove(&id);
                if crate::ui::agent::voice::speak::on() {
                    s.to_speak.push(id.clone());
                }
            }
        });
    }

    /// Send `text` as a message of its own, leaving what is typed and attached
    /// in the box for later (Go's Listen, ide.rs go_agent).
    pub fn say(&mut self, backend: &Backend, ctx: &egui::Context, domain: &str, text: &str, console_tail: &str) {
        self.say_on(backend, ctx, domain, text, console_tail, None);
    }

    /// `say`, on `model` for this one turn when given (Listen's Presenting
    /// pace runs on Sonnet); the picker keeps the person's own choice.
    pub fn say_on(&mut self, backend: &Backend, ctx: &egui::Context, domain: &str, text: &str, console_tail: &str, model: Option<&str>) {
        let typed = std::mem::replace(&mut self.input, text.to_owned());
        let files = std::mem::take(&mut *self.attached.lock().unwrap());
        let mine = model.map(|m| std::mem::replace(&mut self.model, m.to_owned()));
        let dictated = std::mem::take(&mut self.spoken);
        self.send(backend, ctx, domain, console_tail);
        self.spoken = dictated;
        if let Some(m) = mine {
            self.model = m;
        }
        self.input = typed;
        *self.attached.lock().unwrap() = files;
    }

    /// The model this pane's turns run on.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// The mode pill is on Presenting.
    pub fn presenting(&self) -> bool {
        self.mode == "present"
    }

    /// Put `text` in the message box, after anything already typed.
    pub fn add_to_input(&mut self, text: &str) {
        if !self.input.trim().is_empty() {
            self.input.push_str("\n\n");
        }
        self.input.push_str(text.trim());
    }

    fn send(&mut self, backend: &Backend, ctx: &egui::Context, domain: &str, console_tail: &str) {
        let typed = std::mem::take(&mut self.input).trim().to_owned();
        let files: Vec<Attached> = std::mem::take(&mut *self.attached.lock().unwrap());
        let paths: Vec<String> = files.iter().filter_map(|f| f.path.clone()).collect();
        let shots: Vec<(String, egui::TextureHandle)> = files.iter().filter_map(|f| Some((f.path.clone()?, f.thumb.clone()?))).collect();
        if typed.is_empty() && paths.is_empty() {
            return;
        }
        let typed = if typed.is_empty() { "Here are some files for this domain.".to_owned() } else { typed };
        // What the mic dictated into the box goes tagged, with a note first,
        // so the agent reads dictation as dictation (spoken.rs). The pieces
        // are spent: the next message starts typed.
        let typed = crate::ui::agent::spoken::mark(&typed, &std::mem::take(&mut self.spoken));
        let names: Vec<&str> = paths.iter().map(|p| p.rsplit('/').next().unwrap_or(p)).collect();
        let prompt = with_attachments(&crate::ui::agent::spoken::for_agent(&typed), &paths);
        let prompt = if self.share_console && !console_tail.trim().is_empty() { with_console(&prompt, console_tail) } else { prompt };
        // The bubble shows what was typed and the files' names.
        let shown = if names.is_empty() { typed } else { format!("{typed}\n\nAttached: {}", names.join(", ")) };
        self.sent += 1;
        let key = format!("+{}", self.sent);
        {
            let mut s = self.st.lock().unwrap();
            s.turns.push(AgentTurn { id: key.clone(), prompt: shown, at: crate::ui::software::now_secs(), ..Default::default() });
            s.starting += 1;
            if !shots.is_empty() {
                s.shots.insert(key.clone(), shots);
            }
        }
        let st = self.st.clone();
        // Presenting runs as Auto on Sonnet, its instruction first (Listen's
        // Presenting pieces carry it already).
        let (prompt, model, mode) = if self.presenting() {
            let prompt = if prompt.starts_with("(Presenting") { prompt } else { format!("{PRESENTING}\n\n{prompt}") };
            (prompt, crate::ui::agent::voice::present_model(&self.model), "build".to_owned())
        } else {
            (prompt, self.model.clone(), self.mode.to_owned())
        };
        // About a file or a folder (the Focus menu): narrowed as Files' asks
        // were - the prompt says what may change, the run starts there.
        let site = self.scope_label.is_none() || self.scope.as_ref().is_none_or(|sc| !sc.exec_on);
        let (prompt, cwd) = match (self.focus, &self.focus_file) {
            (1, Some(f)) if site => {
                self.wants_save = true;
                (ask_prompt(domain, Some(f), &self.focus_folder, 0, &prompt), Some(folder_of(f).to_owned()))
            }
            (2, _) if site && !self.focus_folder.is_empty() => (ask_prompt(domain, None, &self.focus_folder, 1, &prompt), Some(self.focus_folder.clone())),
            _ => (prompt, None),
        };
        dispatch(st, backend, ctx, key, Query::AgentRun { domain: domain.to_owned(), prompt, model, cwd, mode: Some(mode) });
    }
}

/// Ask for a turn's run; asked again, quietly, if Cove is restarting.
fn dispatch(st: Arc<Mutex<State>>, backend: &Backend, ctx: &egui::Context, key: String, q: Query) {
    let now = ctx.input(|i| i.time);
    let again = q.clone();
    {
        backend.query(q, ctx, move |r: Result<String, String>| {
            let mut s = st.lock().unwrap();
            if let Err(e) = &r {
                if restarting(e) {
                    s.resend.push((key, again, now + 3.0));
                    return;
                }
            }
            s.starting = s.starting.saturating_sub(1);
            let ok = r.as_ref().ok().cloned();
            if let Some(t) = s.turns.iter_mut().find(|t| t.id == key) {
                match r {
                    Ok(id) => t.id = id,
                    Err(e) => {
                        t.events.push(AgentEvent { kind: "error".into(), text: e });
                        t.done = true;
                    }
                }
            }
            if let Some(id) = ok {
                s.aliases.insert(key.clone(), id.clone());
                if let Some(v) = s.shots.remove(&key) {
                    s.shots.insert(id.clone(), v);
                }
                s.live.insert(id, 0);
            }
        });
    }

}

impl AgentPane {
    /// Keep the thread loaded and its runs followed without the Agents tab
    /// drawn: the Inputs page sends to the agent too (operator, 2026-10-15:
    /// "Agents comes first, then R, Py, and Shell, agents treated like the
    /// others"). Cheap when the tab is also showing - a run already being
    /// polled is not asked twice.
    pub fn tick(&mut self, backend: &Backend, ctx: &egui::Context, domain: Option<&DomainInfo>) {
        let Some(d) = domain else { return };
        // Messages sent while Cove restarted, once it may be back.
        let now = ctx.input(|i| i.time);
        let due: Vec<(String, Query)> = {
            let mut s = self.st.lock().unwrap();
            let (go, wait): (Vec<_>, Vec<_>) = std::mem::take(&mut s.resend).into_iter().partition(|(_, _, at)| *at <= now);
            s.resend = wait;
            go.into_iter().map(|(k, q, _)| (k, q)).collect()
        };
        for (key, q) in due {
            dispatch(self.st.clone(), backend, ctx, key, q);
        }
        if !self.st.lock().unwrap().resend.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        if self.st.lock().unwrap().loaded_for != d.host {
            self.load(backend, ctx, &d.host);
            load_know(&self.know, backend, ctx, d);
            self.show_know = None;
        }
        self.poll(backend, ctx);
        self.speak_done(backend, ctx);
    }

    /// Each run that finished with Speak on: its reply goes to brief.rs
    /// `spoken`, and what comes back, if anything, plays.
    fn speak_done(&mut self, backend: &Backend, ctx: &egui::Context) {
        let due: Vec<String> = std::mem::take(&mut self.st.lock().unwrap().to_speak);
        for id in due {
            let Some((asked, reply, _)) = self.turn_text(&id) else { continue };
            backend.query(Query::Spoken { asked, reply }, ctx, |r: Result<crate::api::Spoken, String>| {
                if let Some(s) = r.ok().filter(|s| !s.audio.is_empty()) {
                    crate::ui::agent::voice::play::start(&s.audio);
                }
            });
        }
    }

    /// Send `text` as the person's next turn, as the composer would (the
    /// console's tail goes with it when shared). Returns the turn's key.
    pub fn ask(&mut self, backend: &Backend, ctx: &egui::Context, domain: &str, text: &str, console_tail: &str) -> String {
        let draft = std::mem::replace(&mut self.input, text.to_owned());
        let dictated = std::mem::take(&mut self.spoken);
        self.send(backend, ctx, domain, console_tail);
        self.spoken = dictated;
        self.input = draft;
        format!("+{}", self.sent)
    }

    /// A turn by its key: what was asked, the reply so far, and whether it
    /// has finished.
    pub fn turn_text(&self, key: &str) -> Option<(String, String, bool)> {
        let s = self.st.lock().unwrap();
        let id = s.aliases.get(key).map(String::as_str).unwrap_or(key);
        let t = s.turns.iter().find(|t| t.id == id)?;
        let reply: String = t.events.iter().filter(|e| e.kind == "text" || e.kind == "error").map(|e| e.text.as_str()).collect::<Vec<_>>().join("");
        Some((t.prompt.clone(), reply, t.done))
    }

    fn stop(&self, backend: &Backend, ctx: &egui::Context, id: String) {
        backend.query(Query::AgentStop { id }, ctx, |_: Result<bool, String>| {});
    }

    /// The tab's body: the thread, live, and the prompt. Returns the code to
    /// run in a console: a ▶ clicked under a reply, or what auto-run took.
    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, domain: Option<&DomainInfo>, console_tail: &str) -> Vec<(Lang, String)> {
        let Some(d) = domain else {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Open a domain in App - its agent works here, in that domain's thread.").color(theme::hush())));
            return Vec::new();
        };
        // The box as it is typed, kept (drafts.rs): a restart or reload brings
        // it back; each site has its own.
        if self.draft_for != d.host {
            let kept = crate::ui::agent::drafts::load(&d.host);
            if !self.draft_for.is_empty() || self.input.trim().is_empty() {
                self.input = kept;
            }
            self.draft_for = d.host.clone();
            self.draft_saved = self.input.clone();
        } else if self.input != self.draft_saved {
            crate::ui::agent::drafts::save(&d.host, &self.input);
            self.draft_saved = self.input.clone();
        }
        let mut run: Vec<(Lang, String)> = std::mem::take(&mut self.st.lock().unwrap().outbox);
        if self.st.lock().unwrap().loaded_for != d.host {
            self.load(backend, ctx, &d.host);
            load_know(&self.know, backend, ctx, d);
            self.show_know = None;
        }
        self.poll(backend, ctx);
        let (live, starting, loading, spent) = {
            let s = self.st.lock().unwrap();
            let (mut api, mut plan) = (0.0, 0.0);
            for t in &s.turns {
                if let Some(c) = s.costs.get(&t.id) {
                    *if s.subs.contains(&t.id) { &mut plan } else { &mut api } += c;
                }
            }
            let spent = (api, plan);
            (s.live.keys().cloned().collect::<Vec<_>>(), s.starting, s.loading, spent)
        };
        let going = live.len() + starting;

        let (mut stop_all, mut reload) = (false, false);
        let mut stop: Option<String> = None;
        // A phone's width (operator, 2026-10-15: "Cove looks rough on
        // mobile"): the strip keeps the model, what is running and Stop; the
        // switches move into a ⋯ menu instead of drawing over each other.
        let narrow = ui.available_width() < 640.0;
        // One row (operator, 2026-11-17: "the this site box of two rows of
        // mainly nothing is wildly wasteful of space"), grouped so it does not
        // read as muddled (a person's request, 2026-10-18, which had split it
        // in two): whose thread and how the next message goes out on the
        // left, a divider, what is going on and the actions on the right.
        let mut auto = self.auto_run.load(Ordering::Relaxed);
        let switches = |ui: &mut egui::Ui, share: &mut bool, auto: &mut bool| {
            ui.checkbox(share, RichText::new("see console").size(11.5))
                .on_hover_text("send the end of the R console with each message, so the agent sees what you ran and what it printed");
            ui.checkbox(auto, RichText::new("auto-run").size(11.5))
                .on_hover_text("run the code the agent replies with in the console as soon as it arrives - off: a ▶ under each block runs it");
        };
        // On a phone the row is there only while something runs, to show it
        // and stop it (operator, 2026-11-13: "this site when running stop
        // refresh ... I'm not entirely sure that whole row is also necessary on
        // mobile"): idle, the thread has the room. The console's switches stay
        // at their saved values; the thread reloads by itself.
        // Folded by hand, or a pane too short to read a thread in: then only
        // the composer, its site/Exec row at the top - a short Agents window
        // had cut the composer off at its bottom (operator's screenshot,
        // 2026-11-17: "only show the cove/exec row and the text box ... we
        // want to look at the files as they move and the app").
        let short = ui.available_height() < 240.0;
        let folded = self.thread_folded || short;
        self.short_pane = short;
        if folded {
        } else if narrow {
            if !live.is_empty() || going > 0 {
                theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
                    ui.add_space(6.0);
                    theme::state_mark(ui, theme::State::Active, &format!("{going} running"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(4.0);
                        if !live.is_empty() {
                            stop_all = theme::tool(ui, "■ Stop", "stop every run going in this thread").clicked();
                        }
                    });
                });
            }
        } else {
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(6.0);
            // Whose thread, not the domain again: the project switcher above
            // names it (operator, 2026-10-25: "many repetitive views of the
            // domain name"). The host stays on hover.
            let head = self.scope_label.clone().unwrap_or_else(|| "this site".to_owned());
            ui.add(egui::Label::new(RichText::new(head).monospace().size(11.5).strong().color(theme::text_strong())).truncate()).on_hover_text(format!("the agent for {}", d.host));
            if !narrow {
                theme::tool_sep(ui);
                ui.spacing_mut().item_spacing.x = 8.0;
                switches(ui, &mut self.share_console, &mut auto);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                ui.spacing_mut().item_spacing.x = 10.0;
                if !narrow && theme::tool(ui, if self.split { "Console ✓" } else { "Console" }, "the R console under the agent - watch what it runs").clicked() {
                    self.split = !self.split;
                }
                // A phone keeps one row (operator, 2026-11-07: a second row
                // held only the ⋯ menu - "poor use of white space"): the
                // switches and the console go into the ⋯ at this row's end.
                if narrow {
                    ui.menu_button(RichText::new("•••").size(17.0), |ui| {
                        switches(ui, &mut self.share_console, &mut auto);
                        ui.separator();
                        if ui.button(if self.split { "Hide the console" } else { "Console under the agent" }).clicked() {
                            self.split = !self.split;
                            ui.close();
                        }
                    });
                }
                ui.label(RichText::new("?").size(12.0).color(theme::hush())).on_hover_ui(trust_legend);
                reload = theme::tool(ui, "⟳", "load the thread again (turns from the in-app chat too)").clicked();
                if !live.is_empty() {
                    stop_all = theme::tool(ui, if narrow { "■ Stop runs" } else { "■ Stop this site's runs" }, "stops every agent run going in this site's thread - not other sites, not Exec, and not the app").clicked();
                }
                if going > 0 {
                    theme::state_mark(ui, theme::State::Active, &format!("{going} running"));
                } else {
                    ui.label(RichText::new("idle").size(11.0).color(theme::hush()));
                }
                if !narrow {
                    let (api, plan) = spent;
                    if plan > 0.0 {
                        ui.label(RichText::new(format!("≈${plan:.2} on your plan")).monospace().size(11.0).color(theme::hush()))
                            .on_hover_text("what the turns shown here that your plan paid for would have cost on the API - not money spent");
                    }
                    if api > 0.0 {
                        ui.label(RichText::new(format!("${api:.2}")).monospace().size(11.0).color(theme::hush())).on_hover_text("what the turns shown here billed to the API cost");
                    }
                }
            });
        });
        }
        self.auto_run.store(auto, Ordering::Relaxed);
        if stop_all {
            for id in &live {
                self.stop(backend, ctx, id.clone());
            }
        }
        if reload {
            self.st.lock().unwrap().loaded_for.clear();
        }

        // What it works from, above the thread: open while the thread is empty.
        // One line until opened: the thread keeps the room.
        let open = *self.show_know.get_or_insert(false);
        let (mut toggle, mut picked) = (false, None);
        let mut resend: Option<String> = None;
        let (mut listen, mut listen_stop): (Option<(String, String)>, bool) = (None, false);
        {
            let k = self.know.lock().unwrap();
            if k.loaded_for == d.host && !folded {
                know_panel(ui, &k, open, &mut toggle, &mut self.open, &mut run);
            }
        }
        if toggle {
            self.show_know = Some(!open);
        }

        let input_id = egui::Id::new(("agent-input", self.ns()));
        let out_h = (ui.available_height() - self.composer_h - 12.0).max(60.0);
        if !folded {
        egui::Frame::new().fill(theme::bg()).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
            ui.set_height(out_h);
            // Selecting text past the top or bottom of the thread scrolls it,
            // faster the further out the pointer is, so one drag can take in
            // many turns (operator, 2026-10-18: "when i get to top of window
            // scroll copy stops"). Only for a drag that began in the thread.
            let r = self.thread_rect;
            let auto = ui.input(|i| {
                let (Some(p), Some(o)) = (i.pointer.interact_pos(), i.pointer.press_origin()) else { return 0.0 };
                if !i.pointer.primary_down() || !r.contains(o) || r.width() <= 0.0 {
                    return 0.0;
                }
                let edge = 14.0;
                if p.y < r.top() + edge {
                    -((r.top() + edge - p.y) * 0.6).clamp(3.0, 40.0)
                } else if p.y > r.bottom() - edge {
                    ((p.y - (r.bottom() - edge)) * 0.6).clamp(3.0, 40.0)
                } else {
                    0.0
                }
            });
            let mut area = egui::ScrollArea::vertical().id_salt(("agent-thread", self.ns())).stick_to_bottom(auto == 0.0).auto_shrink([false, false]);
            if auto != 0.0 {
                area = area.vertical_scroll_offset((self.thread_off + auto).max(0.0));
                ctx.request_repaint();
            }
            let out = area.show(ui, |ui| {
                // The thread's width, measured once: one wide thing (a long
                // path in a table) grows egui's layout for everything after it,
                // so every card and bubble is pinned to this instead.
                let tw = ui.available_width();
                ui.set_width(tw);
                let s = self.st.lock().unwrap();
                if loading {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new("loading the thread…").color(theme::muted()));
                    });
                }
                if !loading && s.turns.is_empty() {
                    if self.scope_label.is_some() {
                        ui.label(RichText::new(format!("Tell {}'s agent what to build. It weaves the stack's scientific software - EasyBuild modules, R and Python packages, the tools in the Terminal - into this app: it reads and edits the domain's files, runs code in the consoles, and shows the result in App. The same agent as the in-app chat.", d.host)).color(theme::hush()));
                    } else {
                        ui.add(egui::Label::new(RichText::new(format!("What should {} be?", d.host)).size(26.0).strong().color(theme::text_strong())).wrap());
                        ui.add_space(4.0);
                        ui.add(egui::Label::new(RichText::new("Say it in your own words. The agent builds it while you watch it change in App.").size(15.0).color(theme::text())).wrap());
                    }
                    ui.add_space(10.0);
                    picked = suggestions(ui, d.title == SITE_TITLE || d.title == "Blank");
                }
                let dt = ui.input(|i| i.stable_dt).min(0.1) as f64;
                for t in &s.turns {
                    let going = !t.done && (s.live.contains_key(&t.id) || t.id.starts_with('+'));
                    if !t.prompt.is_empty() {
                        ui.add_space(6.0);
                        // Someone else's turn - a site admin's Exec at work in
                        // this site - says so above it, in amber, and its
                        // bubble is amber, not yours.
                        if !t.by.is_empty() {
                            ui.allocate_ui_with_layout(egui::vec2(tw, 18.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.add_space(4.0);
                                ui.label(RichText::new(format!("{} · site admin, through Exec - not you", t.by)).size(12.5).strong().color(theme::amber()))
                                    .on_hover_text(format!("{} sent this from their own Exec, and it is working in this site", t.by));
                            });
                        }
                        user_bubble_from(ui, tw, shown_prompt(&t.prompt), false, !t.by.is_empty());
                        // Copy it, or send it again as a new run (operator,
                        // 2026-10-16: "click copy it ability and resend ability").
                        ui.allocate_ui_with_layout(egui::vec2(tw, 18.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(4.0);
                            if theme::tool(ui, "Resend", "send this message again - it starts a new run").clicked() {
                                resend = Some(shown_prompt(&t.prompt).to_owned());
                            }
                            if theme::tool(ui, "Copy", "copy this message").clicked() {
                                ui.ctx().copy_text(crate::ui::agent::spoken::plain(shown_prompt(&t.prompt)));
                            }
                            if t.at > 0 {
                                let (short, full) = sent_at(t.at, crate::ui::software::now_secs());
                                ui.add_space(6.0);
                                ui.label(RichText::new(short).size(12.5).color(theme::text())).on_hover_text(full);
                            }
                        });
                        // The pictures it was sent with, under it - a click opens one in Files.
                        if let Some(v) = s.shots.get(&t.id) {
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                let room = ui.available_width();
                                let used: f32 = v.iter().map(|(_, tex)| thumb_size(tex, 220.0, 150.0).x + 6.0).sum();
                                ui.add_space((room - used).max(0.0));
                                for (path, tex) in v {
                                    let size = thumb_size(tex, 220.0, 150.0);
                                    let r = ui.add(egui::Image::new((tex.id(), size)).corner_radius(6).sense(egui::Sense::click()));
                                    if r.on_hover_text(format!("{path}\nclick to open it in Files")).clicked() {
                                        self.open.push(path.clone());
                                    }
                                }
                            });
                        }
                    }
                    // The reply's text, revealed as it streams.
                    let total: usize = t.events.iter().filter(|e| e.kind == "text").map(|e| e.text.chars().count()).sum();
                    if going && !self.reveal.contains_key(&t.id) {
                        self.reveal.insert(t.id.clone(), 0.0);
                    }
                    let budget = match self.reveal.get_mut(&t.id) {
                        Some(r) => {
                            let left = total as f64 - *r;
                            // Fast enough that the steps after the words are never held back long.
                            *r = (*r + dt * (left * 8.0).max(600.0)).min(total as f64);
                            let b = *r as usize;
                            if b < total {
                                ctx.request_repaint();
                            } else if !going {
                                self.reveal.remove(&t.id);
                            }
                            b
                        }
                        None => usize::MAX,
                    };
                    if t.events.is_empty() && !going {
                        continue;
                    }
                    ui.add_space(3.0);
                    agent_card(ui, tw, |ui| {
                        let mut left = budget;
                        let mut group = 0usize;
                        let mut k = 0;
                        let ev = &t.events;
                        while k < ev.len() {
                            match ev[k].kind.as_str() {
                                "tool" => {
                                    let from = k;
                                    while k < ev.len() && ev[k].kind == "tool" {
                                        k += 1;
                                    }
                                    let live_here = going && k == ev.len();
                                    let key = (t.id.clone(), group);
                                    group += 1;
                                    let mut open = self.open_tools.contains(&key);
                                    if tool_group(ui, &ev[from..k], &mut open, live_here) {
                                        if open {
                                            self.open_tools.insert(key);
                                        } else {
                                            self.open_tools.remove(&key);
                                        }
                                    }
                                    continue;
                                }
                                "error" => {
                                    egui::Frame::new().fill(theme::alt().gamma_multiply(0.12)).stroke(egui::Stroke::new(1.0, theme::alt().gamma_multiply(0.5))).corner_radius(6).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(RichText::new(&ev[k].text).size(12.5).color(theme::alt()));
                                        crate::ui::setup::connect_button(ui, &ev[k].text, t.at == 0 && t.id.starts_with('+') || t.at >= crate::ui::setup::session_start());
                                    });
                                }
                                _ => {
                                    let n = ev[k].text.chars().count();
                                    crate::ui::kit::md::show(ui, crate::ui::kit::md::prefix(&ev[k].text, left), 13.0, &mut run);
                                    if left < n {
                                        // The rest has not typed in yet.
                                        break;
                                    }
                                    left = left.saturating_sub(n);
                                }
                            }
                            ui.add_space(3.0);
                            k += 1;
                        }
                        ui.horizontal(|ui| {
                            if going {
                                // Stop sits with the work it stops, not in the bubble.
                                if !t.id.starts_with('+') && crate::ui::kit::icons::button(ui, crate::ui::kit::icons::Icon::Stop, 22.0, theme::surface_hi(), theme::alt(), true).on_hover_text("stop this run").clicked() {
                                    stop = Some(t.id.clone());
                                }
                                ui.spinner();
                                let now = if t.id.starts_with('+') { "starting…".to_owned() } else { ev.iter().rev().find(|e| e.kind == "tool").map_or("thinking…".to_owned(), |e| match edit_target(&e.text) {
                                    // The file it is on, with its folder (request #165).
                                    Some(f) => format!("editing · {}", f.rsplitn(3, '/').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("/")),
                                    None => format!("working · {}", e.text.split(" · ").next().unwrap_or("")),
                                }) };
                                let now = if t.by.is_empty() { now } else { format!("{}'s Exec · {now}", t.by) };
                                ui.add(egui::Label::new(RichText::new(now).size(11.5).color(theme::muted())).truncate());
                            } else {
                                let p = provenance(ev);
                                if let Some(m) = p.marker {
                                    marker(ui, m);
                                }
                                for f in p.changed.iter().take(6) {
                                    let name = f.rsplit('/').next().unwrap_or(f);
                                    if ui.add(egui::Button::new(RichText::new(format!("✎ {name}")).size(11.0).color(theme::text())).fill(theme::surface_hi()).corner_radius(99)).on_hover_text(format!("open {f} in Files")).clicked() {
                                        self.open.push(f.clone());
                                    }
                                }
                                if p.changed.len() > 6 {
                                    ui.label(RichText::new(format!("+{} more", p.changed.len() - 6)).size(11.0).color(theme::hush()));
                                }
                                if theme::tool(ui, "Copy", "copy this reply").clicked() {
                                    let text: Vec<&str> = ev.iter().filter(|e| e.kind == "text").map(|e| e.text.as_str()).collect();
                                    ui.ctx().copy_text(text.join("\n\n"));
                                }
                                // Listen: the whole reply, read aloud (brief.rs
                                // read_aloud) - preparing, then Stop while it plays.
                                if crate::ui::agent::voice::speak::shown() {
                                    let mine = s.reading.as_ref().filter(|(id, _)| *id == t.id).map(|(_, st)| st.clone());
                                    match mine {
                                        Some(Ok(false)) => {
                                            ui.spinner();
                                            ui.label(RichText::new("preparing to read…").size(11.5).color(theme::text()));
                                        }
                                        Some(Ok(true)) if crate::ui::agent::voice::play::playing() => {
                                            ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
                                            if theme::tool(ui, "■ Stop", "stop reading this reply").clicked() {
                                                crate::ui::agent::voice::play::stop();
                                                listen_stop = true;
                                            }
                                        }
                                        other => {
                                            let (label, tip) = match &other {
                                                Some(Err(e)) => ("▶ Listen - couldn't read it".to_owned(), format!("{e}\nclick to try again")),
                                                _ => ("▶ Listen".to_owned(), "read this reply aloud".to_owned()),
                                            };
                                            if theme::tool(ui, &label, &tip).clicked() {
                                                let text: Vec<&str> = ev.iter().filter(|e| e.kind == "text").map(|e| e.text.as_str()).collect();
                                                listen = Some((t.id.clone(), text.join("\n\n")));
                                            }
                                        }
                                    }
                                }
                                if let Some(c) = s.costs.get(&t.id) {
                                    let (text, tip) = if s.subs.contains(&t.id) {
                                        (format!("≈${c:.2} on your plan"), "what this turn would have cost on the API; your plan paid for it")
                                    } else {
                                        (format!("${c:.2}"), "what this turn cost, billed to the API")
                                    };
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(text).monospace().size(10.5).color(theme::hush())).on_hover_text(tip);
                                    });
                                }
                            }
                        });
                    });
                }
                ui.add_space(8.0);
            });
            self.thread_rect = out.inner_rect;
            self.thread_off = out.state.offset.y;
        });
        }

        if let Some(id) = stop {
            self.stop(backend, ctx, id);
        }
        // Never blocked: a message sent while others work starts its own run
        // beside them, as exec's composer does.
        let mut send = false;
        if ui.memory(|m| m.has_focus(input_id)) {
            send = ui.input_mut(|i| {
                let mut hit = false;
                i.events.retain(|e| match e {
                    egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if !modifiers.shift && !hit => {
                        hit = true;
                        false
                    }
                    _ => true,
                });
                hit
            });
        }
        let composer_top = ui.cursor().top();
        if let Some(r) = crate::ui::kit::paste::take(&format!("agent-{}", self.ns())) {
            match r {
                Ok(t) if !t.is_empty() => {
                    if !self.input.is_empty() && !self.input.ends_with(char::is_whitespace) {
                        self.input.push(' ');
                    }
                    self.input.push_str(&t);
                    self.paste_err = None;
                }
                Ok(_) => self.paste_err = Some("Paste: the clipboard has no text".into()),
                Err(e) => self.paste_err = Some(format!("Paste: {e}")),
            }
        }
        if let Some(e) = self.mic.error().or(self.paste_err.as_deref()) {
            ui.label(RichText::new(e).size(11.0).color(theme::alt()));
        }
        // The composer, one rounded box (operator, 2026-10-15: "could be
        // improved and cleaner"): what is attached, the words on a full-width
        // line, then + (attach), the mic and send under them - nothing beside
        // the text to push Send off a phone's edge.
        let uploading = self.attached.lock().unwrap().iter().any(|f| f.path.is_none() && f.error.is_none());
        let ready = !self.input.trim().is_empty() || self.attached.lock().unwrap().iter().any(|f| f.path.is_some());
        self.exec_bar(ui, narrow);
        egui::Frame::new()
            .fill(theme::surface())
            .stroke(egui::Stroke::new(1.0, if ui.memory(|m| m.has_focus(input_id)) { theme::primary().gamma_multiply(0.6) } else { theme::pane_brd() }))
            .corner_radius(14)
            .inner_margin(egui::Margin { left: 10, right: 8, top: 6, bottom: 6 })
            .outer_margin(egui::Margin { left: 6, right: 6, top: 4, bottom: 6 })
            .show(ui, |ui| {
                // Never wider than the pane: on a phone, four attached
                // screenshots widened this box past the screen and Send sat
                // off its right edge (operator, 2026-10-15: "can't submit on
                // mobile").
                // And never past what is on screen: a too-wide row elsewhere
                // could still have widened the layout under it.
                let w = ui.available_width().min(ui.clip_rect().right() - ui.cursor().left() - 2.0).max(120.0);
                ui.set_width(w);
                ui.set_max_width(w);
                self.attachment_chips(ui);
                let hint = match (narrow, going > 0) {
                    (true, _) => "Message the agent…",
                    (false, true) => "Send another - it runs alongside the ones working · Shift+Enter for a new line",
                    (false, false) => "Tell the agent what to build · Enter sends · Shift+Enter for a new line",
                };
                // Long text scrolls inside the box, which stops growing at
                // about 40% of the pane: a pasted recipe grew it past the
                // screen and Send went with it, with nothing left to scroll
                // (operator, 2026-11-18: "the submit button disappeared and I
                // couldn't scroll or do anything").
                let max_h = (ui.clip_rect().height() * 0.4).clamp(64.0, 360.0);
                let input = &mut self.input;
                // Dictated words in their own ink and italic, typed ones in
                // the strong ink (operator, 2026-11-19: "a clearer color on
                // the screen for transcribed audio or typed in").
                let spoken_now = self.spoken.clone();
                let mut layouter = |ui: &egui::Ui, text: &dyn egui::TextBuffer, wrap: f32| {
                    let job = crate::ui::agent::spoken::composer_job(text.as_str(), &spoken_now, &egui::FontId::proportional(13.5), theme::text_strong(), theme::net(), wrap);
                    crate::ui::agent::spoken::galley(ui, job)
                };
                let box_r = egui::ScrollArea::vertical()
                    .id_salt(("agent-input", input_id))
                    .max_height(max_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(input)
                                .id(input_id)
                                .layouter(&mut layouter)
                                .frame(egui::Frame::NONE)
                                .desired_rows(if narrow { 1 } else { 2 })
                                .desired_width(ui.available_width())
                                .font(egui::FontId::proportional(13.5))
                                .return_key(Some(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, Key::Enter)))
                                .hint_text(RichText::new(hint).color(theme::hush())),
                        )
                    })
                    .inner;
                // How long it is, once it is long enough to scroll.
                let lines = self.input.lines().count();
                if lines > 12 {
                    ui.label(RichText::new(format!("{lines} lines · {} characters", self.input.chars().count())).size(11.0).color(theme::hush()));
                }
                // An empty thread is a question waiting: the cursor is in the
                // answer. Not on a phone, where it would throw up the keyboard
                // over the question.
                // Nor on a headset, where a focus taken without a click
                // brings up no keyboard and the box only looks ready.
                let empty = { let s = self.st.lock().unwrap(); !s.loading && s.loaded_for == d.host && s.turns.is_empty() };
                // On a headset a real button lies over the box until it has
                // the cursor: its click is what lets the Quest's keyboard up
                // (paste.rs, web/index.html "keys:").
                if crate::ui::kit::device::headset() {
                    let keys = format!("keys:agent-{}", self.ns());
                    if crate::ui::kit::paste::take(&keys).is_some() {
                        box_r.request_focus();
                    } else {
                        // Only the part on screen, and never over a window
                        // drawn above the box: the button would take its click.
                        let r = box_r.rect.intersect(ui.clip_rect());
                        let on_top = ctx.layer_id_at(r.center()) == Some(ui.layer_id());
                        if !box_r.has_focus() && !ctx.any_popup_open() && on_top && r.is_positive() {
                            crate::ui::kit::paste::place(ctx, &keys, r);
                        }
                    }
                }
                if empty && !narrow && !crate::ui::kit::device::coarse() && self.scope_label.is_none() && self.focused_for != d.host {
                    self.focused_for = d.host.clone();
                    box_r.request_focus();
                }
                ui.horizontal(|ui| {
                    // In Go the mic and Send are the big, thumb-first pair -
                    // the one record button and the one Send on the screen
                    // (operator, 2026-10-18: "two send buttons and recording
                    // buttons ... seems like a lot"). Attach is the same size
                    // beside them (operator, 2026-10-29: "+ button on go too
                    // small").
                    // On a phone the IDE's composer is the thumb's too: 44
                    // points, a touch target, between the desk's 32 and
                    // Go's 56 (operator, 2026-11-13: "we used to have the go
                    // mode which had larger icons").
                    let size = if self.big { 56.0 } else if narrow { 44.0 } else { 32.0 };
                    if crate::ui::kit::icons::quiet(ui, crate::ui::kit::icons::Icon::Plus, size).on_hover_text("attach files or supporting docs - they go into this domain's supporting_docs/ and the agent reads them").clicked() {
                        self.attach_pick(backend, ctx, &d.path);
                    }
                    let Self { mic, input, .. } = self;
                    mic.keep = Some(d.path.clone());
                    mic.ui_sized(ui, ctx, backend, input, size);
                    if let Some(t) = mic.take_last() {
                        self.spoken.push(t);
                    }
                    // Listen / Act as I talk, the one icon right of the mic.
                    if let Some((mode, enabled, tip)) = &self.listen {
                        let r = crate::ui::agent::voice::listen_button(ui, *mode, *enabled, tip, size);
                        if r.clicked() {
                            self.listen_clicked = true;
                        }
                    }
                    crate::ui::agent::voice::speak_button(ui, size);
                    // Paste on a touch screen, where the browser's own Paste
                    // never comes up over Cove's box (paste.rs).
                    if crate::ui::kit::paste::wanted() {
                        let r = ui.add(egui::Button::new(RichText::new("Paste").size(13.0)).min_size(egui::vec2(0.0, size)));
                        if !ctx.any_popup_open() {
                            crate::ui::kit::paste::place(ctx, &format!("agent-{}", self.ns()), r.rect);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let r = crate::ui::kit::icons::button(ui, crate::ui::kit::icons::Icon::Send, size, theme::primary(), theme::bg(), ready && !uploading);
                        if r.on_hover_text(if uploading { "waiting for the files to upload" } else { "send · Enter" }).clicked() {
                            send = true;
                        }
                    });
                });
            });
        let h = ui.cursor().top() - composer_top;
        if (h - self.composer_h).abs() > 0.5 {
            self.composer_h = h;
            ctx.request_repaint();
        }
        if let Some(q) = picked {
            self.input = q.to_owned();
            send = true;
        }
        if let Some(q) = resend {
            self.input = q;
            send = true;
        }
        if listen_stop {
            self.st.lock().unwrap().reading = None;
        }
        if let Some((id, text)) = listen {
            crate::ui::agent::voice::play::stop();
            self.st.lock().unwrap().reading = Some((id.clone(), Ok(false)));
            let st = self.st.clone();
            backend.query(Query::ReadAloud { text }, ctx, move |r: Result<crate::api::Spoken, String>| {
                let mut s = st.lock().unwrap();
                // Only if it is still the reply asked for: another Listen took over.
                if s.reading.as_ref().is_some_and(|(r_id, _)| *r_id == id) {
                    match r {
                        Ok(sp) if !sp.audio.is_empty() => {
                            crate::ui::agent::voice::play::start(&sp.audio);
                            s.reading = Some((id, Ok(true)));
                        }
                        Ok(_) => s.reading = Some((id, Err("nothing came back to play".into()))),
                        Err(e) => s.reading = Some((id, Err(e))),
                    }
                }
            });
        }
        if send && !uploading {
            self.send(backend, ctx, &d.host, console_tail);
        }
        run
    }

    /// The execution bar, right above the message box (operator, 2026-10-28,
    /// with two screenshots: "the exec button below would be better above"):
    /// who the turn goes to (this site or Exec), the model and how it may act
    /// (Auto, Plan, Ask). These are settings of the session, not of the
    /// message, so they sit apart from what is typed, and the top strip gives
    /// their room to the thread. It folds to one line on a phone.
    fn exec_bar(&mut self, ui: &mut egui::Ui, narrow: bool) {
        let model = MODELS.iter().find(|(m, _)| *m == self.model).map_or(self.model.as_str(), |(_, l)| l);
        let model = if narrow { model.split(" · ").next().unwrap_or(model) } else { model }.to_owned();
        let model = model.as_str();
        let (_, mode, tip) = MODES.iter().find(|(m, _, _)| *m == self.mode).copied().unwrap_or(MODES[0]);
        // Never wider than the pane: one row too wide widened the whole
        // column, and on a phone the message box and Send went off the edge
        // with it (operator, 2026-11-27, IMG_8990: "buttons in ui are going
        // off screen"). On a phone the row keeps who, model and mode; what it
        // is about, Listen's interval, fold and the thread switch go in its ⋯
        // (ui/kit/bar.rs: one row per bar, the rest in ⋯).
        // Clipped at the sides only: a touch screen's controls are taller than
        // a fixed row, and a height clip cut the bottom of the Exec | site
        // toggle off (IMG_8992, 2026-11-27).
        let w = ui.available_width();
        ui.horizontal(|ui| {
            ui.set_max_width(w);
            let mut clip = ui.clip_rect();
            clip.max.x = clip.max.x.min(ui.max_rect().left() + w);
            ui.set_clip_rect(clip);
            ui.add_space(8.0);
            ui.spacing_mut().item_spacing.x = 6.0;
            if self.exec_folded {
                let who = match &self.scope {
                    Some(sc) if sc.exec_on => format!("Exec{}", if sc.exec_busy { " ●" } else { "" }),
                    Some(Scope { site: Some(h), .. }) => site_word(h),
                    _ => String::new(),
                };
                let line = [who.as_str(), model, mode].iter().filter(|w| !w.is_empty()).copied().collect::<Vec<_>>().join(" · ");
                if ui.add(egui::Label::new(RichText::new(format!("{line}  ▾")).size(11.5).color(theme::muted())).truncate().sense(egui::Sense::click()))
                    .on_hover_text("show who this goes to, the model and how it may act")
                    .clicked()
                {
                    self.exec_folded = false;
                }
                return;
            }
            if let Some(sc) = &self.scope {
                if let Some(all) = scope_toggle(ui, sc, false) {
                    self.scope_pick = Some(all);
                }
            }
            // What it is about: the site, the file open in Files, or the
            // folder its tree is in - Files' own composer, folded in here.
            let about = self.scope.as_ref().is_none_or(|sc| !sc.exec_on);
            let short = |p: &str| p.rsplit('/').next().unwrap_or(p).to_owned();
            if self.focus == 1 && self.focus_file.is_none() {
                self.focus = 0;
            }
            let focus_rows = |ui: &mut egui::Ui, focus: &mut u8, file: &Option<String>, folder: &str| {
                ui.selectable_value(focus, 0, "whole site").on_hover_text("the agent may work anywhere in the project");
                if let Some(f) = file.clone() {
                    ui.selectable_value(focus, 1, format!("file · {}", short(&f))).on_hover_text(format!("change only {f} - the file open in Files"));
                }
                if !folder.is_empty() {
                    let d = folder.to_owned();
                    ui.selectable_value(focus, 2, format!("folder · {}/", short(d.trim_end_matches('/')))).on_hover_text(format!("read and edit only in {d} - the folder Files shows"));
                }
            };
            if about && !narrow {
                let shown = match self.focus {
                    1 => format!("file · {}", short(self.focus_file.as_deref().unwrap_or(""))),
                    2 => format!("folder · {}/", short(self.focus_folder.trim_end_matches('/'))),
                    _ => "whole site".to_owned(),
                };
                let (file, folder) = (self.focus_file.clone(), self.focus_folder.clone());
                egui::ComboBox::from_id_salt(("agent-focus", self.ns()))
                    .width(150.0)
                    .selected_text(RichText::new(shown).size(11.5))
                    .show_ui(ui, |ui| focus_rows(ui, &mut self.focus, &file, &folder))
                    .response
                    .on_hover_text("what this message is about");
            }
            let picked = egui::ComboBox::from_id_salt(("agent-model", self.ns()))
                .width(if narrow { 76.0 } else { 200.0 })
                .height(480.0)
                .selected_text(RichText::new(model).size(11.5))
                .show_ui(ui, |ui| model_menu(ui, &mut self.model))
                .response;
            // Anthropic's Commercial Terms D.3 (tell people Claude's factual
            // claims are not to be relied on unchecked), in the picker's help
            // bubble and only while Claude is the agent - not a line under the
            // composer on every turn (operator, 2026-11-22: "maybe in a help
            // bubble and not when other agents are used").
            let claude = !["gpt-", "ollama-", "opencode"].iter().any(|p| self.model.starts_with(p));
            picked.on_hover_text(if claude {
                "the model the agent runs on. It is Claude, an AI: check its work before you rely on it"
            } else {
                "the model the agent runs on"
            });
            egui::ComboBox::from_id_salt(("agent-mode", self.ns()))
                .width(if narrow { 52.0 } else { 70.0 })
                .selected_text(RichText::new(mode).size(11.5))
                .show_ui(ui, |ui| {
                    for (m, l, t) in MODES {
                        ui.selectable_value(&mut self.mode, m, l).on_hover_text(t);
                    }
                })
                .response
                .on_hover_text(format!("{mode}: {tip}"));
            // How often Listen hands over a piece, beside the mode it works with.
            if self.listen.is_some() && !narrow {
                crate::ui::agent::voice::piece_field(ui, &mut self.listen_secs, self.listen_left);
            }
            // The thread put away or brought back; while away, a run going
            // still shows here.
            if self.thread_folded || self.short_pane {
                if let Some(w) = self.doing() {
                    theme::state_mark(ui, theme::State::Active, &w);
                    ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                }
            }
            let thread_word = if self.thread_folded { "Show thread" } else { "Hide thread" };
            let thread_tip = if self.thread_folded { "the replies and what it works from, back above" } else { "only this panel: who it goes to and your message - the replies put away" };
            if narrow {
                let (file, folder) = (self.focus_file.clone(), self.focus_folder.clone());
                let listening = self.listen.is_some();
                let short_pane = self.short_pane;
                let (focus, secs) = (&mut self.focus, &mut self.listen_secs);
                let (mut fold, mut thread) = (false, false);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    bar::more(ui, "more: what it is about, Listen, fold, the thread", |ui| {
                        if about {
                            ui.label(RichText::new("About").size(11.5).color(theme::text()));
                            focus_rows(ui, focus, &file, &folder);
                            ui.separator();
                        }
                        if listening {
                            ui.label(RichText::new("Listen hands over every").size(11.5).color(theme::text()));
                            for p in crate::ui::agent::voice::PIECES {
                                ui.selectable_value(secs, p, crate::ui::agent::voice::piece_words(p));
                            }
                            ui.separator();
                        }
                        if ui.button("Fold to one line").clicked() {
                            fold = true;
                            ui.close();
                        }
                        if !short_pane && ui.button(thread_word).on_hover_text(thread_tip).clicked() {
                            thread = true;
                            ui.close();
                        }
                    });
                });
                if fold {
                    self.exec_folded = true;
                }
                if thread {
                    self.thread_folded = !self.thread_folded;
                }
                return;
            }
            if ui.add(egui::Button::new(RichText::new("▴").size(13.0).color(theme::hush())).frame(false)).on_hover_text("fold to one line").clicked() {
                self.exec_folded = true;
            }
            // A short pane has no room for the thread: no switch to offer.
            if !self.short_pane && theme::tool(ui, thread_word, thread_tip).clicked() {
                self.thread_folded = !self.thread_folded;
            }
        });
    }

    /// Attach a picture (a data URL: the App panel's screenshot, a paste): a
    /// thumbnail now, uploaded into `domain_dir`/supporting_docs/ as `name`.
    pub fn attach_image(&self, backend: &Backend, ctx: &egui::Context, domain_dir: &str, name: &str, data_url: &str) -> Result<(), String> {
        let b64 = data_url.split_once(',').map(|x| x.1).ok_or("not a picture")?;
        let bytes = crate::api::b64_decode(b64).ok_or("the picture did not decode")?;
        let img = image::load_from_memory(&bytes).map_err(|e| format!("not an image: {e}"))?.thumbnail(480, 480).to_rgba8();
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        let thumb = ctx.load_texture(format!("shot-{name}"), ci, egui::TextureOptions::LINEAR);
        let name = name.to_owned();
        self.attached.lock().unwrap().push(Attached { name: name.clone(), thumb: Some(thumb), ..Default::default() });
        let s = self.attached.clone();
        backend.upload(&format!("{domain_dir}/supporting_docs"), &name.clone(), bytes, ctx, move |r| {
            let mut v = s.lock().unwrap();
            if let Some(a) = v.iter_mut().find(|a| a.name == name && a.path.is_none() && a.error.is_none()) {
                match r {
                    Ok(p) => a.path = Some(p),
                    Err(e) => a.error = Some(e),
                }
            }
        });
        Ok(())
    }

    /// Pick files on this device and upload each into `domain_dir`/supporting_docs/.
    fn attach_pick(&self, backend: &Backend, ctx: &egui::Context, domain_dir: &str) {
        let (backend, ctx, slot) = (backend.clone(), ctx.clone(), self.attached.clone());
        let dir = format!("{domain_dir}/supporting_docs");
        let pick = async move {
            let Some(files) = crate::ui::kit::pick::files("Attach for the agent", true).await else { return };
            for f in files {
                let name = f.file_name();
                slot.lock().unwrap().push(Attached { name: name.clone(), ..Default::default() });
                ctx.request_repaint();
                let bytes = f.read().await;
                let (s, n) = (slot.clone(), name.clone());
                backend.upload(&dir, &n, bytes, &ctx, move |r| {
                    let mut v = s.lock().unwrap();
                    if let Some(a) = v.iter_mut().find(|a| a.name == name && a.path.is_none() && a.error.is_none()) {
                        match r {
                            Ok(p) => a.path = Some(p),
                            Err(e) => a.error = Some(e),
                        }
                    }
                });
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || pollster::block_on(pick));
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(pick);
    }

    /// The files attached to the message being written: name, a spinner while
    /// it uploads, × to drop it.
    fn attachment_chips(&self, ui: &mut egui::Ui) {
        let list = self.attached.lock().unwrap().clone();
        if list.is_empty() {
            return;
        }
        let mut drop = None;
        ui.horizontal_wrapped(|ui| {
            for (i, a) in list.iter().enumerate() {
                egui::Frame::new().fill(theme::surface_hi()).corner_radius(99).inner_margin(egui::Margin { left: 10, right: 4, top: 3, bottom: 3 }).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if let Some(tex) = &a.thumb {
                            ui.add(egui::Image::new((tex.id(), thumb_size(tex, 72.0, 48.0))).corner_radius(4));
                        }
                        if a.path.is_none() && a.error.is_none() {
                            ui.add(egui::Spinner::new().size(11.0));
                        }
                        let c = if a.error.is_some() { theme::alt() } else { theme::text() };
                        // Shortened here, not by the label: a truncating label
                        // takes all the width the row offers, so the chips never
                        // wrapped and pushed the composer (and Send) wider.
                        let r = ui.add(egui::Label::new(RichText::new(chip_name(&a.name)).size(11.5).color(c)));
                        if let Some(e) = &a.error {
                            r.on_hover_text(format!("not uploaded: {e}"));
                        } else if let Some(p) = &a.path {
                            r.on_hover_text(p);
                        }
                        if ui.add(egui::Button::new(RichText::new("×").size(12.0).color(theme::muted())).frame(false)).on_hover_text("take it off this message (the uploaded copy stays in supporting_docs/)").clicked() {
                            drop = Some(i);
                        }
                    });
                });
            }
        });
        if let Some(i) = drop {
            let mut v = self.attached.lock().unwrap();
            if i < v.len() {
                v.remove(i);
            }
        }
        ui.add_space(2.0);
    }
}

// ── what the agent works from, and how far to trust an answer ───────────────
//
// commons.ndexr.io runs posit-dev/commons, whose agents answer from three
// layers - trusted calculations, a context layer of documents, the raw data -
// and mark every answer by how it was produced: Trusted, Cited or Untrusted,
// derived from what the agent actually did, never from what it says
// (operator, 2026-10-15: "the agents in cove.ndexr.io should be inspired by
// commons.ndexr.io's posit commons app ... take the best ideas and themes").
// In Cove the domain's own docs are the context layer, the commands those docs
// give are the trusted recipes, and each finished turn gets a marker from its
// tool steps.

/// A command a domain's docs give, under the heading it sits beneath.
#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub label: String,
    pub lang: Lang,
    pub code: String,
    /// The doc it came from (a file name).
    pub from: String,
}

#[derive(Default)]
struct Know {
    loaded_for: String,
    loading: bool,
    /// The domain's docs: CLAUDE.md, CONTEXT.md, README, AGENTS*.md, docs/*.md.
    docs: Vec<String>,
    recipes: Vec<Recipe>,
}

/// Docs the agent reads as context, by name at a domain's root.
fn is_doc(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    matches!(n.as_str(), "claude.md" | "context.md" | "readme.md" | "agents.md") || (n.starts_with("agents") && n.ends_with(".md"))
}

fn load_know(know: &Arc<Mutex<Know>>, backend: &Backend, ctx: &egui::Context, d: &DomainInfo) {
    // A project not on this machine yet has no folder: listing "" read the
    // server's own working folder and asked for /CLAUDE.md, /README.md …
    if d.path.is_empty() {
        *know.lock().unwrap() = Know { loaded_for: d.host.clone(), ..Default::default() };
        return;
    }
    *know.lock().unwrap() = Know { loaded_for: d.host.clone(), loading: true, ..Default::default() };
    let (k, host, root, b, c) = (know.clone(), d.host.clone(), d.path.clone(), backend.clone(), ctx.clone());
    backend.query(Query::ListDir { path: root.clone() }, ctx, move |r: Result<Listing, String>| {
        let Ok(l) = r else {
            k.lock().unwrap().loading = false;
            return;
        };
        let mut docs: Vec<String> = l.entries.iter().filter(|e| !e.dir && is_doc(&e.name)).map(|e| format!("{root}/{}", e.name)).collect();
        // CLAUDE.md, then CONTEXT.md, then the rest.
        docs.sort_by_key(|p| match p.rsplit('/').next().unwrap_or("").to_ascii_lowercase().as_str() {
            "claude.md" => 0,
            "context.md" => 1,
            _ => 2,
        });
        {
            let mut g = k.lock().unwrap();
            if g.loaded_for != host {
                return;
            }
            g.docs = docs.clone();
            g.loading = false;
        }
        let has_docs = l.entries.iter().any(|e| e.dir && e.name == "docs");
        for p in docs.iter().take(4) {
            let (k, host) = (k.clone(), host.clone());
            b.query(Query::ReadFile { path: p.clone() }, &c, move |r: Result<FileText, String>| {
                let Ok(f) = r else { return };
                let mut g = k.lock().unwrap();
                if g.loaded_for != host {
                    return;
                }
                let from = f.path.rsplit('/').next().unwrap_or("").to_owned();
                for r in recipes(&f.text, &from) {
                    if g.recipes.len() < 12 && !g.recipes.iter().any(|x| x.code == r.code) {
                        g.recipes.push(r);
                    }
                }
            });
        }
        if has_docs {
            let (k, host, root) = (k.clone(), host.clone(), root.clone());
            b.query(Query::ListDir { path: format!("{root}/docs") }, &c, move |r: Result<Listing, String>| {
                let Ok(l) = r else { return };
                let mut g = k.lock().unwrap();
                if g.loaded_for == host {
                    g.docs.extend(l.entries.iter().filter(|e| !e.dir && e.name.ends_with(".md")).take(12).map(|e| format!("{root}/docs/{}", e.name)));
                }
            });
        }
    });
}

/// The short fenced blocks of a doc a console can run, each under the heading
/// above it: the commands the domain itself documents.
pub fn recipes(text: &str, from: &str) -> Vec<Recipe> {
    let mut out = Vec::new();
    let mut heading = String::new();
    let mut open: Option<(Lang, String)> = None;
    let mut skip = false;
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with("```") {
            match open.take() {
                Some((lang, code)) => {
                    let code = code.trim_end().to_owned();
                    let n = code.lines().count();
                    if (1..=8).contains(&n) && !code.contains('<') {
                        let label = if heading.is_empty() { from.to_owned() } else { heading.clone() };
                        out.push(Recipe { label, lang, code, from: from.to_owned() });
                    }
                }
                None if skip => skip = false,
                None => {
                    // An untagged fence under a "Check it" / "Run" / "Build"
                    // heading is a shell recipe too (commons.ndexr.io's are).
                    let tag = t.trim_start_matches('`').trim();
                    let h = heading.to_ascii_lowercase();
                    let shellish = tag.is_empty() && ["check", "run", "build", "test", "start", "install", "deploy", "verify", "usage"].iter().any(|w| h.contains(w));
                    match crate::ui::kit::md::fence_lang(tag).or(if shellish { Some(Lang::Shell) } else { None }) {
                        Some(l) => open = Some((l, String::new())),
                        None => skip = true,
                    }
                }
            }
            continue;
        }
        if let Some((_, code)) = &mut open {
            code.push_str(line);
            code.push('\n');
        } else if !skip && t.starts_with('#') {
            heading = t.trim_start_matches('#').trim().to_owned();
        }
    }
    out
}

/// How an answer was produced, read off its tool steps the way commons
/// derives its provenance outcome: deterministically, never from the model.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Marker {
    /// It only read and searched the domain: nothing changed, nothing ran.
    Looked,
    /// It changed files or ran commands, after reading the domain's docs.
    Grounded,
    /// It changed files or ran commands without reading the docs: check it.
    Unchecked,
}

#[derive(Default)]
pub struct Provenance {
    pub marker: Option<Marker>,
    /// The files it wrote or edited, in order, once each.
    pub changed: Vec<String>,
}

/// The file a tool step writes or edits ("Edit · /d/x/module.r"), if it is
/// one of those steps.
pub fn edit_target(step: &str) -> Option<&str> {
    let (name, what) = step.split_once(" · ")?;
    (matches!(name, "Edit" | "Write" | "MultiEdit" | "NotebookEdit") && !what.is_empty()).then_some(what)
}

pub fn provenance(events: &[AgentEvent]) -> Provenance {
    let mut p = Provenance::default();
    let (mut tools, mut acted, mut grounded) = (0, false, false);
    for e in events.iter().filter(|e| e.kind == "tool") {
        tools += 1;
        let (name, what) = e.text.split_once(" · ").unwrap_or((e.text.as_str(), ""));
        match name {
            "Edit" | "Write" | "MultiEdit" | "NotebookEdit" => {
                acted = true;
                if !what.is_empty() && !p.changed.iter().any(|c| c == what) {
                    p.changed.push(what.to_owned());
                }
            }
            // A command that says it only looks ("Read every guide doc",
            // "Count the rows") is looking; any other command is acting.
            "Bash" if looks_only(what) => {}
            "Bash" | "Task" | "Agent" => acted = true,
            _ => {
                let w = what.to_ascii_lowercase();
                if ["claude.md", "context.md", "readme", "agents", "/docs/"].iter().any(|d| w.contains(d)) {
                    grounded = true;
                }
            }
        }
    }
    p.marker = match (tools, acted, grounded) {
        (0, _, _) => None,
        (_, false, _) => Some(Marker::Looked),
        (_, true, true) => Some(Marker::Grounded),
        (_, true, false) => Some(Marker::Unchecked),
    };
    p
}

/// A step summary that starts with a verb that only reads.
fn looks_only(what: &str) -> bool {
    const READ: [&str; 18] = ["read", "list", "count", "show", "check", "find", "print", "look", "inspect", "get", "search", "grep", "cat", "view", "verify", "see", "compare", "measure"];
    let first = what.split_whitespace().next().unwrap_or("").to_ascii_lowercase();
    READ.contains(&first.as_str())
}

impl Marker {
    fn look(self) -> (&'static str, &'static str, egui::Color32, &'static str) {
        match self {
            Marker::Looked => ("✓", "Looked only", theme::ok(), "It read and searched this domain (and ran only commands that look) and changed nothing - the answer comes from the domain's own files."),
            Marker::Grounded => ("“", "Grounded", theme::primary(), "It changed files or ran commands, after reading this domain's docs (CLAUDE.md, CONTEXT.md, docs/)."),
            Marker::Unchecked => ("!", "Check it", theme::warn(), "It changed files or ran commands without reading this domain's docs first - look over what it did."),
        }
    }
}

fn marker(ui: &mut egui::Ui, m: Marker) {
    let (icon, word, color, tip) = m.look();
    egui::Frame::new().fill(color.gamma_multiply(0.14)).stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.6))).corner_radius(99).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
        ui.label(RichText::new(format!("{icon} {word}")).size(11.0).color(color));
    })
    .response
    .on_hover_text(tip);
}

fn trust_legend(ui: &mut egui::Ui) {
    ui.set_max_width(320.0);
    ui.label(RichText::new("Each finished answer is marked by what the agent did - read off its steps, not its words:").size(12.0));
    for m in [Marker::Looked, Marker::Grounded, Marker::Unchecked] {
        let (icon, word, color, tip) = m.look();
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(format!("{icon} {word}")).size(12.0).color(color));
            ui.label(RichText::new(tip).size(11.5).color(theme::muted()));
        });
    }
    ui.label(RichText::new("No marker: it answered without using a tool.").size(11.5).color(theme::muted()));
}

/// Where to start, on an empty thread: a click sends it.
const SUGGESTIONS: [&str; 4] = [
    "What is this domain, and how is it wired together?",
    "Run this domain's checks from its docs and tell me what fails.",
    "Which stack software does this app use, and what could it use?",
    "Add one small, useful feature to the app and show it in App.",
];

/// The title in the node-site starter's CONTEXT.md: a person's own site, not
/// yet told what it is.
pub const SITE_TITLE: &str = "Your site";

/// Where to start on that site. What the button says, and what it sends: enough
/// for the agent to build something real at once, for a person to correct.
const SITE_IDEAS: [(&str, &str); 4] = [
    ("A portfolio of my work", "Make this a portfolio of my work: a strong headline, three project cards and a contact link. Write believable placeholder text I can replace."),
    ("A landing page for my idea", "Make this a landing page for an idea of mine: a headline, three reasons it matters and a sign-up box. Invent a plausible idea, then ask me for the real one."),
    ("A small game", "Make this a small game I can play with a mouse or a finger, with a score and a restart button. Keep it simple and make it feel good."),
    ("A live chart of something interesting", "Make this a page with a large animated chart of something interesting, with a control I can drag to change what it shows. Generate the data in the page."),
];

fn suggestions(ui: &mut egui::Ui, site: bool) -> Option<&'static str> {
    let mut picked = None;
    let mut offer = |ui: &mut egui::Ui, says: &'static str, sends: &'static str| {
        let b = egui::Button::new(RichText::new(says).size(13.5).color(theme::text())).fill(theme::surface().gamma_multiply(0.85)).stroke(egui::Stroke::new(1.0, theme::pane_brd())).corner_radius(10).min_size(egui::vec2(ui.available_width().min(460.0), 32.0));
        if ui.add(b).on_hover_text("send this to the agent").clicked() {
            picked = Some(sends);
        }
        ui.add_space(4.0);
    };
    if site {
        for (says, sends) in SITE_IDEAS {
            offer(ui, says, sends);
        }
    } else {
        for q in SUGGESTIONS {
            offer(ui, q, q);
        }
    }
    picked
}

/// The panel above the thread: the docs the agent reads (a click opens one in
/// Files) and the recipes they give (▶ runs one in its console - trusted
/// code, nothing the model wrote).
fn know_panel(ui: &mut egui::Ui, k: &Know, open: bool, toggle: &mut bool, opens: &mut Vec<String>, run: &mut Vec<(Lang, String)>) {
    egui::Frame::new().fill(theme::surface().gamma_multiply(0.7)).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let head = if k.loading {
            "Works from … reading the domain".to_owned()
        } else {
            format!("{} Works from · {} doc{} · {} recipe{}", if open { "⌄" } else { "›" }, k.docs.len(), if k.docs.len() == 1 { "" } else { "s" }, k.recipes.len(), if k.recipes.len() == 1 { "" } else { "s" })
        };
        if ui.add(egui::Label::new(RichText::new(head).size(11.5).color(theme::muted())).sense(egui::Sense::click())).on_hover_text("the domain's docs the agent reads, and the commands they give").clicked() {
            *toggle = true;
        }
        if !open || k.loading {
            return;
        }
        ui.add_space(2.0);
        if k.docs.is_empty() {
            ui.label(RichText::new("No CLAUDE.md, CONTEXT.md or docs/ here yet - the agent works from the code alone. Ask it to write a CONTEXT.md.").size(11.5).color(theme::hush()));
        }
        ui.horizontal_wrapped(|ui| {
            for p in &k.docs {
                let name = p.rsplit('/').next().unwrap_or(p);
                let name = if p.contains("/docs/") { format!("docs/{name}") } else { name.to_owned() };
                if ui.add(egui::Button::new(RichText::new(format!("“ {name}")).size(11.0).color(theme::primary())).fill(theme::primary().gamma_multiply(0.08)).corner_radius(99)).on_hover_text(format!("context the agent reads - open {p} in Files")).clicked() {
                    opens.push(p.clone());
                }
            }
        });
        if k.recipes.is_empty() {
            return;
        }
        ui.add_space(4.0);
        egui::ScrollArea::vertical().id_salt(("agent-recipes", k.loaded_for.as_str())).max_height(120.0).auto_shrink([false, true]).show(ui, |ui| {
            for r in &k.recipes {
                ui.horizontal(|ui| {
                    let to = match r.lang {
                        Lang::Shell => "Terminal",
                        _ => "Console",
                    };
                    if theme::tool(ui, "▶", &format!("run it in the {to} - from {}, not written by the model", r.from)).clicked() {
                        run.push((r.lang, r.code.clone()));
                    }
                    ui.label(RichText::new(&r.label).size(11.5).color(theme::text()));
                    let first = r.code.lines().next().unwrap_or("");
                    let more = if r.code.lines().count() > 1 { " …" } else { "" };
                    ui.add(egui::Label::new(RichText::new(format!("{first}{more}")).monospace().size(10.5).color(theme::hush())).truncate()).on_hover_text(&r.code);
                });
            }
        });
    });
}

/// A thumbnail's size within `w` × `h`, its shape kept.
fn thumb_size(tex: &egui::TextureHandle, w: f32, h: f32) -> egui::Vec2 {
    let [tw, th] = tex.size();
    let k = (w / tw.max(1) as f32).min(h / th.max(1) as f32).min(1.0);
    egui::vec2(tw as f32 * k, th as f32 * k)
}

/// What the person asked: a teal bubble on the right, as on the Cove
/// landing page's cards. True when its ■ (stop this run) was clicked.
/// The prompt as the person typed it: a turn sent with the console's tail
/// carries that after a rule (with_console) - it is for the agent, not the bubble.
pub(crate) fn shown_prompt(p: &str) -> &str {
    // The project's brief Cove puts in front of a turn (host.rs, "[This
    // project - from Cove, not typed by the person]" ... "\n---\n") is the
    // agent's, not the bubble's: the Sites wall showed it as every prompt.
    let p = match p.strip_prefix("[This project - from Cove, not typed by the person]") {
        Some(rest) => rest.split_once("\n---\n").map_or(p, |(_, typed)| typed),
        None => p,
    };
    let p = p.split("\n\n---\nI am working in ").next().unwrap_or(p);
    let p = p.split("\n\nAttached to this message (uploaded into").next().unwrap_or(p).trim();
    // The notes that tell the agent where words came from are for it, not the
    // bubble: the spoken pieces' ink says it there. Listen's own note opens a
    // piece it tagged whole.
    let p = p.strip_prefix(crate::ui::agent::spoken::NOTE).map(str::trim_start).unwrap_or(p);
    if p.starts_with('(') && p.contains(crate::ui::agent::spoken::OPEN) && (p.starts_with("(Said aloud in Cove") || p.starts_with("(Presenting: said aloud")) {
        if let Some((_, rest)) = p.split_once("\n\n") {
            return rest.trim_start();
        }
    }
    p
}

/// `other`: sent by someone else (a site admin's Exec), in amber so it never
/// reads as the viewer's own.
fn user_bubble_from(ui: &mut egui::Ui, w: f32, text: &str, stoppable: bool, other: bool) -> bool {
    let tint = if other { theme::amber() } else { theme::primary() };
    let mut stop = false;
    let max = (w * if w < 520.0 { 0.9 } else { 0.78 }).max(120.0);
    // Sized to its words: a nested right-to-left layout inside the bubble
    // stretched it to the whole height left, a tall empty box on a phone.
    let font = egui::FontId::proportional(13.0);
    let text_w = ui.fonts_mut(|f| f.layout(crate::ui::agent::spoken::plain(text), font.clone(), theme::text_strong(), max - 24.0)).size().x;
    let inner = text_w.max(if stoppable { 64.0 } else { 0.0 });
    ui.horizontal_top(|ui| {
        ui.add_space((w - inner - 22.0).max(0.0));
        egui::Frame::new()
            .fill(tint.gamma_multiply(0.13))
            .stroke(egui::Stroke::new(1.0, tint.gamma_multiply(0.35)))
            .corner_radius(egui::CornerRadius { nw: 12, ne: 12, sw: 12, se: 3 })
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                ui.set_width(inner);
                ui.vertical(|ui| {
                    // The person's domains in what they typed are links into
                    // Cove, as in a reply (md.rs typed).
                    // Dictated pieces in the cool ink, typed ones in the strong
                    // one, links live in both (spoken.rs).
                    let segs = crate::ui::agent::spoken::segments(text);
                    let parts: Vec<(&str, egui::Color32)> = segs.iter().map(|(said, t)| (*t, if *said { theme::net() } else { theme::text_strong() })).collect();
                    crate::ui::kit::md::typed_parts(ui, &parts, font.size, max - 24.0);
                    if stoppable {
                        stop = theme::tool(ui, "■ stop", "stop this run").clicked();
                    }
                });
            });
    });
    stop
}

/// The agent's side: a card holding the reply, full width, its contents top
/// to bottom. A Frame takes its parent's layout, so inside the old
/// `horizontal_top` every line of a reply sat side by side off the right
/// edge - a tall, empty-looking card (operator, 2026-10-15: "nothing fits in
/// the box, lots and lots of white space").
fn agent_card(ui: &mut egui::Ui, w: f32, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(theme::surface().gamma_multiply(0.85))
        .stroke(egui::Stroke::new(1.0, theme::pane_brd()))
        .corner_radius(egui::CornerRadius { nw: 3, ne: 10, sw: 10, se: 10 })
        .inner_margin(egui::Margin::symmetric(10, 6))
        .show(ui, |ui| {
            let inner = (w - 22.0).max(80.0);
            ui.set_width(inner);
            ui.set_max_width(inner);
            ui.vertical(|ui| {
                ui.set_max_width(inner);
                ui.spacing_mut().item_spacing.y = 3.0;
                body(ui);
            });
        });
}

/// A run of tool steps, folded to one quiet line ("4 steps · Read, Edit,
/// Bash"); a click opens the list. The group still working shows its steps
/// as they come, the last 12 (operator, 2026-10-15: "i did like the live
/// output"). True when it was toggled.
fn tool_group(ui: &mut egui::Ui, tools: &[AgentEvent], open: &mut bool, live: bool) -> bool {
    let mut names: Vec<&str> = Vec::new();
    for t in tools {
        let n = t.text.split(" · ").next().unwrap_or("");
        if !names.contains(&n) {
            names.push(n);
        }
    }
    let head = format!(
        "{} {} {} · {}",
        if *open { "⌄" } else { "›" },
        tools.len(),
        if tools.len() == 1 { "step" } else { "steps" },
        names.iter().take(5).copied().collect::<Vec<_>>().join(", ")
    );
    // Truncated, not as wide as it likes: a long head pushed the card past
    // the right edge of a phone (2026-10-15).
    let r = ui.add(egui::Label::new(RichText::new(head).size(11.5).color(theme::hush())).truncate().sense(egui::Sense::click()));
    let clicked = r.on_hover_text(if *open { "fold the steps" } else { "see each step" }).clicked();
    if clicked {
        *open = !*open;
    }
    if *open || live {
        let from = if *open { 0 } else { tools.len().saturating_sub(12) };
        egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 0, top: 1, bottom: 1 }).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            let last = tools.len() - 1;
            for (i, t) in tools.iter().enumerate().skip(from) {
                let color = if live && i == last { theme::text() } else { theme::muted() };
                ui.add(egui::Label::new(RichText::new(format!("▸ {}", t.text.trim_end().trim_end_matches('·').trim_end())).monospace().size(11.0).color(color)).truncate());
            }
        });
    }
    clicked
}

// ── a message about one file or folder (the Agents Focus menu) ─────────────


/// The folder a path is in.
pub fn folder_of(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some(("", _)) => "/",
        Some((d, _)) => d,
        None => ".",
    }
}

/// `file` is the open file, if any; `folder` the folder the Files tree is in.
/// Reach 0 is the file alone; 1.. the tree's folder and its levels below.
fn ask_prompt(domain: &str, file: Option<&str>, folder: &str, reach: u8, what: &str) -> String {
    let (reach, dir) = match file {
        Some(f) if reach == 0 => (0, folder_of(f).to_owned()),
        _ => (reach.max(1), folder.to_owned()),
    };
    let scope = match reach {
        0 => format!("Change only this file, {}. Read other files if you need to, but edit nothing else.", file.unwrap_or_default()),
        1 => format!("You may read and edit the files in {dir} (this folder only, not its subfolders); nothing outside it."),
        n => format!("You may read and edit files in {dir} and up to {} level{} of subfolders below it; nothing outside {dir}.", n - 1, if n == 2 { "" } else { "s" }),
    };
    let open = file.map(|f| format!("Open file: {f}\n")).unwrap_or_default();
    format!(
        "A request from Cove's Files, in {domain}.\n\n{open}Working folder: {dir}\nReach: {scope}\n\nRequest: {what}\n\nMake the change in the file(s) directly - the person is watching them update. Then say what you changed in one or two sentences."
    )
}


#[cfg(test)]
mod tests {
    /// What setup chose is what a new session starts on; anything not in the
    /// menus is ignored, never passed on as a model or a mode.
    #[test]
    fn setup_defaults_reach_a_new_session() {
        use super::*;
        set_defaults("gpt-5.5", "supervised");
        let p = AgentPane::default();
        assert_eq!((p.model.as_str(), p.mode), ("gpt-5.5", "supervised"));
        set_defaults("rm -rf /", "root");
        let p = AgentPane::default();
        assert_eq!((p.model.as_str(), p.mode), ("gpt-5.5", "supervised"), "unknown values leave the defaults as they were");
        let prefs = crate::ui::setup::Prefs { autonomy: "manual".into(), provider: "claude".into(), ..Default::default() };
        prefs.apply();
        let p = AgentPane::default();
        assert_eq!((p.model.as_str(), p.mode), (MODELS[0].0, "plan"), "Manual proposes and changes nothing");
        set_defaults("", "");
    }
    use super::*;

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_turn_says_when_it_was_sent() {
        // 2026-11-08 17:23:15 UTC and later the same day, then a day and a year on.
        let at = 1_794_158_595;
        assert_eq!(sent_at(at, at + 600).0, "17:23 UTC");
        assert_eq!(sent_at(at, at + 86_400).0, "Nov 8 · 17:23 UTC");
        assert_eq!(sent_at(at, at + 400 * 86_400).0, "Nov 8, 2026 · 17:23 UTC");
        assert_eq!(sent_at(at, at + 600).1, "sent Nov 8, 2026 at 17:23 UTC - 10 min ago");
        assert_eq!(sent_at(951_782_400, 951_782_400).0, "00:00 UTC", "2000-02-29, a leap day");
        assert_eq!(sent_at(951_782_400, 951_782_400 + 86_400).0, "Feb 29 · 00:00 UTC");
    }

    #[test]
    fn chip_names_stay_short_and_keep_the_extension() {
        assert_eq!(chip_name("IMG_8117.png"), "IMG_8117.png");
        let long = chip_name("a-screenshot-with-a-very-long-name.png");
        assert!(long.chars().count() <= 18 && long.ends_with("….png"), "{long}");
        assert!(chip_name("no_extension_but_far_too_long_to_show").chars().count() <= 18);
    }

    #[test]
    fn attachments_are_named_to_the_agent_and_kept_out_of_the_bubble() {
        let p = with_attachments("summarise these", &["/d/x.ndexr.io/supporting_docs/a.pdf".to_owned()]);
        assert!(p.starts_with("summarise these\n\nAttached to this message") && p.ends_with("\n- /d/x.ndexr.io/supporting_docs/a.pdf"));
        assert_eq!(with_attachments("plain", &[]), "plain");
        assert_eq!(shown_prompt(&with_console(&p, "> x")), "summarise these");
    }

    #[test]
    fn a_bubble_drops_the_notes_that_are_for_the_agent() {
        let said = format!("{}\n\nfix the header <spoken>and the cove one</spoken>", crate::ui::agent::spoken::NOTE);
        assert_eq!(shown_prompt(&said), "fix the header <spoken>and the cove one</spoken>");
        let listen = "(Said aloud in Cove while I keep talking - the last minute, transcribed. Do what you can.)\n\n<spoken>the cove one</spoken>";
        assert_eq!(shown_prompt(listen), "<spoken>the cove one</spoken>");
        assert_eq!(shown_prompt("(Said aloud by me, typed)"), "(Said aloud by me, typed)");
    }

    #[test]
    fn a_bubble_shows_what_was_typed_not_the_console() {
        assert_eq!(shown_prompt(&with_console("make it blue", "> x")), "make it blue");
        assert_eq!(shown_prompt("old\n\n---\nI am working in the Cove IDE. The end of"), "old");
        assert_eq!(shown_prompt("[This project - from Cove, not typed by the person]\nx.ndexr.io's app is the one in this folder. (AGENTS-shared.md 7b).\n---\nmake the plot blue\n\n---\nI am working in Cove"), "make the plot blue");
        assert_eq!(shown_prompt("  plain  "), "plain");
    }

    #[test]
    fn fenced_blocks_go_to_the_console_they_name() {
        let b = runnable("Try this:\n```r\nsummary(cars)\n```\nor\n```{python}\nprint(1)\n```\n```bash\nml av\n```\n```toml\nx = 1\n```");
        assert_eq!(b.len(), 4);
        assert_eq!(b[0], (Some(Lang::R), "summary(cars)".to_owned()));
        assert_eq!(b[1].0, Some(Lang::Python));
        assert_eq!(b[2], (Some(Lang::Shell), "ml av".to_owned()));
        assert_eq!(b[3].0, None);
    }

    #[test]
    fn a_file_ask_runs_at_its_folder_and_says_how_far_it_reaches() {
        assert_eq!(folder_of("/a/b/c.R"), "/a/b");
        assert_eq!(folder_of("/c.R"), "/");
        let p = ask_prompt("x.ndexr.io", Some("/d/x.ndexr.io/r/plot.R"), "/d/x.ndexr.io", 0, "make it blue");
        assert!(p.contains("Change only this file, /d/x.ndexr.io/r/plot.R") && p.contains("Request: make it blue"));
        // Reach 1.. is the tree's folder, not the file's.
        let p = ask_prompt("x", Some("/d/r/f.R"), "/d", 2, "w");
        assert!(p.contains("Working folder: /d\n") && p.contains("up to 1 level of subfolders"));
        assert!(ask_prompt("x", Some("/d/f.R"), "/d", 1, "w").contains("this folder only"));
        assert!(ask_prompt("x", None, "/d", 3, "w").contains("up to 2 levels"));
        // No file open: never "this file only".
        assert!(ask_prompt("x", None, "/d", 0, "w").contains("this folder only"));
    }


    #[test]
    fn recipes_are_the_short_runnable_blocks_under_their_heading() {
        let doc = "# X\n## Check it\n```bash\nsystemctl --user status x\ncurl -s localhost\n```\n```text\nnot run\n```\n## Long\n```r\n1\n2\n3\n4\n5\n6\n7\n8\n9\n```\n```bash\nndexr refer --to <domain>\n```";
        let r = recipes(doc, "CLAUDE.md");
        assert_eq!(r.len(), 1);
        let u = recipes("## Check it\n```\nsystemctl --user status a\n```\n## Wiring\n```\nbrowser -> nginx\n```", "CLAUDE.md");
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].lang, Lang::Shell);
        assert_eq!(r[0].label, "Check it");
        assert_eq!(r[0].lang, Lang::Shell);
        assert_eq!(r[0].code, "systemctl --user status x\ncurl -s localhost");
    }

    #[test]
    fn an_answer_is_marked_by_what_it_did() {
        let ev = |k: &str, t: &str| AgentEvent { kind: k.into(), text: t.into() };
        assert_eq!(provenance(&[ev("text", "hi")]).marker, None);
        assert_eq!(provenance(&[ev("tool", "Read · /d/x/module.r"), ev("tool", "Grep · plot")]).marker, Some(Marker::Looked));
        let g = provenance(&[ev("tool", "Read · /d/x/CONTEXT.md"), ev("tool", "Edit · /d/x/module.r"), ev("tool", "Edit · /d/x/module.r")]);
        assert_eq!(g.marker, Some(Marker::Grounded));
        assert_eq!(g.changed, vec!["/d/x/module.r".to_owned()]);
        assert_eq!(provenance(&[ev("tool", "Bash · restart it")]).marker, Some(Marker::Unchecked));
        assert_eq!(provenance(&[ev("tool", "Bash · Read every guide doc")]).marker, Some(Marker::Looked));
    }

    #[test]
    fn images_and_pdfs_preview_text_does_not() {
        use crate::api::preview_type;
        assert_eq!(preview_type("/a/Plot.PNG"), Some("image/png"));
        assert_eq!(preview_type("/a/paper.pdf"), Some("application/pdf"));
        assert_eq!(preview_type("/a/logo.svg"), Some("image/svg+xml"));
        assert_eq!(preview_type("/a/module.r"), None);
        assert_eq!(preview_type("/a/Makefile"), None);
    }
}
