//! The walkthroughs (operator, 2026-10-27: "an interactive, first-time-user
//! walkthrough ... a functional, interactive walkthrough - not a static help
//! page"): a spotlight on the real control, a short card beside it, and steps
//! that move on when the person has done the thing - read from the IDE's own
//! state each frame, never from a timer or a pretend success.
//!
//! Three pieces, one way each:
//! - **Targets.** A widget that a step may point at calls [`mark`] with a
//!   stable id where it is drawn (`"project-button"`, `"app:up"`,
//!   `"file:<path>"`, `"tab:App"`...). Marks live for one frame, so a target
//!   that is not on screen is simply missing and the card says where to find
//!   it instead of pointing at nothing.
//! - **State.** [`Seen`] is what the IDE is doing, built by `Ide::tour_seen`
//!   and the shell each frame: the open project, the New project dialog and
//!   explorer, open files, saves, the app's status and its last job.
//! - **Steps.** [`STEPS`] is the table: what a step points at, what its card
//!   says, and when it is done (`done(seen, at_start)`). [`TOURS`] are named
//!   lists of step ids; Help's "Show me how" searches them.
//!
//! The tour only ever asks the IDE to do what a person can (open the real New
//! project dialog, bring a tab forward, open a file): [`Act`], applied by
//! app.rs through the IDE's own methods. It never writes a file.
//!
//! Adding a step: an entry in [`STEPS`] (id, targets, words, `done`), its id
//! in a tour, and a [`mark`] call on the widget it points at.

use crate::theme;
use eframe::egui::{self, Color32, Id, Order, Pos2, Rect, RichText, Stroke, Vec2};
use serde::{Deserialize, Serialize};

// ── targets ───────────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
struct Marked {
    rect: Rect,
    frame: u64,
}

/// A widget a step may point at, drawn this frame at `rect`. When it is the
/// step's target and sits in a scroll area, it is scrolled into view once.
pub fn mark(ui: &egui::Ui, id: &str, rect: Rect) {
    let ctx = ui.ctx();
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(Id::new(("tour-mark", id)), Marked { rect, frame }));
    let wanted = ctx.data(|d| d.get_temp::<String>(Id::new("tour-want")));
    if wanted.as_deref() == Some(id) {
        let done = ctx.data(|d| d.get_temp::<String>(Id::new("tour-scrolled"))).as_deref() == Some(id);
        if !done {
            ui.scroll_to_rect(rect, Some(egui::Align::Center));
            ctx.data_mut(|d| d.insert_temp(Id::new("tour-scrolled"), id.to_owned()));
        }
    }
}

/// [`mark`] for a rect with no Ui at hand (a modal's whole window).
pub fn mark_at(ctx: &egui::Context, id: &str, rect: Rect) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(Id::new(("tour-mark", id)), Marked { rect, frame }));
}

/// Where `id` was drawn this frame or the one before (a menu or a pane drawn
/// after the overlay asks still counts), if at all.
pub fn marked(ctx: &egui::Context, id: &str) -> Option<Rect> {
    let now = ctx.cumulative_frame_nr();
    ctx.data(|d| d.get_temp::<Marked>(Id::new(("tour-mark", id)))).filter(|m| now.saturating_sub(m.frame) <= 1).map(|m| m.rect)
}

// ── what the IDE is doing ─────────────────────────────────────────────────

/// The IDE's state as a step sees it, every frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Seen {
    /// The open project: its host and folder.
    pub project: Option<(String, String)>,
    /// "port" (its own server), "module" (a Shiny module the platform
    /// serves), "" not known yet.
    pub kind: String,
    /// The project's language as its app.env says: r, python, julia, rust, node.
    pub language: String,
    /// The New project dialog (name it) and the explorer (what it starts as).
    pub naming: bool,
    pub choosing: Option<String>,
    /// Files open in the editor, and whether any has unsaved edits.
    pub open_files: Vec<String>,
    pub dirty: bool,
    /// Saves inside the open project so far this session (Ide `site_seq`).
    pub saves: u64,
    /// Up jobs that ended well this session.
    pub ups: u64,
    /// The app answers: a port app is up, a module app's site answers.
    pub running: bool,
    /// Build output exists (a Rust app must Build before Up).
    pub built: bool,
    /// The project has these verbs (build, up, down).
    pub verbs: Vec<String>,
    /// The last Build/Up/Down: still running, how it ended, its last line.
    pub job_running: bool,
    pub job_failed: Option<String>,
    /// open, login, password or down - the site's gate (Ide `gate`).
    pub gate: String,
    /// The environment the consoles run in (env/Python/3.13.15 ...).
    pub env_module: String,
    /// The App pane is on screen.
    pub app_shown: bool,
    /// A phone-width window.
    pub narrow: bool,
}

impl Seen {
    fn host(&self) -> Option<&str> {
        self.project.as_ref().map(|(h, _)| h.as_str())
    }
    fn path(&self) -> Option<&str> {
        self.project.as_ref().map(|(_, p)| p.as_str())
    }
    /// The file that is the application, the first of these the starters use.
    pub fn entry(&self) -> Option<String> {
        let dir = self.path()?;
        let name = match (self.kind.as_str(), self.language.as_str()) {
            ("module", _) | (_, "r") => "module.r",
            (_, "python") => "app.py",
            (_, "node") => "server.js",
            (_, "julia") => "server.jl",
            (_, "rust") => "src/main.rs",
            _ => return None,
        };
        Some(format!("{}/{name}", dir.trim_end_matches('/')))
    }
    /// The file whose words show on the page - the place for a first change.
    pub fn page_file(&self) -> Option<String> {
        let dir = self.path()?.trim_end_matches('/').to_owned();
        Some(match (self.kind.as_str(), self.language.as_str()) {
            ("module", _) | (_, "r") => format!("{dir}/module.r"),
            (_, "rust") => format!("{dir}/src/page.html"),
            _ => format!("{dir}/static/index.html"),
        })
    }
    fn port_app(&self) -> bool {
        self.kind == "port"
    }
}

// ── steps ─────────────────────────────────────────────────────────────────

/// What a step asks the IDE to do when it starts, or a card button does.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    /// The real New project dialog (ide.rs new_project_dialog).
    NewProject,
    /// Pick this starter in the open explorer (ide.rs `choose_starter`).
    Recommend(&'static str),
    /// Bring a tab forward (ide.rs `bring`).
    Bring(crate::ide::Tab),
    /// Open a file in the editor.
    Open(String),
    /// The app job's log.
    AppLog,
}

