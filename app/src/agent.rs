//! The Agent tab beside the consoles: the open domain's agent, exec.ndexr.io's
//! workflow built into the IDE (operator, 2026-10-14). Each turn goes through
//! the platform's own host_run_claude (host.rs agent_run) into the domain's
//! thread - the same thread the in-app chat shows, resuming its last session -
//! and its stream-json is followed live (AgentPoll): what it says, every tool
//! it runs, its cost. Nothing here talks to a model itself.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Key, RichText};

use crate::api::{AgentChunk, AgentEvent, AgentTurn, DomainInfo, FileText, Lang, Listing, Query};
use crate::backend::Backend;
use crate::theme;

/// How a turn may act - exec.ndexr.io's modes (operator, 2026-10-16: "exec and
/// agents need auto, plan, etc modes"): (mode, label, what it means).
const MODES: [(&str, &str, &str); 3] = [
    ("build", "Auto", "does the work: edits files and runs commands"),
    ("plan", "Plan", "thinks it through and writes a plan - changes nothing"),
    ("ask", "Ask", "answers questions - reads, never changes"),
];

/// What a turn runs on: every Claude model the CLI knows (`claude --model`
/// aliases), each on a subscription (`sub-`: the person's own connected
/// Claude, or the shared seat) or on the API (operator, 2026-10-26: "all model
/// options for claude for users to pick, subscription or api too"). The
/// wrapper strips `sub-` and runs the alias on OAuth (ndexr-claude-run).
/// The first two stay sub-opus and sub-sonnet: the agent's and file-ask's
/// defaults are indexes into this list.
const MODELS: [(&str, &str); 16] = [
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
];