pub struct Step {
    pub id: &'static str,
    /// What to spotlight, first found wins.
    pub targets: fn(&Seen) -> Vec<String>,
    pub title: &'static str,
    pub body: fn(&Seen) -> String,
    /// Done: the step moves on by itself. None = a Next step.
    pub done: Option<fn(&Seen, &Seen) -> bool>,
    /// Not wanted for this project (a module app has no Up to press).
    pub skip: fn(&Seen) -> bool,
    /// Where to find the target when it is not on screen.
    pub find: &'static str,
    /// Done when it starts.
    pub on_start: fn(&Seen) -> Option<Act>,
}

fn none(_: &Seen) -> Option<Act> {
    None
}
fn never(_: &Seen) -> bool {
    false
}
fn t(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

/// The recommended first application in each language: small, interactive,
/// quick to start (Julia compiles packages on its first start).
pub const RECOMMENDED: [(&str, &str, &str, &str); 4] = [
    ("R", "shiny-geyser", "Old Faithful in Shiny", "Data analysis, statistical computing and Shiny applications."),
    ("Python", "python-ml", "ML Playground", "Data science, automation and interactive web applications."),
    ("Julia", "julia-fractals", "Fractal Explorer", "Numerical computing and scientific simulation."),
    ("Rust", "rust-particles", "Particle Universe", "High-performance applications and reliable compiled software."),
];

pub static STEPS: &[Step] = &[
    // 1 · the workspace
    Step {
        id: "ws-project",
        targets: |_| t(&["project-tabs", "project-button"]),
        title: "Your projects are tabs",
        body: |_| "Every application is a project, and each open project is a tab, as in a browser. Click a tab to switch to it. + at the end opens Projects, where you go to a project, create one, or star one as a favorite. Right-click a tab to pin, favorite, reload or close it; ⌄ lists every tab, your favorites and the ones closed lately.".into(),
        done: None,
        skip: never,
        find: "The tabs run along the top of the window.",
        on_start: none,
    },
    Step {
        id: "ws-agents",
        targets: |_| t(&["tab:Agents", "pane:Agents"]),
        title: "The agent",
        body: |_| "Agents is the agent working beside your code in this project. Say what you want in words: it changes the project's own application in place, runs it, and answers here. Each reply shows when it was written, and Listen reads it aloud.".into(),
        done: None,
        skip: never,
        find: "Agents is a tab in one of the windows; on a phone, the switcher beside the Cove menu.",
        on_start: none,
    },
    Step {
        id: "ws-editor",
        targets: |_| t(&["tab:Files", "editor"]),
        title: "Files and the editor",
        body: |_| "Files lists the project's folder; a file opens in the editor beside it. Ctrl+S saves.".into(),
        done: None,
        skip: never,
        find: "Files is a tab in one of the windows.",
        on_start: none,
    },
    Step {
        id: "ws-console",
        targets: |_| t(&["tab:Console", "tab:Terminal"]),
        title: "Console and terminal",
        body: |_| "The Console runs R, with Python through reticulate, one line at a time; an environment with no R gives a shell console. The Terminal is a full shell in your project.".into(),
        done: None,
        skip: never,
        find: "Console is a tab in one of the windows.",
        on_start: none,
    },
    Step {
        id: "ws-packages",
        targets: |_| t(&["tab:Software", "tab:Environment"]),
        title: "Packages",
        body: |_| "Packages is the software your project can use: every module and every R, Python, Julia and Rust package built for the stack, with where it came from. When the stack is built on this computer, This computer at the top shows the install and its build.".into(),
        done: None,
        skip: never,
        find: "Packages is a tab in one of the windows.",
        on_start: none,
    },
    Step {
        id: "ws-app",
        targets: |_| t(&["tab:App"]),
        title: "Your application",
        body: |_| "App shows the running application, live, beside its code. Click its name at the top to reload it.".into(),
        done: None,
        skip: never,
        find: "App is a tab in one of the windows.",
        on_start: none,
    },
    Step {
        id: "ws-run",
        targets: |_| t(&["app-state", "app-strip", "tab:App"]),
        title: "Run controls",
        body: |_| "At the top of App, above the site: the app's state (click it for the last job's log), then Start, Restart, Stop and Build, which run the project's .cove/up.sh, down.sh and build.sh - the same scripts an agent or a terminal would. Logs shows the app's own output.".into(),
        done: None,
        skip: never,
        find: "The run controls are at the top of the App tab.",
        on_start: |_| Some(Act::Bring(crate::ide::Tab::App)),
    },
    Step {
        id: "ws-modes",
        targets: |_| t(&["modes"]),
        title: "Beyond the workspace",
        body: |_| "Workspace is where your projects are. Connections lists every app you may open - yours, shared with you and public. Builds shows the build machines and every build, live. Atlas maps the whole stack, kernel to application. Account is you: your cell, Claude, your domains and keys.".into(),
        done: None,
        skip: never,
        find: "The modes are in the top bar; on a phone, along the bottom.",
        on_start: none,
    },
    Step {
        id: "ws-status",
        targets: |_| t(&["status-bar"]),
        title: "The status bar",
        body: |_| "Along the bottom: Messages, Claude, and where the stack comes from - mounted with CernVM-FS, remote on your cell, or built on this computer; click it to change. On the right: the runtime (R and Python, click for the detail), Save, and Help (F1).".into(),
        done: None,
        skip: |s| s.narrow,
        find: "",
        on_start: none,
    },
    // 2-4 · a new project
    Step {
        id: "new",
        targets: |_| t(&["project-button"]),
        title: "Create your first project",
        body: |_| "Every application begins with a project. Click + at the end of the tabs to open Projects.".into(),
        done: Some(|s, _| s.naming || s.choosing.is_some()),
        skip: never,
        find: "+ is at the end of the project tabs, along the top.",
        on_start: none,
    },
    Step {
        id: "name",
        targets: |_| t(&["new-project-dialog"]),
        title: "Name it",
        body: |_| "Projects searches as you type: a name you have opens it, a new one offers + Create. Use letters, digits and hyphens - it becomes the project's address, <name>.ndexr.io. Press Enter or + Create.".into(),
        done: Some(|s, _| s.choosing.is_some()),
        skip: |s| s.choosing.is_some(),
        find: "",
        on_start: |s| (!s.naming && s.choosing.is_none()).then_some(Act::NewProject),
    },
    Step {
        id: "choose",
        targets: |_| t(&["explorer"]),
        title: "Choose a language and an application",
        body: |_| "Each experiment is a working application: pick one, then its language, then Create. Blank, at the end, is a minimal start. A good first one, small and interactive:".into(),
        done: Some(|s, start| s.choosing.is_none() && s.host().is_some() && s.host() == start.choosing.as_deref()),
        skip: never,
        find: "",
        on_start: none,
    },
    // 5 · the editor
    Step {
        id: "editor",
        targets: |s| {
            let mut v = Vec::new();
            if let Some(e) = s.entry() {
                v.push(format!("file:{e}"));
            }
            v.extend(t(&["files-tree", "tab:Files"]));
            v
        },
        title: "Your application's code",
        body: |s| {
            let e = s.entry().map(|e| e.rsplit('/').next().unwrap_or(&e).to_owned()).unwrap_or_else(|| "its main file".into());
            format!("This folder is the project; your starter already has everything it needs to run. Open {e} - it is where the application starts.")
        },
        done: Some(|s, _| match s.entry() {
            Some(e) => s.open_files.iter().any(|f| *f == e),
            None => !s.open_files.is_empty(),
        }),
        skip: |s| s.project.is_none(),
        find: "Files is a tab in one of the windows.",
        on_start: |_| Some(Act::Bring(crate::ide::Tab::Files)),
    },
    // 6 · run it
    Step {
        id: "run",
        targets: |s| {
            if !s.port_app() {
                t(&["tab:App"])
            } else if !s.built && s.verbs.iter().any(|v| v == "build") {
                t(&["app:build", "app-strip"])
            } else {
                t(&["app:up", "app-strip"])
            }
        },
        title: "Run it",
        body: |s| {
            if let Some(e) = &s.job_failed {
                return format!("It did not start: {e}\n\nOpen the log to see why - fix it here or ask the agent, then press Start again.");
            }
            if !s.port_app() {
                return "A Shiny project runs on the platform as soon as it is saved - nothing to start. Open App to see it.".into();
            }
            if s.job_running {
                return "Starting… its output shows in the log; a first start can take a minute while packages load.".into();
            }
            if !s.built && s.verbs.iter().any(|v| v == "build") {
                return "Ready to see your project come alive? Click Build to compile it, then Start.".into();
            }
            "Ready to see your project come alive? Click Start, above the site in App.".into()
        },
        done: Some(|s, _| s.running && (s.app_shown || s.port_app())),
        skip: |s| s.project.is_none(),
        find: "The run controls are at the top of the App tab.",
        on_start: |_| Some(Act::Bring(crate::ide::Tab::App)),
    },
    // 7 · use it
    Step {
        id: "preview",
        targets: |_| t(&["pane:App", "tab:App"]),
        title: "Your application is running",
        body: |s| {
            let try_it = match s.language.as_str() {
                "r" => "Move a slider or switch a view and watch the plot answer.",
                "python" => "Press Train, or change the data, and watch the boundary move.",
                "julia" => "Zoom into the fractal - each frame is computed in Julia.",
                "rust" => "Change the preset or drop a heavy body and watch the bodies answer.",
                _ => "Try its controls.",
            };
            format!("It is live, beside its code. {try_it}")
        },
        done: None,
        skip: |s| s.project.is_none(),
        find: "App is a tab in one of the windows.",
        on_start: |_| Some(Act::Bring(crate::ide::Tab::App)),
    },
    // 8 · change it
    Step {
        id: "change",
        targets: |s| {
            let mut v = Vec::new();
            if let Some(p) = s.page_file() {
                v.push(format!("file:{p}"));
            }
            v.extend(t(&["editor", "tab:Files"]));
            v
        },
        title: "Make your first change",
        body: |s| {
            let f = s.page_file().map(|p| p.rsplit('/').next().unwrap_or(&p).to_owned()).unwrap_or_else(|| "a file".into());
            format!("Open {f}, change a word of its title, and save it (Ctrl+S). The code is the application: what you change here is what runs.")
        },
        done: Some(|s, start| s.saves > start.saves),
        skip: |s| s.project.is_none(),
        find: "Files is a tab in one of the windows.",
        on_start: |_| Some(Act::Bring(crate::ide::Tab::Files)),
    },
    Step {
        id: "rerun",
        targets: |s| {
            if s.running && s.language != "rust" {
                t(&["app:restart", "app:down", "app-strip"])
            } else if s.running {
                t(&["app:down", "app-strip"])
            } else if !s.built {
                t(&["app:build", "app-strip"])
            } else {
                t(&["app:up", "app-strip"])
            }
        },
        title: "Run it again",
        body: |s| {
            if let Some(e) = &s.job_failed {
                return format!("It did not start: {e}\n\nOpen the log, fix it, then Start again.");
            }
            if s.language == "rust" {
                "A Rust app runs what was compiled: Stop, Build, then Start to see your change.".into()
            } else {
                "Your app has its own server: Restart stops it and starts it with your change.".into()
            }
        },
        done: Some(|s, start| s.ups > start.ups && s.running),
        skip: |s| !s.port_app(),
        find: "The run controls are at the top of the App tab.",
        on_start: |_| Some(Act::Bring(crate::ide::Tab::App)),
    },
    Step {
        id: "seen",
        targets: |_| t(&["pane:App", "tab:App"]),
        title: "There is your change",
        body: |s| if s.port_app() { "The page now shows what you saved." } else { "The platform reloaded it when you saved - the page shows your change." }.into(),
        done: None,
        skip: |s| s.project.is_none(),
        find: "App is a tab in one of the windows.",
        on_start: |_| Some(Act::Bring(crate::ide::Tab::App)),
    },
    // 9 · the environment
    Step {
        id: "env",
        targets: |_| t(&["env-picker", "stack-chip", "tab:Software"]),
        title: "What it runs on",
        body: |s| {
            let env = if s.env_module.is_empty() { "a named environment".to_owned() } else { s.env_module.clone() };
            format!(
                "Your project keeps five things apart:\n· its source - the files you edit\n· its packages - the libraries it uses\n· its runtime - {env}, the R, Python, Julia or Rust it runs on\n· running it - Start, Stop and Build\n· provenance - where every package came from, kept by the stack\n\nThe runtime chip at the bottom shows what this session runs; Packages lists what is available and where each one was built."
            )
        },
        done: None,
        skip: never,
        find: "The runtime is in the status bar at the bottom; Packages is a tab.",
        on_start: none,
    },
    // 10 · its address
    Step {
        id: "domain",
        targets: |_| t(&["app-url", "project-button"]),
        title: "Its address",
        body: |s| {
            let host = s.host().unwrap_or("<project>.ndexr.io");
            let who = match s.gate.as_str() {
                "open" => "anyone with the address can open it",
                "login" => "it asks visitors to sign in to ndexr first",
                "password" => "it asks for the site's password",
                "down" => "it is not answering right now",
                _ => "its access is being checked",
            };
            format!("Your application answers at https://{host}/ - {who}. Open ↗ shows it in its own browser tab. Who may see it is the project's setting; to share it with a person, use Messages. The tabs along the top switch between your projects.")
        },
        done: None,
        skip: |s| s.project.is_none(),
        find: "The address is at the top of the App tab.",
        on_start: |_| Some(Act::Bring(crate::ide::Tab::App)),
    },
];

/// A named walkthrough: which steps, and how Help finds it.
pub struct Tour {
    pub id: &'static str,
    pub title: &'static str,
    pub keywords: &'static str,
    pub steps: &'static [&'static str],
}