/// The model picker's list: subscription models, then API ones, a heading
/// over each.
fn model_menu(ui: &mut egui::Ui, model: &mut String) {
    for (head, sub) in [("Subscription", true), ("API", false)] {
        if !sub {
            ui.separator();
        }
        ui.label(RichText::new(head).size(10.5).strong().color(theme::hush()));
        for (m, l) in MODELS.iter().filter(|(m, _)| m.starts_with("sub-") == sub) {
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
    /// Code the agent gave while auto-run was on, for the consoles to take.
    outbox: Vec<(Lang, String)>,
    /// A turn's stand-in key ("+N") to the run id it was given, so the
    /// Inputs page can follow a turn it sent across the swap.
    aliases: HashMap<String, String>,
    /// The images a turn was sent with: (path, thumbnail), by turn id - its
    /// stand-in key until the run has an id.
    shots: HashMap<String, Vec<(String, egui::TextureHandle)>>,
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
                open = Some((crate::md::fence_lang(t.trim_start_matches('`')), String::new()));
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

/// The tail of the R console handed to the agent with a turn, and how to send
/// code back (operator, 2026-10-14: "the agent send things to the console in
/// real time ... like a single command").
fn with_console(prompt: &str, tail: &str) -> String {
    format!(
        "{prompt}\n\n---\nI am working in Cove, weaving the stack's scientific software into this domain's app. The end of my live R console (R, with Python through reticulate) is below. \
         To run something in it for me, reply with ONE fenced ```r block (```python for reticulate, ```bash for my terminal) - \
         the IDE runs it in that console with one click. Keep it to a single command or short block. \
         This project builds, starts and stops with its own ./build.sh, ./up.sh and ./down.sh (app.env names its language and port; \
         APP-CONTRACT.md) - run them when it makes sense; Cove's Build / Up / Down buttons run the same files.\n\n```text\n{tail}\n```"
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
    egui::Frame::new().fill(theme::bg()).stroke(egui::Stroke::new(1.0, theme::pane_brd())).corner_radius(10).inner_margin(egui::Margin::same(2)).show(ui, |ui| {
        ui.spacing_mut().item_spacing.x = 2.0;
        for (i, (word, tip, on, enabled)) in items.iter().enumerate() {
            let text = RichText::new(word).size(size).color(if *on { theme::bg() } else { theme::text() });
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
    let items = [
        (word, tip, !sc.exec_on, sc.site.is_some()),
        (format!("Exec{dot}"), "the executive agent of all your sites - one thread across every domain".to_owned(), sc.exec_on, true),
    ];
    segmented(ui, big, &items).map(|i| i == 1)
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
    mic: crate::voice::Mic,
    /// Shown in Go: the composer's mic and Send drawn big (ide.rs go_agent).
    pub big: bool,
    /// Which agent the composer talks to, drawn beside the mic (ide.rs sets
    /// it when one agent is shown at a time; None side by side, where both
    /// are on screen).
    pub scope: Option<Scope>,
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

impl Default for AgentPane {
    fn default() -> Self {
        AgentPane {
            st: Default::default(),
            input: String::new(),
            model: MODELS[0].0.into(),
            mode: MODES[0].0,
            asked: f64::NEG_INFINITY,
            sent: 0,
            share_console: true,
            auto_run: Default::default(),
            split: false,
            composer_h: 120.0,
            reveal: HashMap::new(),
            open_tools: HashSet::new(),
            mic: Default::default(),
            big: false,
            scope: None,
            scope_pick: None,
            thread_rect: egui::Rect::NOTHING,
            thread_off: 0.0,
            know: Default::default(),
            show_know: None,
            open: Vec::new(),
            attached: Default::default(),
            scope_label: None,
        }
    }
}

impl AgentPane {
    /// The Exec tab's pane: the person's executive agent over every domain
    /// they own (operator, 2026-10-15), kept apart from the Agents tab's.
    pub fn exec() -> Self {
        AgentPane { scope_label: Some("all sites".to_owned()), ..Default::default() }
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

    /// Type `text` into the prompt for the person to read and send.
    pub fn prefill(&mut self, text: &str) {
        self.input = text.to_owned();
    }

    fn load(&self, backend: &Backend, ctx: &egui::Context, domain: &str) {
        {
            let mut s = self.st.lock().unwrap();
            *s = State { loaded_for: domain.to_owned(), loading: true, ..Default::default() };
        }
        let st = self.st.clone();
        let d = domain.to_owned();
        backend.query(Query::AgentHistory { domain: d.clone() }, ctx, move |r: Result<Vec<AgentTurn>, String>| {
            let mut s = st.lock().unwrap();
            if s.loaded_for != d {
                return;
            }
            s.loading = false;
            match r {
                Ok(mut turns) => {
                    // Turns still running (here a moment ago, the in-app chat,
                    // exec): follow each from the start of its stream.
                    for t in turns.iter_mut().filter(|t| !t.done && !t.id.is_empty()) {
                        s.live.insert(t.id.clone(), 0);
                        t.events.clear();
                    }
                    s.turns = turns;
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
            }
        });
    }

    /// Send `text` as a message of its own, leaving what is typed and attached
    /// in the box for later (Go's Listen, ide.rs go_agent).
    pub fn say(&mut self, backend: &Backend, ctx: &egui::Context, domain: &str, text: &str, console_tail: &str) {
        let typed = std::mem::replace(&mut self.input, text.to_owned());
        let files = std::mem::take(&mut *self.attached.lock().unwrap());
        self.send(backend, ctx, domain, console_tail);
        self.input = typed;
        *self.attached.lock().unwrap() = files;
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
        let names: Vec<&str> = paths.iter().map(|p| p.rsplit('/').next().unwrap_or(p)).collect();
        let prompt = with_attachments(&typed, &paths);
        let prompt = if self.share_console && !console_tail.trim().is_empty() { with_console(&prompt, console_tail) } else { prompt };
        // The bubble shows what was typed and the files' names.
        let shown = if names.is_empty() { typed } else { format!("{typed}\n\nAttached: {}", names.join(", ")) };
        self.sent += 1;
        let key = format!("+{}", self.sent);
        {
            let mut s = self.st.lock().unwrap();
            s.turns.push(AgentTurn { id: key.clone(), prompt: shown, ..Default::default() });
            s.starting += 1;
            if !shots.is_empty() {
                s.shots.insert(key.clone(), shots);
            }
        }
        let st = self.st.clone();
        backend.query(Query::AgentRun { domain: domain.to_owned(), prompt, model: self.model.clone(), cwd: None, mode: Some(self.mode.to_owned()) }, ctx, move |r: Result<String, String>| {
            let mut s = st.lock().unwrap();
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

    /// Keep the thread loaded and its runs followed without the Agents tab
    /// drawn: the Inputs page sends to the agent too (operator, 2026-10-15:
    /// "Agents comes first, then R, Py, and Shell, agents treated like the
    /// others"). Cheap when the tab is also showing - a run already being
    /// polled is not asked twice.
    pub fn tick(&mut self, backend: &Backend, ctx: &egui::Context, domain: Option<&DomainInfo>) {
        let Some(d) = domain else { return };
        if self.st.lock().unwrap().loaded_for != d.host {
            self.load(backend, ctx, &d.host);
            load_know(&self.know, backend, ctx, d);
            self.show_know = None;
        }
        self.poll(backend, ctx);
    }

    /// Send `text` as the person's next turn, as the composer would (the
    /// console's tail goes with it when shared). Returns the turn's key.
    pub fn ask(&mut self, backend: &Backend, ctx: &egui::Context, domain: &str, text: &str, console_tail: &str) -> String {
        let draft = std::mem::replace(&mut self.input, text.to_owned());
        self.send(backend, ctx, domain, console_tail);
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
        let mut run: Vec<(Lang, String)> = std::mem::take(&mut self.st.lock().unwrap().outbox);
        if self.st.lock().unwrap().loaded_for != d.host {
            self.load(backend, ctx, &d.host);
            load_know(&self.know, backend, ctx, d);
            self.show_know = None;
        }
        self.poll(backend, ctx);
        let (live, starting, loading, spent) = {
            let s = self.st.lock().unwrap();
            let spent: f64 = s.turns.iter().filter_map(|t| s.costs.get(&t.id)).sum();
            (s.live.keys().cloned().collect::<Vec<_>>(), s.starting, s.loading, spent)
        };
        let going = live.len() + starting;

        let (mut stop_all, mut reload) = (false, false);
        let mut stop: Option<String> = None;
        // A phone's width (operator, 2026-10-15: "Cove looks rough on
        // mobile"): the strip keeps the model, what is running and Stop; the
        // switches move into a ⋯ menu instead of drawing over each other.
        let narrow = ui.available_width() < 640.0;
        // Two rows, not one (a person's request, 2026-10-18: "agent,
        // <their domain>, opus-subscription-auto, and then console,
        // one-running, stop-all, and the checkbox there, that's all kind of
        // muddled together and it's hard to read"). The first says whose
        // thread this is and what is going on in it; the second is how the
        // next message goes out.
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(6.0);
            // Whose thread, not the domain again: the project switcher above
            // names it (operator, 2026-10-25: "many repetitive views of the
            // domain name"). The host stays on hover.
            let head = self.scope_label.clone().unwrap_or_else(|| "this site".to_owned());
            ui.add(egui::Label::new(RichText::new(head).monospace().size(11.5).strong().color(theme::text_strong())).truncate()).on_hover_text(format!("the agent for {}", d.host));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(RichText::new("?").size(12.0).color(theme::hush())).on_hover_ui(trust_legend);
                reload = theme::tool(ui, "⟳", "load the thread again (turns from the in-app chat too)").clicked();
                if !live.is_empty() {
                    stop_all = theme::tool(ui, if narrow { "■ Stop" } else { "■ Stop all" }, "stop every run going in this thread").clicked();
                }
                if going > 0 {
                    ui.label(RichText::new(format!("● {going} running")).size(11.0).color(theme::warn()));
                } else {
                    ui.label(RichText::new("idle").size(11.0).color(theme::hush()));
                }
                if spent > 0.0 && !narrow {
                    ui.label(RichText::new(format!("${spent:.2}")).monospace().size(11.0).color(theme::hush())).on_hover_text("what the turns shown here cost");
                }
            });
        });
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = 8.0;
            let shown = MODELS.iter().find(|(m, _)| *m == self.model).map_or(self.model.as_str(), |(_, l)| l);
            let shown = if narrow { shown.split(" · ").next().unwrap_or(shown) } else { shown };
            if !narrow {
                ui.label(RichText::new("model").size(11.0).color(theme::hush()));
            }
            egui::ComboBox::from_id_salt(("agent-model", self.ns())).width(if narrow { 84.0 } else { 200.0 }).height(480.0).selected_text(RichText::new(shown).size(11.5)).show_ui(ui, |ui| model_menu(ui, &mut self.model));
            // Auto, Plan or Ask - beside the model, on every width.
            let (_, shown_mode, tip) = MODES.iter().find(|(m, _, _)| *m == self.mode).copied().unwrap_or(MODES[0]);
            egui::ComboBox::from_id_salt(("agent-mode", self.ns()))
                .width(if narrow { 56.0 } else { 70.0 })
                .selected_text(RichText::new(shown_mode).size(11.5))
                .show_ui(ui, |ui| {
                    for (m, l, t) in MODES {
                        ui.selectable_value(&mut self.mode, m, l).on_hover_text(t);
                    }
                })
                .response
                .on_hover_text(format!("{shown_mode}: {tip}"));
            let mut auto = self.auto_run.load(Ordering::Relaxed);
            let switches = |ui: &mut egui::Ui, share: &mut bool, auto: &mut bool| {
                ui.checkbox(share, RichText::new("see console").size(11.5))
                    .on_hover_text("send the end of the R console with each message, so the agent sees what you ran and what it printed");
                ui.checkbox(auto, RichText::new("auto-run").size(11.5))
                    .on_hover_text("run the code the agent replies with in the console as soon as it arrives - off: a ▶ under each block runs it");
            };
            if !narrow {
                theme::tool_sep(ui);
                switches(ui, &mut self.share_console, &mut auto);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                if narrow {
                    // A thumb's worth of target: the menu was a few pixels of dots in the corner.
                    ui.spacing_mut().button_padding = egui::vec2(14.0, 7.0);
                    ui.menu_button(RichText::new("•••").size(17.0), |ui| {
                        switches(ui, &mut self.share_console, &mut auto);
                        ui.separator();
                        if ui.button(if self.split { "Hide the console" } else { "Console under the agent" }).clicked() {
                            self.split = !self.split;
                            ui.close();
                        }
                    });
                } else if theme::tool(ui, if self.split { "Console ✓" } else { "Console" }, "the R console under the agent - watch what it runs").clicked() {
                    self.split = !self.split;
                }
            });
            self.auto_run.store(auto, Ordering::Relaxed);
        });
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
        {
            let k = self.know.lock().unwrap();
            if k.loaded_for == d.host {
                know_panel(ui, &k, open, &mut toggle, &mut self.open, &mut run);
            }
        }
        if toggle {
            self.show_know = Some(!open);
        }

        let input_id = egui::Id::new(("agent-input", self.ns()));
        let out_h = (ui.available_height() - self.composer_h - 12.0).max(60.0);
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
                    ui.label(RichText::new(format!("Tell {}'s agent what to build. It weaves the stack's scientific software - EasyBuild modules, R and Python packages, the tools in the Terminal - into this app: it reads and edits the domain's files, runs code in the consoles, and shows the result in App. The same agent as the in-app chat.", d.host)).color(theme::hush()));
                    ui.add_space(10.0);
                    picked = suggestions(ui);
                }
                let dt = ui.input(|i| i.stable_dt).min(0.1) as f64;
                for t in &s.turns {
                    let going = !t.done && (s.live.contains_key(&t.id) || t.id.starts_with('+'));
                    if !t.prompt.is_empty() {
                        ui.add_space(6.0);
                        user_bubble(ui, tw, shown_prompt(&t.prompt), false);
                        // Copy it, or send it again as a new run (operator,
                        // 2026-10-16: "click copy it ability and resend ability").
                        ui.allocate_ui_with_layout(egui::vec2(tw, 18.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add_space(4.0);
                            if theme::tool(ui, "Resend", "send this message again - it starts a new run").clicked() {
                                resend = Some(shown_prompt(&t.prompt).to_owned());
                            }
                            if theme::tool(ui, "Copy", "copy this message").clicked() {
                                ui.ctx().copy_text(shown_prompt(&t.prompt).to_owned());
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
                                        crate::setup::connect_button(ui, &ev[k].text);
                                    });
                                }
                                _ => {
                                    let n = ev[k].text.chars().count();
                                    crate::md::show(ui, crate::md::prefix(&ev[k].text, left), 13.0, &mut run);
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
                                if !t.id.starts_with('+') && crate::icons::button(ui, crate::icons::Icon::Stop, 22.0, theme::surface_hi(), theme::alt(), true).on_hover_text("stop this run").clicked() {
                                    stop = Some(t.id.clone());
                                }
                                ui.spinner();
                                let now = if t.id.starts_with('+') { "starting…".to_owned() } else { ev.iter().rev().find(|e| e.kind == "tool").map_or("thinking…".to_owned(), |e| format!("working · {}", e.text.split(" · ").next().unwrap_or(""))) };
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
                                if let Some(c) = s.costs.get(&t.id) {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(format!("${c:.2}")).monospace().size(10.5).color(theme::hush())).on_hover_text("what this turn cost");
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
        if let Some(e) = self.mic.error() {
            ui.label(RichText::new(e).size(11.0).color(theme::alt()));
        }
        // The composer, one rounded box (operator, 2026-10-15: "could be
        // improved and cleaner"): what is attached, the words on a full-width
        // line, then + (attach), the mic and send under them - nothing beside
        // the text to push Send off a phone's edge.
        let uploading = self.attached.lock().unwrap().iter().any(|f| f.path.is_none() && f.error.is_none());
        let ready = !self.input.trim().is_empty() || self.attached.lock().unwrap().iter().any(|f| f.path.is_some());
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
                ui.add(
                    egui::TextEdit::multiline(&mut self.input)
                        .id(input_id)
                        .frame(egui::Frame::NONE)
                        .desired_rows(if narrow { 1 } else { 2 })
                        .desired_width(ui.available_width())
                        .font(egui::FontId::proportional(13.5))
                        .return_key(Some(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, Key::Enter)))
                        .hint_text(RichText::new(hint).color(theme::hush())),
                );
                ui.horizontal(|ui| {
                    if crate::icons::quiet(ui, crate::icons::Icon::Plus, 32.0).on_hover_text("attach files or supporting docs - they go into this domain's supporting_docs/ and the agent reads them").clicked() {
                        self.attach_pick(backend, ctx, &d.path);
                    }
                    // In Go the mic and Send are the big, thumb-first pair -
                    // the one record button and the one Send on the screen
                    // (operator, 2026-10-18: "two send buttons and recording
                    // buttons ... seems like a lot").
                    let size = if self.big { 56.0 } else { 32.0 };
                    let Self { mic, input, .. } = self;
                    mic.ui_sized(ui, ctx, backend, input, size);
                    // Who hears it, beside the mic (operator, 2026-10-26:
                    // "next to the mic and there it shows the domain or
                    // executive as the option toggled").
                    if let Some(sc) = &self.scope {
                        if let Some(all) = scope_toggle(ui, sc, self.big) {
                            self.scope_pick = Some(all);
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let r = crate::icons::button(ui, crate::icons::Icon::Send, size, theme::primary(), theme::bg(), ready && !uploading);
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
        if send && !uploading {
            self.send(backend, ctx, &d.host, console_tail);
        }
        run
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
            let Some(files) = rfd::AsyncFileDialog::new().set_title("Attach for the agent").pick_files().await else { return };
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
                    match crate::md::fence_lang(tag).or(if shellish { Some(Lang::Shell) } else { None }) {
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

fn suggestions(ui: &mut egui::Ui) -> Option<&'static str> {
    let mut picked = None;
    ui.label(RichText::new("Try one").size(11.0).color(theme::hush()));
    ui.add_space(2.0);
    for q in SUGGESTIONS {
        let b = egui::Button::new(RichText::new(q).size(12.5).color(theme::text())).fill(theme::surface().gamma_multiply(0.85)).stroke(egui::Stroke::new(1.0, theme::pane_brd())).corner_radius(10).min_size(egui::vec2(ui.available_width().min(460.0), 30.0));
        if ui.add(b).on_hover_text("send this to the agent").clicked() {
            picked = Some(q);
        }
        ui.add_space(4.0);
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
fn shown_prompt(p: &str) -> &str {
    let p = p.split("\n\n---\nI am working in ").next().unwrap_or(p);
    p.split("\n\nAttached to this message (uploaded into").next().unwrap_or(p).trim()
}

fn user_bubble(ui: &mut egui::Ui, w: f32, text: &str, stoppable: bool) -> bool {
    let mut stop = false;
    let max = (w * if w < 520.0 { 0.9 } else { 0.78 }).max(120.0);
    // Sized to its words: a nested right-to-left layout inside the bubble
    // stretched it to the whole height left, a tall empty box on a phone.
    let font = egui::FontId::proportional(13.0);
    let text_w = ui.fonts_mut(|f| f.layout(text.to_owned(), font.clone(), theme::text_strong(), max - 24.0)).size().x;
    let inner = text_w.max(if stoppable { 64.0 } else { 0.0 });
    ui.horizontal_top(|ui| {
        ui.add_space((w - inner - 22.0).max(0.0));
        egui::Frame::new()
            .fill(theme::primary().gamma_multiply(0.13))
            .stroke(egui::Stroke::new(1.0, theme::primary().gamma_multiply(0.35)))
            .corner_radius(egui::CornerRadius { nw: 12, ne: 12, sw: 12, se: 3 })
            .inner_margin(egui::Margin::symmetric(10, 5))
            .show(ui, |ui| {
                ui.set_width(inner);
                ui.vertical(|ui| {
                    // The person's domains in what they typed are links into
                    // Cove, as in a reply (md.rs typed).
                    crate::md::typed(ui, text, font.size, theme::text_strong(), max - 24.0);
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

// ── a file ask: the agent on one file, from the file itself ─────────────────

/// How far a file ask may reach (operator, 2026-10-14: "run file only or depth
/// of file reach commands"): the file alone, or its folder and N levels below.
pub const REACH: [(u8, &str); 4] = [(0, "this file"), (1, "its folder"), (2, "folder + 1 level"), (3, "folder + 2 levels")];

#[derive(Default)]
struct AskState {
    /// The file the run is about.
    path: String,
    prompt: String,
    events: Vec<AgentEvent>,
    running: Option<String>,
    offset: u64,
    starting: bool,
    cost: Option<f64>,
    /// When the run ended (ui time), so the file is read once more after it.
    ended: Option<f64>,
}

/// The bar under an open file: say what you want in it, in a sentence, and
/// watch the file change as the agent works. The same agent and machinery as
/// the Agents tab (AgentRun / AgentPoll, the domain's thread), run at the
/// file's folder and told how far it may reach.
pub struct FileAsk {
    st: Arc<Mutex<AskState>>,
    pub input: String,
    pub reach: u8,
    model: String,
    asked: f64,
    inflight: Arc<AtomicBool>,
    refreshed: f64,
    mic: crate::voice::Mic,
    /// How tall the thread above the ask box is: dragged with the handle on
    /// the bar's top edge (operator, 2026-10-16: "want to be able to drag the
    /// agent section in files under the file up at times").
    thread_h: f32,
}

/// The thread's height until it is dragged, and the least it may be.
const ASK_THREAD_H: f32 = 110.0;

impl Default for FileAsk {
    fn default() -> Self {
        // Quick asks: Sonnet on the subscription unless picked otherwise.
        FileAsk { st: Default::default(), input: String::new(), reach: 0, model: MODELS[1].0.into(), asked: f64::NEG_INFINITY, inflight: Default::default(), refreshed: f64::NEG_INFINITY, mic: Default::default(), thread_h: ASK_THREAD_H }
    }
}

/// The folder a path is in.
pub fn folder_of(path: &str) -> &str {
    match path.rsplit_once('/') {
        Some(("", _)) => "/",
        Some((d, _)) => d,
        None => ".",
    }
}

fn ask_prompt(domain: &str, path: &str, reach: u8, what: &str) -> String {
    let dir = folder_of(path);
    let scope = match reach {
        0 => format!("Change only this file, {path}. Read other files if you need to, but edit nothing else."),
        1 => format!("You may read and edit the files in {dir} (this folder only, not its subfolders); nothing outside it."),
        n => format!("You may read and edit files in {dir} and up to {} level{} of subfolders below it; nothing outside {dir}.", n - 1, if n == 2 { "" } else { "s" }),
    };
    format!(
        "A request from the Cove editor, about one file of {domain}.\n\nFile: {path}\nWorking folder: {dir}\nReach: {scope}\n\nRequest: {what}\n\nMake the change in the file(s) directly - the person is watching the file update. Then say what you changed in one or two sentences."
    )
}

impl FileAsk {
    /// A run about `path` is starting or streaming.
    pub fn busy_on(&self, path: &str) -> bool {
        let s = self.st.lock().unwrap();
        s.path == path && (s.starting || s.running.is_some())
    }

    fn busy(&self) -> bool {
        let s = self.st.lock().unwrap();
        s.starting || s.running.is_some()
    }

    /// The file to read again from disk now, if any: every 1.5 s while a run
    /// works, and once just after it ends, so its edits show as they land.
    pub fn refresh_due(&mut self, now: f64) -> Option<String> {
        let s = self.st.lock().unwrap();
        let live = s.starting || s.running.is_some() || s.ended.is_some_and(|t| now - t < 3.0);
        if !live || s.path.is_empty() || now - self.refreshed < 1.5 {
            return None;
        }
        self.refreshed = now;
        Some(s.path.clone())
    }

    fn poll(&mut self, backend: &Backend, ctx: &egui::Context) {
        let (id, offset) = {
            let s = self.st.lock().unwrap();
            match &s.running {
                Some(id) => (id.clone(), s.offset),
                None => return,
            }
        };
        let now = ctx.input(|i| i.time);
        ctx.request_repaint_after(std::time::Duration::from_millis(700));
        if self.inflight.load(Ordering::Relaxed) || now - self.asked < 0.7 {
            return;
        }
        self.asked = now;
        self.inflight.store(true, Ordering::Relaxed);
        let (st, inflight) = (self.st.clone(), self.inflight.clone());
        backend.query(Query::AgentPoll { id: id.clone(), offset }, ctx, move |r: Result<AgentChunk, String>| {
            inflight.store(false, Ordering::Relaxed);
            let Ok(c) = r else { return };
            let mut s = st.lock().unwrap();
            if s.running.as_deref() != Some(id.as_str()) {
                return;
            }
            s.offset = c.offset;
            if c.cost.is_some() {
                s.cost = c.cost;
            }
            s.events.extend(c.events);
            if c.done {
                s.running = None;
                s.ended = Some(now);
            }
        });
    }

    /// Run the ask typed into the bar, about `path`, in `domain`'s thread.
    pub fn start(&mut self, backend: &Backend, ctx: &egui::Context, domain: &str, path: &str) {
        let what = std::mem::take(&mut self.input).trim().to_owned();
        if what.is_empty() {
            return;
        }
        {
            let mut s = self.st.lock().unwrap();
            *s = AskState { path: path.to_owned(), prompt: what.clone(), starting: true, ..Default::default() };
        }
        let st = self.st.clone();
        let q = Query::AgentRun { domain: domain.to_owned(), prompt: ask_prompt(domain, path, self.reach, &what), model: self.model.clone(), cwd: Some(folder_of(path).to_owned()), mode: None };
        backend.query(q, ctx, move |r: Result<String, String>| {
            let mut s = st.lock().unwrap();
            s.starting = false;
            match r {
                Ok(id) => {
                    s.running = Some(id);
                    s.offset = 0;
                }
                Err(e) => {
                    s.events.push(AgentEvent { kind: "error".into(), text: e });
                    s.ended = Some(f64::NEG_INFINITY);
                }
            }
        });
    }

    fn stop(&self, backend: &Backend, ctx: &egui::Context) {
        let Some(id) = self.st.lock().unwrap().running.clone() else { return };
        backend.query(Query::AgentStop { id }, ctx, |_: Result<bool, String>| {});
    }

    /// The bar, drawn at the bottom of the file `path` (a chat window under
    /// it): the last ask about this file and what the agent did, then the
    /// reach, the line to type in and Ask. Returns true when the person asked:
    /// the caller saves the file if it has unsaved edits - so the agent works
    /// on what is on screen - then calls `start`.
    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, domain: Option<&DomainInfo>, path: &str) -> bool {
        self.poll(backend, ctx);
        let busy = self.busy();
        let here = self.busy_on(path);
        let (prompt, events, cost, mine) = {
            let s = self.st.lock().unwrap();
            (s.prompt.clone(), s.events.clone(), s.cost, s.path == path)
        };
        let mut sent = false;
        let shown = mine && (!prompt.is_empty() || !events.is_empty());
        // The handle: drag up for more of the thread, down for more of the
        // file; a double-click puts it back. Never more than 70% of the window,
        // so the file above stays on screen.
        if shown {
            let (r, h) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 7.0), egui::Sense::click_and_drag());
            let top = ctx.content_rect().height() * 0.7;
            if h.dragged() {
                self.thread_h = (self.thread_h - h.drag_delta().y).clamp(ASK_THREAD_H * 0.6, top.max(ASK_THREAD_H));
            }
            if h.double_clicked() {
                self.thread_h = ASK_THREAD_H;
            }
            let hot = h.hovered() || h.dragged();
            if hot {
                ctx.set_cursor_icon(egui::CursorIcon::ResizeVertical);
            }
            ui.painter().rect_filled(r, 0.0, theme::pane_hdr());
            ui.painter().line_segment(
                [r.center() - egui::vec2(18.0, 0.0), r.center() + egui::vec2(18.0, 0.0)],
                egui::Stroke::new(2.0, if hot { theme::primary() } else { theme::hush() }),
            );
            h.on_hover_text("drag up to see more of the agent, down to see more of the file · double-click to reset");
        }
        let grown = self.thread_h > ASK_THREAD_H + 0.5;
        egui::Frame::new().fill(theme::pane_hdr()).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            // What the last ask about this file did: its words and the tools
            // it ran, newest at the bottom - a few lines tall, or as tall as
            // the handle above was dragged.
            if shown {
                egui::ScrollArea::vertical()
                    .id_salt(("ask-thread", path))
                    .max_height(self.thread_h)
                    .min_scrolled_height(if grown { self.thread_h } else { 0.0 })
                    .stick_to_bottom(true)
                    .auto_shrink([false, !grown])
                    .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(format!("› {prompt}")).size(12.0).color(theme::primary()));
                    let mut ignored = Vec::new();
                    for e in &events {
                        match e.kind.as_str() {
                            "tool" => {
                                ui.add(egui::Label::new(RichText::new(format!("▸ {}", e.text)).monospace().size(11.0).color(theme::hush())).truncate());
                            }
                            "error" => {
                                ui.label(RichText::new(&e.text).size(12.0).color(theme::alt()));
                                crate::setup::connect_button(ui, &e.text);
                            }
                            _ => crate::md::show(ui, &e.text, 12.0, &mut ignored),
                        }
                    }
                    if here {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(RichText::new("working on the file…").size(11.5).color(theme::muted()));
                        });
                    }
                });
                ui.add_space(4.0);
            }
            let Some(d) = domain else {
                ui.label(RichText::new("Open a domain in App to ask its agent about this file.").size(11.5).color(theme::hush()));
                return;
            };
            let input_id = egui::Id::new(("file-ask", path));
            let mut send = false;
            if ui.memory(|m| m.has_focus(input_id)) && !busy {
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
            // On a phone the two pickers take a row of their own, so the
            // line to type in gets the width.
            let narrow = ui.available_width() < 640.0;
            let pickers = |ui: &mut egui::Ui, reach: &mut u8, model: &mut String| {
                let shown = REACH.iter().find(|(r, _)| *r == *reach).map_or("this file", |(_, l)| l);
                egui::ComboBox::from_id_salt("file-ask-reach").width(if narrow { 96.0 } else { 120.0 }).selected_text(RichText::new(shown).size(11.5)).show_ui(ui, |ui| {
                    for (r, l) in REACH {
                        ui.selectable_value(reach, r, l);
                    }
                })
                .response
                .on_hover_text("how far the agent may reach: the file alone, or its folder and the levels below");
                let shown = MODELS.iter().find(|(m, _)| *m == model.as_str()).map_or(model.as_str(), |(_, l)| l).to_owned();
                egui::ComboBox::from_id_salt("file-ask-model").width(if narrow { 110.0 } else { 130.0 }).selected_text(RichText::new(shown).size(11.5)).show_ui(ui, |ui| {
                    model_menu(ui, model);
                });
            };
            if narrow {
                ui.horizontal(|ui| pickers(ui, &mut self.reach, &mut self.model));
            }
            if let Some(e) = self.mic.error() {
                ui.label(RichText::new(e).size(11.0).color(theme::alt()));
            }
            ui.horizontal(|ui| {
                if !narrow {
                    pickers(ui, &mut self.reach, &mut self.model);
                }
                let hint = match (busy, narrow) {
                    (true, _) => "the agent is working - Stop to interrupt it".to_owned(),
                    (false, true) => "what do you want in this file?".to_owned(),
                    (false, false) => format!("what do you want in this file? · {}'s agent · Enter sends · ● to speak", d.host),
                };
                let right = if busy { 64.0 } else { 58.0 } + if crate::voice::Mic::shown() && !busy { 38.0 } else { 0.0 };
                ui.add_enabled(
                    !busy,
                    egui::TextEdit::multiline(&mut self.input)
                        .id(input_id)
                        .desired_rows(1)
                        .desired_width((ui.available_width() - right).max(80.0))
                        .return_key(Some(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, Key::Enter)))
                        .hint_text(RichText::new(hint).color(theme::hush())),
                );
                if !busy {
                    let Self { mic, input, .. } = self;
                    mic.ui(ui, ctx, backend, input);
                }
                if busy {
                    if theme::tool(ui, "■ Stop", "stop this run").clicked() {
                        self.stop(backend, ctx);
                    }
                } else if ui.add_enabled(!self.input.trim().is_empty(), theme::primary_button("Ask")).clicked() {
                    send = true;
                }
                if let Some(c) = cost.filter(|_| mine) {
                    ui.label(RichText::new(format!("${c:.2}")).monospace().size(10.5).color(theme::hush()));
                }
            });
            if send && !busy && !self.input.trim().is_empty() {
                sent = true;
            }
        });
        sent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_bubble_shows_what_was_typed_not_the_console() {
        assert_eq!(shown_prompt(&with_console("make it blue", "> x")), "make it blue");
        assert_eq!(shown_prompt("old\n\n---\nI am working in the Cove IDE. The end of"), "old");
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
        let p = ask_prompt("x.ndexr.io", "/d/x.ndexr.io/r/plot.R", 0, "make it blue");
        assert!(p.contains("Change only this file, /d/x.ndexr.io/r/plot.R") && p.contains("Request: make it blue"));
        assert!(ask_prompt("x", "/d/f.R", 1, "w").contains("this folder only"));
        assert!(ask_prompt("x", "/d/f.R", 2, "w").contains("up to 1 level of subfolders"));
        assert!(ask_prompt("x", "/d/f.R", 3, "w").contains("up to 2 levels"));
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