pub const TOURS: &[Tour] = &[
    Tour {
        id: "first",
        title: "Take the first-project tour",
        keywords: "start begin first project tour new create run edit everything",
        steps: &["ws-project", "ws-agents", "ws-editor", "ws-console", "ws-packages", "ws-app", "ws-run", "ws-modes", "ws-status", "new", "name", "choose", "editor", "run", "preview", "change", "rerun", "seen", "env", "domain"],
    },
    Tour { id: "workspace", title: "Show me around the workspace", keywords: "workspace layout windows panes tabs where agent agents status bar modes", steps: &["ws-project", "ws-agents", "ws-editor", "ws-console", "ws-packages", "ws-app", "ws-run", "ws-modes", "ws-status"] },
    Tour { id: "tabs", title: "Projects, tabs and favorites", keywords: "tabs tab favorites favourite star pin switch open projects reopen closed", steps: &["ws-project", "new", "name"] },
    Tour { id: "new", title: "Create a new project", keywords: "new project create start template starter language", steps: &["new", "name", "choose", "editor"] },
    Tour { id: "run", title: "How to run an application", keywords: "run start up build down launch preview server shiny app", steps: &["run", "preview"] },
    Tour { id: "change", title: "Change my application", keywords: "edit change modify save code reload", steps: &["change", "rerun", "seen"] },
    Tour { id: "files", title: "Understanding files and packages", keywords: "files packages libraries dependencies environment install module", steps: &["ws-editor", "editor", "ws-packages", "env"] },
    Tour { id: "stack", title: "Where the stack comes from", keywords: "stack cvmfs cernvm mount local build airgap remote cell runtime", steps: &["ws-status", "env"] },
    Tour { id: "here", title: "Tour the project open now", keywords: "this project current open tour", steps: &["editor", "run", "preview", "change", "rerun", "seen", "domain"] },
    Tour { id: "domain", title: "Application domains and sharing", keywords: "domain address url share publish deploy public password sign in", steps: &["domain"] },
];

pub fn step(id: &str) -> Option<&'static Step> {
    STEPS.iter().find(|s| s.id == id)
}

pub fn tour(id: &str) -> Option<&'static Tour> {
    TOURS.iter().find(|t| t.id == id)
}

/// Help's "Show me how": the tours a search names, best first.
pub fn search(q: &str) -> Vec<&'static Tour> {
    let words: Vec<String> = q.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 1).map(str::to_owned).collect();
    if words.is_empty() {
        return TOURS.iter().collect();
    }
    let mut hits: Vec<(usize, &Tour)> = TOURS
        .iter()
        .map(|t| {
            let hay = format!("{} {}", t.title.to_lowercase(), t.keywords);
            (words.iter().filter(|w| hay.split_whitespace().any(|h| h.starts_with(w.as_str()))).count(), t)
        })
        .filter(|(n, _)| *n > 0)
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0));
    hits.into_iter().map(|(_, t)| t).collect()
}

// ── the controller ────────────────────────────────────────────────────────

/// Saved with the window state (app.rs `Saved`): the welcome is answered once.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Prefs {
    /// The welcome was answered (any of its three ways, or closed).
    #[serde(default)]
    pub welcomed: bool,
    /// A first-project tour reached its end.
    #[serde(default)]
    pub finished: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Run {
    pub tour: &'static str,
    pub at: usize,
    /// The state when this step began: what `done` compares against.
    pub start: Seen,
    /// The step's own act was asked for.
    acted: bool,
}

/// Help pages that are words, not a walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Shortcuts,
    Troubleshooting,
    Documentation,
}

#[derive(Default)]
pub struct Walk {
    pub prefs: Prefs,
    pub run: Option<Run>,
    /// The finish card, after a first-project tour.
    pub finished_card: bool,
    pub page: Option<Page>,
    /// Help's search box.
    pub find: String,
}

impl Walk {
    /// The welcome is due: never answered, no tour running.
    pub fn welcome_due(&self) -> bool {
        !self.prefs.welcomed && self.run.is_none()
    }

    pub fn start(&mut self, tour: &'static str, seen: &Seen) {
        self.prefs.welcomed = true;
        self.finished_card = false;
        self.run = Some(Run { tour, at: 0, start: seen.clone(), acted: false });
    }

    pub fn stop(&mut self) {
        self.run = None;
    }

    fn steps(&self) -> &'static [&'static str] {
        self.run.as_ref().and_then(|r| tour(r.tour)).map_or(&[], |t| t.steps)
    }

    pub fn current(&self) -> Option<&'static Step> {
        let r = self.run.as_ref()?;
        step(self.steps().get(r.at)?)
    }

    /// Step `n` of how many, skipped steps not counted.
    pub fn progress(&self, seen: &Seen) -> (usize, usize) {
        let Some(r) = &self.run else { return (0, 0) };
        let live: Vec<usize> = self.steps().iter().enumerate().filter(|(_, id)| step(id).is_some_and(|s| !(s.skip)(seen))).map(|(i, _)| i).collect();
        let n = live.iter().position(|i| *i >= r.at).unwrap_or(live.len().saturating_sub(1));
        (n + 1, live.len().max(1))
    }

    fn go(&mut self, to: usize, seen: &Seen) {
        let n = self.steps().len();
        let tour = self.run.as_ref().map(|r| r.tour);
        if to >= n {
            self.run = None;
            if tour == Some("first") {
                self.prefs.finished = true;
                self.finished_card = true;
            }
            return;
        }
        if let Some(r) = &mut self.run {
            r.at = to;
            r.start = seen.clone();
            r.acted = false;
        }
    }

    pub fn next(&mut self, seen: &Seen) {
        if let Some(r) = &self.run {
            let to = r.at + 1;
            self.go(to, seen);
        }
    }

    /// Back to the previous step this project wants.
    pub fn back(&mut self, seen: &Seen) {
        let Some(r) = &self.run else { return };
        let steps = self.steps();
        let to = (0..r.at).rev().find(|&i| step(steps[i]).is_some_and(|s| !(s.skip)(seen)));
        if let Some(i) = to {
            self.go(i, seen);
        }
    }

    /// One frame: skip what this project does not want, move on from a step
    /// that is done, and say what the new step asks of the IDE (once).
    pub fn update(&mut self, seen: &Seen) -> Option<Act> {
        for _ in 0..STEPS.len() + 1 {
            let s = self.current()?;
            if (s.skip)(seen) {
                self.next(seen);
                continue;
            }
            let r = self.run.as_ref()?;
            if s.done.is_some_and(|d| d(seen, &r.start)) {
                self.next(seen);
                continue;
            }
            break;
        }
        let s = self.current()?;
        let r = self.run.as_mut()?;
        if r.acted {
            return None;
        }
        r.acted = true;
        (s.on_start)(seen)
    }

    /// The step's target as found on screen: the first of its targets drawn.
    pub fn target(&self, ctx: &egui::Context, seen: &Seen) -> Option<(String, Rect)> {
        let s = self.current()?;
        (s.targets)(seen).into_iter().find_map(|id| marked(ctx, &id).map(|r| (id, r)))
    }
}

// ── drawing ───────────────────────────────────────────────────────────────

/// Where the card goes: beside the target without covering it, inside the
/// screen; a bottom sheet (or top, when the target is low) on a phone; the
/// middle when there is no target.
pub fn place(screen: Rect, target: Option<Rect>, card: Vec2, narrow: bool) -> Rect {
    let m = 12.0;
    if narrow {
        let w = screen.width() - 2.0 * m;
        let h = card.y.min(screen.height() * 0.5);
        let low = target.is_some_and(|t| t.center().y > screen.center().y);
        // Above the phone's bottom bar (IDE · Go · Builds…), which steps use.
        let y = if low { screen.top() + m } else { screen.bottom() - h - 64.0 };
        return Rect::from_min_size(Pos2::new(screen.left() + m, y), Vec2::new(w, h));
    }
    let Some(t) = target else {
        return Rect::from_center_size(screen.center(), card);
    };
    let clamp = |r: Rect| {
        let x = r.left().clamp(screen.left() + m, (screen.right() - m - r.width()).max(screen.left() + m));
        let y = r.top().clamp(screen.top() + m, (screen.bottom() - m - r.height()).max(screen.top() + m));
        Rect::from_min_size(Pos2::new(x, y), r.size())
    };
    let gap = 14.0;
    let tries = [
        Rect::from_min_size(Pos2::new(t.left(), t.bottom() + gap), card),
        Rect::from_min_size(Pos2::new(t.left(), t.top() - gap - card.y), card),
        Rect::from_min_size(Pos2::new(t.right() + gap, t.top()), card),
        Rect::from_min_size(Pos2::new(t.left() - gap - card.x, t.top()), card),
    ];
    for r in tries {
        let c = clamp(r);
        if !c.intersects(t.expand(4.0)) {
            return c;
        }
    }
    clamp(Rect::from_min_size(Pos2::new(screen.right() - card.x - m, screen.bottom() - card.y - m), card))
}

/// What the card's buttons asked for this frame.
#[derive(Default)]
pub struct Said {
    pub act: Option<Act>,
    pub start: Option<&'static str>,
}

const CARD_W: f32 = 340.0;

impl Walk {
    /// The welcome, a running step's spotlight and card, the finish card and
    /// Help pages - drawn above everything, last in the frame.
    pub fn show(&mut self, ctx: &egui::Context, seen: &Seen, setup_open: bool) -> Said {
        let mut said = Said::default();
        if let Some(p) = self.page {
            self.page_window(ctx, p, &mut said);
        }
        if setup_open {
            return said;
        }
        if self.welcome_due() {
            self.welcome(ctx, seen, &mut said);
            return said;
        }
        if self.finished_card {
            self.finish(ctx, seen, &mut said);
            return said;
        }
        let Some(s) = self.current() else {
            ctx.data_mut(|d| d.remove::<String>(Id::new("tour-want")));
            return said;
        };
        let found = self.target(ctx, seen);
        // The target a scroll area should bring into view (tour::mark).
        let want = (s.targets)(seen).into_iter().next().unwrap_or_default();
        ctx.data_mut(|d| d.insert_temp(Id::new("tour-want"), want));
        let screen = ctx.content_rect();
        if let Some((_, r)) = &found {
            spotlight(ctx, screen, r.expand(5.0));
        }
        let typing = ctx.memory(|m| m.focused().is_some());
        let modal = ctx.memory(|m| m.top_modal_layer().is_some());
        let (n, of) = self.progress(seen);
        let can_next = s.done.is_none();
        let mut go_next = false;
        let mut go_back = false;
        let mut skip = false;
        // Keys: Esc leaves, → next and ← back - only when no field is being
        // typed in and no dialog would take the key for itself.
        if !typing && !modal {
            ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                    skip = true;
                }
                if can_next && i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) {
                    go_next = true;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) {
                    go_back = true;
                }
            });
        }
        let body = (s.body)(seen);
        let missing = found.is_none() && !s.find.is_empty();
        let lines: usize = body.lines().map(|l| 1 + l.chars().count() / 44).sum();
        let extra = if s.id == "choose" && seen.choosing.is_some() { 4.0 * 46.0 } else { 0.0 } + if missing { 30.0 } else { 0.0 };
        let size = Vec2::new(CARD_W, 110.0 + 18.0 * lines as f32 + extra);
        // A target inside a menu or popup: the card keeps clear of the whole
        // popup, not just the item (it covered the project menu's list).
        let keep_clear = found.as_ref().map(|(_, r)| {
            let popup = ctx.layer_id_at(r.center()).filter(|l| l.order == Order::Foreground).and_then(|l| ctx.memory(|m| m.area_rect(l.id)));
            popup.filter(|p| p.width() < screen.width() * 0.9).map_or(*r, |p| r.union(p))
        });
        let at = place(screen, keep_clear, size, seen.narrow);
        let failed = seen.job_failed.is_some() && matches!(s.id, "run" | "rerun");
        let resp = card(ctx, "tour-card", at, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(format!("{n} of {of}")).size(11.5).color(theme::muted()));
                dots(ui, n, of);
            });
            ui.add_space(2.0);
            ui.label(RichText::new(s.title).size(16.0).strong().color(theme::text_strong()));
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new(&body).size(13.5).color(theme::text())).wrap());
            if missing {
                ui.add_space(4.0);
                ui.add(egui::Label::new(RichText::new(format!("Not on screen: {}", s.find)).size(12.5).color(theme::warn())).wrap());
            }
            if s.id == "choose" && seen.choosing.is_some() {
                ui.add_space(6.0);
                for (lang, starter, app, what) in RECOMMENDED {
                    let b = egui::Button::new(RichText::new(format!("{lang} · {app}")).size(13.0).strong().color(theme::text_strong()))
                        .fill(theme::surface())
                        .stroke(Stroke::new(1.0, theme::primary().gamma_multiply(0.5)))
                        .min_size(egui::vec2(ui.available_width(), 26.0));
                    if ui.add(b).on_hover_text("pick it in the explorer - then press Create").clicked() {
                        said.act = Some(Act::Recommend(starter));
                    }
                    ui.label(RichText::new(what).size(12.0).color(theme::muted()));
                }
            }
            if failed && ui.add(egui::Button::new(RichText::new("Open the log").size(12.5))).clicked() {
                said.act = Some(Act::AppLog);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if n > 1 && ui.button("Back").clicked() {
                    go_back = true;
                }
                if can_next {
                    if ui.add(theme::primary_button(if n == of { "Finish" } else { "Next" })).clicked() {
                        go_next = true;
                    }
                } else {
                    ui.label(RichText::new("waiting for you…").size(12.0).color(theme::hush()));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("Skip tour").size(12.0)).frame(false)).clicked() {
                        skip = true;
                    }
                });
            });
        });
        ctx.data_mut(|d| d.insert_temp(Id::new("tour-card-rect"), resp));
        if skip {
            self.stop();
        } else if go_back {
            self.back(seen);
        } else if go_next {
            self.next(seen);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        said
    }

    fn welcome(&mut self, ctx: &egui::Context, seen: &Seen, said: &mut Said) {
        let screen = ctx.content_rect();
        let at = place(screen, None, Vec2::new(420.0, 230.0), seen.narrow);
        let mut close = false;
        card(ctx, "tour-welcome", at, |ui| {
            ui.label(RichText::new("Welcome to your workspace").size(20.0).strong().color(theme::text_strong()));
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new("Build, run and share scientific applications here, with an agent beside you. Your projects open as tabs along the top. Let's create your first one.").size(14.0).color(theme::text())).wrap());
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add(theme::primary_button("Start guided tour")).clicked() {
                    said.start = Some("first");
                }
                if seen.project.is_some() && ui.button("Tour this project").on_hover_text("the editor, running and changing the project open now").clicked() {
                    said.start = Some("here");
                }
                if ui.button("Create a project").clicked() {
                    said.act = Some(Act::NewProject);
                    close = true;
                }
                if ui.add(egui::Button::new("Explore on my own").frame(false)).clicked() {
                    close = true;
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new("Help (F1) › Show me how starts it again any time.").size(12.5).color(theme::text()));
        });
        if close {
            self.prefs.welcomed = true;
        }
    }

    fn finish(&mut self, ctx: &egui::Context, seen: &Seen, said: &mut Said) {
        let at = place(ctx.content_rect(), None, Vec2::new(420.0, 230.0), seen.narrow);
        let mut close = false;
        card(ctx, "tour-finish", at, |ui| {
            ui.label(RichText::new("Your first project is running!").size(20.0).strong().color(theme::text_strong()));
            ui.add_space(4.0);
            ui.add(egui::Label::new(RichText::new("You've created a project, explored the code, launched your application, and seen your changes come to life.").size(14.0).color(theme::text())).wrap());
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add(theme::primary_button("Open my project")).clicked() {
                    close = true;
                }
                if ui.button("Explore another language").clicked() {
                    said.act = Some(Act::NewProject);
                    close = true;
                }
                if ui.button("Packages and environments").clicked() {
                    said.start = Some("files");
                }
                if ui.button("Deployment and sharing").clicked() {
                    said.start = Some("domain");
                }
            });
        });
        if close || said.start.is_some() {
            self.finished_card = false;
        }
    }

    fn page_window(&mut self, ctx: &egui::Context, p: Page, said: &mut Said) {
        let mut open = true;
        let title = match p {
            Page::Shortcuts => "Keyboard shortcuts",
            Page::Troubleshooting => "Troubleshooting",
            Page::Documentation => "Documentation",
        };
        egui::Window::new(title).open(&mut open).default_width(460.0).collapsible(false).show(ctx, |ui| match p {
            Page::Shortcuts => {
                egui::Grid::new("tour-keys").num_columns(2).spacing([18.0, 6.0]).show(ui, |ui| {
                    for (k, what) in SHORTCUTS {
                        ui.label(RichText::new(*k).monospace().size(12.5).color(theme::text_strong()));
                        ui.label(RichText::new(*what).size(13.0).color(theme::text()));
                        ui.end_row();
                    }
                });
                ui.label(RichText::new("Ctrl is ⌘ on a Mac.").size(12.0).color(theme::hush()));
            }
            Page::Troubleshooting => {
                for (q, a) in TROUBLE {
                    ui.label(RichText::new(*q).size(13.5).strong().color(theme::text_strong()));
                    ui.add(egui::Label::new(RichText::new(*a).size(13.0).color(theme::text())).wrap());
                    ui.add_space(6.0);
                }
                if ui.button("Run the walkthrough for running an app").clicked() {
                    said.start = Some("run");
                }
            }
            Page::Documentation => {
                ui.add(egui::Label::new(RichText::new("Every project carries its own documents; they open in the editor.").size(13.0).color(theme::text())).wrap());
                ui.add_space(6.0);
                for (name, what) in [("CONTEXT.md", "what the project is, its files and decisions - the agent reads it too"), ("APP-CONTRACT.md", "how .cove/build.sh, up.sh and down.sh run your app (port apps)"), ("README.md", "how to use it, where the starter has one")] {
                    ui.horizontal_wrapped(|ui| {
                        if ui.button(name).clicked() {
                            said.act = Some(Act::Open(name.to_owned()));
                        }
                        ui.label(RichText::new(what).size(12.5).color(theme::text()));
                    });
                }
            }
        });
        if !open {
            self.page = None;
        }
    }
}

pub const SHORTCUTS: &[(&str, &str)] = &[
    ("Ctrl+S", "save the file"),
    ("Ctrl+Enter", "run the line or selection in the console"),
    ("Ctrl+Shift+Enter", "run the whole file"),
    ("Ctrl+Alt+R", "run every chunk (Quarto, R Markdown)"),
    ("Ctrl+Shift+K", "render the document"),
    ("Ctrl+F", "find in the file"),
    ("Ctrl+Shift+F", "find in every file"),
    ("Ctrl+Shift+N", "a new R script"),
    ("Ctrl+Shift+1", "Files full-window"),
    ("Ctrl+Shift+2", "Console full-window"),
    ("Ctrl+Shift+3", "App full-window"),
    ("Ctrl+Shift+0", "every window back"),
    ("Ctrl+Shift+F10", "restart the console's language"),
    ("Ctrl+L", "clear the console"),
    ("F11", "full screen"),
    ("F1", "Help: show me how, shortcuts, troubleshooting"),
    ("← →", "the previous or next project tab"),
    ("Middle-click", "close a project tab"),
    ("Esc", "leave a walkthrough"),
];

const TROUBLE: &[(&str, &str)] = &[
    ("Start does nothing, or says it exited", "Click the app's state at the top of App for the last job's log, or Logs for the app's own output: the last lines say why. A Rust app needs Build before Start; a first Julia start compiles packages for a few minutes."),
    ("The App tab is blank", "The project's name at the top of App reloads it. If it says Stopped, the app is not running - press Start, beside it. If it asks you to sign in, that is the site's access setting."),
    ("A package is missing", "Packages lists what your environment has. Ask the agent to add it to your project, or send a request from Help."),
    ("R, Python or a module is not found", "The stack chip in the status bar says where the stack comes from. Mounting it with CernVM-FS is the default; click the chip to set it up or change it."),
    ("My change does not show", "Save the file (Ctrl+S). A Shiny project reloads on save; a project with its own server needs Restart."),
    ("The console stopped answering", "Ctrl+Shift+F10 restarts it in the same environment."),
    ("I am not signed in", "A request still goes: Help › Send a request, or Messages, files it as anonymous - leave an email if you want the answer. Threads, chat and the agent need an account."),
];

/// A floating card above everything; returns where it was drawn.
fn card(ctx: &egui::Context, id: &str, at: Rect, add: impl FnOnce(&mut egui::Ui)) -> Rect {
    let r = egui::Area::new(Id::new(id)).order(Order::Tooltip).fixed_pos(at.min).interactable(true).show(ctx, |ui| {
        egui::Frame::new()
            .fill(theme::surface_hi())
            .stroke(Stroke::new(1.0, theme::primary().gamma_multiply(0.7)))
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(16, 12))
            .shadow(egui::epaint::Shadow { offset: [0, 6], blur: 24, spread: 0, color: Color32::from_black_alpha(110) })
            .show(ui, |ui| {
                ui.set_width(at.width() - 32.0);
                add(ui);
            });
    });
    // Above the spotlight's dim, which shares the top order.
    ctx.move_to_top(r.response.layer_id);
    r.response.rect
}

/// The screen dimmed except `hole`, and a soft ring round it. Painted only:
/// the dim never takes a click, so the spotlit control and the rest stay usable.
fn spotlight(ctx: &egui::Context, screen: Rect, hole: Rect) {
    // An area of its own, not a bare layer painter: egui paints bare layers
    // after every area of their order, so the dim landed on the card. As an
    // area that takes no input it sits under the card (moved to top after
    // it) and never catches a click.
    egui::Area::new(Id::new("tour-dim")).order(Order::Tooltip).interactable(false).fixed_pos(screen.min).show(ctx, |ui| {
        ui.set_clip_rect(screen);
        dim(ui.painter(), ctx, screen, hole);
    });
}

fn dim(p: &egui::Painter, ctx: &egui::Context, screen: Rect, hole: Rect) {
    let dim = Color32::from_black_alpha(96);
    let h = hole.intersect(screen);
    for r in [
        Rect::from_min_max(screen.min, Pos2::new(screen.right(), h.top())),
        Rect::from_min_max(Pos2::new(screen.left(), h.bottom()), screen.max),
        Rect::from_min_max(Pos2::new(screen.left(), h.top()), Pos2::new(h.left(), h.bottom())),
        Rect::from_min_max(Pos2::new(h.right(), h.top()), Pos2::new(screen.right(), h.bottom())),
    ] {
        if r.is_positive() {
            p.rect_filled(r, 0.0, dim);
        }
    }
    let t = ctx.input(|i| i.time) as f32;
    let pulse = 0.55 + 0.45 * (t * 2.4).sin().abs();
    p.rect_stroke(h, 8.0, Stroke::new(6.0, theme::primary().gamma_multiply(0.18 * pulse)), egui::StrokeKind::Outside);
    p.rect_stroke(h, 8.0, Stroke::new(2.0, theme::primary().gamma_multiply(pulse)), egui::StrokeKind::Outside);
}

fn dots(ui: &mut egui::Ui, n: usize, of: usize) {
    let shown = of.min(17);
    let (r, _) = ui.allocate_exact_size(Vec2::new(shown as f32 * 8.0, 10.0), egui::Sense::hover());
    for i in 0..shown {
        let c = Pos2::new(r.left() + 4.0 + i as f32 * 8.0, r.center().y);
        let on = i + 1 <= n;
        ui.painter().circle_filled(c, if i + 1 == n { 3.2 } else { 2.4 }, if on { theme::primary() } else { theme::hush().gamma_multiply(0.6) });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn port_project() -> Seen {
        Seen { project: Some(("demo.ndexr.io".into(), "/d/demo.ndexr.io".into())), kind: "port".into(), language: "python".into(), built: true, verbs: vec!["build".into(), "up".into(), "down".into()], ..Default::default() }
    }

    fn at(w: &Walk) -> &'static str {
        w.current().map_or("-", |s| s.id)
    }

    #[test]
    fn every_tour_names_real_steps() {
        for t in TOURS {
            for id in t.steps {
                assert!(step(id).is_some(), "{} names unknown step {id}", t.id);
            }
        }
        // The welcome's "Tour this project" and the finish card start these.
        for id in ["first", "here", "files", "domain", "run", "new", "change"] {
            assert!(tour(id).is_some(), "{id}");
        }
    }

    /// Every target a step names is marked somewhere in the UI. The project
    /// button was drawn by a function nothing called once the tabs took its
    /// place (2026-11-14), and the first step of every tour pointed at nothing.
    #[test]
    fn every_target_is_marked_somewhere() {
        let src = [include_str!("ide.rs"), include_str!("app.rs")].concat();
        let mut kinds = vec![Seen::default(), port_project()];
        let mut r = port_project();
        r.running = true;
        kinds.push(r.clone());
        r.language = "rust".into();
        r.built = false;
        kinds.push(r);
        for s in STEPS {
            for seen in &kinds {
                for id in (s.targets)(seen) {
                    let found = if let Some(tab) = id.strip_prefix("tab:").or_else(|| id.strip_prefix("pane:")) {
                        src.contains(&format!("Tab::{tab} =>"))
                    } else if id.starts_with("file:") {
                        src.contains("tour::mark(ui, &format!(\"file:{full}\")")
                    } else {
                        src.contains(&format!("\"{id}\""))
                    };
                    assert!(found, "step {} points at {id}, which nothing marks", s.id);
                }
            }
        }
    }

    #[test]
    fn action_steps_wait_for_the_real_thing() {
        let mut w = Walk::default();
        let mut s = Seen::default();
        w.start("new", &s);
        assert_eq!(at(&w), "new");
        assert_eq!(w.update(&s), None);
        assert_eq!(at(&w), "new", "nothing done yet: it waits");
        // The person opened the New project dialog.
        s.naming = true;
        w.update(&s);
        assert_eq!(at(&w), "name");
        // Named it: the explorer is open for that host.
        s.naming = false;
        s.choosing = Some("rix.ndexr.io".into());
        w.update(&s);
        assert_eq!(at(&w), "choose");
        // Created and opened: the explorer closes on that very project.
        s.choosing = None;
        s.project = Some(("rix.ndexr.io".into(), "/d/rix.ndexr.io".into()));
        s.kind = "port".into();
        s.language = "python".into();
        w.update(&s);
        assert_eq!(at(&w), "editor");
        // Another project opening instead does not count as created.
        let mut w2 = Walk::default();
        let mut s2 = Seen { choosing: Some("rix.ndexr.io".into()), ..Default::default() };
        w2.start("new", &s2);
        w2.update(&s2);
        assert_eq!(at(&w2), "choose");
        s2.choosing = None;
        s2.project = Some(("other.ndexr.io".into(), "/d/other".into()));
        w2.update(&s2);
        assert_eq!(at(&w2), "choose", "closing the explorer onto another project is not creating one");
    }

    #[test]
    fn opening_the_entry_file_and_running_move_on_only_when_true() {
        let mut w = Walk::default();
        let mut s = port_project();
        w.start("first", &s);
        w.run.as_mut().unwrap().at = TOURS[0].steps.iter().position(|x| *x == "editor").unwrap();
        assert_eq!(w.update(&s), Some(Act::Bring(crate::ide::Tab::Files)));
        assert_eq!(w.update(&s), None, "a step's act is asked for once");
        s.open_files = vec!["/d/demo.ndexr.io/README.md".into()];
        w.update(&s);
        assert_eq!(at(&w), "editor", "another file is not the entry point");
        s.open_files.push("/d/demo.ndexr.io/app.py".into());
        w.update(&s);
        assert_eq!(at(&w), "run");
        // Clicking Up is not success: the job runs, then fails.
        s.job_running = true;
        w.update(&s);
        assert_eq!(at(&w), "run");
        s.job_running = false;
        s.job_failed = Some("port 8840 already in use".into());
        w.update(&s);
        assert_eq!(at(&w), "run", "a failed start stays, with the error");
        assert!((step("run").unwrap().body)(&s).contains("port 8840 already in use"));
        s.job_failed = None;
        s.running = true;
        w.update(&s);
        assert_eq!(at(&w), "preview");
    }

    #[test]
    fn a_change_counts_only_when_saved_and_rerun() {
        let mut w = Walk::default();
        let mut s = port_project();
        s.running = true;
        s.saves = 3;
        s.ups = 1;
        w.start("change", &s);
        w.update(&s);
        assert_eq!(at(&w), "change");
        s.dirty = true;
        w.update(&s);
        assert_eq!(at(&w), "change", "typing is not saving");
        s.dirty = false;
        s.saves = 4;
        w.update(&s);
        assert_eq!(at(&w), "rerun");
        s.ups = 2;
        w.update(&s);
        assert_eq!(at(&w), "seen");
        // A Shiny project reloads on save: no rerun step.
        let mut m = Walk::default();
        let mut sm = Seen { project: Some(("geo.ndexr.io".into(), "/d/geo".into())), kind: "module".into(), language: "r".into(), running: true, ..Default::default() };
        m.start("change", &sm);
        m.update(&sm);
        sm.saves = 1;
        m.update(&sm);
        assert_eq!(at(&m), "seen");
    }

    #[test]
    fn a_running_app_skips_the_start_and_back_skips_what_does_not_apply() {
        let mut w = Walk::default();
        let mut s = port_project();
        s.running = true;
        w.start("run", &s);
        w.update(&s);
        assert_eq!(at(&w), "preview", "already up: no Up to press");
        w.back(&s);
        assert_eq!(at(&w), "run");
        // Back from the first step stays there.
        let mut f = Walk::default();
        f.start("workspace", &s);
        f.back(&s);
        assert_eq!(at(&f), "ws-project");
    }

    #[test]
    fn welcome_once_restart_any_time_and_finish_is_remembered() {
        let mut w = Walk::default();
        assert!(w.welcome_due());
        let s = Seen::default();
        w.start("workspace", &s);
        assert!(!w.welcome_due() && w.prefs.welcomed);
        w.stop();
        assert!(!w.welcome_due(), "never forced again");
        // Help starts it again from the top, whatever was before.
        w.start("first", &s);
        assert_eq!(at(&w), "ws-project");
        for _ in 0..TOURS[0].steps.len() {
            w.next(&s);
        }
        assert!(w.run.is_none() && w.prefs.finished && w.finished_card);
        // Another tour's end is not the first project's.
        let mut o = Walk::default();
        o.start("domain", &s);
        o.next(&s);
        assert!(!o.prefs.finished && !o.finished_card);
    }

    #[test]
    fn the_card_stays_on_screen_and_off_its_target() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0));
        let card = Vec2::new(340.0, 180.0);
        for t in [
            Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(120.0, 24.0)),
            Rect::from_min_size(Pos2::new(1200.0, 760.0), Vec2::new(70.0, 30.0)),
            Rect::from_min_size(Pos2::new(400.0, 300.0), Vec2::new(500.0, 400.0)),
        ] {
            let r = place(screen, Some(t), card, false);
            assert!(screen.contains_rect(r), "{r:?} off screen for {t:?}");
            assert!(!r.intersects(t), "{r:?} covers {t:?}");
        }
        // No target: the middle. A phone: a sheet along an edge.
        assert_eq!(place(screen, None, card, false).center(), screen.center());
        let phone = Rect::from_min_size(Pos2::ZERO, Vec2::new(390.0, 844.0));
        let high = place(phone, Some(Rect::from_min_size(Pos2::new(10.0, 40.0), Vec2::new(80.0, 30.0))), card, true);
        assert!(high.bottom() > 700.0 && high.bottom() <= 844.0 - 64.0 && (high.width() - 366.0).abs() < 1.0);
        let low = place(phone, Some(Rect::from_min_size(Pos2::new(10.0, 700.0), Vec2::new(80.0, 30.0))), card, true);
        assert!(low.top() < 20.0);
    }

    #[test]
    fn show_me_how_finds_tours_by_task() {
        assert_eq!(search("how do I run my shiny app").first().map(|t| t.id), Some("run"));
        assert_eq!(search("publish and share").first().map(|t| t.id), Some("domain"));
        assert_eq!(search("install a package").first().map(|t| t.id), Some("files"));
        assert_eq!(search("").len(), TOURS.len());
        assert!(search("zzz").is_empty());
    }

    #[test]
    fn the_recommended_starters_exist_and_have_the_files_the_tour_points_at() {
        for (lang, starter, _, _) in RECOMMENDED {
            assert!(crate::starters::FOLDER_STARTERS.contains(&starter), "{lang}: {starter} is not a starter");
            let files = crate::host::starter_file_names(starter);
            let kind_lang = match lang {
                "R" => ("module", "r"),
                "Python" => ("port", "python"),
                "Julia" => ("port", "julia"),
                _ => ("port", "rust"),
            };
            let s = Seen { project: Some(("x.ndexr.io".into(), "/p".into())), kind: kind_lang.0.into(), language: kind_lang.1.into(), ..Default::default() };
            for want in [s.entry().unwrap(), s.page_file().unwrap()] {
                let rel = want.trim_start_matches("/p/");
                assert!(files.iter().any(|f| f == rel), "{starter} has no {rel} for the tour to point at");
            }
        }
    }
}
