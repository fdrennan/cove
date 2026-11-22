//! The app: RStudio Server's frame, drawn natively, around ide.rs's panes.
//! Everything that touches a machine goes through `Backend` (backend.rs), so
//! this same UI runs as the desktop app and, compiled to WebAssembly, in a
//! browser against `cove serve` — the server version cove.ndexr.io shows.
//!
//!   global toolbar  ☰ (File · View · Session · Help) · ◎ Atlas · Save ·
//!                   environment · domain … Claude · session; no menu bar
//!                   (operator, 2026-10-18)
//!   windows         up to four, one per corner (`Dock`): by default
//!                     Console · Terminal · Agents · Files (left) |
//!                     App · Software · Plots · Data (right)
//!
//! Drag a tab to a corner and it gets a window there (two, three or four of
//! them), as PyCharm and VS Code do. The splitters drag and keep where they
//! were left (egui's panel state, persisted). Any window zooms to the whole
//! screen and back: its control at the top right, View › Panes, or
//! Ctrl+Shift+1…4 and Ctrl+Shift+0, as in RStudio. Tools › Global Options is the pimp's module picker
//! (Inlet 0004): the Lmod module each kernel starts inside - and
//! the Environment: the ndexr stack on this machine, set up in the background
//! when it is missing, and the CVMFS client's proxy and cache.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui::{self, Key, KeyboardShortcut, Modifiers, RichText};
use serde::{Deserialize, Serialize};

use crate::api::{self, CvmfsSettings, JobView, Lang, Modules, ProxyCheck, Query, Settings, SettingsView};
use crate::backend::Backend;
use crate::dock::{At, Drop, Grid};
use crate::ide::{Ide, Tab};
use crate::theme;
#[cfg(not(target_arch = "wasm32"))]
use crate::update::{self, Status, Updater};

type Slot<T> = Arc<Mutex<Option<T>>>;

/// The 2×2 corners layouts were saved in before 2026-10-19 (0 top-left,
/// 1 top-right, 2 bottom-left, 3 bottom-right): read once and turned into a
/// `Grid` (dock.rs), so nobody's saved layout is lost.
#[derive(Clone, Default, Serialize, Deserialize)]
struct Corners {
    cells: [Vec<Tab>; 4],
    front: [Option<Tab>; 4],
}

/// A tab being dragged to another window, and where the windows were drawn
/// this frame (the drop target and its preview are read from them).
struct Drag {
    tab: Option<Tab>,
    area: egui::Rect,
    rects: Vec<(At, egui::Rect)>,
    strips: Vec<(At, egui::Rect)>,
    /// The gaps between windows this frame, each a splitter: (gap, between columns).
    gaps: Vec<(egui::Rect, bool)>,
}

impl Default for Drag {
    fn default() -> Self {
        Drag { tab: None, area: egui::Rect::NOTHING, rects: Vec::new(), strips: Vec::new(), gaps: Vec::new() }
    }
}

/// The gap between windows, in points.
fn win_gap() -> f32 {
    f32::from(theme::WIN_GAP as u8)
}

/// Where each window of `g` sits in `area`: columns by their widths, each
/// column's windows by their heights, `win_gap()` between them. Also each
/// splitter: (gap, between columns, column, the boundary's index).
fn grid_rects(area: egui::Rect, g: &Grid) -> (Vec<(At, egui::Rect)>, Vec<(egui::Rect, bool, usize, usize)>) {
    let gap = win_gap();
    let n = g.cols.len();
    let usable_w = (area.width() - gap * n.saturating_sub(1) as f32).max(1.0);
    let (mut out, mut gaps) = (Vec::new(), Vec::new());
    let mut x = area.left();
    for (c, col) in g.cols.iter().enumerate() {
        let w = usable_w * g.widths.get(c).copied().unwrap_or(1.0 / n as f32);
        let usable_h = (area.height() - gap * col.len().saturating_sub(1) as f32).max(1.0);
        let mut y = area.top();
        for r in 0..col.len() {
            let h = usable_h * g.heights.get(c).and_then(|h| h.get(r)).copied().unwrap_or(1.0 / col.len() as f32);
            out.push(((c, r), egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h))));
            y += h;
            if r + 1 < col.len() {
                gaps.push((egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, gap)), false, c, r));
                y += gap;
            }
        }
        x += w;
        if c + 1 < n {
            gaps.push((egui::Rect::from_min_size(egui::pos2(x, area.top()), egui::vec2(gap, area.height())), true, c, c));
            x += gap;
        }
    }
    (out, gaps)
}

/// The tab a page was opened for (`?tab=Plots`, a window's "Open in a new
/// page"): that tab alone fills it. None for the whole IDE.
fn solo_tab() -> Option<Tab> {
    #[cfg(target_arch = "wasm32")]
    {
        let want = web_sys::window()
            .and_then(|w| w.location().search().ok())
            .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok())
            .and_then(|p| p.get("tab"))?;
        return Tab::ALL.into_iter().find(|t| format!("{t:?}").eq_ignore_ascii_case(&want) || t.name().eq_ignore_ascii_case(&want));
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

/// The agent a link asks for: ?agent=all (or exec) for all sites, ?agent=site
/// for this site's. None when the address says nothing.
fn url_agent() -> Option<bool> {
    #[cfg(target_arch = "wasm32")]
    {
        let want = web_sys::window()
            .and_then(|w| w.location().search().ok())
            .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok())
            .and_then(|p| p.get("agent"))?;
        return match want.to_ascii_lowercase().as_str() {
            "all" | "exec" | "all-sites" => Some(true),
            "site" | "this" | "this-site" => Some(false),
            _ => None,
        };
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

/// The address of a page holding only `t`: this page's own, with ?tab=.
fn solo_url(t: Tab) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let l = web_sys::window()?.location();
        return Some(format!("{}{}?tab={t:?}", l.origin().ok()?, l.pathname().ok()?));
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = t;
        None
    }
}

fn no_grid() -> Grid {
    Grid { cols: Vec::new(), widths: Vec::new(), heights: Vec::new() }
}

/// Cove's modes, one at a time (header mode_tabs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Ide,
    Go,
    Connect,
    Builds,
    Atlas,
    /// Who you are, your cell, Claude, your domains and API keys (request
    /// #167, 2026-11-01): in Files' toolbar from 2026-10-28 until then.
    Account,
}

/// What survives a restart (the splitters persist on their own).
#[derive(Default, Serialize, Deserialize)]
struct Saved {
    /// The tab whose window fills the screen (Ctrl+Shift+1…4, a window's zoom control).
    #[serde(default)]
    zoom_tab: Option<Tab>,
    /// The windows (dock.rs). Empty when a layout from before 2026-10-19
    /// was read: then `dock`, the corners it was saved in, makes it.
    #[serde(default = "no_grid")]
    grid: Grid,
    #[serde(default)]
    dock: Corners,
    /// The domain this person was in, and the files open in each domain:
    /// the next start steps back into it with those files.
    #[serde(default)]
    domain: Option<String>,
    #[serde(default)]
    open_by_domain: std::collections::HashMap<String, Vec<String>>,
    /// Where the splitter between the left and right windows sits, as the
    /// left column's share of the width.
    #[serde(default)]
    split_x: Option<f32>,
    /// Dark, Light or the system's (setup.rs; View › Theme).
    #[serde(default)]
    theme: crate::setup::ThemePref,
    /// First-start setup has been through (or skipped) on this machine / browser.
    #[serde(default)]
    setup_seen: bool,
    /// The Agents tab's Site/Exec layout and its divider (ide.rs agents_tab).
    #[serde(default)]
    agents_layout: crate::ide::AgentsLayout,
    #[serde(default)]
    agents_frac: Option<f32>,
    /// The agent chosen beside the project: all sites (Exec) or this site.
    #[serde(default)]
    agent_all: bool,
    /// The projects stepped into, newest first (the ☰ menu's Recent projects).
    #[serde(default)]
    recent_domains: Vec<String>,
    /// The open projects as tabs (tabs.rs): their order, the pinned ones,
    /// the ones closed lately.
    #[serde(default)]
    tabs: crate::tabs::Tabs,
    /// The walkthroughs' welcome answered, the first-project tour finished (tour.rs).
    #[serde(default)]
    tour: crate::tour::Prefs,
    /// Sites with live refresh off (ide.rs `live_off`).
    #[serde(default)]
    live_off: std::collections::BTreeSet<String>,
    /// Speak beside Listen (voice.rs `speak`).
    #[serde(default)]
    speak: bool,
    /// Minutes between Listen's pieces, 2026-11-11 to 11-13: still written
    /// (whole minutes, at least 1) for a session on an older release, read
    /// only when `listen_secs` is not saved yet.
    #[serde(default)]
    listen_minutes: u8,
    /// Seconds between Listen's pieces (voice.rs PIECES); 0, never saved.
    #[serde(default)]
    listen_secs: u32,
}

/// The modes, as the top row names them (the review, 2026-11-14: "global
/// navigation: Workspace · Builds · Atlas · Connections · Account"). IDE was
/// "IDE" and Connect "Connect" until then.
const MODES: [(Mode, &str, &str); 6] = [
    (Mode::Ide, "Workspace", "your projects: agents, files, consoles and the app"),
    (Mode::Go, "Go", "on the go: the recap, the agent and Exec, a big record button"),
    (Mode::Connect, "Connections", "every app you may open - yours, shared with you, and public - by what it is built with"),
    (Mode::Builds, "Builds", "the build machines - start and shut them down here - and where every build is, live, with its history"),
    (Mode::Atlas, "Atlas", "the whole stack, kernel to application"),
    (Mode::Account, "Account", "you: your cell, Claude, your domains and API keys"),
];

pub struct App {
    backend: Backend,
    saved: Saved,
    ide: Ide,
    /// A first visit may open the person's own ndexr, once their own site has
    /// had its turn.
    ndexr_due: bool,
    /// The machine: None until it answers.
    probe: Slot<api::Probe>,
    settings: Slot<SettingsView>,
    /// Global Options, while open: its edit buffer.
    options: Option<Settings>,
    /// Global Options' category, as RStudio's sidebar (OPTION_PAGES).
    options_page: usize,
    /// The module picker's choices, asked when Global Options opens.
    modules: Slot<Modules>,
    /// The domain whose environment was last put in effect (`follow_domain_env`).
    env_host: Option<String>,
    /// The layered-modules menu's filter.
    extra_filter: String,
    /// ☰ › Project › Settings… is open: the project's environment and modules.
    project_settings: bool,
    /// Kernels to restart once a save has landed, in their new module.
    restart_after_save: Arc<Mutex<Vec<Lang>>>,
    /// A Pin that landed: the domain, and what its .ndexr-env names now.
    pinned: Arc<Mutex<Option<(String, String)>>>,
    /// Opened Global Options on its own once, when nothing was chosen yet.
    asked_options: bool,
    /// On a phone (compact): the one window showing, picked from the bottom bar.
    compact_cell: usize,
    /// A tab being dragged between windows.
    drag: Drag,
    /// The middle splitter is being dragged (the site's frame hides meanwhile).
    splitting: bool,
    /// A page opened for one tab (`?tab=`): that tab alone, and the layout
    /// is not saved from it - it shares the browser's store with the page
    /// that opened it.
    solo: Option<Tab>,
    /// A kernel was busy last frame (compact: a run brings the console forward).
    was_busy: bool,
    /// When this page first found its session gone (backend::session_ended).
    ended_at: Option<f64>,
    /// ndexr.io's night photograph behind the windows (backdrop.rs).
    backdrop: crate::backdrop::Backdrop,
    /// The still lake the restart notice opens with.
    calm: crate::backdrop::Calm,
    /// Help › Send a request, while open; and what sending it answered.
    request: Option<RequestDraft>,
    request_sent: Arc<Mutex<Option<Result<i64, String>>>>,
    /// Messages: requests and their answers, people, chat (inbox.rs).
    inbox: crate::inbox::Inbox,
    /// Project › Access (access.rs): who may open the open project's app.
    access: crate::access::Access,
    /// Connections' management of the person's own projects (manage.rs).
    manage: crate::manage::Manage,
    /// Files dropped on the window while Send a request is open, as they are read.
    request_drops: Arc<Mutex<Vec<crate::api::Attachment>>>,
    /// The request's microphone: say it instead of typing (voice.rs).
    request_mic: crate::voice::Mic,
    /// Reaching the cell from this window (account_button): whether a try is
    /// out, what the last one answered, when to try again, and whether the
    /// window had focus last frame (coming back to it retries at once).
    #[cfg(not(target_arch = "wasm32"))]
    cell_try: Arc<Mutex<(bool, Option<Result<(), String>>)>>,
    #[cfg(not(target_arch = "wasm32"))]
    cell_retry_at: f64,
    #[cfg(not(target_arch = "wasm32"))]
    was_focused: bool,
    /// Connect to a server (server_modal): open, the address and token typed,
    /// and the probe out on a thread (Some(answer) when it came back).
    #[cfg(not(target_arch = "wasm32"))]
    server_form: Option<(String, String)>,
    #[cfg(not(target_arch = "wasm32"))]
    server_try: Arc<Mutex<(bool, Option<Result<(), String>>)>>,
    /// A folder asked for at launch (?folder= / --folder) or picked with
    /// File › Open Folder, opened as the project once the saved settings
    /// have arrived.
    folder: Option<String>,
    /// File › Open Folder's pick, from the dialog's thread to the next frame.
    #[cfg(not(target_arch = "wasm32"))]
    picked_folder: Arc<Mutex<Option<String>>>,
    about: bool,
    /// Help, docked at the right (help_panel): searchable, never a cascade.
    help_open: bool,
    /// This run has asked for the stack mode once (auto_setup).
    mode_asked: bool,
    /// Just signed in on this computer, with a cell: ask whether to go to it
    /// (cell_offer_modal).
    cell_offer: bool,
    /// Cove's other sessions, in a window of their own.
    sessions_open: bool,
    /// Setting the stack up or applying CVMFS settings: the job, polled
    /// while it runs; and whether this window has started a setup itself.
    stack_job: Arc<Mutex<JobView>>,
    stack_asked: f64,
    stack_inflight: Arc<Mutex<bool>>,
    auto_setup: bool,
    /// This launch is the person's first start: the stack is theirs to
    /// choose (setup's Stack step), so nothing sets it up unasked; a browser
    /// on a server with no stack gets that step once settings arrive.
    first_start: bool,
    /// Global Options' proxy test, while it runs and after.
    proxy_test: Slot<Result<Vec<ProxyCheck>, String>>,
    testing: bool,
    /// Messages from answers that arrive off the UI thread.
    notes: Arc<Mutex<Vec<String>>>,
    toast: Option<(String, f64)>,
    /// A star's source, once its workspace is ready (atlas Source, source.rs).
    source_ready: Slot<Result<crate::source::SourceView, String>>,
    /// The new project's domain registered as theirs (or why it was not).
    claim_ready: Slot<Result<Vec<String>, String>>,
    /// The person's storage against their plan (storage.rs), asked every 30 s;
    /// None where nothing measures it. The dialog, while open.
    storage: Slot<api::StorageView>,
    storage_asked: f64,
    storage_open: bool,
    /// "Let the agent clear it" sent and not yet answered.
    storage_clearing: Arc<Mutex<bool>>,
    /// When the shown storage was asked for (its grace counts from there).
    storage_at: Arc<Mutex<f64>>,
    #[cfg(not(target_arch = "wasm32"))]
    updater: Option<Updater>,
    /// Signed in to ndexr (account.rs): the desktop app's way to the
    /// person's cell. A browser session is signed in by its server instead.
    #[cfg(not(target_arch = "wasm32"))]
    account: crate::account::Shared,
    /// First-start setup: look, account, Claude (setup.rs).
    setup: crate::setup::Setup,
    /// The walkthroughs and Help's pages (tour.rs).
    walk: crate::tour::Walk,
    /// The atlas (atlas.rs): the stack from the kernel up to the person, what
    /// Cove opens on; the IDE is a step into one of its domains.
    atlas: crate::atlas::Atlas,
    /// Which of Cove's modes fills the window (header mode_tabs). One value,
    /// not a flag each: five flags had to be cleared together at every turn
    /// into the IDE, and Connect would have made six.
    mode: Mode,
    /// Connect's search, the framework chosen to narrow it (a mark's key),
    /// and whose apps: 0 all, 1 yours, 2 shared with you, 3 public.
    connect_find: String,
    connect_fw: Option<String>,
    /// Connections' Visibility filter: a registry level ("public", "login"…).
    connect_level: Option<String>,
    /// Visibility checked from outside (Query::SiteGate per host): what each
    /// answered an anonymous visitor, the hosts still to ask, and how many
    /// are being asked now.
    connect_gates: Arc<Mutex<std::collections::BTreeMap<String, String>>>,
    connect_queue: Vec<String>,
    connect_asking: Arc<std::sync::atomic::AtomicUsize>,
    connect_whose: u8,
    /// Connect's owner filter (a username; "you" for the person's own) and
    /// its sort: 0 last changed, newest first; 1 name.
    connect_owner: Option<String>,
    /// 0 last changed, 1 name, 2 created newest first, 3 created oldest first.
    connect_sort: u8,
    /// The serving path for the atlas's serving column (Query::Serving, an
    /// admin's only): the last answer and when it was asked.
    serving: std::sync::Arc<std::sync::Mutex<Option<crate::api::Serving>>>,
    serving_at: f64,
    /// Go's machines (admin only: a refusal hides them), when last asked,
    /// and the last action's job.
    go_boxes: Arc<Mutex<Option<Result<Vec<crate::api::BoxView>, String>>>>,
    go_boxes_at: f64,
    /// The machine's CPU, memory and disk for an admin's header chip
    /// (Query::HostVitals); refused once, it is not asked again this session.
    vitals: Arc<Mutex<Option<Result<crate::api::Vitals, String>>>>,
    vitals_at: f64,

    /// How tall Builds' top part (machines and their cards) measured last
    /// frame: its panel is sized from it, up to the cap.
    builds_top_h: f32,
    go_box_job: Arc<Mutex<Option<JobView>>>,
    go_confirm: Option<String>,
    /// The machine the last action was for, when it was pressed, when its job
    /// was last asked, and whether its end has been announced (api.rs start_phase).
    go_job_server: Option<String>,
    go_job_since: f64,
    go_job_poll: f64,
    go_job_told: bool,
    /// Go's recap window in minutes (4 h or a day, go_recap_bar).
    go_recap_mins: u32,
    /// Go's Briefing (brief.rs): the last answer, a question out, when to ask
    /// again while it is being made, and the episode last started playing.
    go_brief: Slot<Result<crate::api::RecapBrief, String>>,
    go_brief_busy: Arc<std::sync::atomic::AtomicBool>,
    go_brief_poll: f64,
    go_brief_played: i64,
    /// The front page's slides over the atlas, the same as the public view's.
    showcase: crate::showcase::Showcase,
    /// A newer published Cove, offered at the top (release.rs).
    release: crate::release::ReleaseBar,
    recents: crate::recents::Recents,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, backend: Backend, folder: Option<String>) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut saved: Saved = cc.storage.and_then(|s| eframe::get_value(s, eframe::APP_KEY)).unwrap_or_default();
        if saved.grid.cols.is_empty() {
            saved.grid = Grid::from_corners(&saved.dock.cells, &saved.dock.front, saved.split_x);
        }
        saved.grid.mend();
        // Cove opens on the agent, then Inputs and Files beside it - and not
        // zoomed into another tab a saved layout left full-window.
        saved.grid.agents_first();
        if saved.zoom_tab.is_some_and(|t| t != Tab::Agents) {
            saved.zoom_tab = None;
        }
        crate::setup::follow_theme(saved.theme, &cc.egui_ctx);
        let mut setup = crate::setup::Setup::default();
        // Easy to get into (operator, 2026-10-19: "do not ask questions about
        // setting up claude, you give the person their free commands"): in a
        // browser, signed in already, nothing to answer - a welcome, and the
        // free commands from the first prompt. The desktop still signs in.
        //
        // The first decision is what to build (operator, 2026-10-30: "the most
        // important conversion"), asked by the Agents pane beside the site, so
        // no welcome or tour stands in front of it. Help still has the tour.
        let first_visit = !saved.setup_seen && cfg!(target_arch = "wasm32");
        // In a browser, every visit looks at the server's stack: the question
        // follows the server, not the browser that saw an earlier one (operator,
        // 2026-11-22: a fresh server under the same name never asked).
        let first_start = !saved.setup_seen || cfg!(target_arch = "wasm32");
        if !saved.setup_seen {
            if first_visit {
                saved.setup_seen = true;
                saved.tour.welcomed = true;
            } else {
                setup.start();
            }
        }
        let saved_tour = saved.tour.clone();
        let probe = Slot::default();
        {
            let probe = probe.clone();
            backend.query(Query::Probe, &cc.egui_ctx, move |r: Result<api::Probe, String>| {
                *probe.lock().unwrap() = Some(r.unwrap_or_default());
            });
        }
        let settings = Slot::default();
        {
            let settings = settings.clone();
            backend.query(Query::Settings, &cc.egui_ctx, move |r: Result<SettingsView, String>| {
                *settings.lock().unwrap() = r.ok();
            });
        }
        // The toolbar's environment switch lists these from the start.
        let modules: Slot<Modules> = Slot::default();
        {
            let modules = modules.clone();
            backend.query(Query::Modules, &cc.egui_ctx, move |r: Result<Modules, String>| {
                *modules.lock().unwrap() = Some(r.unwrap_or_else(|e| Modules { source: e, ..Default::default() }));
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        let account = {
            let a = crate::account::Shared::default();
            if let Backend::Cell { base, .. } = &backend {
                a.lock().unwrap().on_cell = Some(base.clone());
            }
            crate::account::check(a.clone(), cc.egui_ctx.clone());
            a
        };
        App {
            #[cfg(not(target_arch = "wasm32"))]
            account,
            ide: {
                let mut ide = Ide::new(backend.clone(), &cc.egui_ctx);
                ide.first_site = first_visit;
                ide.restore = saved.domain.clone();
                ide.agents_layout = saved.agents_layout;
                ide.agents_frac = saved.agents_frac.unwrap_or(0.5);
                // ?agent=all|site in the address wins over what was saved.
                ide.exec_on = url_agent().unwrap_or(saved.agent_all);
                ide.open_by_domain = saved.open_by_domain.clone();
                ide.recent_hosts = saved.recent_domains.clone();
                ide.tabs = saved.tabs.clone();
                ide.live_off = saved.live_off.clone();
                ide.listen_secs = match (saved.listen_secs, saved.listen_minutes) {
                    (0, 0) => 60,
                    (0, m) => crate::voice::piece(m as u32 * 60),
                    (s, _) => crate::voice::piece(s),
                };
                crate::voice::speak::set(saved.speak);
                ide
            },
            #[cfg(not(target_arch = "wasm32"))]
            // The app on this machine updates itself wherever its kernels run.
            updater: (backend.is_local() || matches!(backend, Backend::Cell { .. })).then(|| Updater::start(&cc.egui_ctx)),
            backend,
            saved,
            probe,
            settings,
            options: None,
            options_page: 0,
            modules,
            env_host: None,
            extra_filter: String::new(),
            project_settings: false,
            restart_after_save: Default::default(),
            pinned: Default::default(),
            asked_options: false,
            compact_cell: 0,
            drag: Drag::default(),
            splitting: false,
            solo: solo_tab(),
            was_busy: false,
            ended_at: None,
            backdrop: crate::backdrop::Backdrop::start(&cc.egui_ctx),
            calm: Default::default(),
            request: None,
            request_sent: Default::default(),
            inbox: Default::default(),
            access: Default::default(),
            manage: Default::default(),
            request_drops: Default::default(),
            request_mic: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            cell_try: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            cell_retry_at: 0.0,
            #[cfg(not(target_arch = "wasm32"))]
            was_focused: true,
            #[cfg(not(target_arch = "wasm32"))]
            server_form: None,
            #[cfg(not(target_arch = "wasm32"))]
            server_try: Default::default(),
            folder: folder.clone(),
            #[cfg(not(target_arch = "wasm32"))]
            picked_folder: Default::default(),
            about: false,
            help_open: false,
            mode_asked: false,
            cell_offer: false,
            sessions_open: false,
            stack_job: Default::default(),
            stack_asked: f64::NEG_INFINITY,
            stack_inflight: Default::default(),
            auto_setup: false,
            first_start,
            proxy_test: Slot::default(),
            testing: false,
            notes: Default::default(),
            toast: None,
            ndexr_due: false,
            source_ready: Default::default(),
            claim_ready: Default::default(),
            storage: Slot::default(),
            storage_asked: f64::NEG_INFINITY,
            storage_open: false,
            storage_clearing: Arc::default(),
            storage_at: Arc::default(),
            walk: crate::tour::Walk { prefs: saved_tour, ..Default::default() },
            setup,
            atlas: Default::default(),
            serving: Default::default(),
            serving_at: f64::NEG_INFINITY,
            // Cove always opens in the IDE (operator, 2026-10-24: "cove
            // should always start in IDE mode", replacing 2026-10-23's atlas
            // start): a first visit, a ?folder= link and a popped-out ?tab=
            // page alike. The atlas is a tap away in the mode bar.
            mode: Mode::Ide,
            connect_find: String::new(),
            connect_fw: None,
            connect_level: None,
            connect_gates: Default::default(),
            connect_queue: Vec::new(),
            connect_asking: Default::default(),
            connect_whose: 0,
            connect_owner: None,
            connect_sort: 0,
            go_boxes: Default::default(),
            go_boxes_at: f64::NEG_INFINITY,
            vitals: Default::default(),
            vitals_at: f64::NEG_INFINITY,
            builds_top_h: 240.0,
            go_box_job: Default::default(),
            go_confirm: None,
            go_job_server: None,
            go_job_since: 0.0,
            go_job_poll: f64::NEG_INFINITY,
            go_job_told: false,
            go_recap_mins: 240,
            go_brief: Default::default(),
            go_brief_busy: Default::default(),
            go_brief_poll: f64::INFINITY,
            go_brief_played: 0,
            showcase: Default::default(),
            release: Default::default(),
            recents: Default::default(),
        }
    }

    /// Something main() wants said once the window is up.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn startup_note(&mut self, msg: String, ctx: &egui::Context) {
        self.account.lock().unwrap().cell_error = Some(msg.clone());
        // The launch wanted the cell and fell back here: keep trying it.
        self.cell_retry_at = ctx.input(|i| i.time) + 30.0;
        self.toast = Some((msg, ctx.input(|i| i.time) + 10.0));
    }

    /// Run somewhere else from the next launch, and launch now.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_on(&mut self, r: crate::account::RunOn, ctx: &egui::Context) {
        crate::account::prefer(r);
        self.relaunch(ctx);
    }

    /// Try the cell from this window, in the background. The backend is fixed
    /// for a window's life, so success relaunches onto the cell - but only
    /// once it has answered: a blind relaunch spent up to 90 s with no window
    /// and came back to "Not on your cell" with nothing changed (2026-10-15).
    #[cfg(not(target_arch = "wasm32"))]
    fn try_cell(&mut self, ctx: &egui::Context) {
        let mut t = self.cell_try.lock().unwrap();
        if t.0 {
            return;
        }
        *t = (true, None);
        drop(t);
        self.cell_retry_at = ctx.input(|i| i.time) + 60.0;
        let (slot, ctx) = (self.cell_try.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = crate::account::connect_cell().map(|_| ());
            *slot.lock().unwrap() = (false, Some(r));
            ctx.request_repaint();
        });
    }

    /// Signed in, on this computer: their ndexr apps are on their cell. Go
    /// there (this window starts again on it), or stay here - remembered.
    #[cfg(not(target_arch = "wasm32"))]
    fn cell_offer_modal(&mut self, ctx: &egui::Context) {
        if !self.cell_offer {
            return;
        }
        let a = self.account.lock().unwrap().clone();
        let (Some(email), Some(cell)) = (a.email.clone(), a.cell.clone()) else {
            self.cell_offer = false;
            return;
        };
        let mut pick: Option<crate::account::RunOn> = None;
        egui::Modal::new(egui::Id::new("cell-offer")).show(ctx, |ui| {
            ui.set_width(460.0f32.min(ctx.content_rect().width() - 40.0));
            ui.label(RichText::new(format!("Signed in as {email}")).size(18.0).strong().color(theme::text_strong()));
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new("Your projects and apps on ndexr are on your cell. This window works on this computer - its own folders and its own stack - so they are not listed here.").size(13.5).color(theme::text())).wrap());
            ui.add_space(4.0);
            ui.label(RichText::new(cell.trim_start_matches("https://")).monospace().size(12.0).color(theme::text_strong()));
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                if ui.add(theme::primary_button("Open my cell's projects")).on_hover_text("this window starts again on your cell: your ndexr projects, apps and agents").clicked() {
                    pick = Some(crate::account::RunOn::Cell);
                }
                if ui.button("Stay on this computer").clicked() {
                    pick = Some(crate::account::RunOn::Local);
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new("The chip at the left of the status bar shows which you are on, and switches.").size(12.0).color(theme::text()));
        });
        if let Some(r) = pick {
            self.cell_offer = false;
            if r == crate::account::RunOn::Cell {
                self.run_on(r, ctx);
            } else {
                crate::account::prefer(r);
            }
        }
    }

    /// The Cove server this window drives, when it is one: a managed server or
    /// the person's company's (account.rs `saved_server`), or `--connect`.
    #[cfg(not(target_arch = "wasm32"))]
    fn on_server(&self) -> Option<String> {
        match &self.backend {
            Backend::Http { base } => Some(base.clone()),
            _ => None,
        }
    }

    /// Connect to a server: its address and token, checked before anything is
    /// kept; a server that answers is saved and this window starts again on it.
    /// Needs no ndexr account (operator, 2026-11-22: a company runs its own).
    #[cfg(not(target_arch = "wasm32"))]
    fn server_modal(&mut self, ctx: &egui::Context) {
        let Some((mut url, mut token)) = self.server_form.clone() else { return };
        let (trying, answer) = {
            let mut t = self.server_try.lock().unwrap();
            (t.0, t.1.take())
        };
        let mut close = false;
        let mut error = None;
        match answer {
            Some(Ok(())) => match crate::account::server_base(&url).and_then(|b| crate::account::save_server(&b, &token)) {
                Ok(()) => {
                    self.server_form = None;
                    self.relaunch(ctx);
                    return;
                }
                Err(e) => error = Some(e),
            },
            Some(Err(e)) => error = Some(e),
            None => {}
        }
        if let Some(e) = error {
            self.toast = Some((e, ctx.input(|i| i.time) + 10.0));
        }
        egui::Modal::new(egui::Id::new("server-connect")).show(ctx, |ui| {
            ui.set_width(480.0f32.min(ctx.content_rect().width() - 40.0));
            ui.label(RichText::new("Connect to a server").size(18.0).strong().color(theme::text_strong()));
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new("A Cove server ndexr runs for you, or one your company runs. Its address and token are on billing.ndexr.io › Servers, or with whoever runs it (the token is ~/.config/ndexr/cove-token on the server).").size(13.5).color(theme::text())).wrap());
            ui.add_space(8.0);
            egui::Grid::new("server-connect-grid").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Address");
                ui.add(egui::TextEdit::singleline(&mut url).hint_text("cove.example.com").desired_width(320.0));
                ui.end_row();
                ui.label("Token");
                ui.add(egui::TextEdit::singleline(&mut token).password(true).font(egui::TextStyle::Monospace).desired_width(320.0));
                ui.end_row();
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let ready = !trying && !url.trim().is_empty() && !token.trim().is_empty();
                let word = if trying { "Checking…" } else { "Connect" };
                if ui.add_enabled(ready, theme::primary_button(word)).clicked() {
                    match crate::account::server_base(&url) {
                        Ok(base) => {
                            *self.server_try.lock().unwrap() = (true, None);
                            let (slot, ctx2, t) = (self.server_try.clone(), ctx.clone(), token.clone());
                            std::thread::spawn(move || {
                                let r = crate::account::probe_server(&base, &t);
                                *slot.lock().unwrap() = (false, Some(r));
                                ctx2.request_repaint();
                            });
                        }
                        Err(e) => self.toast = Some((e, ctx.input(|i| i.time) + 6.0)),
                    }
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
        self.server_form = if close { None } else { Some((url, token)) };
    }

    /// Where this window works, always on screen (operator, 2026-11-15: "it
    /// should be obvious somewhere if you are in local or remote connection
    /// mode"): this computer, or the person's cell on ndexr; its menu says
    /// what that means and switches.
    #[cfg(not(target_arch = "wasm32"))]
    fn connection_chip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let a = self.account.lock().unwrap().clone();
        let on_cell = a.on_cell.is_some();
        if let Some(base) = self.on_server().filter(|_| !on_cell) {
            let host = base.trim_start_matches("https://").trim_start_matches("http://").to_owned();
            let r = ui.scope(|ui| theme::state_mark(ui, theme::State::Good, &format!("Server · {host}"))).inner.interact(egui::Sense::click());
            let r = r.on_hover_text("working on a Cove server: its folders, its stack, its apps - click to switch");
            egui::Popup::menu(&r).show(|ui| {
                theme::chrome_menu(ui, 300.0);
                ui.label(RichText::new(format!("On the server {host}")).size(12.0).color(theme::text_strong()));
                if ui.button("Connect to a different server…").clicked() {
                    self.server_form = Some((String::new(), String::new()));
                    ui.close();
                }
                if ui.button("Work on this computer instead").clicked() {
                    crate::account::forget_server();
                    self.relaunch(ctx);
                    ui.close();
                }
            });
            return;
        }
        let (st, word) = if on_cell { (theme::State::Good, "Your cell · ndexr") } else { (theme::State::Idle, "This computer") };
        let r = ui.scope(|ui| theme::state_mark(ui, st, word)).inner.interact(egui::Sense::click());
        let tip = if on_cell {
            "working on your ndexr cell: your projects, apps and agents there".to_owned()
        } else if a.email.is_some() {
            "working on this computer: its folders, its stack, its apps at localhost. Your ndexr projects are on your cell - click to go there".to_owned()
        } else {
            "working on this computer, with no ndexr connection: its folders, its stack, its apps at localhost".to_owned()
        };
        let r = r.on_hover_text(tip);
        egui::Popup::menu(&r).show(|ui| {
            theme::chrome_menu(ui, 300.0);
            if on_cell {
                ui.label(RichText::new(format!("On your cell: {}", a.on_cell.clone().unwrap_or_default().trim_start_matches("https://"))).size(12.0).color(theme::text_strong()));
                if ui.button("Work on this computer instead").clicked() {
                    self.run_on(crate::account::RunOn::Local, ctx);
                    ui.close();
                }
            } else {
                ui.label(RichText::new("On this computer: its folders and stack; apps run at localhost").size(12.0).color(theme::text_strong()));
                match (&a.email, &a.cell) {
                    (Some(_), Some(_)) => {
                        if ui.button("Open my cell's projects (ndexr)").clicked() {
                            self.run_on(crate::account::RunOn::Cell, ctx);
                            ui.close();
                        }
                    }
                    (Some(_), None) => {
                        ui.label(RichText::new("Your ndexr account has no cell yet.").size(12.0).color(theme::text()));
                    }
                    (None, _) => {
                        if ui.button("Sign in to reach your ndexr projects").clicked() {
                            match crate::account::sign_in(self.account.clone(), ctx.clone()) {
                                Ok(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                                Err(e) => self.toast(e, ctx),
                            }
                            ui.close();
                        }
                    }
                }
                // A managed server, or the company's own: no ndexr account needed.
                if ui.button("Connect to a server…").on_hover_text("a Cove server ndexr runs for you, or one your company runs: its address and token").clicked() {
                    let saved = crate::account::saved_server().map(|(u, _)| u).unwrap_or_default();
                    self.server_form = Some((saved, String::new()));
                    ui.close();
                }
            }
        });
    }

    /// Start again (account.rs `choose` and `connect_cell` run at launch only).
    #[cfg(not(target_arch = "wasm32"))]
    fn relaunch(&mut self, ctx: &egui::Context) {
        match crate::account::relaunch() {
            Ok(()) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Err(e) => self.toast(e, ctx),
        }
    }

    fn toast(&mut self, msg: impl Into<String>, ctx: &egui::Context) {
        self.toast = Some((msg.into(), ctx.input(|i| i.time) + 4.0));
    }

    /// A domain stepped into: the environment its `.ndexr-env` names (the
    /// domain's own, whoever opens it - RStudio's project R version), else the
    /// one it was last worked in here (Positron's affiliated runtime), put
    /// back if another is in effect (R, Python and the shell restart in it);
    /// a domain with neither keeps the one in effect, which becomes its.
    fn follow_domain_env(&mut self, ctx: &egui::Context) {
        let Some((host, pin, by_language, path)) = self.ide.domain.as_ref().map(|d| (d.host.clone(), d.env.clone(), d.env_default.clone(), d.path.clone())) else { return };
        if self.env_host.as_deref() == Some(host.as_str()) || self.settings.lock().unwrap().is_none() {
            return;
        }
        self.env_host = Some(host.clone());
        let before = self.saved_settings();
        // Its .ndexr-env, else the environment of its language - env/R for an
        // R project, env/Python, env/Julia, env/Rust, nodejs - else the
        // person's default (operator, 2026-10-26: "make sure the environment is
        // selected at the beginning automatically for the user so that we can
        // prevent having an R, Py, Julia, etc shell"). What was last used for
        // the project is not a source: that is how every project inherited the
        // combined R and Python one. A choice is pinned (☰ › Project › Settings).
        let pinned = !pin.is_empty();
        let module = if pinned { pin } else if !by_language.is_empty() { by_language } else { before.module.clone() };
        // A new project is a new environment (operator, 2026-10-18: "changing
        // projects should create new environments for the console and clear
        // the other ones"): the domain becomes the project, so R (Python
        // inside it), the shell and the Terminal start again there, fresh, in
        // the domain's environment - one save, one restart (save_options).
        if path.starts_with('/') && (before.project_dir.trim_end_matches('/') != path || module != before.module) {
            let shown = if module.is_empty() { "the programs on PATH".to_owned() } else { module.clone() };
            let why = if pinned { " (its .ndexr-env)" } else { "" };
            let consoles = crate::ide::env_consoles(&module).iter().map(|l| l.word()).collect::<Vec<_>>().join(", ");
            self.save_options(Settings { module, project_dir: path, ..before }, ctx);
            self.toast(format!("{host}: {consoles} in {shown}{why}"), ctx);
        }
    }

    fn saved_settings(&self) -> Settings {
        self.settings.lock().unwrap().as_ref().map(|v| v.settings.clone()).unwrap_or_default()
    }

    fn open_options(&mut self, ctx: &egui::Context) {
        self.options = Some(self.saved_settings());
        *self.proxy_test.lock().unwrap() = None;
        self.refresh_settings(ctx);
        let modules = self.modules.clone();
        self.backend.query(Query::Modules, ctx, move |r: Result<Modules, String>| {
            *modules.lock().unwrap() = Some(r.unwrap_or_else(|e| Modules { source: e, ..Default::default() }));
        });
    }

    /// The stack's modules, when they have not arrived yet.
    fn ask_modules(&self, ctx: &egui::Context) {
        if self.modules.lock().unwrap().is_some() {
            return;
        }
        let modules = self.modules.clone();
        self.backend.query(Query::Modules, ctx, move |r: Result<Modules, String>| {
            *modules.lock().unwrap() = Some(r.unwrap_or_else(|e| Modules { source: e, ..Default::default() }));
        });
    }

    /// ☰ › Project › Settings…: the environment this project runs in, and
    /// the modules layered on it - out of the toolbar (operator, 2026-10-26).
    /// A pick here is the project's: R, Python and the shell restart in it,
    /// and it is written to the project's .ndexr-env so it opens in it for
    /// everyone, in Cove, Inlet and Positron (what the toolbar's Pin did).
    fn project_settings_dialog(&mut self, ctx: &egui::Context) {
        if let Some((host, env)) = self.pinned.lock().unwrap().take() {
            self.ide.set_domain_env(&host, &env);
        }
        if !self.project_settings {
            return;
        }
        let Some(d) = self.ide.domain.clone() else {
            self.project_settings = false;
            return;
        };
        let before = self.saved_settings();
        let mut pick = before.module.clone();
        let modules = self.modules.lock().unwrap().clone();
        let w = (ctx.content_rect().width() - 32.0).clamp(280.0, 520.0);
        let mut close = false;
        let r = egui::Modal::new(egui::Id::new("project-settings")).show(ctx, |ui| {
            ui.set_width(w);
            ui.label(RichText::new(format!("{} settings", d.host)).size(20.0).strong().color(theme::text_strong()));
            ui.add_space(10.0);
            ui.label(RichText::new("Environment").size(13.0).strong().color(theme::text_strong()));
            ui.horizontal(|ui| {
                module_menu(ui, "project-module", &mut pick, &modules, (w - 140.0).max(160.0));
                extra_menu(ui, &mut pick, &modules, &mut self.extra_filter);
            });
            let note = if d.env.is_empty() {
                "Not saved to the project yet: pick one and it opens in it for everyone.".to_owned()
            } else {
                format!("Saved to {}/.ndexr-env: it opens in {} for everyone.", d.host, d.env)
            };
            ui.label(RichText::new(note).size(12.5).color(theme::text()));
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                close = ui.add(theme::primary_button("Done")).clicked();
            });
        });
        if pick != before.module && self.settings.lock().unwrap().is_some() {
            let shown = if pick.is_empty() { "the programs on PATH".to_owned() } else { pick.clone() };
            let consoles = crate::ide::env_consoles(&pick).iter().map(|l| l.word()).collect::<Vec<_>>().join(", ");
            self.env_host = Some(d.host.clone());
            let (slot, notes, host) = (self.pinned.clone(), self.notes.clone(), d.host.clone());
            self.backend.query(Query::PinEnv { dir: d.path.clone(), modules: pick.clone() }, ctx, move |r: Result<String, String>| match r {
                Ok(env) => *slot.lock().unwrap() = Some((host, env)),
                Err(e) => notes.lock().unwrap().push(format!("not saved to the project: {e}")),
            });
            self.save_options(Settings { module: pick, ..before }, ctx);
            self.toast(format!("{consoles} restarting in {shown}"), ctx);
        }
        if close || r.should_close() {
            self.project_settings = false;
        }
    }

    /// Ask the machine again: the stack's state moves under the app (a
    /// setup finishing, a publish).
    fn refresh_settings(&self, ctx: &egui::Context) {
        let settings = self.settings.clone();
        self.backend.query(Query::Settings, ctx, move |r: Result<SettingsView, String>| {
            if let Ok(v) = r {
                *settings.lock().unwrap() = Some(v);
            }
        });
    }

    /// Save Global Options on the machine; once that lands, restart each
    /// kernel whose module or project changed, so it starts in the new one,
    /// and hand changed CVMFS settings to this machine's client.
    fn save_options(&mut self, s: Settings, ctx: &egui::Context) {
        let before = self.saved_settings();
        let restart: Vec<Lang> = Lang::CONSOLES.into_iter().filter(|&l| s.module(l) != before.module(l) || s.project_dir != before.project_dir || s.stack_dir != before.stack_dir).collect();
        let on_client = self.settings.lock().unwrap().as_ref().map(|v| (v.stack.local.clone(), v.stack.client));
        let apply = match on_client {
            Some((local, true)) if local != s.cvmfs => Some(s.cvmfs.clone()),
            _ => None,
        };
        let (settings, notes, after) = (self.settings.clone(), self.notes.clone(), self.restart_after_save.clone());
        let (backend, ctx2, job) = (self.backend.clone(), ctx.clone(), self.stack_job.clone());
        self.backend.query(Query::SaveSettings { settings: s }, ctx, move |r: Result<SettingsView, String>| match r {
            Ok(v) => {
                *settings.lock().unwrap() = Some(v);
                after.lock().unwrap().extend(restart);
                if let Some(cvmfs) = apply {
                    start_stack_job(&backend, &ctx2, Query::StackApply { cvmfs }, job, notes);
                }
            }
            Err(e) => notes.lock().unwrap().push(format!("not saved: {e}")),
        });
    }

    /// Set the stack up on this machine (`ndexr install cvmfs`), in the
    /// background.
    fn setup_stack(&mut self, ctx: &egui::Context) {
        self.auto_setup = true;
        start_stack_job(&self.backend, ctx, Query::StackSetup, self.stack_job.clone(), self.notes.clone());
        self.stack_asked = f64::NEG_INFINITY;
    }

    /// While a stack job runs: its progress every second; when it ends, the
    /// machine is asked again (the stack is mounted now, or why not).
    fn poll_stack(&mut self, ctx: &egui::Context) {
        let running = self.stack_job.lock().unwrap().running;
        let now = ctx.input(|i| i.time);
        if !running || *self.stack_inflight.lock().unwrap() || now - self.stack_asked < 1.0 {
            return;
        }
        self.stack_asked = now;
        *self.stack_inflight.lock().unwrap() = true;
        let (job, inflight, settings, backend, ctx2) = (self.stack_job.clone(), self.stack_inflight.clone(), self.settings.clone(), self.backend.clone(), ctx.clone());
        self.backend.query(Query::StackJob, ctx, move |r: Result<JobView, String>| {
            if let Ok(v) = r {
                let ended = !v.running;
                *job.lock().unwrap() = v;
                if ended {
                    backend.query(Query::Settings, &ctx2, move |r: Result<SettingsView, String>| {
                        if let Ok(v) = r {
                            *settings.lock().unwrap() = Some(v);
                        }
                    });
                }
            }
            *inflight.lock().unwrap() = false;
        });
        ctx.request_repaint_after(Duration::from_secs(1));
    }

    /// The stack is missing here: set it up unasked where that needs no
    /// prompt (operator, 2026-10-13: "cvmfs setup will be in the background
    /// for them"); where it needs one, the strip below the toolbar offers it.
    ///
    /// A first start never does (operator, 2026-11-13: the desktop and the
    /// server "should go through inital setup of installing the stack
    /// themselves or consuming it through cvmfs"): setup's Stack step asks.
    /// Nor does airgap mode, or a machine already building its own.
    /// Asked once, on this computer (api.rs Settings::report): may a failed
    /// stack build or setup send an anonymous report home? Not while setup or
    /// a walkthrough is on screen, and never in airgap mode, where nothing is
    /// sent. The answer is saved with the settings; Global Options changes it.
    #[cfg(not(target_arch = "wasm32"))]
    fn report_consent(&mut self, ctx: &egui::Context) {
        let Some(v) = self.settings.lock().unwrap().clone() else { return };
        if v.settings.report.is_some() || v.settings.airgap || self.setup.open || self.walk.run.is_some() || self.walk.welcome_due() {
            return;
        }
        let mut answer: Option<bool> = None;
        egui::Modal::new(egui::Id::new("report-consent")).show(ctx, |ui| {
            ui.set_width(460.0f32.min(ctx.content_rect().width() - 40.0));
            ui.label(RichText::new("Help improve the ndexr stack?").size(18.0).strong().color(theme::text_strong()));
            ui.add_space(6.0);
            ui.add(egui::Label::new(RichText::new("When building or setting up the stack fails on this computer, Cove can send an anonymous report so we can fix it. It arrives as a request, and the fix ships in a release.").size(13.5).color(theme::text())).wrap());
            ui.add_space(6.0);
            for line in [
                "Sent: the step that failed, its exit code and error lines, and this computer's shape - OS, architecture, kernel, glibc, cores, memory",
                "Never sent: your name, email, user or host name, paths, IP address or files - they are removed from the error lines first",
                "Only failures, each once, under a random id kept on this computer",
            ] {
                ui.add(egui::Label::new(RichText::new(format!("· {line}")).size(12.5).color(theme::text_strong())).wrap());
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.add(theme::primary_button("Send anonymous reports")).clicked() {
                    answer = Some(true);
                }
                if ui.button("No thanks").clicked() {
                    answer = Some(false);
                }
            });
            ui.add_space(4.0);
            ui.label(RichText::new("Change it any time in Tools › Global Options.").size(12.0).color(theme::text()));
        });
        if let Some(yes) = answer {
            let mut s = v.settings.clone();
            s.report = Some(yes);
            // Shown as answered at once, so the dialog does not come back
            // while the save is on its way.
            if let Some(view) = self.settings.lock().unwrap().as_mut() {
                view.settings.report = Some(yes);
            }
            let slot = self.settings.clone();
            self.backend.query(Query::SaveSettings { settings: s }, ctx, move |r: Result<SettingsView, String>| {
                if let Ok(v) = r {
                    *slot.lock().unwrap() = Some(v);
                }
            });
        }
    }

    fn auto_setup(&mut self, ctx: &egui::Context) {
        // Where the stack comes from is asked at every start until it is
        // chosen, whatever else was skipped (operator, 2026-11-14: "i
        // installed the tool locally and bypassed setup ... the user is still
        // not asked").
        #[cfg(not(target_arch = "wasm32"))]
        if !self.mode_asked && crate::account::stack_mode().is_none() && !self.setup.open {
            self.mode_asked = true;
            // A stack already here answers the question (operator, 2026-11-14:
            // "if it has been set up locally it should auto detect"): the
            // CVMFS mount is CernVM-FS, a folder built or copied here is
            // Installed (host.rs stack_root found it). Asked only when none is.
            match crate::host::use_sh().map(|_| crate::host::stack_root()) {
                Some(root) => crate::account::set_stack_mode(if root == std::path::Path::new("/opt/ndexr/stack") { crate::api::StackMode::Cvmfs } else { crate::api::StackMode::Installed }),
                None => self.setup.start_stack(),
            }
        }
        if self.first_start {
            let missing = self.settings.lock().unwrap().as_ref().map(|v| stack_missing(&v.stack) && !v.settings.airgap);
            if let Some(missing) = missing {
                self.first_start = false;
                self.auto_setup = true;
                if missing && cfg!(target_arch = "wasm32") && !self.setup.open {
                    self.setup.start_stack();
                }
            }
            return;
        }
        if self.auto_setup {
            return;
        }
        let quiet = self.settings.lock().unwrap().as_ref().is_some_and(|v| stack_missing(&v.stack) && !v.settings.airgap && !v.stack.building && matches!(v.stack.root_by.as_str(), "root" | "sudo"));
        // Only for CernVM-FS chosen: an installed stack, or the remote one, is
        // someone's choice not to mount it (on the desktop, where it is asked).
        #[cfg(not(target_arch = "wasm32"))]
        let quiet = quiet && crate::account::stack_mode() == Some(crate::api::StackMode::Cvmfs);
        if quiet {
            self.setup_stack(ctx);
        }
    }

    /// Fill the screen with `t`'s window, or give the screen back.
    fn zoom(&mut self, t: Tab) {
        self.ide.bring(t);
        self.saved.zoom_tab = if self.saved.zoom_tab.is_some_and(|z| self.saved.grid.find(z) == self.saved.grid.find(t)) { None } else { Some(t) };
    }

    /// File › Open Folder: the platform's own folder dialog, off the frame
    /// (it blocks), its answer handed to `folder` on the next frame.
    #[cfg(not(target_arch = "wasm32"))]
    fn pick_folder(&self, ctx: &egui::Context) {
        let (slot, ctx) = (self.picked_folder.clone(), ctx.clone());
        let start = crate::host::project_dir(&self.settings.lock().unwrap().as_ref().map(|v| v.settings.clone()).unwrap_or_default());
        std::thread::spawn(move || {
            let pick = rfd::AsyncFileDialog::new().set_title("Open a folder as the project").set_directory(&start).pick_folder();
            if let Some(dir) = pollster::block_on(pick) {
                *slot.lock().unwrap() = Some(dir.path().display().to_string());
                ctx.request_repaint();
            }
        });
    }

    // ── keys RStudio users already have in their hands ─────────────────────
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let hit = |k: Key| ctx.input_mut(|i| i.consume_shortcut(&KeyboardShortcut::new(cs, k)));
        for (k, t) in [(Key::Num1, Tab::Files), (Key::Num2, Tab::Console), (Key::Num3, Tab::App), (Key::Num4, Tab::App)] {
            if hit(k) {
                self.zoom(t);
            }
        }
        if hit(Key::Num0) {
            self.saved.zoom_tab = None;
        }
        if hit(Key::N) {
            self.ide.new_script(Lang::R);
        }
        if hit(Key::F10) {
            self.ide.restart(self.ide.lang, ctx);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if self.backend.is_local() && hit(Key::O) {
            self.pick_folder(ctx);
        }
        // Full screen (operator, 2026-10-14: "cove as an app should be able to
        // go full screen on desktop or site"): F11 everywhere, and macOS's own
        // Ctrl+Cmd+F. Taken before the terminal sees F11.
        let mac = Modifiers::CTRL | Modifiers::MAC_CMD;
        if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::F11) || i.consume_shortcut(&KeyboardShortcut::new(mac, Key::F))) {
            if let Err(why) = toggle_fullscreen(ctx) {
                self.toast(why, ctx);
            }
        }
    }

    /// The Cove menu, behind the three dots at the top right (header).
    fn menu_items(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let small = compact(ctx);
        // A phone has no keyboard: its shortcuts are noise there.
        let item = |ui: &mut egui::Ui, label: &str, keys: &str| ui.add(egui::Button::new(label).shortcut_text(if small { "" } else { keys })).clicked();
        let lang = self.ide.lang;
        // What can I do, in a short list (the operator's review, 2026-11-14;
        // 280-340 px). Project, View and Console are sections that open
        // beside it on hover, as an IDE's menus do (2026-11-18: "hover over
        // options shows highlight and auto open the menu like normal menubar
        // ide"); the flat list had run past a phone's screen, Restart Shell
        // out of reach. Help is a panel; the other sessions a window.
        crate::theme::chrome_menu(ui, 300.0);
        if small {
            ui.spacing_mut().interact_size.y = 40.0;
        }
        let room = (ctx.content_rect().height() - 90.0).max(240.0);
        egui::ScrollArea::vertical().max_height(room).auto_shrink([false, true]).show(ui, |ui| {
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::LEFT), |ui| {
                if item(ui, "New tab", "") {
                    self.ide.new_tab();
                    self.mode = Mode::Ide;
                    ui.close();
                }
                // Only when there is one to reopen: a greyed row is noise.
                if let Some(h) = self.ide.tabs.closed.last().map(|h| crate::tabs::Tabs::short(h).to_owned()) {
                    if item(ui, &format!("Reopen closed tab · {h}"), "") {
                        self.ide.reopen_tab(ctx);
                        self.mode = Mode::Ide;
                        ui.close();
                    }
                }
                if item(ui, "Other sessions…", "") {
                    self.sessions_open = true;
                    ui.close();
                }
                // This computer's Cove in a browser (localserve.rs): server mode
                // without a server of anyone else's.
                #[cfg(unix)]
                if self.backend.is_local() {
                    if item(ui, "Open in a browser", "") {
                        match crate::localserve::open() {
                            Ok(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                            Err(e) => self.toast(e, ctx),
                        }
                        ui.close();
                    }
                    if crate::localserve::running() && item(ui, "Stop the browser's Cove", "") {
                        crate::localserve::stop();
                        ui.close();
                    }
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Any folder on this computer as the project - no domain
                    // needed (operator, 2026-10-19). Only where the consoles run
                    // on this computer; on a cell a local path means nothing.
                    let local = self.backend.is_local();
                    let open = ui
                        .add_enabled(local, egui::Button::new("Open folder…").shortcut_text(if small { "" } else { "Ctrl+Shift+O" }))
                        .on_disabled_hover_text("This window runs on your cell: open a folder there, in Files.");
                    if open.clicked() {
                        self.pick_folder(ctx);
                        ui.close();
                    }
                }
                // The open project, a section beside the menu.
                if let Some(host) = self.ide.domain.as_ref().map(|d| d.host.clone()) {
                    crate::theme::menu_sep(ui);
                    let short = crate::tabs::Tabs::short(&host).to_owned();
                    crate::theme::menu_fold(ui, "Project", &short, false, |ui| {
                        if item(ui, "Project settings…", "") {
                            self.project_settings = true;
                            self.ask_modules(ctx);
                            ui.close();
                        }
                        if item(ui, "History (Git)…", "") {
                            self.ide.open_versions(ctx);
                            ui.close();
                        }
                        if item(ui, "Save file", "Ctrl+S") {
                            self.ide.save_current(ctx);
                            ui.close();
                        }
                        if item(ui, "Close file", "") {
                            self.ide.close_current();
                            ui.close();
                        }
                        // Archive and Delete last, apart, in the colour of what destroys.
                        crate::theme::menu_sep(ui);
                        if ui.add(egui::Button::new(RichText::new(format!("Archive {short}…")).color(theme::State::Failed.color()))).clicked() {
                            self.ide.ask_remove(&host, false);
                            ui.close();
                        }
                        if ui.add(egui::Button::new(RichText::new(format!("Delete {short}…")).color(theme::State::Failed.color()))).clicked() {
                            self.ide.ask_remove(&host, true);
                            ui.close();
                        }
                    });
                }
                crate::theme::menu_sep(ui);
                let theme_now = self.saved.theme;
                let z = ctx.zoom_factor();
                let view_state = if (z - 1.0).abs() > 0.01 { format!("{} · {:.0}%", theme_now.label(), z * 100.0) } else { theme_now.label().to_owned() };
                crate::theme::menu_fold(ui, "View", &view_state, false, |ui| {
                    // Zoom: its size is the label (a click puts it back to
                    // 100%), − and + beside it - one row, not four controls
                    // pushed against the edge.
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let h = if small { 38.0 } else { 30.0 };
                        let side = egui::vec2(h + 4.0, h);
                        let w = (ui.available_width() - 2.0 * side.x - 8.0).max(80.0);
                        if ui.add(egui::Button::new(format!("Zoom {:.0}%", z * 100.0)).min_size(egui::vec2(w, h))).on_hover_text("back to 100%").clicked() {
                            ctx.set_zoom_factor(1.0);
                        }
                        if ui.add(egui::Button::new("−").min_size(side)).on_hover_text("smaller (Ctrl+-)").clicked() {
                            ctx.set_zoom_factor((z - 0.1).max(0.5));
                        }
                        if ui.add(egui::Button::new("+").min_size(side)).on_hover_text("larger (Ctrl+=)").clicked() {
                            ctx.set_zoom_factor((z + 0.1).min(3.0));
                        }
                    });
                    if item(ui, if is_fullscreen(ctx) { "Exit full screen" } else { "Full screen" }, "F11") {
                        if let Err(why) = toggle_fullscreen(ctx) {
                            self.toast(why, ctx);
                        }
                        ui.close();
                    }
                    // Theme: each in its own colours, the one in use ticked. A
                    // pick applies at once and the menu stays, to compare.
                    crate::theme::menu_fold(ui, "Theme", theme_now.label(), true, |ui| {
                        for p in crate::setup::ThemePref::ALL {
                            let pl = &theme::PALETTES[p.palette(ctx)];
                            if theme::theme_row(ui, p.label(), pl, theme_now == p, small) {
                                self.saved.theme = p;
                                crate::setup::follow_theme(p, ctx);
                            }
                        }
                    });
                    // Drag a tab to a corner for a window there; this puts them back.
                    if item(ui, "Reset window layout", "") {
                        self.saved.grid = Grid::default();
                        self.saved.zoom_tab = None;
                        ui.close();
                    }
                });
                crate::theme::menu_sep(ui);
                // What RStudio called Session: the consoles - the one in front.
                let busy = self.ide.lang_busy(lang);
                let word = lang.word();
                let console_state = if busy { format!("{word} · busy") } else { word.to_owned() };
                crate::theme::menu_fold(ui, "Console", &console_state, false, |ui| {
                    if busy && item(ui, &format!("Interrupt {word}"), "Esc") {
                        self.ide.interrupt(lang, ctx);
                        ui.close();
                    }
                    if item(ui, &format!("Restart {word}"), if lang == Lang::R { "Ctrl+Shift+F10" } else { "" }) {
                        self.ide.restart(lang, ctx);
                        ui.close();
                    }
                    if lang != Lang::Shell && item(ui, "Restart Shell", "") {
                        self.ide.restart(Lang::Shell, ctx);
                        ui.close();
                    }
                    if lang != Lang::Shell && item(ui, &format!("Clear workspace ({word})"), "") {
                        self.ide.clear_workspace(ctx);
                        ui.close();
                    }
                });
                crate::theme::menu_sep(ui);
                if item(ui, "Settings…", "") {
                    self.open_options(ctx);
                    ui.close();
                }
                if item(ui, "Help…", "F1") {
                    self.help_open = true;
                    ui.close();
                }
            });
        });
    }

    /// Help, docked at the right (the operator's review, 2026-11-14: "Help:
    /// searchable panel"): Show me how, the request, the pages, updates and
    /// About - beside the work, not over it. F1 or ? opens it.
    fn help_panel(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        if ctx.input(|i| i.key_pressed(egui::Key::F1)) {
            self.help_open = !self.help_open;
        }
        if !self.help_open {
            return;
        }
        let item = |ui: &mut egui::Ui, label: &str| ui.add(egui::Button::new(label).min_size(egui::vec2(0.0, 30.0))).clicked();
        egui::Panel::right("rs-help").resizable(false).exact_size(320.0).frame(egui::Frame::new().fill(theme::bg()).inner_margin(egui::Margin::same(12))).show(root, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Help").size(16.0).strong().color(theme::text_strong()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new("✕").min_size(egui::vec2(32.0, 32.0))).on_hover_text("close help (Esc, F1)").clicked() {
                        self.help_open = false;
                    }
                });
            });
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.help_open = false;
            }
            ui.add_space(6.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                self.help_items(ui);
                ui.separator();
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(u) = &self.updater {
                    if item(ui, "Check for updates") {
                        u.check_now(ctx);
                        self.release.check_now(ctx);
                    }
                }
                if crate::release::ReleaseBar::applies(&self.backend) {
                    if item(ui, "Check for updates") {
                        self.release.check_now(ctx);
                    }
                    let mut auto = self.release.auto();
                    if ui.checkbox(&mut auto, "Update without asking").on_hover_text("a newer Cove restarts this session's R, Python and terminal as soon as it is ready; your files, settings and agents stay").changed() {
                        self.release.set_auto(auto);
                    }
                }
                if item(ui, "Set up Cove…") {
                    self.setup.start();
                }
                if item(ui, "Connect Claude…") {
                    self.setup.start_claude(ctx);
                }
                // The install instructions for every environment set the
                // Builds view shows the status of (repo.ndexr.io, 2026-10-28).
                if item(ui, "How to use the environments ↗") {
                    ctx.open_url(egui::OpenUrl::new_tab("https://repo.ndexr.io"));
                }
                if item(ui, "About Cove") {
                    self.about = true;
                }
            });
        });
    }

    /// What is running and what needs you, along the bottom at the desk (the
    /// review, 2026-11-14: "running environments, language versions, CPU,
    /// memory, and connections belong in a persistent status area - not the
    /// application menu"). Left: Messages, and Claude. Right: the runtime,
    /// the machine, Save, and Help. A phone keeps them in its menu bar's
    /// place: the modes take the bottom there.
    fn status_bar(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        if compact(ctx) {
            return;
        }
        let frame = egui::Frame::new().fill(theme::ground()).inner_margin(egui::Margin::symmetric(8, 2));
        egui::Panel::bottom("rs-status").resizable(false).show_separator_line(true).exact_size(30.0).frame(frame).show(root, |ui| {
            crate::tour::mark(ui, "status-bar", ui.max_rect());
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.connection_chip(ui, ctx);
                    ui.separator();
                }
                let unread = self.inbox.unread();
                let (st, word) = if unread > 0 { (theme::State::Attention, format!("Messages · {unread} new")) } else { (theme::State::Idle, "Messages".to_owned()) };
                let r = ui.scope(|ui| theme::state_mark(ui, st, &word)).inner.interact(egui::Sense::click());
                if r.on_hover_text("requests, answers and people - click to open").clicked() {
                    if self.inbox.open {
                        self.inbox.open = false;
                    } else {
                        self.inbox.show_requests();
                    }
                }
                ui.separator();
                if crate::setup::chip(ui, self.setup.status()) {
                    self.setup.start_claude(ctx);
                }
                // Where the stack comes from, and the way to change it.
                #[cfg(not(target_arch = "wasm32"))]
                {
                    ui.separator();
                    let (st, word) = match crate::account::stack_mode() {
                        Some(m) => (theme::State::Good, m.words().to_owned()),
                        None => (theme::State::Attention, "Stack · not chosen".to_owned()),
                    };
                    let r = ui.scope(|ui| theme::state_mark(ui, st, &word)).inner.interact(egui::Sense::click());
                    crate::tour::mark(ui, "stack-chip", r.rect);
                    if r.on_hover_text("where R, Python and every module come from: installed here, CernVM-FS here, or remote on your cell - click to change").clicked() {
                        self.setup.start_stack();
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let help = ui.add(egui::Button::selectable(self.help_open, RichText::new("? Help").size(12.5)).min_size(egui::vec2(0.0, 26.0)));
                    crate::tour::mark(ui, "help-button", help.rect);
                    if help.on_hover_text("help: show me how, a request, shortcuts (F1)").clicked() {
                        self.help_open = !self.help_open;
                    }
                    if self.mode == Mode::Ide {
                        self.ide.push_button(ui, ctx, "Save");
                    }
                    ui.separator();
                    self.vitals_chip(ui, ctx);
                    #[cfg(not(target_arch = "wasm32"))]
                    self.account_button(ui, ctx);
                    self.session_blocks(ui);
                });
            });
        });
    }

    /// Cove's other sessions, in a window of their own (they were a submenu).
    fn sessions_window(&mut self, ctx: &egui::Context) {
        if !self.sessions_open {
            return;
        }
        let mut open = true;
        egui::Window::new("Other sessions").open(&mut open).order(egui::Order::Foreground).default_width(320.0).collapsible(false).show(ctx, |ui| {
            self.recents.sessions_list(ui, &self.backend);
        });
        self.sessions_open = open;
    }

    /// Where RStudio Server shows who is signed in: the machine, and the
    /// interpreters running on it.
    fn session_blocks(&mut self, ui: &mut egui::Ui) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(u) = &self.updater {
            if let Status::Ready(v) = u.status() {
                if ui.add(theme::primary_button(&format!("restart for v{v}"))).on_hover_text(format!("v{v} is installed in place; R and Python start clean")).clicked() {
                    u.restart(ui.ctx());
                }
            }
        }
        // One chip for the runtime, its detail one click away (operator,
        // 2026-10-26: "R 4.6.1, Python 3.13.15, and ubuntu@... should not
        // permanently occupy the header ... best displayed in a runtime
        // inspector"). Was three labels: user@host, Python and R.
        let probe = self.probe.lock().unwrap().clone();
        // One session: "R version 4.6.1 (…) · env/… · Python 3.13.15".
        let banner = self.ide.banner(Lang::R);
        let after = |key: &str| banner.split(" · ").find_map(|p| p.strip_prefix(key)).and_then(|v| v.split_whitespace().next()).map(str::to_owned);
        let r_version = banner.split_whitespace().find(|w| w.starts_with(|c: char| c.is_ascii_digit())).map(str::to_owned);
        let py = after("Python ");
        let chip = match (&r_version, &py) {
            (Some(r), Some(p)) => format!("R {r} · Py {p} ▾"),
            (Some(r), None) => format!("R {r} ▾"),
            _ => "starting… ▾".to_owned(),
        };
        let quota = self.ide.app_quota();
        let env_r = ui.menu_button(RichText::new(chip).monospace().size(11.0).color(theme::text()), |ui| {
            ui.set_min_width(300.0);
            ui.label(RichText::new("Runtime").size(13.0).strong().color(theme::text_strong()));
            let row = |ui: &mut egui::Ui, k: &str, v: &str| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(k).size(12.0).strong().color(theme::text()));
                    ui.add(egui::Label::new(RichText::new(v).monospace().size(12.0).color(theme::text_strong())).wrap());
                });
            };
            row(ui, "R", if banner.is_empty() { "starting…" } else { banner.split(" · ").next().unwrap_or("") });
            row(ui, "Python", py.as_deref().unwrap_or("not started (reticulate starts it on first use)"));
            if let Some(m) = banner.split(" · ").find(|p| p.starts_with("env/") || p.contains('/')) {
                row(ui, "Environment", m);
            }
            match &probe {
                Some(p) => {
                    let user = p.home.rsplit('/').next().unwrap_or("");
                    row(ui, "Signed in as", &if user.is_empty() { p.host.clone() } else { format!("{user}@{}", p.host) });
                    row(ui, "System", &p.os);
                    row(ui, "Home", &p.home);
                }
                None => {
                    ui.spinner();
                }
            }
            // The person's share of the machine (their cgroup).
            if let Some(q) = quota {
                let gb = |b: u64| b as f64 / (1u64 << 30) as f64;
                let mut text = if q.memory_max > 0 { format!("{:.1} of {:.1} GB", gb(q.memory_now), gb(q.memory_max)) } else { format!("{:.1} GB in use", gb(q.memory_now)) };
                if q.cpus > 0.0 {
                    text.push_str(&format!(" · {} cores", q.cpus.round() as u64));
                }
                row(ui, "Your share", &text);
                if q.oom_kills > 0 {
                    ui.label(RichText::new(format!("{} process(es) of yours were stopped for running out of memory", q.oom_kills)).size(12.0).color(theme::alt()));
                }
            }
        });
        crate::tour::mark(ui, "env-picker", env_r.response.rect);
        env_r
        .response
        .on_hover_text("the runtime this session works in - click for the detail");
        // Storage against the free plan's cap; nothing shown without a cap.
        let storage = self.storage.lock().unwrap().clone();
        if let Some(s) = storage.filter(|s| s.cap > 0 && s.measured > 0) {
            let color = if s.exceeded() {
                theme::alt()
            } else if s.used * 10 >= s.cap * 9 {
                theme::warn()
            } else {
                theme::muted()
            };
            let label = egui::Label::new(RichText::new(format!("{} of {}", api::gb(s.used), api::gb(s.cap))).monospace().size(11.0).color(color)).sense(egui::Sense::click());
            if ui.add(label).on_hover_text("your files on ndexr against the free plan - click for what takes the room").clicked() {
                self.storage_open = true;
            }
        }
    }

    /// The person's storage, every 30 s (or now, with `refresh`, which also
    /// asks the server to measure again).
    fn poll_storage(&mut self, refresh: bool, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if !refresh && now - self.storage_asked < 30.0 {
            return;
        }
        self.storage_asked = now;
        let (slot, at) = (self.storage.clone(), self.storage_at.clone());
        self.backend.query(Query::Storage { refresh }, ctx, move |r: Result<Option<api::StorageView>, String>| {
            if let Ok(v) = r {
                *slot.lock().unwrap() = v;
                *at.lock().unwrap() = now;
            }
        });
        ctx.request_repaint_after(Duration::from_secs(30));
    }

    /// Seconds of grace left now: the server's count at the last poll, run
    /// down on this clock between polls.
    fn storage_grace(&self, s: &api::StorageView, ctx: &egui::Context) -> u64 {
        let since = (ctx.input(|i| i.time) - *self.storage_at.lock().unwrap()).max(0.0) as u64;
        s.grace_left.saturating_sub(since)
    }

    /// Over the free cap: say so where it stays seen, with the three ways
    /// out (storage.rs): the agent clears what rebuilds itself, five minutes
    /// to clear it themselves, or Upgrade on their account.
    fn storage_strip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(s) = self.storage.lock().unwrap().clone() else { return };
        let left = self.storage_grace(&s, ctx);
        let (mut look, mut clear) = (false, false);
        let clearing = *self.storage_clearing.lock().unwrap();
        ui.horizontal_centered(|ui| {
            ui.add_space(4.0);
            let msg = if left > 0 {
                ctx.request_repaint_after(Duration::from_secs(1));
                format!(
                    "Your files use {} - over the {} the free plan includes. {}:{:02} to clear it before uploads, new domains and agent work pause.",
                    api::gb(s.used),
                    api::gb(s.cap),
                    left / 60,
                    left % 60
                )
            } else {
                format!("Your files use {} - over the {} the free plan includes. Uploads, new domains and agent work wait until you clear it or upgrade.", api::gb(s.used), api::gb(s.cap))
            };
            ui.label(RichText::new(msg).size(11.5).color(theme::warn()));
            clear = ui
                .add_enabled(!clearing, egui::Button::new(if clearing { "Clearing…" } else { "Let the agent clear it" }).small())
                .on_hover_text("removes only what rebuilds itself: ~/.cache, node_modules, __pycache__, cargo target folders and the like - never your code or data")
                .clicked();
            look = ui.small_button("What's using it").clicked();
            if ui.add(theme::primary_button("Upgrade")).on_hover_text("subscribe on your account - no storage cap").clicked() {
                ctx.open_url(egui::OpenUrl::new_tab(api::SUBSCRIBE_URL));
            }
        });
        if look {
            self.storage_open = true;
        }
        if clear {
            self.clear_storage(ctx);
        }
    }

    /// Option (a): clear what rebuilds itself, then measure again; if that
    /// was not enough, the agent helps choose the rest.
    fn clear_storage(&mut self, ctx: &egui::Context) {
        *self.storage_clearing.lock().unwrap() = true;
        let (busy, notes, slot) = (self.storage_clearing.clone(), self.notes.clone(), self.storage.clone());
        self.backend.query(Query::StorageClear, ctx, move |r: Result<api::StorageCleared, String>| {
            *busy.lock().unwrap() = false;
            let note = match r {
                Ok(c) => {
                    let still = slot.lock().unwrap().as_ref().is_some_and(|s| s.used.saturating_sub(c.freed) > s.cap);
                    format!(
                        "cleared {} from {} cache and build folders{}",
                        api::gb(c.freed),
                        c.removed.len(),
                        if still { " - still over: ask the agent what else can go" } else { " - measuring again" }
                    )
                }
                Err(e) => e,
            };
            notes.lock().unwrap().push(note);
        });
        // The server measures within ~10 s of the clear; ask after it has.
        self.storage_asked = ctx.input(|i| i.time) - 18.0;
    }

    /// The Agents tab, with a request to help free space typed in (not sent:
    /// the person reads it first). Every turn while over carries the brief
    /// (storage.rs `agent_brief`) whatever is typed.
    fn ask_agent_to_free(&mut self, ctx: &egui::Context) {
        self.storage_open = false;
        self.ide.ask_agent("I'm over my storage. Show me what takes the most room and help me choose what to remove, one command at a time.");
        if self.ide.domain.is_none() {
            self.toast("open one of your domains first - its agent does the helping", ctx);
        }
    }

    fn storage_dialog(&mut self, ctx: &egui::Context) {
        if !self.storage_open {
            return;
        }
        let Some(s) = self.storage.lock().unwrap().clone() else {
            self.storage_open = false;
            return;
        };
        let (mut close, mut ask, mut again, mut clear) = (false, false, false, false);
        let r = egui::Modal::new("storage".into()).show(ctx, |ui| {
            ui.set_width(560.0f32.min(ctx.content_rect().width() - 24.0));
            ui.heading("Your storage");
            let plan = if s.plan.is_empty() { "unknown".to_owned() } else { s.plan.clone() };
            ui.label(RichText::new(format!("{} of {} on the {plan} plan - your home on ndexr and your domains.", api::gb(s.used), api::gb(s.cap))).size(12.0).color(theme::muted()));
            ui.add(egui::ProgressBar::new((s.used as f32 / s.cap.max(1) as f32).min(1.0)).fill(if s.exceeded() { theme::alt() } else { theme::primary() }));
            if s.exceeded() {
                let when = match self.storage_grace(&s, ctx) {
                    0 => "Uploads and new domains wait, and the agent only helps clear space,".to_owned(),
                    left => format!("In {}:{:02} uploads and new domains pause, and the agent only helps clear space,", left / 60, left % 60),
                };
                ui.label(RichText::new(format!("{} over. {when} until you are under or upgrade.", api::gb(s.used - s.cap))).size(12.0).color(theme::warn()));
            }
            ui.add_space(6.0);
            ui.label(RichText::new("Biggest, two levels down (a folder includes what is inside it)").size(11.0).color(theme::hush()));
            egui::ScrollArea::vertical().max_height(260.0).show(ui, |ui| {
                egui::Grid::new("storage-top").num_columns(2).striped(true).show(ui, |ui| {
                    for i in &s.top {
                        ui.label(RichText::new(api::gb(i.bytes)).monospace().size(11.5));
                        ui.label(RichText::new(&i.path).monospace().size(11.5).color(theme::text()));
                        ui.end_row();
                    }
                });
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if s.exceeded() && ui.add_enabled(!*self.storage_clearing.lock().unwrap(), egui::Button::new("Let the agent clear it")).on_hover_text("removes only what rebuilds itself - caches and build output").clicked() {
                    clear = true;
                }
                ask = ui.add(theme::primary_button("Ask the agent to help")).clicked();
                if ui.button("Upgrade").clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(api::SUBSCRIBE_URL));
                }
                again = ui.button("Measure again").on_hover_text("after removing something - takes about 10 s").clicked();
                close = ui.button("Close").clicked();
            });
        });
        if again {
            self.poll_storage(true, ctx);
            self.toast("measuring again - the numbers update in a few seconds", ctx);
            // The server measures within ~10 s; ask once more after it has.
            self.storage_asked = ctx.input(|i| i.time) - 18.0;
        }
        if ask {
            self.ask_agent_to_free(ctx);
        }
        if clear {
            self.clear_storage(ctx);
        }
        if close || r.should_close() {
            self.storage_open = false;
        }
    }

    // ── global toolbar ─────────────────────────────────────────────────────
    /// Sign in / who you are, at the toolbar's right end (desktop only).
    #[cfg(not(target_arch = "wasm32"))]
    fn account_button(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let a = self.account.lock().unwrap().clone();
        if let Some(note) = a.note.clone() {
            self.account.lock().unwrap().note = None;
            // Just signed in: the window follows the ACCOUNT. The cell is
            // chosen once, at launch, for the key there then - so a window
            // already on another account's cell kept that account's files and
            // folders after a different person signed in (2026-10-13). A new
            // account, or a first sign-in with no "this computer" pick (remote
            // first, every machine), relaunches onto that account's cell.
            let fresh = note.starts_with("signed in as ");
            let other_cell = match (&a.on_cell, &a.cell) {
                (Some(on), Some(cell)) => !on.starts_with(cell.trim_end_matches('/')),
                (Some(_), None) => true,
                _ => false,
            };
            if fresh && other_cell {
                self.toast(format!("{note} - reconnecting to this account's cell"), ctx);
                self.relaunch(ctx);
                return;
            }
            if fresh && a.on_cell.is_none() && a.cell.is_some() && crate::account::choose() == crate::account::RunOn::Cell {
                self.toast(format!("{note} - connecting to your cell"), ctx);
                self.run_on(crate::account::RunOn::Cell, ctx);
                return;
            }
            // Signed in on this computer, with a cell: the apps on ndexr are
            // there, not here - say so and offer it (operator, 2026-11-15: "I
            // signed in and on the desktop version I didn't see my apps I'd
            // deployed on ndexr"). Asked at each sign-in, whatever was picked.
            if fresh && a.on_cell.is_none() && a.cell.is_some() {
                self.cell_offer = true;
            }
            self.toast(note, ctx);
        }
        if let Some(url) = &a.waiting {
            if theme::tool(ui, "Signing in…", "finish in your browser - click to open the page again").clicked() {
                ctx.open_url(egui::OpenUrl::new_tab(url));
            }
            return;
        }
        // A Mac has no stack: off the cell it has no modules at all, so say so
        // where it stays seen, with the way back (2026-10-14: it read as a
        // stack "mid-publish" instead).
        // Linux that wanted the cell (no stack here, no "this computer" pick)
        // and did not reach it is in the same place, so it gets the same way back.
        let signed_in = a.email.is_some();
        // On a server already (managed, or the company's): the cell is not wanted.
        let wants_cell = self.on_server().is_none() && (!cfg!(target_os = "linux") || (signed_in && crate::account::choose() == crate::account::RunOn::Cell));
        if wants_cell && a.on_cell.is_none() {
            let can = signed_in && a.cell.is_some();
            // A try from this window answered: on the cell → go there; else say why.
            let answered = self.cell_try.lock().unwrap().1.take();
            match answered {
                Some(Ok(())) => {
                    self.toast("your cell answered - moving this window onto it", ctx);
                    self.run_on(crate::account::RunOn::Cell, ctx);
                    return;
                }
                Some(Err(e)) => self.account.lock().unwrap().cell_error = Some(e),
                None => {}
            }
            // Coming back to the window (e.g. from the cell's page in the
            // browser), and once a minute, tries again by itself.
            let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
            let now = ctx.input(|i| i.time);
            if can && ((focused && !self.was_focused) || (self.cell_retry_at > 0.0 && now >= self.cell_retry_at)) {
                self.try_cell(ctx);
            }
            self.was_focused = focused;
            if self.cell_retry_at > 0.0 {
                ctx.request_repaint_after(std::time::Duration::from_secs(5));
            }
            let trying = self.cell_try.lock().unwrap().0;
            let why = self.account.lock().unwrap().cell_error.clone().unwrap_or_else(|| {
                if cfg!(target_os = "linux") { "R and Python modules come from your cell; this computer has no stack of its own".into() } else { "R and Python modules come from your cell; this Mac has none of its own".into() }
            });
            let label = match (trying, can) {
                (true, _) => "Connecting to your cell…",
                (false, true) => "⚠ Not on your cell - connect",
                (false, false) if !signed_in => "⚠ Not on your cell - sign in",
                _ => "⚠ Not on your cell",
            };
            if theme::tool(ui, label, &why).clicked() && !trying {
                if can {
                    // A cell that is asleep or not serving is started by a
                    // visit to its page; the try below keeps going meanwhile.
                    let asleep = why.contains("did not start") || why.contains("answered 5") || why.contains("could not be reached");
                    if asleep {
                        if let Some(c) = &a.cell {
                            ctx.open_url(egui::OpenUrl::new_tab(c));
                        }
                    }
                    self.try_cell(ctx);
                } else if !signed_in {
                    match crate::account::sign_in(self.account.clone(), ctx.clone()) {
                        Ok(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                        Err(e) => self.toast(e, ctx),
                    }
                } else {
                    self.toast(why, ctx);
                }
            }
            theme::tool_sep(ui);
        }
        match &a.email {
            Some(email) => {
                let label = if a.on_cell.is_some() { format!("{email} · cell") } else { email.clone() };
                ui.menu_button(RichText::new(label).size(12.0), |ui| {
                    ui.label(RichText::new(if a.unverified { "a key is here, but ndexr could not be reached" } else { "signed in to ndexr" }).size(11.0).color(theme::hush()));
                    ui.separator();
                    ui.label(RichText::new("Run R, Python and the shell on").size(11.0).color(theme::hush()));
                    let on_cell = a.on_cell.is_some();
                    let cell_name = a.cell.as_deref().map(|c| c.trim_start_matches("https://").to_owned());
                    let cell_label = match &cell_name {
                        Some(c) => format!("my cell - every stack module ({c})"),
                        None => "my cell (none yet)".to_owned(),
                    };
                    if ui.add_enabled(a.cell.is_some() && !on_cell, egui::Button::selectable(on_cell, cell_label)).clicked() {
                        self.run_on(crate::account::RunOn::Cell, ctx);
                        ui.close();
                    }
                    let here = if !cfg!(target_os = "linux") {
                        "this computer - no stack modules on macOS"
                    } else if crate::host::use_sh().is_some() {
                        "this computer - its own stack"
                    } else {
                        "this computer - its own R and python3 only"
                    };
                    if ui.add_enabled(on_cell, egui::Button::selectable(!on_cell, here)).clicked() {
                        self.run_on(crate::account::RunOn::Local, ctx);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Manage keys at api.ndexr.io").clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(format!("{}/", crate::account::api_base())));
                    }
                    if ui.button("Sign out on this computer").clicked() {
                        crate::account::sign_out(&self.account);
                        ui.close();
                        // A window on the cell would go on driving the signed-out
                        // account's cell until its session lapsed: start again here.
                        if on_cell {
                            self.relaunch(ctx);
                        }
                    }
                });
            }
            None => {
                if theme::tool(ui, "Sign in", "sign in to ndexr - your cell's environments, from this computer").clicked() {
                    match crate::account::sign_in(self.account.clone(), ctx.clone()) {
                        Ok(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                        Err(e) => self.toast(e, ctx),
                    }
                }
            }
        }
    }

    // ── the windows ────────────────────────────────────────────────────────
    fn quadrants(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        for (t, soft) in self.ide.take_focus() {
            self.saved.grid.bring(t, soft);
        }
        // A page opened for one tab: that tab, the whole page.
        if let Some(t) = self.solo {
            egui::CentralPanel::no_frame().show(ui, |ui| self.solo_pane(ui, ctx, t));
            return;
        }
        if compact(ctx) {
            self.compact_panes(ui, ctx);
            return;
        }
        let area = ui.available_rect_before_wrap();
        self.drag.area = area;
        self.drag.rects.clear();
        self.drag.strips.clear();
        self.drag.gaps.clear();
        if let Some(at) = self.saved.zoom_tab.and_then(|t| self.saved.grid.find(t)) {
            self.splitting = false;
            egui::CentralPanel::no_frame().show(ui, |ui| self.pane(ui, ctx, at));
        } else {
            let (wins, gaps) = grid_rects(area, &self.saved.grid);
            for (at, r) in wins {
                ui.scope_builder(egui::UiBuilder::new().max_rect(r).layout(egui::Layout::top_down(egui::Align::Min)), |ui| {
                    ui.set_clip_rect(r.intersect(ui.clip_rect()));
                    self.pane(ui, ctx, at);
                });
            }
            // The splitters, after the windows so they win the pointer at
            // their edges. Each follows where the pointer IS (operator,
            // 2026-10-15: "vertical mid bar has poor function for x
            // shifting"), and the App tab's site frame hides while one moves.
            let mut splitting = false;
            for (gap_r, upright, c, i) in gaps {
                let id = egui::Id::new(("dock-split", upright, c, i));
                let handle = ui.interact(gap_r.expand2(if upright { egui::vec2(4.0, 0.0) } else { egui::vec2(0.0, 4.0) }), id, egui::Sense::drag());
                if handle.hovered() || handle.dragged() {
                    ctx.set_cursor_icon(if upright { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
                }
                if handle.dragged() {
                    splitting = true;
                    if let Some(p) = ctx.pointer_interact_pos() {
                        self.split_to(area, upright, c, i, p);
                    }
                }
                if handle.double_clicked() {
                    self.split_even(upright, c);
                }
                self.drag.gaps.push((gap_r, upright));
            }
            self.splitting = splitting;
        }
        self.grips(ui, ctx);
        self.drag_tab(ctx);
    }

    /// Move the splitter after column `i` (upright) or after window `i` of
    /// column `c` to the pointer: only its two neighbours change, and
    /// neither gets narrower than a usable window.
    fn split_to(&mut self, area: egui::Rect, upright: bool, c: usize, i: usize, p: egui::Pos2) {
        let gap = win_gap();
        let g = &mut self.saved.grid;
        let (shares, total, start, at, min) = if upright {
            let n = g.cols.len();
            let total = area.width() - gap * n.saturating_sub(1) as f32;
            let before: f32 = g.widths[..i].iter().sum::<f32>() * total + gap * i as f32;
            (&mut g.widths, total, area.left() + before, p.x, 220.0)
        } else {
            let n = g.cols[c].len();
            let total = area.height() - gap * n.saturating_sub(1) as f32;
            let before: f32 = g.heights[c][..i].iter().sum::<f32>() * total + gap * i as f32;
            (&mut g.heights[c], total, area.top() + before, p.y, 120.0)
        };
        if total <= 0.0 || i + 1 >= shares.len() {
            return;
        }
        let pair = shares[i] + shares[i + 1];
        let lo = (min / total).min(pair / 2.0);
        let first = ((at - gap / 2.0 - start) / total).clamp(lo, pair - lo);
        shares[i] = first;
        shares[i + 1] = pair - first;
    }

    /// A splitter double-clicked: that list of windows shares evenly again.
    fn split_even(&mut self, upright: bool, c: usize) {
        let g = &mut self.saved.grid;
        let v = if upright { &mut g.widths } else { &mut g.heights[c] };
        let n = v.len().max(1);
        *v = vec![1.0 / n as f32; n];
    }

    /// Every splitter shows where it is: a short bar in the middle of its gap,
    /// lit, with a rule the length of the gap, while the pointer is on it or
    /// dragging it - so a border is something to take hold of, not to hunt for.
    fn grips(&self, ui: &egui::Ui, ctx: &egui::Context) {
        let (hover, down, origin) = ctx.input(|i| (i.pointer.hover_pos(), i.pointer.any_down(), i.pointer.press_origin()));
        let p = ui.painter();
        for &(gap, upright) in &self.drag.gaps {
            let near = |q: Option<egui::Pos2>| q.is_some_and(|q| gap.expand(2.0).contains(q));
            let lit = self.drag.tab.is_none() && (near(hover) || (down && near(origin)));
            let c = gap.center();
            let bar = if upright { egui::vec2(3.0, 36.0) } else { egui::vec2(36.0, 3.0) };
            if lit {
                let rule = if upright { egui::Rect::from_center_size(c, egui::vec2(1.0, gap.height())) } else { egui::Rect::from_center_size(c, egui::vec2(gap.width(), 1.0)) };
                p.rect_filled(rule, 0.0, theme::primary().gamma_multiply(0.5));
            }
            p.rect_filled(egui::Rect::from_center_size(c, bar), 1.5, if lit { theme::primary() } else { theme::pane_brd() });
        }
    }

    /// A phone: windows side by side in ~430 px squeezed every pane until its
    /// toolbars drew over each other (operator screenshot, 2026-10-13). So one
    /// window at a time, full width, picked from a bar at the bottom where the
    /// thumb is (named for the tab in front); running code brings the
    /// console's window forward.
    fn compact_panes(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let wins = self.saved.grid.windows();
        let busy = self.ide.busy();
        if busy && !self.was_busy {
            if let Some(at) = self.saved.grid.find(Tab::Console) {
                self.compact_cell = wins.iter().position(|w| *w == at).unwrap_or(0);
            }
        }
        self.was_busy = busy;
        if self.compact_cell >= wins.len() {
            self.compact_cell = 0;
        }
        if let Some(&at) = wins.get(self.compact_cell) {
            egui::CentralPanel::no_frame().show(ui, |ui| self.pane(ui, ctx, at));
        }
    }

    /// A page opened for one tab (`?tab=`): its name and tools over it, and
    /// nothing else of the IDE.
    fn solo_pane(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, t: Tab) {
        theme::pane(ui, |ui| {
            theme::strip(ui, theme::TAB_H, theme::pane_hdr(), |ui| {
                let (label, _, _) = self.ide.tab_head(t);
                theme::tab(ui, &label, true, None, false);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    self.ide.tab_tools(ui, ctx, t);
                });
            });
            self.ide.tab_body(ui, ctx, t, true);
        });
    }

    /// One window: its tab strip (each tab draggable to another window, or
    /// out to a page of its own), the tools of the tab in front and the zoom
    /// control, then that tab.
    fn pane(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, at: At) {
        let Some(win) = self.saved.grid.win(at).cloned() else { return };
        let tabs = win.tabs.clone();
        let Some(front) = win.shown() else { return };
        let zoomed = self.saved.zoom_tab.is_some();
        self.drag.rects.push((at, ui.available_rect_before_wrap()));
        let toggled = theme::pane(ui, |ui| {
            let (mut z, mut click, mut out) = (false, None, None);
            // On a phone the agent's window shows the agent: its console,
            // terminal and files are a desk's (operator, 2026-11-13: "when we
            // are in IDE agents. I don't think that we need to see console or
            // files"). One of them already in front stays, so it can be left.
            let phone = compact(ctx);
            let strip = theme::strip(ui, theme::TAB_H, theme::pane_hdr(), |ui| {
                // The tools first, from the right; the tabs get what is left
                // and scroll sideways when they need more. Drawn tabs-first,
                // a phone's tabs ran under the tools ("Settings" under the
                // window buttons; operator's screenshot, 2026-11-14: "looks
                // bad on mobile").
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    z = theme::zoom_button(ui, zoomed);
                    if solo_url(front).is_some() && theme::tool(ui, "↗", &format!("open {} in a new page of its own", front.name())).clicked() {
                        out = Some(front);
                    }
                    self.ide.tab_tools(ui, ctx, front);
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt(("pane-tabs", at))
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    for &t in &tabs {
                                        if phone && t != front && matches!(t, Tab::Console | Tab::Terminal | Tab::Files) {
                                            continue;
                                        }
                                        // No status light in the strip (operator, 2026-10-20: "we
                                        // dont need the * in the tabs"); the state is in the tip.
                                        let (label, _, tip) = self.ide.tab_head(t);
                                        let (r, _) = theme::tab(ui, &label, t == front, None, false);
                                        crate::tour::mark(ui, &format!("tab:{t:?}"), r.rect);
                                        let r = r.interact(egui::Sense::drag());
                                        if r.drag_started() {
                                            self.drag.tab = Some(t);
                                        }
                                        let r = if tip.is_empty() || self.drag.tab.is_some() { r } else { r.on_hover_text(tip) };
                                        if r.clicked() {
                                            click = Some(t);
                                        }
                                        if solo_url(t).is_some() {
                                            r.context_menu(|ui| {
                                                if ui.button(format!("Open {} in a new page", t.name())).clicked() {
                                                    out = Some(t);
                                                    ui.close();
                                                }
                                            });
                                        }
                                    }
                                });
                            });
                    });
                });
                ui.max_rect()
            });
            self.drag.strips.push((at, strip));
            let mut front = front;
            if let Some(t) = click {
                if let Some(w) = self.saved.grid.win_mut(at) {
                    w.front = Some(t);
                }
                self.ide.clicked(t);
                front = t;
            }
            if let Some(url) = out.and_then(solo_url) {
                ctx.open_url(egui::OpenUrl::new_tab(url));
            }
            let body = ui.available_rect_before_wrap();
            z |= self.ide.tab_body(ui, ctx, front, zoomed);
            crate::tour::mark(ui, &format!("pane:{front:?}"), body);
            (z, front)
        });
        if toggled.0 {
            self.zoom(toggled.1);
        }
    }

    /// Where a tab let go at `p` goes. On a window's tab strip, or the middle
    /// of a window: into that window. Near a window's left or right edge: a
    /// new column there - between two columns when that edge is between them;
    /// near its top or bottom: a new window above or below it in its column.
    /// Beyond 4×4 an edge takes the tab into the window instead.
    fn drop_target(&self, t: Tab, p: egui::Pos2) -> Option<Drop> {
        if let Some(&(at, _)) = self.drag.strips.iter().find(|(_, r)| r.contains(p)) {
            return Some(Drop::Join(at.0, at.1));
        }
        let g = &self.saved.grid;
        if self.saved.zoom_tab.is_some() {
            return self.drag.rects.first().map(|&((c, r), _)| Drop::Join(c, r));
        }
        // The window under the pointer, or (in a gap) the nearest one.
        let &((c, r), rect) = self.drag.rects.iter().min_by(|a, b| a.1.distance_to_pos(p).total_cmp(&b.1.distance_to_pos(p)))?;
        let band_x = (rect.width() * 0.22).clamp(36.0, 180.0);
        let band_y = (rect.height() * 0.22).clamp(36.0, 160.0);
        let edges = [
            (p.x - rect.left(), band_x, Drop::Col(c)),
            (rect.right() - p.x, band_x, Drop::Col(c + 1)),
            (p.y - rect.top(), band_y, Drop::Row(c, r)),
            (rect.bottom() - p.y, band_y, Drop::Row(c, r + 1)),
        ];
        let edge = edges.into_iter().filter(|(d, band, _)| *d < *band).min_by(|a, b| (a.0 / a.1).total_cmp(&(b.0 / b.1))).map(|e| e.2);
        Some(match edge {
            Some(d) if g.fits(t, d) => d,
            _ => Drop::Join(c, r),
        })
    }

    /// A tab being dragged: the layout it would make, drawn over the windows,
    /// and the move once it is let go (Esc, or letting go outside, leaves it).
    fn drag_tab(&mut self, ctx: &egui::Context) {
        let Some(t) = self.drag.tab else { return };
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        let p = ctx.pointer_latest_pos().filter(|p| self.drag.area.contains(*p));
        let target = p.and_then(|p| self.drop_target(t, p));
        if let (Some(p), Some(d)) = (p, target) {
            let after = self.saved.grid.dropped(t, d);
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("dock-drop")));
            let (wins, _) = grid_rects(self.drag.area, &after);
            let lands = after.find(t);
            // The layout it would make: every window outlined faintly, the
            // one the tab lands in lit.
            for (at, r) in wins {
                let r = r.shrink(1.0);
                if Some(at) == lands {
                    painter.rect_filled(r, theme::WIN_RADIUS, theme::primary().gamma_multiply(0.16));
                    painter.rect_stroke(r, theme::WIN_RADIUS, egui::Stroke::new(1.5, theme::primary()), egui::StrokeKind::Inside);
                } else {
                    painter.rect_stroke(r, theme::WIN_RADIUS, egui::Stroke::new(1.0, theme::primary().gamma_multiply(0.35)), egui::StrokeKind::Inside);
                }
            }
            let what = match d {
                Drop::Join(..) => t.name().to_owned(),
                Drop::Col(_) => format!("{} · new column", t.name()),
                Drop::Row(..) => format!("{} · new window", t.name()),
            };
            let g = painter.layout_no_wrap(what, egui::FontId::proportional(12.0), theme::text_strong());
            let size = g.size() + egui::vec2(16.0, 8.0);
            // Beside the pointer, kept on screen: at the bottom edge (a new
            // window below) it had dropped off the page.
            let a = self.drag.area;
            let at = egui::pos2((p.x + 12.0).min(a.right() - size.x), if p.y + 10.0 + size.y > a.bottom() { p.y - 10.0 - size.y } else { p.y + 10.0 });
            let box_ = egui::Rect::from_min_size(at, size);
            painter.rect_filled(box_, 4.0, theme::surface_hi());
            painter.rect_stroke(box_, 4.0, egui::Stroke::new(1.0, theme::amber()), egui::StrokeKind::Inside);
            painter.galley(box_.min + egui::vec2(8.0, 4.0), g, theme::text_strong());
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.drag.tab = None;
        } else if !ctx.input(|i| i.pointer.any_down()) {
            if let Some(d) = target {
                self.saved.grid = self.saved.grid.dropped(t, d);
                self.ide.clicked(t);
                // A zoomed window that loses its tab, or a drop elsewhere, gives the screen back.
                self.saved.zoom_tab = None;
            }
            self.drag.tab = None;
        }
        ctx.request_repaint();
    }

    // ── dialogs ────────────────────────────────────────────────────────────

    /// Tools › Global Options: where the sessions start, in which module
    /// (Inlet 0004), and the Environment they come from. Saved on
    /// the machine, never guessed.
    fn options_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut draft) = self.options.take() else { return };
        let view = self.settings.lock().unwrap().clone();
        let modules = self.modules.lock().unwrap().clone();
        let job = self.stack_job.lock().unwrap().clone();
        let test = self.proxy_test.lock().unwrap().clone();
        let (mut keep, mut save, mut setup, mut run_test) = (true, false, false, false);
        // RStudio's Global Options: a large dialog, categories down the left,
        // the page scrolling on its own and OK / Cancel always in view. (It
        // was a 660 px column whose one scroll hid the interpreters and cut
        // the CVMFS values to "http:…" - operator screenshot, 2026-10-13.)
        let screen = ctx.content_rect();
        let w = (screen.width() * 0.82).clamp(560.0, 1040.0).min(screen.width() - 24.0);
        let side_w = if compact(ctx) { 92.0 } else { 150.0 };
        // Never taller than the window: the title, the body and OK / Cancel
        // all fit, however short the screen.
        let h = (screen.height() * 0.80).min(760.0).max(300.0).min(screen.height() - 32.0);
        let mut page = self.options_page.min(OPTION_PAGES.len() - 1);
        egui::Modal::new("global-options".into()).show(ctx, |ui| {
            ui.set_width(w);
            ui.heading("Global Options");
            ui.add_space(6.0);
            let body_h = (h - 110.0).max(160.0);
            // A fixed-height row. In a plain horizontal_top the vertical
            // separator below took the row's whole available height, and in a
            // modal that is "as tall as the dialog already is": every frame the
            // dialog grew by a little and ran off the bottom of the screen
            // (operator, 2026-10-13: "expands vertically and goes out of screen").
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), body_h), egui::Layout::left_to_right(egui::Align::Min), |ui| {
                ui.set_height(body_h);
                ui.vertical(|ui| {
                    ui.set_width(side_w);
                    ui.set_height(body_h);
                    for (i, (name, about)) in OPTION_PAGES.iter().enumerate() {
                        let text = RichText::new(*name).size(13.0).color(if page == i { theme::text_strong() } else { theme::muted() });
                        if ui.add(egui::Button::selectable(page == i, text).min_size(egui::vec2(side_w, 28.0))).on_hover_text(*about).clicked() {
                            page = i;
                        }
                    }
                });
                ui.add_sized([1.0, body_h], egui::Separator::default().vertical());
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    egui::ScrollArea::vertical().id_salt(("options-page", page)).max_height(body_h).min_scrolled_height(body_h).auto_shrink([false, false]).show(ui, |ui| {
                        ui.set_width(ui.available_width() - 12.0);
                        match page {
                            0 => {
                                heading(ui, "Environment");
                                ui.label(RichText::new("One module for the whole IDE: R, Python and the shell all start in it, and a render uses it. An env/ module brings an R, a Python and a site library together: R starts in it, with that Python inside it through reticulate. The same switch is in the toolbar.").size(12.0).color(theme::muted()));
                                ui.add_space(8.0);
                                let field_w = (ui.available_width() - 150.0).clamp(240.0, 560.0);
                                egui::Grid::new("options-grid").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                                    ui.label("Module");
                                    module_menu(ui, "options-module", &mut draft.module, &modules, field_w);
                                    ui.end_row();
                                });
                                if let Some(m) = &modules {
                                    let none = m.env.is_empty() && m.r.is_empty() && m.python.is_empty();
                                    let text = if none { m.source.clone() } else { format!("{} env · {} R · {} Python modules, from {}", m.env.len(), m.r.len(), m.python.len(), m.source) };
                                    ui.label(RichText::new(text).size(10.5).color(if none { theme::warn() } else { theme::hush() }));
                                }
                                ui.add_space(14.0);
                                heading(ui, "Project");
                                ui.label(RichText::new("Where R, Python and the shell start, and where Files opens.").size(12.0).color(theme::muted()));
                                ui.add_space(4.0);
                                egui::Grid::new("options-project").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                                    ui.label("Project folder");
                                    ui.add(egui::TextEdit::singleline(&mut draft.project_dir).font(egui::TextStyle::Monospace).hint_text("/absolute/path — empty: home").desired_width(field_w));
                                    ui.end_row();
                                    // Where New project puts every project (host.rs projects_root).
                                    // The domain this Cove's projects are named under, away
                                    // from ndexr's platform (api.rs Settings.domain).
                                    let (platform, served) = self.settings.lock().unwrap().as_ref().map_or((false, String::new()), |v| (v.platform, crate::api::primary_domain(&v.served_as)));
                                    if !platform {
                                        ui.label("Domains");
                                        // Comma-separated as typed; empty entries are dropped where it is read.
                                        let mut text = draft.domains.join(", ");
                                        let hint = if served.is_empty() { "lab.example.org — empty: name.localhost".to_owned() } else { format!("{served} already · add more, comma-separated") };
                                        let tip = if served.is_empty() { "new projects are named under the first: \"report\" becomes report.lab.example.org; point that name (and its *.) at this server".to_owned() } else { format!("this Cove is reached under {served}, so new projects are named there first: \"report\" becomes report.{served}; add more domains to offer them too") };
                                        if ui.add(egui::TextEdit::singleline(&mut text).font(egui::TextStyle::Monospace).hint_text(hint).desired_width(field_w)).on_hover_text(tip).changed() {
                                            draft.domains = text.split(',').map(|d| d.trim().to_owned()).collect();
                                        }
                                        ui.end_row();
                                    }
                                    ui.label("Projects folder");
                                    ui.add(egui::TextEdit::singleline(&mut draft.projects_dir).font(egui::TextStyle::Monospace).hint_text("/absolute/path — empty: ~/.cove/projects").desired_width(field_w))
                                        .on_hover_text("every new project is made here, in a folder of its own; projects you already have stay where they are and still open. On ndexr's own servers your ndexr projects folder is used");
                                    ui.end_row();
                                    // Changed files show on their own; this is how often to look.
                                    ui.label("Refresh files");
                                    let every = draft.refresh_every() as u8;
                                    egui::ComboBox::from_id_salt("options-refresh").selected_text(format!("every {every} s")).show_ui(ui, |ui| {
                                        for n in 1..=3u8 {
                                            if ui.selectable_label(every == n, format!("every {n} s")).clicked() {
                                                draft.refresh_secs = n;
                                            }
                                        }
                                    })
                                    .response
                                    .on_hover_text("how often open files and the Files folder are checked for changes on disk");
                                    ui.end_row();
                                });
                                ui.add_space(14.0);
                                heading(ui, "Stack");
                                ui.label(RichText::new("Where modules come from. Empty: the CVMFS mount at /opt/ndexr/stack, else a local build at ~/.ndexr/stack. With neither, the consoles do not start: they run on the stack only.").size(12.0).color(theme::muted()));
                                ui.add_space(4.0);
                                egui::Grid::new("options-stack").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                                    ui.label("Stack folder");
                                    ui.add(egui::TextEdit::singleline(&mut draft.stack_dir).font(egui::TextStyle::Monospace).hint_text("/path/with/use.sh — empty: automatic").desired_width(field_w))
                                        .on_hover_text("a stack built or copied to its own folder (it has use.sh, or <platform>/use.sh)");
                                    ui.end_row();
                                    ui.label("Error reports");
                                    let mut on = draft.report == Some(true);
                                    if ui.checkbox(&mut on, "send an anonymous report when a stack build or setup fails")
                                        .on_hover_text("the failed step, its exit code and error lines, and this computer's shape; names, paths and addresses are removed first. Filed as a request; never sent in airgap mode")
                                        .changed()
                                    {
                                        draft.report = Some(on);
                                    }
                                    ui.end_row();
                                });
                            }
                            1 => match &view {
                                Some(v) => environment(ui, &v.stack, &mut draft.cvmfs, &job, &test, self.testing, &mut setup, &mut run_test),
                                None => {
                                    ui.spinner();
                                }
                            },
                            _ => pimped(ui),
                        }
                    });
                });
            });
            ui.separator();
            let bad = draft.cvmfs.check().err();
            if let Some(e) = &bad {
                ui.label(RichText::new(e).size(11.5).color(theme::warn()));
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("OK restarts any session whose module or project changed, and hands changed CVMFS settings to this machine's client.").size(11.0).color(theme::hush()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    keep = !ui.button("Cancel").clicked();
                    let dirty = view.as_ref().is_none_or(|v| draft != v.settings || !v.chosen);
                    save = ui.add_enabled(dirty && bad.is_none(), theme::primary_button("OK")).clicked();
                });
            });
        });
        self.options_page = page;
        if setup {
            crate::setup::ask_cvmfs("app");
        }
        if run_test {
            self.testing = true;
            let (slot, proxy) = (self.proxy_test.clone(), draft.cvmfs.proxy.clone());
            *slot.lock().unwrap() = None;
            self.backend.query(Query::StackTest { proxy }, ctx, move |r: Result<Vec<ProxyCheck>, String>| {
                *slot.lock().unwrap() = Some(r);
            });
        }
        if self.testing && self.proxy_test.lock().unwrap().is_some() {
            self.testing = false;
        }
        if save {
            self.save_options(draft, ctx);
        } else if keep {
            self.options = Some(draft);
        }
    }

    /// Under the toolbar while the stack is missing here or being set up -
    /// the one place the background setup shows itself.
    fn stack_strip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let job = self.stack_job.lock().unwrap().clone();
        let stack = self.settings.lock().unwrap().as_ref().map(|v| v.stack.clone()).unwrap_or_default();
        let (mut setup, mut options) = (false, false);
        ui.horizontal_centered(|ui| {
            ui.add_space(4.0);
            if job.running {
                ui.add(egui::Spinner::new().size(12.0));
                let last = job.lines.last().cloned().unwrap_or_default();
                ui.add(egui::Label::new(RichText::new(format!("{} — {last}", job.what)).size(11.5).color(theme::muted())).truncate());
            } else if job.ok == Some(false) {
                // The cause, not the summary: `ndexr install cvmfs` ends with
                // "preflight failed - fix the ✘ items above", and the ✘ line
                // that says which was off screen (operator, 2026-11-14, a new
                // laptop). The first ✘ line and its → remedy, else the last line.
                let last = job.why();
                ui.label(RichText::new(format!("{} failed:", job.what)).size(11.5).color(theme::warn()));
                ui.add(egui::Label::new(RichText::new(last).monospace().size(11.0).color(theme::muted())).truncate());
                options = ui.small_button("Details").on_hover_text("the setup's whole output, why it failed, and the ways to try again").clicked();
            } else if stack_missing(&stack) {
                ui.label(RichText::new("The ndexr stack is not on this machine yet - R, Python and every module come from it.").size(11.5).color(theme::warn()));
                if stack.root_by.is_empty() {
                    ui.label(RichText::new(format!("An admin runs `{}` once.", stack.setup_cmd)).monospace().size(11.0).color(theme::muted()));
                } else {
                    setup = ui.add(theme::primary_button("Set it up")).on_hover_text(if stack.root_by == "pkexec" { "asks for an admin password once, then runs in the background" } else { "runs in the background" }).clicked();
                }
            }
        });
        if setup {
            crate::setup::ask_cvmfs("app");
        }
        if crate::setup::cvmfs_consent(ctx, "app", &stack) {
            #[cfg(not(target_arch = "wasm32"))]
            crate::account::set_stack_mode(crate::api::StackMode::Cvmfs);
            self.setup_stack(ctx);
        }
        if options {
            self.setup.show_job(job.clone());
        }
    }

    /// The IDE server restarted (every deploy does) and this page's session
    /// went with it: say so, then reopen on the same domain - the front door
    /// starts a fresh session there, and the saved domain and its open files
    /// come back. Instead of every request failing with a raw 404 page.
    ///
    /// A picture first and a few words after (operator, 2026-10-15: "a little
    /// larger of a modal with another calm photo ... let the user know that a
    /// little more clearly and simply ... that we restarted the app just now
    /// and are actively under development and in beta"). 6 s to read it.
    fn session_ended(&mut self, ctx: &egui::Context) {
        const WAIT: f64 = 6.0;
        let now = ctx.input(|i| i.time);
        let since = *self.ended_at.get_or_insert(now);
        let host = self.ide.domain.as_ref().map(|d| d.host.clone());
        let url = host.as_ref().map_or("/".to_owned(), |h| format!("/?domain={h}"));
        let go = now - since > WAIT;
        let w = (ctx.content_rect().width() - 32.0).clamp(280.0, 560.0);
        let photo = self.calm.texture(ctx).map(|t| (t.id(), t.size()));
        let frame = egui::Frame::new()
            .fill(theme::surface())
            .stroke(egui::Stroke::new(1.0, theme::pane_brd()))
            .corner_radius(14)
            .shadow(egui::epaint::Shadow { offset: [0, 10], blur: 40, spread: 0, color: egui::Color32::from_black_alpha(140) });
        egui::Modal::new("session-ended".into()).frame(frame).backdrop_color(egui::Color32::from_black_alpha(150)).show(ctx, |ui| {
            ui.set_width(w);
            ui.spacing_mut().item_spacing.y = 0.0;
            // The photograph across the top, cover-fitted, its top corners rounded with the card.
            let h = (w * 0.5).round();
            let (r, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
            if let Some((id, [iw, ih])) = photo {
                let (iw, ih) = (iw as f32, ih as f32);
                // Cover: the full width, and the share of the height that fits,
                // set a little low so the water and its reflection stay in.
                let vis = ((iw / ih) / (w / h)).min(1.0);
                let top = (0.62 - vis / 2.0).clamp(0.0, 1.0 - vis);
                let uv = egui::Rect::from_min_max(egui::pos2(0.0, top), egui::pos2(1.0, top + vis));
                ui.painter().add(egui::epaint::RectShape::filled(r, egui::CornerRadius { nw: 14, ne: 14, sw: 0, se: 0 }, egui::Color32::WHITE).with_texture(id, uv));
            } else {
                ui.painter().rect_filled(r, egui::CornerRadius { nw: 14, ne: 14, sw: 0, se: 0 }, theme::ground());
            }
            // A soft fade into the card, and the beta badge on the water.
            let fade = egui::Rect::from_min_max(egui::pos2(r.left(), r.bottom() - 36.0), r.max);
            let mut m = egui::Mesh::default();
            m.colored_vertex(fade.left_top(), egui::Color32::TRANSPARENT);
            m.colored_vertex(fade.right_top(), egui::Color32::TRANSPARENT);
            m.colored_vertex(fade.right_bottom(), theme::surface());
            m.colored_vertex(fade.left_bottom(), theme::surface());
            m.add_triangle(0, 1, 2);
            m.add_triangle(0, 2, 3);
            ui.painter().add(m);
            let badge = ui.painter().layout_no_wrap("BETA · IN ACTIVE DEVELOPMENT".into(), egui::FontId::proportional(10.5), theme::text_strong());
            let br = egui::Rect::from_min_size(r.left_top() + egui::vec2(16.0, 16.0), badge.size() + egui::vec2(20.0, 10.0));
            ui.painter().rect(br, 99.0, egui::Color32::from_black_alpha(110), egui::Stroke::new(1.0, theme::primary().gamma_multiply(0.7)), egui::StrokeKind::Inside);
            ui.painter().galley(br.min + egui::vec2(10.0, 5.0), badge, theme::text_strong());

            egui::Frame::new().inner_margin(egui::Margin { left: 24, right: 24, top: 6, bottom: 22 }).show(ui, |ui| {
                ui.set_width(w - 48.0);
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.label(RichText::new("We just updated Cove").size(22.0).strong().color(theme::text_strong()));
                ui.label(RichText::new("Cove is in beta and changes often. Updating restarts it, so your session ended with it - nothing you saved is lost.").size(13.5).color(theme::muted()));
                // A spinner and one thing the atlas says, no countdown or button
                // (operator, 2026-10-18: the timer was "consistently incorrect",
                // and it is fast enough that coming back is assumed).
                let (layer, what) = crate::atlas::fact((since * 1000.0) as usize);
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0).color(theme::primary()));
                    ui.label(RichText::new(layer).size(13.0).strong().color(theme::text_strong()));
                });
                ui.label(RichText::new(format!("{}{}.", what[..1].to_uppercase(), &what[1..])).size(13.0).color(theme::text()));
            });
        });
        ctx.request_repaint_after(Duration::from_millis(250));
        if go && now - since < 60.0 {
            #[cfg(target_arch = "wasm32")]
            if let Some(w) = web_sys::window() {
                let _ = w.location().set_href(&url);
            }
            #[cfg(not(target_arch = "wasm32"))]
            let _ = &url;
        }
    }

    /// Help › Send a request - complaints.ndexr.io's REQUEST, from the IDE:
    /// filed as this person, on the domain open in App, through the platform's
    /// own file_complaint (host.rs send_request). It also lands as a turn in
    /// that domain's thread, so the answer shows in the Agent tab.
    fn request_dialog(&mut self, ctx: &egui::Context) {
        let sent = self.request_sent.lock().unwrap().take();
        if let Some(r) = sent {
            match r {
                Ok(id) => {
                    let to_stack = self.request.as_ref().is_some_and(|d| d.software) || self.mode == Mode::Atlas;
                    self.request = None;
                    self.inbox.show_requests();
                    self.toast(
                        if to_stack {
                            format!("Request #{id} sent to {STACK_DOMAIN} - the stack's builders take it from there")
                        } else {
                            format!("Request #{id} sent - follow it, and the answer, in Messages")
                        },
                        ctx,
                    );
                }
                Err(e) => {
                    if let Some(d) = self.request.as_mut() {
                        d.sending = false;
                    }
                    self.toast(format!("not sent: {e}"), ctx);
                }
            }
        }
        let Some(mut d) = self.request.take() else { return };
        // Pasted anywhere (Ctrl+V of a screenshot), picked with Attach files,
        // or dropped on the window: all of it goes with the request
        // (a person's request, 2026-10-18: "copy and paste an image into it or to
        // provide an image"; #31: "documents or files or audio").
        while let Some(shot) = crate::shots::take() {
            match shot {
                crate::shots::Shot::Pasted(u) | crate::shots::Shot::Site(u) => d.files.push(crate::api::Attachment { name: crate::shots::name("pasted", &u), data: u }),
                crate::shots::Shot::File(n, u) => d.files.push(crate::api::Attachment { name: n, data: u }),
                crate::shots::Shot::Error(e) => self.toast(e, ctx),
            }
        }
        // Dropped: egui hands over handles, read off the frame (as ide.rs
        // upload_dropped), and land here a frame or two later.
        for f in ctx.input(|i| i.raw.dropped_files.clone()) {
            let name = f.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "file".into());
            let (slot, notes, c) = (self.request_drops.clone(), self.notes.clone(), ctx.clone());
            let land = move |r: Result<Vec<u8>, String>| {
                match r {
                    Ok(b) => slot.lock().unwrap().push(crate::api::Attachment { name: name.clone(), data: crate::api::b64_encode(&b) }),
                    Err(e) => notes.lock().unwrap().push(format!("{name}: {e}")),
                }
                c.request_repaint();
            };
            #[cfg(not(target_arch = "wasm32"))]
            std::thread::spawn(move || land(f.bytes().map_err(|e| e.to_string())));
            #[cfg(target_arch = "wasm32")]
            wasm_bindgen_futures::spawn_local(async move { land(f.bytes_async().await.map_err(|e| e.to_string())) });
        }
        d.files.append(&mut self.request_drops.lock().unwrap());
        // Software for the stack, or anything asked from the atlas front
        // page, is the stack's to build: ndexr.ndexr.io, whatever domain the
        // IDE last had open (operator, 2026-10-18: "application requests on
        // cove.ndexr.io front page should not be going to biomarker they
        // should be going to ndexr.ndexr.io"). From inside the IDE, about the
        // domain at hand, it goes to that domain's thread as before.
        let domain = if d.software || self.mode == Mode::Atlas {
            STACK_DOMAIN.to_owned()
        } else {
            self.ide.domain.as_ref().map(|x| x.host.clone()).unwrap_or_else(|| "cove.ndexr.io".into())
        };
        let system = if d.system { self.system_info(ctx, &domain) } else { String::new() };
        let (mut keep, mut send) = (true, false);
        let (mic, backend) = (&mut self.request_mic, &self.backend);
        // One even column: a heading and one line on what happens to it, then
        // each field under its own small label, the same gap between all of
        // them, and the buttons at the bottom right (operator, 2026-10-18:
        // "too close together ... should feel symmetric and clear and easy
        // to ask for").
        egui::Modal::new("send-request".into()).frame(egui::Frame::popup(&ctx.global_style()).inner_margin(egui::Margin::same(22))).show(ctx, |ui| {
            let w = 480.0f32.min(ctx.content_rect().width() - 64.0);
            ui.set_width(w);
            ui.spacing_mut().item_spacing.y = 6.0;
            let field = |ui: &mut egui::Ui, text: &str| {
                ui.add_space(8.0);
                ui.label(RichText::new(text).size(11.5).color(theme::muted()));
            };
            // The fields scroll and Send / Cancel stay in view, however short
            // the window (zoomed in with Ctrl +, operator 2026-10-18).
            let room = (ctx.content_rect().height() - 130.0).max(120.0);
            egui::ScrollArea::vertical().id_salt("request-body").max_height(room).auto_shrink([false, true]).show(ui, |ui| {
            if d.software {
                // Operator, 2026-10-18: "an add software button which sends a
                // request to requests ... should also be able to send verbal
                // request so to record".
                ui.label(RichText::new("Add software").font(theme::display(20.0)).color(theme::text_strong()));
                ui.label(RichText::new(format!("The ndexr team builds it into the stack - filed with {domain}, where the stack is built.")).size(12.0).color(theme::muted()));
                field(ui, "Software");
                ui.add(egui::TextEdit::singleline(&mut d.name).hint_text("data.table, PyTorch, GDAL, samtools…").desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0)));
                ui.columns(2, |c| {
                    field(&mut c[0], "Version (optional)");
                    c[0].add(egui::TextEdit::singleline(&mut d.version).hint_text("latest").desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0)));
                    field(&mut c[1], "Kind");
                    let kw = c[1].available_width();
                    egui::ComboBox::from_id_salt("request-kind").width(kw).selected_text(SOFTWARE_KINDS[d.kind.min(SOFTWARE_KINDS.len() - 1)]).show_ui(&mut c[1], |ui| {
                        for (i, k) in SOFTWARE_KINDS.iter().enumerate() {
                            ui.selectable_value(&mut d.kind, i, *k);
                        }
                    });
                });
                field(ui, "What is it for? Type, or tap the mic and say it.");
            } else {
                ui.label(RichText::new("Send a request").font(theme::display(20.0)).color(theme::text_strong()));
                ui.label(RichText::new(format!("To the ndexr team, about {domain}. The answer shows in this domain's thread.")).size(12.0).color(theme::muted()));
                field(ui, "What do you need, or what is broken?");
            }
            ui.horizontal_top(|ui| {
                let mic_w = if crate::voice::Mic::shown() { 44.0 } else { 0.0 };
                ui.add(egui::TextEdit::multiline(&mut d.what).desired_rows(4).desired_width(ui.available_width() - mic_w).margin(egui::vec2(8.0, 6.0)));
                mic.ui(ui, ctx, backend, &mut d.what);
            });
            if let Some(e) = mic.error() {
                ui.label(RichText::new(e).size(11.0).color(theme::alt()));
            }
            if !d.software {
                field(ui, "What were you trying to do? (optional)");
                ui.add(egui::TextEdit::multiline(&mut d.trying).desired_rows(2).desired_width(f32::INFINITY).margin(egui::vec2(8.0, 6.0)));
            }
            field(ui, "Screenshots, recordings, documents (optional)");
            ui.horizontal_wrapped(|ui| {
                if ui.button("+ Attach files…").on_hover_text("pick files from this computer: images, audio, video, PDFs").clicked() {
                    crate::shots::pick();
                }
                ui.label(RichText::new("or paste a screenshot (Ctrl+V), or drop files here").size(11.5).color(theme::muted()));
            });
            let mut gone = None;
            for (i, f) in d.files.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}  ·  {} KB", f.name, f.size().div_ceil(1024))).size(11.5).color(theme::text()));
                    if ui.small_button("✕").on_hover_text("leave this one out").clicked() {
                        gone = Some(i);
                    }
                });
            }
            if let Some(i) = gone {
                d.files.remove(i);
            }
            let total: usize = d.files.iter().map(|f| f.size()).sum();
            if total > crate::api::REQUEST_FILES_MAX {
                ui.label(RichText::new(format!("{} MB in all - the most a request carries is {} MB", total >> 20, crate::api::REQUEST_FILES_MAX >> 20)).size(11.0).color(theme::warn()));
            }
            ui.add_space(6.0);
            ui.checkbox(&mut d.blocked, "I'm blocked - I can't continue without this");
            // Asked, never assumed (operator, 2026-11-14: "must be granted
            // permission in the request though checkbox selected defaults to
            // no"): ticked, the exact text that goes is shown under it.
            ui.checkbox(&mut d.system, "Send system information, so an agent can understand what failed");
            if d.system {
                egui::CollapsingHeader::new(RichText::new("What is sent").size(12.0)).id_salt("request-system").default_open(true).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&system).monospace().size(11.0).color(theme::text())).wrap());
                });
            }
            });
            ui.add_space(14.0);
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 30.0), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                send = ui.add_enabled(!d.sending && d.ready(), theme::primary_button(if d.sending { "Sending…" } else { "Send request" }).min_size(egui::vec2(120.0, 30.0))).clicked();
                keep = !ui.add(egui::Button::new("Cancel").min_size(egui::vec2(90.0, 30.0))).clicked();
            });
        });
        if send && d.software {
            d.trying = "asked from the Cove atlas's Add software".into();
        }
        if send {
            d.sending = true;
            let slot = self.request_sent.clone();
            // Only what was shown under the box, and only when it is ticked.
            let trying = crate::api::with_system(&d.trying, &system);
            // The desktop app files it itself, with the person's key: the
            // machine's half needs the platform's R, which only a Cove server
            // has (a laptop, a Mac and a cell all failed). A browser on a Cove
            // server still asks that server.
            #[cfg(not(target_arch = "wasm32"))]
            {
                let (what, blocked, ctx2) = (d.filed_what(), d.blocked, ctx.clone());
                std::thread::spawn(move || {
                    *slot.lock().unwrap() = Some(crate::account::send_request(&domain, &what, &trying, blocked));
                    ctx2.request_repaint();
                });
            }
            #[cfg(target_arch = "wasm32")]
            self.backend.query(Query::SendRequest { domain, what: d.filed_what(), trying, blocked: d.blocked, files: d.files.clone() }, ctx, move |r: Result<i64, String>| {
                *slot.lock().unwrap() = Some(r);
            });
        }
        if keep {
            self.request = Some(d);
        }
    }

    /// What a request carries about this Cove when the person allows it:
    /// enough for an agent to place a failure - the build, the machine's
    /// kind, the stack, the runtime, the project and its app's last job, the
    /// window. No user name, host name, home folder or file contents.
    fn system_info(&self, ctx: &egui::Context, domain: &str) -> String {
        let mut l: Vec<String> = Vec::new();
        #[cfg(not(target_arch = "wasm32"))]
        {
            l.push(format!("Cove: v{} desktop · {}", update::VERSION, update::platform()));
            l.push(format!("Stack: {}", crate::account::stack_mode().map_or("not chosen", |m| m.words())));
        }
        #[cfg(target_arch = "wasm32")]
        l.push("Cove: in a browser".to_owned());
        if let Some(p) = self.probe.lock().unwrap().clone() {
            if !p.os.is_empty() {
                l.push(format!("System: {}", p.os));
            }
            if let Some(v) = p.version.filter(|v| !v.is_empty()) {
                l.push(format!("ndexr CLI: {v}"));
            }
        }
        let banner = self.ide.banner(Lang::R);
        if !banner.is_empty() {
            l.push(format!("Runtime: {banner}"));
        }
        let mode = MODES.iter().find(|m| m.0 == self.mode).map_or("", |m| m.1);
        l.push(format!("About: {domain} · in {mode}"));
        let seen = self.ide.tour_seen();
        if seen.project.is_some() {
            let mut app = format!("Project app: {} {}", if seen.language.is_empty() { "?" } else { &seen.language }, if seen.kind.is_empty() { "" } else { &seen.kind });
            app.push_str(if seen.running { " · running" } else { " · not running" });
            if seen.job_running {
                app.push_str(" · a job is running");
            }
            if !seen.env_module.is_empty() {
                app.push_str(&format!(" · {}", seen.env_module));
            }
            l.push(app);
            if let Some(e) = &seen.job_failed {
                l.push(format!("Last app job failed: {}", e.chars().take(600).collect::<String>()));
            }
        }
        let r = ctx.content_rect();
        l.push(format!("Window: {:.0}×{:.0} at {:.2}x", r.width(), r.height(), ctx.pixels_per_point()));
        l.join("\n")
    }

    fn about_dialog(&mut self, ctx: &egui::Context) {
        if !self.about {
            return;
        }
        let r = egui::Modal::new("about".into()).show(ctx, |ui| {
            ui.set_width(520.0f32.min(ctx.content_rect().width() - 40.0));
            ui.heading("Cove");
            ui.label(RichText::new("Scientific software, from the kernel to the application: one R session, with Python inside it through reticulate.").color(theme::muted()));
            #[cfg(not(target_arch = "wasm32"))]
            ui.label(RichText::new(format!("v{} · {}", update::VERSION, update::platform())).monospace().size(11.5));
            for lang in Lang::CONSOLES {
                let b = self.ide.banner(lang);
                ui.label(RichText::new(if b.is_empty() { format!("{}: starting…", lang.word()) } else { b }).monospace().size(11.5).color(theme::text()));
            }
            ui.add_space(6.0);
            egui::ScrollArea::vertical().max_height((ctx.content_rect().height() - 240.0).clamp(60.0, 300.0)).show(ui, pimped);
            ui.add_space(6.0);
            ui.button("Close").clicked()
        });
        if r.inner || r.should_close() {
            self.about = false;
        }
    }

    fn toasts(&mut self, ctx: &egui::Context) {
        let notes: Vec<String> = std::mem::take(&mut *self.notes.lock().unwrap());
        for e in notes.into_iter().chain(self.ide.take_errors()) {
            self.toast = Some((e, ctx.input(|i| i.time) + 8.0));
        }
        if let Some((msg, until)) = &self.toast {
            if ctx.input(|i| i.time) > *until {
                self.toast = None;
            } else {
                egui::Area::new("toast".into()).anchor(egui::Align2::RIGHT_BOTTOM, [-20.0, -20.0]).show(ctx, |ui| {
                    egui::Frame::new().fill(theme::surface_hi()).stroke(egui::Stroke::new(1.0, theme::pane_brd())).corner_radius(0).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                        ui.set_max_width(420.0);
                        ui.label(msg.as_str())
                    });
                });
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }
    }
}

/// Narrower than this (a phone, a split screen) the IDE shows one pane at a
/// time; RStudio's quadrants need about two 380 px columns.
const COMPACT_W: f32 = 760.0;

/// This page is a frame inside a page of the same site - Cove shown in
/// Cove's own App pane. A frame inside another site cannot read that site's
/// address, and is not this case. Asked once.
fn inside_cove() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        static INSIDE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *INSIDE.get_or_init(|| {
            use wasm_bindgen::JsValue;
            let Some(w) = web_sys::window() else { return false };
            let Ok(Some(top)) = w.top() else { return false };
            if JsValue::from(top.clone()) == JsValue::from(w.clone()) {
                return false;
            }
            matches!((top.location().host(), w.location().host()), (Ok(a), Ok(b)) if a == b)
        })
    }
    #[cfg(not(target_arch = "wasm32"))]
    false
}

fn compact(ctx: &egui::Context) -> bool {
    ctx.content_rect().width() < COMPACT_W
}

/// Help › Send a request, being written - or, from the atlas's Add software,
/// a request for a piece of software (`software`).
/// Where a request for the stack itself is filed: the ndexr CLI's domain,
/// whose crate builds every package and module (stack/, pkgs).
const STACK_DOMAIN: &str = "ndexr.ndexr.io";

#[derive(Default)]
struct RequestDraft {
    what: String,
    trying: String,
    blocked: bool,
    sending: bool,
    software: bool,
    name: String,
    version: String,
    /// Index into SOFTWARE_KINDS.
    kind: usize,
    /// Pasted, dropped or picked: screenshots, recordings, documents.
    files: Vec<crate::api::Attachment>,
    /// The person allowed the system information to go with it (`system_info`);
    /// off until they tick it, each request.
    system: bool,
}

/// What a requested piece of software is, as the stack files it.
const SOFTWARE_KINDS: [&str; 5] = ["R package", "Python package", "library or tool (EasyBuild)", "system package (Gentoo Prefix)", "not sure"];

impl RequestDraft {
    /// The request as it is filed: what is asked for, then the person's words.
    fn filed_what(&self) -> String {
        if !self.software {
            return self.what.clone();
        }
        let ver = if self.version.trim().is_empty() { String::new() } else { format!(" {}", self.version.trim()) };
        let mut s = format!("Add software: {}{ver} ({})", self.name.trim(), SOFTWARE_KINDS[self.kind.min(SOFTWARE_KINDS.len() - 1)]);
        if !self.what.trim().is_empty() {
            s.push_str("\n\n");
            s.push_str(self.what.trim());
        }
        s
    }

    fn ready(&self) -> bool {
        if self.files.iter().map(|f| f.size()).sum::<usize>() > crate::api::REQUEST_FILES_MAX {
            return false;
        }
        if self.software { !self.name.trim().is_empty() } else { !self.what.trim().is_empty() }
    }
}

/// Where Cove's source is public (operator, 2026-11-14: "cove should display
/// ... the gitlab repo ... for cove.ndexr.io should be public and accessible,
/// and actually maybe we dont need github"): a repository of its own,
/// written by onprem/public-export.sh - this folder less what is private to
/// the platform. The domain's own repository (ndexr/domains/...) stays
/// private: its history holds phone photos and a work address. On GitHub
/// (operator, 2026-11-13: "lets move it into github simply because thats the
/// most popular location for code ... we track it with github not gitlab"):
/// the public history moved whole from gitlab.com/ndexr/cove, which is no
/// longer pushed to.
pub const SOURCE_URL: &str = "https://github.com/fdrennan/cove";

/// Global Options' categories, as RStudio's sidebar: (name, what is on it).
const OPTION_PAGES: [(&str, &str); 3] = [
    ("General", "which R, Python and shell modules · the project folder"),
    ("Environment", "the ndexr stack on CernVM-FS · your proxy and cache"),
    ("About", "what Cove does differently, on purpose"),
];

/// What Cove does differently, on purpose: the choices Inlet's
/// series made for RStudio, which Cove makes in its own code (Help › About).
fn pimped(ui: &mut egui::Ui) {
    let locks: [(&str, &str); 9] = [
        ("Workspace", "R starts with --no-restore --no-save: .RData is never loaded, never written, and nobody is asked. A session never starts from, or leaves behind, hidden state."),
        ("History", "Commands live in this window only (History, ↑ / ↓). Nothing is written to .Rhistory or ~/.python_history."),
        ("Packages", "CRAN over https only, pinned to the module's dated snapshot — or packagemanager.posit.co/cran/latest when the R sets none."),
        ("Interpreters", "From the stack's Lmod modules (env/, R/, Python/) — the same builds as RStudio-Server, code-server and the cells. The console reports its module."),
        ("Off-site", "No Posit Assistant, no Copilot, no Publish or RPubs, no update or crash pings."),
        ("Plots", "Drawn to files and shown in Plots: R through its display list (base, lattice, ggplot2), Python through matplotlib's Agg."),
        ("Environment", "The ndexr stack on CernVM-FS, set up in the background where it is missing: the person sets a proxy and a cache, never a repository or a path."),
        ("Python", "Inside the R session, through reticulate - never a second interpreter, never rpy2. R starts with RETICULATE_PYTHON at its module's python3 and reticulate attached and initialised; .py files and {python} chunks run in that same Python, where r.x is R's x and py$x is Python's."),
        ("Theme", "ndexr Dark: warm-neutral ground, signal-blue keywords."),
    ];
    for (what, how) in locks {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("✓").color(theme::ok()).strong());
            ui.label(RichText::new(what).font(theme::display(13.0)).color(theme::text_strong()));
        });
        ui.label(RichText::new(how).size(12.0).color(theme::muted()));
        ui.add_space(2.0);
    }
    // Cove is its own program: no RStudio code is in it (checked 2026-10-28,
    // every literal against rstudio/rstudio v2026.04.0+526), so RStudio's
    // AGPL does not reach it, and RStudio's name is Posit's, not ours to use.
    ui.label(RichText::new("Cove is MIT-licensed and contains no RStudio code. RStudio and Positron are trademarks of Posit Software, PBC; Cove is not affiliated with Posit.").size(10.5).color(theme::hush()));
    // The notices of the 469 crates Cove is built from (Cargo.lock), which their
    // licences ask to travel with every copy of Cove.
    ui.horizontal_wrapped(|ui| {
        ui.hyperlink_to(RichText::new("Cove's source code ↗").size(12.5), SOURCE_URL).on_hover_text(SOURCE_URL);
        ui.label(RichText::new("·").color(theme::text()));
        ui.hyperlink_to(RichText::new("Third-party notices").size(12.5), "https://cove.ndexr.io/view/third-party-notices");
    });
}

/// Where every module comes from: the ndexr stack on CVMFS, and the
/// revision this machine's client is on (it follows new ones by itself).
fn stack_line(ui: &mut egui::Ui, s: &api::StackView) {
    let repo = if s.repo.is_empty() { "stack.ndexr.io".to_owned() } else { s.repo.clone() };
    let (text, color) = if s.ok {
        (format!("{repo} at {} · revision {}", s.root, if s.revision.is_empty() { "?" } else { &s.revision }), theme::muted())
    } else if s.triples.is_empty() {
        (format!("{repo} is not mounted at {}", s.root), theme::warn())
    } else {
        (format!("{repo} revision {}: {} has no modules right now (mid-publish?) - back within the hour", s.revision, s.mine), theme::warn())
    };
    let tip = [format!("platforms: {}", if s.triples.is_empty() { "none".to_owned() } else { s.triples.join(", ") }), s.updated.clone()]
        .into_iter()
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    ui.label(RichText::new(text).monospace().size(11.0).color(color)).on_hover_text(tip);
}

/// The module picker - the toolbar's and Global Options' alike: env/ modules
/// first (R + Python + a site library), then R-only and Python-only ones.
/// The environment: an env/ set - R and Python together, R alone or Python
/// alone, or a purpose set - that has a package library (host.rs `modules`).
/// Operator, 2026-10-15: "user should be able to select any of the lmod
/// modules but must have an R/py env combo": the env is the base, and any
/// other module layers on it (`extra_menu`). `current` is the module list
/// the kernels load, the env first; this picks only that first name. A bare
/// R/ or Python/ is not a base any more - it lacks the set's packages - and
/// "none" is offered only where the stack has no env at all.
fn module_menu(ui: &mut egui::Ui, salt: &str, current: &mut String, modules: &Option<Modules>, width: f32) {
    const NONE: &str = "none - R and python3 on PATH";
    let (base, extra) = split_modules(current);
    let mut pick = base.clone();
    let shown = if base.is_empty() { NONE.to_owned() } else { base.clone() };
    egui::ComboBox::from_id_salt(salt).width(width).height(420.0).selected_text(RichText::new(shown).monospace().size(12.0)).show_ui(ui, |ui| {
        match modules {
            None => {
                ui.spinner();
            }
            Some(m) => {
                let group = |ui: &mut egui::Ui, title: &str, names: Vec<&String>, pick: &mut String| {
                    if names.is_empty() {
                        return;
                    }
                    ui.label(RichText::new(title).size(10.5).color(theme::hush()));
                    for name in names {
                        ui.selectable_value(pick, name.clone(), RichText::new(name).monospace());
                    }
                };
                let of = |p: &dyn Fn(&str) -> bool| m.env.iter().filter(|n| p(n.as_str())).collect::<Vec<_>>();
                let combo = |n: &str| n.contains("_py");
                let r_only = |n: &str| n.starts_with("env/R/");
                let py_only = |n: &str| n.starts_with("env/Python/");
                let rs_only = |n: &str| n.starts_with("env/Rust/");
                // Environments, their own layer: one language's set each, or
                // composed of an R and a Python one (operator, 2026-10-18).
                group(ui, "Composed environments · R + Python", of(&combo), &mut pick);
                group(ui, "R environments", of(&r_only), &mut pick);
                group(ui, "Python environments", of(&py_only), &mut pick);
                group(ui, "Rust environments", of(&rs_only), &mut pick);
                group(ui, "Purpose sets", of(&|n| !combo(n) && !r_only(n) && !py_only(n) && !rs_only(n)), &mut pick);
                if m.env.is_empty() {
                    ui.label(RichText::new(if m.source.is_empty() { "no env/ set with a package library on this machine" } else { &m.source }).size(11.0).color(theme::warn()));
                    ui.separator();
                    ui.selectable_value(&mut pick, String::new(), RichText::new(NONE).monospace());
                }
            }
        }
    });
    if pick != base {
        *current = join_modules(&pick, &extra);
    }
}

/// The env first, then what layers on it: "env/r4.6_py3.13-release GDAL/3.10.0".
fn split_modules(list: &str) -> (String, Vec<String>) {
    let mut w = list.split_whitespace();
    let first = w.next().unwrap_or("").to_owned();
    if first.starts_with("env/") || first.is_empty() {
        (first, w.map(str::to_owned).collect())
    } else {
        // An older pick with no env first: everything layers, no base yet.
        (String::new(), list.split_whitespace().map(str::to_owned).collect())
    }
}

fn join_modules(base: &str, extra: &[String]) -> String {
    std::iter::once(base).chain(extra.iter().map(String::as_str)).filter(|m| !m.is_empty()).collect::<Vec<_>>().join(" ")
}

/// Modules layered on the env, any of the stack's (Lmod loads them after it,
/// in the order picked): a filter and a tick per module.
fn extra_menu(ui: &mut egui::Ui, current: &mut String, modules: &Option<Modules>, filter: &mut String) {
    let (base, mut extra) = split_modules(current);
    let label = if extra.is_empty() { "+ modules".to_owned() } else { format!("+ {}", extra.len()) };
    let tip = if extra.is_empty() { "layer other modules on the environment (GDAL, CmdStan, Apptainer …)".to_owned() } else { format!("layered on the environment: {}", extra.join(" ")) };
    let mut changed = false;
    ui.menu_button(RichText::new(label).size(12.0).color(theme::muted()), |ui| {
        ui.set_max_height(420.0);
        ui.add(egui::TextEdit::singleline(filter).hint_text("filter modules").desired_width(260.0));
        if !extra.is_empty() && ui.button("clear all").clicked() {
            extra.clear();
            changed = true;
        }
        ui.separator();
        let Some(m) = modules else {
            ui.spinner();
            return;
        };
        let f = filter.to_ascii_lowercase();
        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
            for name in m.other.iter().chain(&m.r).chain(&m.python).filter(|n| f.is_empty() || n.to_ascii_lowercase().contains(&f)).take(400) {
                let mut on = extra.contains(name);
                if ui.checkbox(&mut on, RichText::new(name).monospace().size(12.0)).changed() {
                    if on {
                        extra.push(name.clone());
                    } else {
                        extra.retain(|x| x != name);
                    }
                    changed = true;
                }
            }
        });
    })
    .response
    .on_hover_text(tip);
    if changed {
        *current = join_modules(&base, &extra);
    }
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).font(theme::display(14.0)).color(theme::text_strong()));
    ui.add_space(2.0);
}

/// Nothing of the stack on this machine: not mounted, nothing published under
/// the root. A machine mid-publish is not missing it.
fn stack_missing(s: &api::StackView) -> bool {
    s.unsupported.is_empty() && s.triples.is_empty() && !s.ok
}

fn start_stack_job(backend: &Backend, ctx: &egui::Context, q: Query, job: Arc<Mutex<JobView>>, notes: Arc<Mutex<Vec<String>>>) {
    let what = if matches!(q, Query::StackSetup) { "Setting up the ndexr stack" } else { "Applying CVMFS settings" };
    *job.lock().unwrap() = JobView { what: what.into(), running: true, ..Default::default() };
    backend.query(q, ctx, move |r: Result<JobView, String>| match r {
        Ok(v) => *job.lock().unwrap() = v,
        Err(e) => {
            let mut j = job.lock().unwrap();
            j.running = false;
            j.ok = Some(false);
            j.lines.push(e.clone());
            notes.lock().unwrap().push(e);
        }
    });
}

/// Global Options › Environment: the stack (ours, read-only), the client
/// that brings it, and the person's proxy and cache for that client.
#[allow(clippy::too_many_arguments)]
fn environment(ui: &mut egui::Ui, s: &api::StackView, c: &mut CvmfsSettings, job: &JobView, test: &Option<Result<Vec<ProxyCheck>, String>>, testing: bool, setup: &mut bool, run_test: &mut bool) {
    ui.label(RichText::new("R, Python and every module come from the ndexr stack: stack.ndexr.io on CernVM-FS, read-only, at the path it was built for. The repository is ours; the proxy and cache are yours.").size(12.0).color(theme::muted()));
    ui.add_space(4.0);
    if !s.unsupported.is_empty() {
        ui.label(RichText::new(&s.unsupported).size(12.0).color(theme::warn()));
        return;
    }
    ui.label(RichText::new(format!("This is Linux on {}, a platform the stack is built for: besides mounting it, you can build the environments on this machine from source (Software › Install).", std::env::consts::ARCH)).size(11.5).color(theme::muted()));
    ui.add_space(4.0);
    egui::Grid::new("stack-grid").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label("Stack");
        stack_line(ui, s);
        ui.end_row();
        ui.label("Prefix");
        ui.label(RichText::new(if s.prefix.is_empty() { "not reachable yet".to_owned() } else { s.prefix.clone() }).monospace().size(11.0).color(theme::muted()))
            .on_hover_text("the Gentoo Prefix every module is built on: its own C library, loader and Lmod. The Shell console works inside it.");
        ui.end_row();
        ui.label("Client");
        let client = if s.client { "CernVM-FS client installed" } else { "no CernVM-FS client on this machine" };
        ui.label(RichText::new(client).size(11.5).color(if s.client { theme::muted() } else { theme::warn() }));
        ui.end_row();
    });
    if stack_missing(s) || job.running || job.ok == Some(false) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let can = !s.root_by.is_empty() && !job.running;
            *setup = ui.add_enabled(can, theme::primary_button("Set up the stack")).on_hover_text(format!("runs `{}` in the background", s.setup_cmd)).clicked();
            let how = match s.root_by.as_str() {
                "root" | "sudo" => "no prompt needed here".to_owned(),
                "pkexec" => "asks for an admin password once".to_owned(),
                _ => format!("this machine needs an admin to run `{}` once", s.setup_cmd),
            };
            ui.label(RichText::new(how).size(11.0).color(theme::hush()));
        });
    }
    if !job.what.is_empty() {
        ui.add_space(4.0);
        let state = match (job.running, job.ok) {
            (true, _) => "running…",
            (false, Some(true)) => "done",
            (false, Some(false)) => "failed",
            _ => "",
        };
        ui.label(RichText::new(format!("{} {state}", job.what)).size(11.5).color(if job.ok == Some(false) { theme::warn() } else { theme::muted() }));
        egui::Frame::new().fill(theme::surface()).inner_margin(6).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            theme::log_scroll(egui::ScrollArea::vertical().id_salt("stack-job").max_height(140.0).stick_to_bottom(true), ui, |ui| {
                let from = job.lines.len().saturating_sub(200);
                theme::selectable_lines(ui, &job.lines[from..], egui::FontId::monospace(10.5), theme::text(), true);
            });
        });
    }
    if !s.config.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new("In force on this machine (cvmfs_config showconfig)").size(11.5).color(theme::text_strong()));
        // The value column gets the room left over (it was cut to "http:…").
        let val_w = (ui.available_width() - 360.0).max(180.0);
        egui::Grid::new("cvmfs-config").num_columns(3).striped(true).spacing([10.0, 3.0]).show(ui, |ui| {
            for l in &s.config {
                ui.label(RichText::new(&l.key).monospace().size(11.0).color(theme::primary()));
                ui.add_sized([val_w, 16.0], egui::Label::new(RichText::new(&l.value).monospace().size(11.0).color(theme::text())).truncate()).on_hover_text(&l.value);
                ui.label(RichText::new(l.from.rsplit('/').next().unwrap_or("")).size(10.5).color(theme::hush())).on_hover_text(&l.from);
                ui.end_row();
            }
        });
    }
    ui.add_space(8.0);
    ui.label(RichText::new("Your client settings").size(11.5).color(theme::text_strong()));
    egui::Grid::new("cvmfs-own").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        ui.label("HTTP proxy");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut c.proxy).font(egui::TextStyle::Monospace).hint_text("DIRECT · or http://squid.site:3128;DIRECT").desired_width((ui.available_width() - 70.0).clamp(240.0, 520.0)));
            *run_test = ui.add_enabled(!testing, egui::Button::new("Test")).on_hover_text("fetch the repository's manifest from each of our servers through it").clicked();
        });
        ui.end_row();
        ui.label("Cache folder");
        ui.add(egui::TextEdit::singleline(&mut c.cache_base).font(egui::TextStyle::Monospace).hint_text("the client's default (/var/lib/cvmfs)").desired_width((ui.available_width() - 10.0).clamp(240.0, 520.0)));
        ui.end_row();
        ui.label("Cache size");
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut c.quota_mb).font(egui::TextStyle::Monospace).hint_text("the client's default").desired_width(120.0));
            ui.label(RichText::new("MB").size(11.5).color(theme::muted()));
        });
        ui.end_row();
    });
    if testing {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("trying our servers…").size(11.5).color(theme::muted()));
        });
    }
    match test {
        Some(Ok(v)) => {
            for t in v {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(if t.ok { "✓" } else { "✗" }).color(if t.ok { theme::ok() } else { theme::warn() }).strong());
                    ui.label(RichText::new(format!("{} → {}", t.proxy, t.server)).monospace().size(10.5).color(theme::muted()));
                    ui.label(RichText::new(&t.said).size(11.0).color(theme::text()));
                });
            }
        }
        Some(Err(e)) => {
            ui.label(RichText::new(e).size(11.5).color(theme::warn()));
        }
        None => {}
    }
    let where_ = if s.client {
        "OK writes them to /etc/cvmfs/config.d/stack.ndexr.io.local (the client's own override file) and reloads the client; a new cache folder is used from the next mount."
    } else {
        "They are handed to the client when the stack is set up."
    };
    ui.label(RichText::new(where_).size(10.5).color(theme::hush()));
}

impl App {
    /// The IDE: RStudio's frame, the windows.
    fn ide_view(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        let bar = |fill| egui::Frame::new().fill(fill).inner_margin(egui::Margin::symmetric(6, 2));
        // The app's controls are the App pane's, right above its site
        // (operator, 2026-11-14: "node/start/build/logs should go into the
        // app sub menu ... where the app is running right above its viewer
        // and we should remove that top second row"); this keeps its state.
        if self.ide.domain.is_some() {
            self.ide.app_strip(ctx);
        }
        let strip = {
            let job = self.stack_job.lock().unwrap();
            job.running || job.ok == Some(false) || self.settings.lock().unwrap().as_ref().is_some_and(|v| stack_missing(&v.stack))
        };
        if strip {
            egui::Panel::top("rs-stack").resizable(false).show_separator_line(true).exact_size(26.0).frame(bar(theme::surface())).show(root, |ui| self.stack_strip(ui, ctx));
        }
        if self.storage.lock().unwrap().as_ref().is_some_and(api::StorageView::exceeded) {
            egui::Panel::top("rs-storage").resizable(false).show_separator_line(true).exact_size(26.0).frame(bar(theme::surface())).show(root, |ui| self.storage_strip(ui, ctx));
        }
        egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(theme::WIN_GAP)).show(root, |ui| self.quadrants(ui, ctx));
    }

    /// A star's source is ready: into the IDE, its workspace in Files.
    fn poll_source(&mut self, ctx: &egui::Context) {
        let got = self.source_ready.lock().unwrap().take();
        match got {
            // A workspace from before projects had domains: a folder.
            Some(Ok(v)) if v.host.is_empty() => {
                self.mode = Mode::Ide;
                self.saved.zoom_tab = None;
                self.ide.open_folder(&v.path, ctx);
                self.ide.bring(Tab::Files);
                let how = if v.fresh { "fetched" } else { "opened again - your work is as you left it" };
                self.toast(format!("{} ({how}). Branch upstream is the source as it came; change it on work.", v.path), ctx);
            }
            // A project with a domain of its own: registered as theirs (the
            // paradigm parent first), then opened as a domain like any other.
            Some(Ok(v)) => {
                self.mode = Mode::Ide;
                self.saved.zoom_tab = None;
                let slot = self.claim_ready.clone();
                self.backend.query(Query::DomainClaim { hosts: vec![v.parent.clone(), v.host.clone()] }, ctx, move |r: Result<Vec<String>, String>| {
                    *slot.lock().unwrap() = Some(r);
                });
                let d = crate::api::DomainInfo { host: v.host.clone(), path: v.path.clone(), title: String::new(), ..Default::default() };
                self.ide.open_domain(d, ctx);
                self.ide.bring(Tab::Files);
                let how = if v.fresh { "fetched into it" } else { "opened again - your work is as you left it" };
                self.toast(format!("{} ({how}). Each version is two branches: upstream/<version> as it came, work/<version> to change. Save keeps every version.", v.host), ctx);
            }
            Some(Err(e)) => self.toast(format!("source: {e}"), ctx),
            None => {}
        }
        let claimed = self.claim_ready.lock().unwrap().take();
        if let Some(r) = claimed {
            match r {
                Ok(hosts) => {
                    self.ide.refresh_domains(ctx);
                    // The project itself goes up with its starter page (its
                    // paradigm parent is a name, not a site).
                    if let Some(h) = hosts.last() {
                        self.ide.publish_domain(h.clone(), ctx);
                    }
                }
                Err(e) => self.toast(format!("the project is open here, but its domain is not registered to you yet: {e}"), ctx),
            }
        }
    }

    /// The one header every view draws through (operator, 2026-10-26:
    /// "Build one shared workspace shell that receives contextual actions from
    /// IDE, Go, Builds, Packages, and Atlas"). The views keep their own bodies;
    /// what sits around them is decided here, by the view and the width:
    ///
    /// - desk: one 40 px row - ☰, the modes, and in the IDE the project, its
    ///   agent, the runtime, Claude and Save; Messages and Help at the end.
    ///   It replaces the 30 px mode bar plus the IDE's 28 px toolbar.
    /// - phone: 44 px on top (☰, the project, the IDE's panes) and the modes as
    ///   a bottom bar; Save, Messages, Help, the runtime and the agent's scope
    ///   are in ☰. It replaces four rows (mode, project, toolbar, panes).
    /// The top of the window (operator, 2026-11-14: "we need to be able to
    /// move around and reorganize the tabs, should be the top most thing and
    /// the rest of the menubar goes into the side button on top right"): one
    /// row, the project tabs (ide.rs tab_strip) and ⋮ at its right - the Cove
    /// menu (menu_items), which holds the modes, Messages, Help, Save, what is
    /// running and the rest. In every mode: a tab chosen from Atlas or Builds
    /// brings the IDE back on it. On a phone the modes stay along the bottom,
    /// where a thumb reaches them, and the IDE's Agents / App switcher sits
    /// beside ⋮.
    fn header(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        let small = compact(ctx);
        let ide = self.mode == Mode::Ide;
        if small {
            // The modes where a thumb reaches them, each a 44-pt target
            // (operator, 2026-11-13: "padding and margin that could be
            // decreased slightly for mobile").
            let nav_frame = egui::Frame::new().fill(theme::ground().gamma_multiply(0.9)).inner_margin(egui::Margin::symmetric(3, 2));
            egui::Panel::bottom("rs-nav").resizable(false).show_separator_line(true).exact_size(44.0).frame(nav_frame).show(root, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.spacing_mut().button_padding.x = 4.0;
                    // Five on a phone: Go is a desk view (operator, 2026-11-07).
                    let tab = (ui.available_width() - 4.0 * 2.0) / 5.0;
                    self.mode_tabs(ui, true, tab);
                    crate::tour::mark(ui, "modes", ui.min_rect());
                });
            });
        }
        let frame = egui::Frame::new().fill(theme::ground()).inner_margin(egui::Margin { left: 6, right: 4, top: 3, bottom: 0 });
        let mut chosen = false;
        egui::Panel::top("rs-top").resizable(false).show_separator_line(true).exact_size(if small { 40.0 } else { 34.0 }).frame(frame).show(root, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                // The menu button first, so it is always on screen: Cove's
                // own mark (operator, 2026-11-14: "tab on top right should
                // have a cove icon"; three dots before it). No unread count
                // here: Messages at the bottom left carries it (operator,
                // 2026-11-20: "doesnt need the number on the top right as we
                // have the number next to messages in the bottom left").
                let side = if small { 26.0 } else { 22.0 };
                let btn = egui::Button::new("").frame(false).min_size(egui::vec2(side + if small { 10.0 } else { 8.0 }, if small { 34.0 } else { 28.0 }));
                let (r, _) = egui::containers::menu::MenuButton::from_button(btn).ui(ui, |ui| self.menu_items(ui, ctx));
                crate::tour::mark(ui, "cove-menu", r.rect);
                let c = egui::pos2(r.rect.right() - 4.0 - side / 2.0, r.rect.center().y);
                match crate::frameworks::cove_mark(ctx) {
                    Some(t) => {
                        let at = egui::Rect::from_center_size(c, egui::vec2(side, side));
                        egui::Image::new(egui::load::SizedTexture::new(t.id(), at.size())).corner_radius(5).paint_at(ui, at);
                    }
                    None => {
                        for dy in [-5.5, 0.0, 5.5] {
                            ui.painter().circle_filled(c + egui::vec2(0.0, dy), if small { 2.2 } else { 1.9 }, theme::text_strong());
                        }
                    }
                }
                r.on_hover_text("Cove menu: tabs, the project and file, view, the console, settings and help");
                // Full screen at hand, left of the menu and not only inside
                // it (operator, 2026-11-20: "need a full screen pusher left of
                // the cove icon in the top right, not hidden in the menu").
                let full = is_fullscreen(ctx);
                let (g, tip) = if full { (crate::icons::Glyph::Collapse, "Exit full screen (F11)") } else { (crate::icons::Glyph::Expand, "Full screen (F11)") };
                if crate::icons::tool(ui, g, None, tip).clicked() {
                    if let Err(why) = toggle_fullscreen(ctx) {
                        self.toast(why, ctx);
                    }
                }
                // The modes, always on screen at the desk (the review: "global
                // navigation" its own level, not inside a menu). In their own
                // left-to-right box, so the row's right-to-left order does not
                // reverse them.
                if !small {
                    theme::tool_sep(ui);
                    let font = egui::FontId::proportional(12.5);
                    let pad = ui.spacing().button_padding.x * 2.0 + ui.spacing().item_spacing.x;
                    let w: f32 = MODES.iter().filter(|m| m.0 != Mode::Go || self.go_is_admin()).map(|(_, n, _)| ui.fonts_mut(|f| f.layout_no_wrap((*n).to_owned(), font.clone(), egui::Color32::WHITE).size().x) + pad).sum::<f32>() + 4.0;
                    let h = ui.available_height();
                    let modes = ui.allocate_ui_with_layout(egui::vec2(w, h), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        self.mode_tabs(ui, false, 0.0);
                    });
                    crate::tour::mark(ui, "modes", modes.response.rect);
                    theme::tool_sep(ui);
                }
                if small && ide {
                    self.pane_switcher(ui);
                }
                if !small && self.mode == Mode::Go && self.go_is_admin() {
                    self.go_recap_bar(ui, ctx);
                    theme::tool_sep(ui);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    // ← and → step between tabs (operator, 2026-11-14), except
                    // where they already mean something: the Atlas walks its
                    // layers with them, and a walkthrough its steps.
                    let keys = self.mode != Mode::Atlas && self.walk.run.is_none();
                    chosen = self.ide.tab_strip(ui, ctx, keys);
                });
            });
        });
        if chosen && !ide {
            self.mode = Mode::Ide;
            self.saved.zoom_tab = None;
        }
    }

    /// The IDE's windows by name, the one in front lit: on a phone only one
    /// shows at a time (compact_panes), and this picks it - in the top bar,
    /// where the 40 px pane bar along the bottom used to be.
    fn pane_switcher(&mut self, ui: &mut egui::Ui) {
        let wins = self.saved.grid.windows();
        for (i, &at) in wins.iter().enumerate() {
            let on = self.compact_cell == i;
            let name = self.saved.grid.win(at).and_then(|w| w.shown()).map_or("", Tab::name);
            let text = RichText::new(name).size(13.0).color(if on { theme::text_strong() } else { theme::muted() });
            if ui.add(egui::Button::selectable(on, text).min_size(egui::vec2(56.0, 36.0))).clicked() {
                self.compact_cell = i;
            }
        }
    }

    fn mode_tabs(&mut self, ui: &mut egui::Ui, small: bool, w: f32) {
        let tabs = MODES;
        // On a phone every tab is `w` wide, and the widest name must fit in it
        // with the button's padding: at 14.5 "Account" was wider than its
        // sixth of a phone, so its button grew and the row ran past the edge
        // (operator, 2026-11-03: "account goes slightly over in bottom bar").
        let size = if small {
            let pad = ui.spacing().button_padding.x * 2.0 + 2.0;
            let widest = tabs.iter().filter(|t| t.0 != Mode::Go).map(|(_, n, _)| ui.ctx().fonts_mut(|f| f.layout_no_wrap((*n).to_owned(), egui::FontId::proportional(14.5), egui::Color32::WHITE).size().x)).fold(0.0, f32::max);
            (14.5 * ((w - pad) / widest).min(1.0)).max(9.0)
        } else {
            12.5
        };
        for (m, name, tip) in tabs {
            if (small || !self.go_is_admin()) && m == Mode::Go {
                continue;
            }
            let on = m == self.mode;
            let t = RichText::new(name).size(size).color(if on { theme::text_strong() } else { theme::muted() });
            let b = egui::Button::selectable(on, t).min_size(egui::vec2(w, if small { 34.0 } else { 0.0 }));
            if ui.add(b).on_hover_text(tip).clicked() && !on {
                self.mode = m;
                if m == Mode::Ide {
                    self.saved.zoom_tab = None;
                }
            }
        }
    }

    /// Help's entries: in the header, and in the Cove menu on a phone.
    fn help_items(&mut self, ui: &mut egui::Ui) {
        if ui.add(egui::Button::new(RichText::new("Send a request - what you need or what is broken").size(13.0))).clicked() && self.request.is_none() {
            self.request = Some(Default::default());
            ui.close();
        }
        ui.separator();
        ui.label(RichText::new("Show me how").size(12.0).strong().color(theme::text()));
        ui.add(egui::TextEdit::singleline(&mut self.walk.find).hint_text("a task: run my app, share it, add a package…").desired_width(f32::INFINITY));
        let hits = crate::tour::search(&self.walk.find);
        let mut start = None;
        for t in hits {
            if ui.add(egui::Button::new(RichText::new(format!("▶ {}", t.title)).size(13.0)).min_size(egui::vec2(0.0, 26.0))).clicked() {
                start = Some(t.id);
            }
        }
        if self.walk.find.trim().len() > 1 && crate::tour::search(&self.walk.find).is_empty() {
            ui.label(RichText::new("No walkthrough for that yet - Send a request asks us.").size(12.0).color(theme::hush()));
        }
        ui.separator();
        let mut page = None;
        for (p, name) in [(crate::tour::Page::Shortcuts, "Keyboard shortcuts"), (crate::tour::Page::Troubleshooting, "Troubleshooting"), (crate::tour::Page::Documentation, "Documentation")] {
            if ui.add(egui::Button::new(RichText::new(name).size(13.0))).clicked() {
                page = Some(p);
            }
        }
        if ui.add(egui::Button::new(RichText::new("Cove's source code ↗").size(13.0))).on_hover_text(SOURCE_URL).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(SOURCE_URL));
            ui.close();
        }
        if let Some(id) = start {
            self.walk.find.clear();
            self.start_tour(id);
            ui.close();
        }
        if let Some(p) = page {
            self.walk.page = Some(p);
            ui.close();
        }
    }

    /// The IDE's state for the walkthrough, with what only the shell knows:
    /// whether the App window is on screen, and a phone-width window.
    fn tour_seen(&self, ctx: &egui::Context) -> crate::tour::Seen {
        let mut seen = self.ide.tour_seen();
        let ide_shown = self.mode == Mode::Ide;
        seen.app_shown = ide_shown && crate::tour::marked(ctx, "pane:App").is_some();
        seen.narrow = ctx.content_rect().width() < 700.0;
        seen
    }

    /// A walkthrough from the top, in the IDE: the other modes step aside so
    /// its controls are on screen. The workspace itself is left as it is.
    fn start_tour(&mut self, id: &'static str) {
        self.mode = Mode::Ide;
        let seen = self.ide.tour_seen();
        self.walk.start(id, &seen);
    }

    /// What a walkthrough asks of the IDE: only what a person could do.
    fn tour_act(&mut self, a: crate::tour::Act, ctx: &egui::Context) {
        use crate::tour::Act;
        match a {
            Act::NewProject => {
                self.mode = Mode::Ide;
                self.ide.start_new_project();
            }
            Act::Recommend(s) => self.ide.choose_starter(s),
            Act::Bring(t) => {
                self.mode = Mode::Ide;
                self.ide.bring(t);
            }
            Act::Open(p) => self.ide.open_file(&p, ctx),
            Act::AppLog => self.ide.show_app_log(),
        }
    }

    /// Go (operator, 2026-10-18: "an on-the-go mode ... a more focused screen
    /// to where we can choose to start or stop those boxes to build that
    /// software. Swapping through the agent easily and the executive agent
    /// easily, and being able to basically push a big button to record, and
    /// then submit that"). The machines first - for an admin only; anyone
    /// else's answer is a refusal and the section is not drawn - then the
    /// agent or Exec with the big record bar.
    fn go_view(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        self.poll_boxes(ctx);
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::bg().gamma_multiply(0.9)).inner_margin(egui::Margin::symmetric(10, 8))).show(root, |ui| {
            // One centred column, phone-wide at most (operator, 2026-10-18:
            // "cove go looks bad on desktop" - stretched across a wide screen,
            // Start sat a metre from the machine it starts).
            // Short (a laptop zoomed in with Ctrl +, operator 2026-10-18: the
            // record button and Send were below the bottom edge): the column
            // scrolls, and the agent keeps a usable height under the recap -
            // the room left when there is that much, so nothing is cut off.
            // The build machines are Builds' (machines_panel), not Go's.
            let min_agent = 240.0;
            egui::ScrollArea::vertical().id_salt("go-column").auto_shrink([false, false]).show(ui, |ui| {
            // Measured inside the scroll area, less the row's own spacing: taken
            // outside it, three cards ran past a phone's edge (IMG_8269,
            // 2026-10-24: gpu cut off).
            let col = (ui.available_width() - ui.spacing().item_spacing.x).min(760.0);
            let pad = ((ui.available_width() - col) / 2.0 - ui.spacing().item_spacing.x).max(0.0);
            ui.horizontal_top(|ui| {
            ui.add_space(pad);
            ui.vertical(|ui| {
            ui.set_width(col);
            ui.set_max_width(col);
            // The recap first, for an admin (the machines answer only them).
            self.ide.go_agent(ui, ctx, min_agent);
            });
            });
            });
        });
    }

    /// The build machines and the action in flight, polled: the list every
    /// 30 s, and while an action runs its job every 2 s and the list every
    /// 10 s, until it has finished - so Start is watched, not just sent. Go
    /// asks too: the list answering at all is what shows its recap (admins).
    fn poll_boxes(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        if now - self.go_boxes_at > 30.0 {
            self.go_boxes_at = now;
            let slot = self.go_boxes.clone();
            self.backend.query(Query::Boxes, ctx, move |r: Result<Vec<crate::api::BoxView>, String>| {
                *slot.lock().unwrap() = Some(r);
            });
        }
        // The action in flight: its job every 2 s, the machines every 10 s,
        // until it has finished - so Start is watched, not just sent.
        let job_now = self.go_box_job.lock().unwrap().clone();
        if let Some(server) = self.go_job_server.clone() {
            let running = job_now.as_ref().is_none_or(|j| j.running);
            if running && now - self.go_job_poll > 2.0 {
                self.go_job_poll = now;
                let job = self.go_box_job.clone();
                self.backend.query(Query::BoxJob { server }, ctx, move |r: Result<JobView, String>| {
                    if let Ok(v) = r {
                        if !v.what.is_empty() || !v.lines.is_empty() {
                            *job.lock().unwrap() = Some(v);
                        }
                    }
                });
                if now - self.go_boxes_at > 10.0 {
                    self.go_boxes_at = now - 30.0;
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(1000));
        }
    }

    /// The one place build machines are started and shut down (operator,
    /// 2026-10-26: "Builds machines doesn't belong in Go it should be in builds
    /// somewhere clear and obvious, mobile friendly is nice, we should only
    /// start/stop builds in one place"): the top of Builds, a tile per machine -
    /// its dot, name, state and step, Start or Shut down (asked twice), and
    /// the action as it goes - with 36-pt buttons the tile's width. Nothing
    /// for anyone the machines do not answer (only admins).
    fn machines_panel(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.poll_boxes(ctx);
        let now = ctx.input(|i| i.time);
        let job_now = self.go_box_job.lock().unwrap().clone();
        let Some(Ok(bs)) = self.go_boxes.lock().unwrap().clone() else { return };
        let mut act: Option<(String, String)> = None;
        let mut told: Option<(String, bool)> = None;
        let col = ui.available_width();
        ui.horizontal(|ui| {
            ui.label(RichText::new("🖥").size(14.0).color(theme::primary())).on_hover_text("build machines");
            let on = bs.iter().filter(|b| b.up).count();
            ui.label(RichText::new(format!("{on}/{}", bs.len())).size(12.5).color(theme::text())).on_hover_text(format!("{on} of {} build machines on", bs.len()));
        });
        ui.add_space(2.0);
        // One line per machine (operator, 2026-11-06: "too much text and white
        // space ... not enough icons"): dot, name, step, and an icon button that
        // starts or shuts it down - the words are its hover. As many to a row as
        // fit at 150 points; five in a line on a desk, two or three on a phone.
        let gap = 6.0;
        let per_row = ((col + gap) / (150.0 + gap)).floor().clamp(1.0, bs.len().max(1) as f32) as usize;
        let w = (col - gap * (per_row as f32 - 1.0)) / per_row as f32;
        for row in bs.chunks(per_row) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for b in row {
                    let c = match b.state.as_str() {
                        "running" => theme::ok(),
                        "stale" | "failed" => theme::alt(),
                        "starting" => theme::amber(),
                        _ => theme::hush(),
                    };
                    egui::Frame::new()
                        .fill(theme::surface().gamma_multiply(if b.up { 1.0 } else { 0.7 }))
                        .stroke(egui::Stroke::new(1.0, if b.up { c.gamma_multiply(0.7) } else { theme::pane_brd() }))
                        .corner_radius(6)
                        .inner_margin(egui::Margin::symmetric(8, 4))
                        .show(ui, |ui| {
                            ui.set_width(w - 18.0);
                            ui.horizontal(|ui| {
                                let step = if b.steps > 0 && !b.step.is_empty() && b.state != "off" { format!("{}/{}", b.step_index, b.steps) } else { String::new() };
                                ui.label(RichText::new(if b.up { "●" } else { "○" }).size(13.0).color(c)).on_hover_text(&b.state);
                                ui.add(egui::Label::new(RichText::new(b.server.trim_start_matches("four-box-")).size(14.5).strong().color(theme::text_strong())).truncate())
                                    .on_hover_text(if step.is_empty() { b.state.clone() } else { format!("{} · step {step}{}", b.state, if b.step.is_empty() { String::new() } else { format!(": {}", b.step) }) });
                                if !step.is_empty() {
                                    ui.label(RichText::new(&step).size(12.5).monospace().color(c));
                                }
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    let size = egui::vec2(30.0, 26.0);
                                    if !matches!(b.state.as_str(), "running" | "starting") {
                                        if ui.add(theme::primary_button("▶").min_size(size)).on_hover_text("start it").clicked() {
                                            act = Some((b.server.clone(), "up".into()));
                                        }
                                    } else if b.up {
                                        let armed = self.go_confirm.as_deref() == Some(b.server.as_str());
                                        let icon = RichText::new(if armed { "⏹?" } else { "⏹" }).size(14.0).color(if armed { theme::alt() } else { theme::text_strong() });
                                        if ui.add(egui::Button::new(icon).min_size(size)).on_hover_text(if armed { "press again to shut it down" } else { "shut down - its disk stays; press twice" }).clicked() {
                                            if armed {
                                                self.go_confirm = None;
                                                act = Some((b.server.clone(), "shutdown".into()));
                                            } else {
                                                self.go_confirm = Some(b.server.clone());
                                            }
                                        }
                                    }
                                    if self.go_job_server.as_deref() == Some(b.server.as_str()) {
                                        if let Some(j) = &job_now {
                                            let (what, done, failed) = crate::api::start_phase(j, Some(b));
                                            let secs = (now - self.go_job_since).max(0.0) as u64;
                                            let col = if failed { theme::alt() } else if done { theme::ok() } else { theme::amber() };
                                            let r = if done {
                                                ui.label(RichText::new(if failed { "✕" } else { "✓" }).size(14.0).color(col))
                                            } else {
                                                ui.add(egui::Spinner::new().size(13.0).color(col))
                                            };
                                            r.on_hover_text(format!("{what} · {}m {:02}s", secs / 60, secs % 60));
                                            if done && !self.go_job_told {
                                                told = Some((format!("{}: {what}", b.server.trim_start_matches("four-box-")), failed));
                                            }
                                        }
                                    }
                                });
                            });
                        });
                }
            });
            ui.add_space(gap);
        }

        if let Some((msg, failed)) = told {
            self.go_job_told = true;
            self.toast(if failed { msg } else { format!("{msg} ✓") }, ctx);
        }
        if let Some((server, verb)) = act {
            // Shown at once, before the server has even answered.
            *self.go_box_job.lock().unwrap() = Some(JobView { what: format!("{server}: {verb}"), running: true, ..Default::default() });
            self.go_job_server = Some(server.clone());
            self.go_job_since = now;
            self.go_job_poll = now;
            self.go_job_told = false;
            let job = self.go_box_job.clone();
            self.backend.query(Query::BoxAct { server, verb }, ctx, move |r: Result<JobView, String>| {
                *job.lock().unwrap() = Some(r.unwrap_or_else(|e| JobView { what: "box".into(), ok: Some(false), lines: vec![e], ..Default::default() }));
            });
            self.go_boxes_at = now - 25.0;
        }
    }

    /// Go's Recap, in the top line beside Cove (operator, 2026-10-27: "move
    /// the recap button ... into the top line next to cove - always have 4h or
    /// day, remove play the briefing (automatic w time) and the counts").
    /// Pressing a window is the ask: the briefing - what changed and why,
    /// written and voiced on the server (brief.rs) - plays the moment it is
    /// ready, with no text to read. Pressed again while it plays, it stops.
    /// Before 2026-10-18's operator: "push a button and hear ... a voice
    /// summary".
    fn go_recap_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let playing = crate::voice::play::playing();
        let brief = self.go_brief.lock().unwrap().clone();
        let making = matches!(&brief, Some(Ok(b)) if b.state == "writing" || b.state == "recording");
        if playing || making {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        let mut ask = None;
        for (m, w) in [(240, "4 h"), (1440, "day")] {
            let mine = self.go_recap_mins == m;
            let text = match (&brief, mine) {
                _ if mine && playing => format!("■ {}", crate::voice::play::clock()),
                (Some(Ok(b)), true) if b.state == "writing" || b.state == "recording" => format!("{w} …"),
                _ => format!("▶ {w}"),
            };
            let tip = match &brief {
                Some(Ok(b)) if mine && b.state == "failed" => format!("the last briefing could not be made: {}", b.error),
                Some(Err(e)) if mine => e.clone(),
                _ => format!("hear what changed in the last {w}, and why - plays when it is ready"),
            };
            let b = egui::Button::new(RichText::new(text).size(13.0).color(if mine && (playing || making) { theme::bg() } else { theme::text() }))
                .fill(if mine && (playing || making) { theme::primary() } else { egui::Color32::TRANSPARENT })
                .corner_radius(8)
                .min_size(egui::vec2(0.0, 30.0));
            if ui.add_enabled(!(making && !mine), b).on_hover_text(tip).clicked() {
                if mine && playing {
                    crate::voice::play::stop();
                } else if !(mine && making) {
                    ask = Some(m);
                }
            }
        }
        if let Some(m) = ask {
            crate::voice::play::stop();
            if self.go_recap_mins != m {
                self.go_recap_mins = m;
                *self.go_brief.lock().unwrap() = None;
            }
            // A briefing already made for this window replays; otherwise ask.
            match brief.as_ref().filter(|_| self.go_recap_mins == m) {
                Some(Ok(b)) if b.state == "ready" && !b.audio.is_empty() && b.minutes == m => crate::voice::play::start(&b.audio),
                _ => self.go_brief_ask(ctx, now),
            }
        } else if making && now >= self.go_brief_poll {
            self.go_brief_ask(ctx, now);
        }
        // Played the moment it lands: the press was the ask.
        if let Some(Ok(b)) = &brief {
            if b.state == "ready" && !b.audio.is_empty() && b.made_at != self.go_brief_played {
                self.go_brief_played = b.made_at;
                crate::voice::play::start(&b.audio);
            }
        }
    }

    /// Ask the server for the briefing of `go_recap_mins` (it answers with
    /// its state while it writes and records), at most every 4 s.
    /// The machines answer only an admin, and the recap is theirs too; it
    /// is heard, so only where this Cove can play it (the browser's).
    /// The machine's CPU, memory and free disk, for an ndexr admin, as the
    /// ndexr bar shows them (operator, 2026-10-28: "need to show disk space
    /// remaining and computer utilization like we do in the ndexr bar for
    /// admins in cove"). Asked every 10 s; anyone else is refused once and
    /// nothing shows.
    fn vitals_chip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        let refused = matches!(*self.vitals.lock().unwrap(), Some(Err(_)));
        if !refused && now - self.vitals_at > 10.0 {
            self.vitals_at = now;
            let slot = self.vitals.clone();
            self.backend.query(Query::HostVitals, ctx, move |r: Result<crate::api::Vitals, String>| {
                *slot.lock().unwrap() = Some(r);
            });
        }
        if !refused {
            ctx.request_repaint_after(std::time::Duration::from_secs(10));
        }
        let Some(Ok(v)) = self.vitals.lock().unwrap().clone() else { return };
        // Binary GB, as `df -h` and the ndexr bar (handle_snapshot.rs disk)
        // count them: decimal here showed 229 beside the bar's 213 for the
        // same disk (operator, 2026-10-28: "there is a discrepancy").
        let gb = |b: u64| b as f64 / 1024f64.powi(3);
        let free_share = if v.disk_total > 0 { v.disk_free as f64 / v.disk_total as f64 } else { 1.0 };
        let hot = v.cpu_pct > 90.0 || v.mem_pct > 90.0 || free_share < 0.10;
        let text = format!("CPU {:.0}% · Mem {:.0}% · {:.0} GB free", v.cpu_pct, v.mem_pct, gb(v.disk_free));
        theme::tool_sep(ui);
        ui.label(RichText::new(text).monospace().size(11.0).color(if hot { theme::warn() } else { theme::text() })).on_hover_text(format!(
            "this machine: {} cores, {:.0}% busy · memory {:.0}% in use · disk {:.0} GB free of {:.0} GB ({:.0}% used)",
            v.cores,
            v.cpu_pct,
            v.mem_pct,
            gb(v.disk_free),
            gb(v.disk_total),
            100.0 * (1.0 - free_share)
        ));
    }

    fn go_is_admin(&self) -> bool {
        crate::voice::play::supported() && matches!(*self.go_boxes.lock().unwrap(), Some(Ok(_)))
    }

    fn go_brief_ask(&mut self, ctx: &egui::Context, now: f64) {
        use std::sync::atomic::Ordering;
        if self.go_brief_busy.load(Ordering::Relaxed) {
            return;
        }
        self.go_brief_busy.store(true, Ordering::Relaxed);
        self.go_brief_poll = now + 4.0;
        let (slot, flag) = (self.go_brief.clone(), self.go_brief_busy.clone());
        let q = Query::RecapBrief { minutes: self.go_recap_mins, fresh: false };
        self.backend.query(q, ctx, move |r: Result<crate::api::RecapBrief, String>| {
            *slot.lock().unwrap() = Some(r);
            flag.store(false, Ordering::Relaxed);
        });
    }

    /// Builds, full screen: where every build is now, across the machines,
    /// over the Builds views (curves, depth, timeline, queue, logs).
    fn builds_view(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::bg().gamma_multiply(0.92)).inner_margin(egui::Margin::symmetric(10, 8))).show(root, |ui| {
            // At most a little under half the height, scrolling past that: on
            // a phone the machines and the cards took the whole screen and
            // the views below (Curves, Timeline, ...) got a sliver.
            // Sized from what it measured last frame: a scroll area inside a
            // panel only grows into the height the panel already had, so left
            // to itself it stayed a 64-point sliver.
            let cap = (ui.available_height() * 0.45).max(160.0);
            let h = (self.builds_top_h + 6.0).clamp(80.0, cap);
            egui::Panel::top("builds-now").resizable(false).show_separator_line(false).frame(egui::Frame::NONE).exact_size(h).show(ui, |ui| {
                let vw = ui.available_width();
                let out = egui::ScrollArea::vertical().id_salt("builds-now-scroll").auto_shrink([false, false]).show(ui, |ui| {
                    // No wider than the screen: a vertical scroll area lets its
                    // content keep the width of its widest line last frame.
                    ui.set_max_width(vw - 4.0);
                    // This computer's own build first, when the stack is installed here.
                    if self.ide.local_wanted(ctx) {
                        let backend = self.backend.clone();
                        self.ide.local.ui(ui, ctx, &backend, false);
                        ui.add_space(8.0);
                    }
                    // The machines: start and shut them down here, and only here.
                    self.machines_panel(ui, ctx);
                    self.ide.builds_now(ui, ctx);
                });
                if (out.content_size.y - self.builds_top_h).abs() > 1.0 {
                    self.builds_top_h = out.content_size.y;
                    ctx.request_repaint();
                }
            });
            egui::CentralPanel::no_frame().show(ui, |ui| {
                theme::pane(ui, |ui| self.ide.stack(ui, ctx));
            });
        });
    }

    /// Every domain the person may see, for the atlas and Connect: their own
    /// (which open in the IDE), then everyone else's they may visit.
    fn visible_apps(&mut self, ctx: &egui::Context) -> Option<Result<Vec<crate::api::DomainInfo>, String>> {
        // The person's own domains, which open in the IDE, then everyone
        // else's that anyone may visit (operator, 2026-10-17: every
        // non-password domain, all users, in the visuals) - those have no
        // path here, and Enter on one opens the site.
        // Whose canopy: this session's own domains.json (multiuser answers it
        // for the signed-in person, host.rs visible_domains) - login, admin
        // and owner-only sites only for those allowed them, and a site with a
        // password never, not even the person's own (operator, 2026-10-18).
        let url = match &self.backend {
            Backend::Http { base } => format!("{base}/domains.json"),
            #[allow(unreachable_patterns)]
            _ => format!("{}/domains.json", crate::showcase::base()),
        };
        let canopy = self.showcase.canopy(&url, ctx);
        let locked: std::collections::HashSet<String> = canopy.iter().filter(|d| d.target == "password").map(|d| d.host.clone()).collect();
        let who = self.probe.lock().unwrap().as_ref().and_then(|p| p.home.trim_end_matches('/').rsplit('/').next().map(str::to_owned)).unwrap_or_default();
        // Whose each is (the atlas's person view): the registry's owner, and
        // for a domain of the person's own the registry does not list, them.
        let owner_of: std::collections::HashMap<&str, &str> = canopy.iter().filter(|d| !d.owner.is_empty()).map(|d| (d.host.as_str(), d.owner.as_str())).collect();
        let domains = self.ide.domain_list(ctx).map(|r| {
            r.map(|own| {
                let mut own: Vec<_> = own.into_iter().filter(|d| !locked.contains(&d.host)).collect();
                for d in &mut own {
                    d.owner = owner_of.get(d.host.as_str()).map_or_else(|| who.clone(), |o| (*o).to_owned());
                }
                let mine: std::collections::HashSet<String> = own.iter().map(|d| d.host.clone()).collect();
                own.extend(canopy.iter().filter(|d| d.target != "password" && !mine.contains(&d.host)).cloned());
                own
            })
        });
        domains
    }

    /// Connect (operator, 2026-10-28: "a tab called connect in cove which is
    /// like the landing page in posit connect where given the permissions the
    /// user has given the right people can see the content"): every app the
    /// person may open - their own, the ones shared with them, and the ones
    /// open to everyone signed in or to the public - each under the mark of
    /// what it is built with, the same mark the IDE shows beside the project.
    /// Who sees what is host.rs visible_domains, the platform's own levels;
    /// nothing here widens them.
    /// Account mode: the account pane in one centred column, as Connect lays out.
    fn account_view(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        let pad: i8 = if compact(ctx) { 8 } else { 16 };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::bg().gamma_multiply(0.92)).inner_margin(egui::Margin::symmetric(pad, pad.min(12)))).show(root, |ui| {
            let col = ui.available_width().min(1120.0);
            let side = ((ui.available_width() - col) / 2.0).max(0.0);
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(col);
                    self.ide.account_pane(ui, ctx);
                });
            });
        });
    }

    fn connect_view(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        let apps = self.visible_apps(ctx);
        // Who may open each, by host: the registry's level as visible_domains
        // reads it. The person's own rows come from their folders, which do
        // not carry it.
        let url = match &self.backend {
            Backend::Http { base } => format!("{base}/domains.json"),
            #[allow(unreachable_patterns)]
            _ => format!("{}/domains.json", crate::showcase::base()),
        };
        // Who may open each and when it was created, by host: the registry's
        // level and created_at as visible_domains reads them. The person's
        // own rows come from their folders, which carry neither.
        let canopy = self.showcase.canopy(&url, ctx);
        let created: std::collections::HashMap<String, u64> = canopy.iter().map(|d| (d.host.clone(), d.created)).collect();
        let levels: std::collections::HashMap<String, String> = canopy.into_iter().map(|d| (d.host, d.target)).collect();
        let now = crate::software::now_secs();
        let mut open_ide = None;
        let mut acts: Vec<crate::manage::Act> = Vec::new();
        let backend = self.backend.clone();
        // Laid out like Posit Connect's content list (operator's screenshot,
        // 2026-10-28, "example connect view"): one centred column - the
        // search, the title, the tabs with their counts, Filter and Sort,
        // then one row per app (operator, same day: "need one row per
        // content").
        let pad: i8 = if compact(ctx) { 8 } else { 16 };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::bg().gamma_multiply(0.92)).inner_margin(egui::Margin::symmetric(pad, pad.min(12)))).show(root, |ui| {
            let col = ui.available_width().min(1120.0);
            // A phone's column (operator, 2026-10-29: "content section should
            // look better on mobile"): short tab words, the filters wrapped
            // two to a line, and rows of three short lines with icon buttons.
            let narrow = col < 640.0;
            let side = ((ui.available_width() - col) / 2.0).max(0.0);
            ui.horizontal_top(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(col);
                    let all = match apps {
                        Some(Ok(mut v)) => {
                            // The registry's date where it has one; else the
                            // folder's (host.rs domain_at).
                            for d in &mut v {
                                if let Some(c) = created.get(&d.host).copied().filter(|c| *c > 0) {
                                    d.created = c;
                                }
                            }
                            v
                        }
                        Some(Err(e)) => {
                            ui.label(RichText::new(e).size(14.0).color(theme::warn()));
                            return;
                        }
                        None => {
                            ui.spinner();
                            return;
                        }
                    };
                    ui.add_space(4.0);
                    ui.add(egui::TextEdit::singleline(&mut self.connect_find).hint_text(if narrow { "Search name, address or owner" } else { "Search for content: name, address or owner" }).desired_width(col).font(egui::FontId::proportional(16.0)).margin(egui::vec2(12.0, 9.0)));
                    ui.add_space(14.0);
                    ui.label(RichText::new("Content").size(if narrow { 22.0 } else { 28.0 }).color(theme::text_strong()));
                    ui.add_space(if narrow { 6.0 } else { 10.0 });
                    let level = |d: &crate::api::DomainInfo| levels.get(&d.host).map_or("", String::as_str);
                    let tab_of = |d: &crate::api::DomainInfo, t: u8| match t {
                        1 => !d.path.is_empty(),
                        2 => level(d) == "shared",
                        3 => level(d) == "public",
                        _ => true,
                    };
                    let n = |t: u8| all.iter().filter(|d| tab_of(d, t)).count();
                    let word = |long: &str, short: &str, c: usize| if narrow { format!("{short} {c}") } else { format!("{long} ({c})") };
                    let tabs = [
                        (word("Yours", "Yours", n(1)), "the apps you own".to_owned(), self.connect_whose == 1, true),
                        (word("Shared with you", "Shared", n(2)), "apps their owners shared with you or a group you are in".to_owned(), self.connect_whose == 2, true),
                        (word("Public", "Public", n(3)), "open to anyone, signed in or not".to_owned(), self.connect_whose == 3, true),
                        (word("All", "All", all.len()), "every app you may open".to_owned(), self.connect_whose == 0, true),
                    ];
                    // In a row: the switch lays its parts out the way its parent
                    // does, and this column is vertical.
                    if let Some(i) = ui.horizontal(|ui| crate::agent::segmented(ui, true, &tabs)).inner {
                        self.connect_whose = [1, 2, 3, 0][i];
                    }
                    let whose = self.connect_whose;
                    let find = self.connect_find.trim().to_lowercase();
                    let owner_of = |d: &crate::api::DomainInfo| if d.path.is_empty() { d.owner.clone() } else { "you".to_owned() };
                    let base: Vec<&crate::api::DomainInfo> = all
                        .iter()
                        .filter(|d| tab_of(d, whose))
                        .filter(|d| find.is_empty() || [&d.host, &d.title, &d.owner].iter().any(|t| t.to_lowercase().contains(&find)))
                        .collect();
                    // Filter: Type (what it is built with, its mark) and Owner,
                    // each listing only what the tab and the search leave.
                    ui.add_space(10.0);
                    // On a phone the three menus wrap, Type and Owner side by
                    // side and Sort under them, each as wide as its share.
                    let half = ((col - 8.0) / 2.0).floor();
                    let (menu_w, sort_w) = if narrow { (half - 8.0, col - 8.0) } else { (190.0, 200.0) };
                    let sorts = ["Last changed (newest)", "Name (A to Z)", "Created (newest)", "Created (oldest)"];
                    let sort_menu = |ui: &mut egui::Ui, sort: &mut u8| {
                        egui::ComboBox::from_id_salt("connect-sort").selected_text(RichText::new(sorts[*sort as usize]).size(14.0)).width(sort_w).show_ui(ui, |ui| {
                            for (i, w) in sorts.iter().enumerate() {
                                if ui.selectable_label(*sort == i as u8, *w).clicked() {
                                    *sort = i as u8;
                                }
                            }
                        });
                    };
                    // A row that is as tall as its menus. `with_layout` took the
                    // column's whole remaining height and centred the menus in
                    // it, so a phone showed an empty band above them and Sort
                    // never wrapped onto its own line (operator, 2026-10-29:
                    // "more wasted white space").
                    let filters = |ui: &mut egui::Ui| {
                        if !narrow {
                            ui.label(RichText::new("Filter:").size(14.0).color(theme::text_strong()));
                        }
                        let mut types: Vec<(String, usize)> = Vec::new();
                        for d in &base {
                            match types.iter_mut().find(|(k, _)| *k == d.framework) {
                                Some(t) => t.1 += 1,
                                None => types.push((d.framework.clone(), 1)),
                            }
                        }
                        types.sort_by(|a, b| b.1.cmp(&a.1));
                        let type_word = |k: &str| crate::frameworks::mark(k).map_or("Not detected", |m| m.name).to_owned();
                        let shown_type = self.connect_fw.as_deref().map_or("Type: any".to_owned(), |k| format!("Type: {}", type_word(k)));
                        egui::ComboBox::from_id_salt("connect-type").selected_text(RichText::new(shown_type).size(14.0)).width(menu_w).show_ui(ui, |ui| {
                            if ui.selectable_label(self.connect_fw.is_none(), "any").clicked() {
                                self.connect_fw = None;
                            }
                            for (k, c) in &types {
                                ui.horizontal(|ui| {
                                    crate::frameworks::icon(ui, k, 18.0);
                                    if ui.selectable_label(self.connect_fw.as_deref() == Some(k.as_str()), format!("{}  ({c})", type_word(k))).clicked() {
                                        self.connect_fw = Some(k.clone());
                                    }
                                });
                            }
                        });
                        let mut owners: Vec<(String, usize)> = Vec::new();
                        for d in &base {
                            let o = owner_of(d);
                            if o.is_empty() {
                                continue;
                            }
                            match owners.iter_mut().find(|(k, _)| *k == o) {
                                Some(t) => t.1 += 1,
                                None => owners.push((o, 1)),
                            }
                        }
                        owners.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
                        let shown_owner = self.connect_owner.as_deref().map_or("Owner: anyone".to_owned(), |o| format!("Owner: {o}"));
                        egui::ComboBox::from_id_salt("connect-owner").selected_text(RichText::new(shown_owner).size(14.0)).width(menu_w).show_ui(ui, |ui| {
                            if ui.selectable_label(self.connect_owner.is_none(), "anyone").clicked() {
                                self.connect_owner = None;
                            }
                            for (o, c) in &owners {
                                if ui.selectable_label(self.connect_owner.as_deref() == Some(o.as_str()), format!("{o}  ({c})")).clicked() {
                                    self.connect_owner = Some(o.clone());
                                }
                            }
                        });
                        // Visibility: who the registry lets open each one
                        // (operator, 2026-11-17: "search by visibility level,
                        // for example see all public and make sure theyre all
                        // properly public").
                        let mut lv: Vec<(String, usize)> = Vec::new();
                        for d in &base {
                            let l = level(d).to_owned();
                            match lv.iter_mut().find(|(k, _)| *k == l) {
                                Some(t) => t.1 += 1,
                                None => lv.push((l, 1)),
                            }
                        }
                        lv.sort_by(|a, b| b.1.cmp(&a.1));
                        let shown_level = self.connect_level.as_deref().map_or("Visibility: any".to_owned(), |l| format!("Visibility: {}", level_word(l)));
                        egui::ComboBox::from_id_salt("connect-level").selected_text(RichText::new(shown_level).size(14.0)).width(menu_w).show_ui(ui, |ui| {
                            if ui.selectable_label(self.connect_level.is_none(), "any").clicked() {
                                self.connect_level = None;
                            }
                            for (l, c) in &lv {
                                if ui.selectable_label(self.connect_level.as_deref() == Some(l.as_str()), format!("{}  ({c})", level_word(l))).clicked() {
                                    self.connect_level = Some(l.clone());
                                }
                            }
                        });
                        if !narrow {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                sort_menu(ui, &mut self.connect_sort);
                                ui.label(RichText::new("Sort:").size(14.0).color(theme::text_strong()));
                            });
                        }
                    };
                    // A phone's Sort has a line of its own: a full-width menu
                    // in the wrapped row ran off the screen's edge instead of
                    // wrapping (measured at 390 px, 2026-10-29).
                    if narrow {
                        ui.horizontal(filters);
                        ui.add_space(4.0);
                        sort_menu(ui, &mut self.connect_sort);
                    } else {
                        ui.horizontal(filters);
                    }
                    let (fw, who, lvl) = (self.connect_fw.clone(), self.connect_owner.clone(), self.connect_level.clone());
                    let mut shown: Vec<&crate::api::DomainInfo> = base
                        .into_iter()
                        .filter(|d| fw.as_deref().is_none_or(|f| d.framework == f))
                        .filter(|d| who.as_deref().is_none_or(|o| owner_of(d) == o))
                        .filter(|d| lvl.as_deref().is_none_or(|l| level(d) == l))
                        .collect();
                    let by_name = |a: &&crate::api::DomainInfo, b: &&crate::api::DomainInfo| connect_name(a).to_lowercase().cmp(&connect_name(b).to_lowercase());
                    match self.connect_sort {
                        0 => shown.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| by_name(a, b))),
                        2 => shown.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| by_name(a, b))),
                        // Oldest first; a domain whose creation is not known last.
                        3 => shown.sort_by(|a, b| (a.created == 0).cmp(&(b.created == 0)).then(a.created.cmp(&b.created)).then_with(|| by_name(a, b))),
                        _ => shown.sort_by(by_name),
                    }
                    ui.add_space(6.0);
                    // Check them from outside: each asked as a stranger would.
                    let pairs: Vec<(String, String)> = shown.iter().map(|d| (d.host.clone(), level(d).to_owned())).collect();
                    if let Some(h) = self.visibility_check(ui, ctx, &pairs) {
                        self.access.open_for(&h, &backend, ctx);
                    }
                    // What the last action said, and the bar for ticked projects.
                    self.manage.settle();
                    self.manage.note_ui(ui);
                    self.manage.queue_ui(ui);
                    if let Some(a) = self.manage.bulk_bar(ui, &backend, ctx) {
                        acts.push(a);
                    }
                    ui.separator();
                    // Only the rows on screen are drawn: hundreds of apps scroll.
                    // The person's own get the working row (manage.rs); others'
                    // stay a row to open.
                    egui::ScrollArea::vertical().id_salt("connect-apps").auto_shrink([false, false]).show_rows(ui, if narrow { CONNECT_ROW_H_NARROW } else { CONNECT_ROW_H }, shown.len(), |ui, range| {
                        for i in range {
                            let d = shown[i];
                            if !narrow && !d.path.is_empty() {
                                if let Some(a) = self.manage.row(ui, d, col, CONNECT_ROW_H, &backend, ctx) {
                                    acts.push(a);
                                }
                            } else if let Some(o) = connect_row(ui, d, level(d), col, now, narrow) {
                                open_ide = Some(o);
                            }
                        }
                    });
                });
            });
        });
        if let Some(a) = self.manage.details_ui(ctx, &backend) {
            acts.push(a);
        }
        // Several ticked for Archive or Delete: the next one, once the
        // dialog is free.
        if self.ide.remove_idle() {
            if let Some((h, delete)) = self.manage.next_remove() {
                acts.push(crate::manage::Act::Remove(h, delete));
            }
        }
        for a in acts {
            use crate::manage::Act;
            match a {
                Act::OpenIde(d) => open_ide = Some(d),
                Act::Share(h) => self.access.open_for(&h, &backend, ctx),
                Act::Publish(h) => self.ide.publish_domain(h, ctx),
                Act::Remove(h, delete) => self.ide.ask_remove(&h, delete),
            }
        }
        if let Some(d) = open_ide {
            self.mode = Mode::Ide;
            self.saved.zoom_tab = None;
            self.ide.open_domain(d, ctx);
        }
    }

    /// Connections' visibility check: "Check N from outside" asks each shown
    /// site, with no sign-in, what it answers (host.rs site_gate: open, login,
    /// password, down), eight at a time; then one line of how many match their
    /// level and a row for each that does not, with Share to fix it. Returns
    /// a host whose Share was asked for.
    fn visibility_check(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, pairs: &[(String, String)]) -> Option<String> {
        use std::sync::atomic::Ordering;
        // Start what is queued, eight at a time.
        while !self.connect_queue.is_empty() && self.connect_asking.load(Ordering::Relaxed) < 8 {
            let host = self.connect_queue.remove(0);
            self.connect_asking.fetch_add(1, Ordering::Relaxed);
            let (gates, asking, c) = (self.connect_gates.clone(), self.connect_asking.clone(), ctx.clone());
            let h = host.clone();
            self.backend.query(Query::SiteGate { host }, ctx, move |r: Result<String, String>| {
                gates.lock().unwrap().insert(h, r.unwrap_or_else(|e| format!("unknown: {e}")));
                asking.fetch_sub(1, Ordering::Relaxed);
                c.request_repaint();
            });
        }
        let busy = self.connect_asking.load(Ordering::Relaxed) > 0 || !self.connect_queue.is_empty();
        if busy {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        let gates = self.connect_gates.lock().unwrap().clone();
        let checked: Vec<&(String, String)> = pairs.iter().filter(|(h, _)| gates.contains_key(h)).collect();
        let wrong: Vec<(&String, &String, &String)> = checked.iter().filter_map(|(h, l)| gates.get(h).filter(|g| !gate_fits(l, g)).map(|g| (h, l, g))).collect();
        let mut fix = None;
        ui.horizontal_wrapped(|ui| {
            let word = if busy { format!("Checking… {} of {}", checked.len(), pairs.len()) } else { format!("Check {} from outside", pairs.len()) };
            if ui.add_enabled(!busy && !pairs.is_empty() && pairs.len() <= 400, egui::Button::new(RichText::new(word).size(13.5)).min_size(egui::vec2(0.0, 28.0)))
                .on_hover_text("open each shown site with no sign-in, as a stranger would, and compare what it does with its visibility")
                .clicked()
            {
                self.connect_gates.lock().unwrap().retain(|h, _| !pairs.iter().any(|(p, _)| p == h));
                self.connect_queue = pairs.iter().map(|(h, _)| h.clone()).collect();
            }
            if busy {
                ui.spinner();
            } else if !checked.is_empty() {
                let ok = checked.len() - wrong.len();
                let (st, said) = if wrong.is_empty() {
                    (theme::State::Good, format!("all {ok} checked do what their visibility says"))
                } else {
                    (theme::State::Failed, format!("{} of {} do not do what their visibility says", wrong.len(), checked.len()))
                };
                theme::state_mark(ui, st, &said);
            }
        });
        for (h, l, g) in &wrong {
            ui.horizontal_wrapped(|ui| {
                theme::state_mark(ui, theme::State::Failed, h);
                ui.label(RichText::new(format!("is {}, but {}", level_word(l), gate_word(g))).size(13.0).color(theme::text_strong()));
                if ui.button("Who can view it…").clicked() {
                    fix = Some((*h).clone());
                }
                if ui.button("Open ↗").clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(format!("https://{h}/")));
                }
            });
        }
        fix
    }

    /// The atlas, full screen over the night sky; its answers step into the IDE.
    /// A star's exact source, fetched into the person's workspace; the answer
    /// opens it in Files (`poll_source`). The atlas's Source and the project
    /// menu's Inlet both come here.
    fn open_source(&mut self, c: &crate::software::Catalog, i: usize, ctx: &egui::Context) {
        use crate::software::Kind;
        let s = c.sw(i);
        let kind = s.kind.key();
        // An EasyBuild star is one installed module: its name and full version
        // (2.42-GCCcore-13.3.0) are the module's.
        let (name, version) = match (s.kind, s.module.split_once('/')) {
            (Kind::EasyBuild | Kind::Env, Some((n, v))) => (n.to_owned(), v.to_owned()),
            _ => (s.name.to_owned(), s.versions.last().cloned().unwrap_or_default()),
        };
        self.toast(format!("fetching the source of {name} {version}…"), ctx);
        let slot = self.source_ready.clone();
        // The project the source becomes works in this session's environment.
        let env = self.saved_settings().module.clone();
        let q = Query::SourceOpen { kind: kind.to_owned(), name, version, sets: s.sets.to_vec(), env };
        self.backend.query(q, ctx, move |r: Result<crate::source::SourceView, String>| {
            *slot.lock().unwrap() = Some(r);
        });
    }

    fn atlas_view(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        let cat = self.ide.software_catalog(ctx);
        let catalog = cat.clone();
        let domains = self.visible_apps(ctx);
        let who = self.probe.lock().unwrap().as_ref().and_then(|p| p.home.trim_end_matches('/').rsplit('/').next().map(str::to_owned)).unwrap_or_default();
        let current = self.ide.domain.as_ref().map(|d| d.host.clone());
        // How these domains are served, once a minute: each apex's DNS and
        // address, every target nginx hands them to (serving.rs). Refused to
        // anyone but an admin; then asked again only every ten minutes.
        let now = ctx.input(|i| i.time);
        if let Some(Ok(doms)) = &domains {
            let every = if self.serving.lock().unwrap().is_some() { 60.0 } else { 600.0 };
            if !doms.is_empty() && now - self.serving_at > every {
                self.serving_at = now;
                let mut apexes: Vec<String> = doms
                    .iter()
                    .map(|d| {
                        let p: Vec<&str> = d.host.split('.').collect();
                        if p.len() <= 2 { d.host.clone() } else { p[p.len() - 2..].join(".") }
                    })
                    .collect();
                apexes.sort();
                apexes.dedup();
                let mut targets: Vec<String> = doms.iter().map(|d| d.target.clone()).collect();
                targets.sort();
                targets.dedup();
                let slot = self.serving.clone();
                self.backend.query(crate::api::Query::Serving { apexes, targets }, ctx, move |r: Result<crate::api::Serving, String>| {
                    if let Ok(s) = r {
                        *slot.lock().unwrap() = Some(s);
                    }
                });
            }
        }
        self.atlas.set_serving(self.serving.lock().unwrap().clone());
        // The panel's whole space, read inside it: its response covers only
        // what the atlas laid out, which is less than it paints.
        let (act, within) = egui::CentralPanel::default().frame(egui::Frame::new()).show(root, |ui| (self.atlas.ui(ui, ctx, cat, domains, &who, current.as_deref()), ui.max_rect())).inner;
        self.showcase.within = Some(within);
        self.showcase.bare = self.atlas.immersive;
        if self.atlas.chosen() {
            self.showcase.hold();
        }
        // On a phone, a star of the person's own takes the bottom of the
        // screen: the tour's card and its "The story" button step aside
        // rather than lie over its details (operator, 2026-10-21, phone
        // screenshots: "menus overlap a lot on mobile").
        let phone_pick = self.atlas.chosen() && ctx.content_rect().width() < 760.0;
        match if phone_pick { None } else { self.showcase.ui(ctx, catalog.as_deref()) } {
            Some(crate::showcase::Point::At(i)) => {
                if !self.atlas.show_item(i) {
                    self.showcase.retry();
                }
            }
            Some(crate::showcase::Point::Kernel(j)) => {
                if !self.atlas.show_kernel(j) {
                    self.showcase.retry();
                }
            }
            Some(crate::showcase::Point::Release) => self.atlas.release(),
            None => {}
        }
        match act {
            Some(crate::atlas::Act::Enter(d)) if d.path.is_empty() => ctx.open_url(egui::OpenUrl::new_tab(format!("https://{}/", d.host))),
            Some(crate::atlas::Act::Enter(d)) => {
                self.mode = Mode::Ide;
                self.saved.zoom_tab = None;
                self.ide.open_domain(d, ctx);
            }
            // Ask for software the stack lacks: the request form, in its
            // software shape, over the atlas.
            Some(crate::atlas::Act::AddSoftware) => self.request = Some(RequestDraft { software: true, ..Default::default() }),
            // A star's software is the stack's: it opens in Builds.
            Some(crate::atlas::Act::Software(i)) => {
                self.mode = Mode::Builds;
                self.ide.reveal_software(i);
            }
            // The star's exact source, fetched into the person's workspace;
            // the answer opens it in Files (`poll_source`).
            Some(crate::atlas::Act::Source(i)) => {
                if let Some(c) = &catalog {
                    self.open_source(c, i, ctx);
                }
            }
            None => {}
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        // Where this Cove names and places new projects (ide.rs on_platform).
        if let Some(v) = self.settings.lock().unwrap().as_ref() {
            self.ide.platform = Some(v.platform);
            let domains = v.domains();
            if self.ide.own_domains != domains {
                self.ide.own_domains = domains;
            }
        }
        // The look follows the pick, and "Match system" follows the system.
        crate::setup::follow_theme(self.saved.theme, ctx);
        // Which Claude the agent runs on: at start, then every ten minutes.
        self.setup.refresh(&self.backend, ctx, 600.0);
        self.backdrop.paint(ctx);
        self.ide.refresh_every = self.settings.lock().unwrap().as_ref().map_or(3.0, |v| v.settings.refresh_every());
        self.ide.poll(ctx);
        self.poll_stack(ctx);
        self.poll_storage(false, ctx);
        self.auto_setup(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.report_consent(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.cell_offer_modal(ctx);
        #[cfg(not(target_arch = "wasm32"))]
        self.server_modal(ctx);
        self.poll_source(ctx);
        // A revision was just made (Files' Make environment): into it.
        if let Some(m) = self.ide.take_switch_env() {
            let modules = self.modules.clone();
            self.backend.query(Query::Modules, ctx, move |r: Result<Modules, String>| {
                if let Ok(v) = r {
                    *modules.lock().unwrap() = Some(v);
                }
            });
            let before = self.saved_settings();
            self.save_options(Settings { module: m.clone(), ..before }, ctx);
            self.toast(format!("{m}: a new environment revision, recorded - R, Python and the shell restarting in it. The published one is still in the picker."), ctx);
        }
        self.follow_domain_env(ctx);
        let restart: Vec<Lang> = std::mem::take(&mut *self.restart_after_save.lock().unwrap());
        // Only the consoles the new environment brings start again; the rest
        // stop - in an env/Julia project, R would otherwise come back as the
        // host's own /usr/bin/R.
        for lang in restart {
            if self.ide.has_console(lang) {
                self.ide.restart(lang, ctx);
                self.toast(format!("{} restarting in its new interpreter", lang.word()), ctx);
            } else {
                self.ide.stop_console(lang, ctx);
            }
        }
        // The first time: the default window on their own site, not settings
        // saved from before (operator, 2026-10-30: "all users start at proper
        // default state in next login"). Saved projects that no longer exist
        // otherwise show as errors in Files.
        if let Some(f) = self.ide.start_fresh.take() {
            self.mode = Mode::Ide;
            self.saved.zoom_tab = None;
            self.saved.grid = Grid::default();
            self.ndexr_due = false;
            self.folder = Some(f);
        }
        // The person's own ndexr (ide.rs domain_bar): the folder below, with
        // the agent of all their sites, which is what spans it. A first visit
        // opens it only when they have no site of their own to open: this used
        // to win over the site whenever `/` sent no ?folder= (a person with a
        // session already), so the first question was never asked (2026-10-30).
        if let Some(home) = self.probe.lock().unwrap().as_ref().map(|p| p.home.clone()) {
            if self.ide.home != home {
                self.ide.home = home.clone();
                self.ndexr_due = self.saved.domain.is_none() && self.folder.is_none() && !home.is_empty();
            }
        }
        if self.ndexr_due && self.ide.own_site_settled() {
            self.ndexr_due = false;
            if self.ide.domain.is_none() {
                self.ide.want_folder = Some(format!("{}/ndexr", self.ide.home.trim_end_matches('/')));
            }
        }
        if let Some(f) = self.ide.want_folder.take() {
            self.ide.ndexr_open = true;
            self.ide.exec_on = true;
            self.folder = Some(f);
        }
        // ?folder=: that folder becomes the project - saved like Global Options'
        // Project field, so the kernels restart in it - and Files opens on it.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(f) = self.picked_folder.lock().unwrap().take() {
            self.folder = Some(f);
        }
        if self.folder.is_some() {
            let saved = self.settings.lock().unwrap().as_ref().map(|v| v.settings.clone());
            if let Some(s) = saved {
                let f = self.folder.take().unwrap_or_default();
                if s.project_dir.trim_end_matches('/') != f {
                    self.save_options(Settings { project_dir: f.clone(), ..s }, ctx);
                    self.toast(format!("opening {f}"), ctx);
                }
                self.ide.open_folder_domain(&f, ctx);
                // The folder is in Files; the agent stays in front.
                self.ide.bring(Tab::Agents);
                self.saved.zoom_tab = None;
            }
        }
        // Nothing chosen on this machine yet: ask, once, rather than guess.
        if !self.asked_options && self.mode != Mode::Atlas && !self.setup.covering() && self.settings.lock().unwrap().as_ref().is_some_and(|v| !v.chosen) {
            self.asked_options = true;
            self.open_options(ctx);
        }
        if self.options.is_none() && !self.about && !self.setup.open && self.mode != Mode::Atlas {
            self.shortcuts(ctx);
        }

        self.ide.hold_shots = self.request.is_some();
        self.release.ui(root, ctx, &self.backend);
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(u) = &self.updater {
            self.release.desktop(root, ctx, u);
        }
        // The consoles follow the environment the session runs in.
        let module = self.settings.lock().unwrap().as_ref().map(|v| v.settings.module.clone());
        if let Some(m) = module {
            self.ide.set_env(&m);
        }
        // The walkthrough reads the IDE as it is and moves on only when the
        // person has done the step (tour.rs); what its new step asks for is
        // done before the windows draw, so they show it this frame.
        let seen = self.tour_seen(ctx);
        if let Some(a) = self.walk.update(&seen) {
            self.tour_act(a, ctx);
        }
        // No Go on a phone (mode_tabs): one left there, or saved there on a
        // desk, opens in the IDE.
        if self.mode == Mode::Go && compact(ctx) {
            self.mode = Mode::Ide;
        }
        // Cove inside Cove's own App pane (operator, 2026-11-01: "nested Cove
        // headers because Cove is being displayed inside Cove ... the inner
        // application should minimize or collapse redundant navigation"):
        // the outer one has the menus; the inner one keeps only its content
        // and a way out to a tab of its own.
        if inside_cove() {
            egui::Area::new(egui::Id::new("own-tab")).order(egui::Order::Foreground).anchor(egui::Align2::RIGHT_TOP, [-8.0, 8.0]).show(ctx, |ui| {
                if ui.add(egui::Button::new(RichText::new("Open in its own tab ↗").size(12.5))).on_hover_text("this Cove is showing inside another Cove's App pane - its menus are there").clicked() {
                    #[cfg(target_arch = "wasm32")]
                    if let Some(href) = web_sys::window().and_then(|w| w.location().href().ok()) {
                        ctx.open_url(egui::OpenUrl::new_tab(href));
                    }
                }
            });
        } else {
            self.header(root, ctx);
            self.status_bar(root, ctx);
            self.help_panel(root, ctx);
        }
        self.sessions_window(ctx);
        match self.mode {
            Mode::Go => self.go_view(root, ctx),
            Mode::Connect => self.connect_view(root, ctx),
            Mode::Builds => self.builds_view(root, ctx),
            Mode::Atlas => self.atlas_view(root, ctx),
            Mode::Account => self.account_view(root, ctx),
            Mode::Ide => self.ide_view(root, ctx),
        }

        self.options_dialog(ctx);
        self.about_dialog(ctx);
        self.request_dialog(ctx);
        // Chat saves files into the domain open here, and shares my sites.
        if self.inbox.open {
            self.inbox.here = self.ide.domain.as_ref().map(|d| (d.host.clone(), d.path.clone()));
            if self.inbox.domains.is_empty() {
                if let Some(Ok(l)) = self.ide.domain_list(ctx) {
                    self.inbox.domains = l.iter().map(|d| d.host.clone()).collect();
                }
            }
        }
        self.inbox.ui(ctx, &self.backend);
        if let Some(h) = self.ide.want_access.take() {
            self.access.open_for(&h, &self.backend, ctx);
        }
        self.access.ui(ctx, &self.backend, &mut self.inbox);
        // Messages' Hand to Exec: the request to Exec, and its progress back.
        if let Some((id, text)) = self.inbox.to_exec.take() {
            let key = self.ide.exec_ask(ctx, &text);
            self.inbox.handed(id, key);
        }
        for (id, key) in self.inbox.exec_keys() {
            if let Some((reply, done)) = self.ide.exec_turn(&key) {
                self.inbox.exec_update(id, reply, done);
            }
        }
        self.storage_dialog(ctx);
        self.project_settings_dialog(ctx);
        self.ide.push_dialog(ctx);
        if !crate::backend::session_ended() {
            let env = crate::setup::Env {
                backend: &self.backend,
                ide: &mut self.ide,
                theme: &mut self.saved.theme,
                settings: &self.settings,
                #[cfg(not(target_arch = "wasm32"))]
                account: &self.account,
            };
            if let crate::setup::Done::Closed(toast) = self.setup.show(ctx, env) {
                self.saved.setup_seen = true;
                // Their Claude may have changed: ask again now.
                self.setup.refresh(&self.backend, ctx, 0.0);
                if let Some(t) = toast {
                    self.toast(t, ctx);
                }
            }
        }
        self.toasts(ctx);
        if let Some(id) = self.ide.want_tour.take() {
            self.start_tour(id);
        }
        // Last, above everything: the welcome, a step's spotlight and card,
        // the finish, Help's pages.
        let seen = self.tour_seen(ctx);
        let said = self.walk.show(ctx, &seen, self.setup.open || self.setup.covering());
        if let Some(a) = said.act {
            self.tour_act(a, ctx);
        }
        if let Some(id) = said.start {
            self.start_tour(id);
        }
        // The site is a page laid over the canvas: hidden while a tour card
        // would sit under it, or a whole-screen card is up.
        let card = ctx.data(|d| d.get_temp::<egui::Rect>(egui::Id::new("tour-card-rect")));
        let app_pane = crate::tour::marked(ctx, "pane:App");
        let tour_covers = (self.walk.welcome_due() && !self.setup.open) || self.walk.finished_card || (self.walk.run.is_some() && card.zip(app_pane).is_some_and(|(c, a)| c.intersects(a)));
        // During Update now the session ends on purpose and the release bar
        // reloads the page itself (release.rs `updating`).
        let ended = crate::backend::session_ended() && !self.release.updating();
        if ended {
            self.session_ended(ctx);
        }
        // A tab being dragged hides the site's frame too: it would swallow the pointer.
        // Messages too: the site's frame is a page over the canvas, so it hid
        // the chat's composer and Send (operator, 2026-10-18:
        // "sees messages behind their site ... the message portal needs to
        // stay on top").
        //
        // Any dialog, read from egui rather than listed here (operator,
        // 2026-10-24: "all of the modals, like the domain project selector
        // view, gets hidden behind windows already existing. So I tried to
        // create a new project but I couldnt see what it was asking me"): the
        // list had Options and Help but not New project's chooser, the launch
        // window or Find in files. A modal is open, or a window, menu or
        // dropdown sits on the foreground - one big enough to be one; a toast
        // is smaller and leaves the site showing.
        let floating = ctx.memory(|m| {
            m.top_modal_layer().is_some()
                || m.areas().visible_layer_ids().iter().any(|l| l.order == egui::Order::Foreground && m.area_rect(l.id).is_some_and(|r| r.width() >= 200.0 && r.height() >= 150.0))
        });
        // What opened this frame goes in front of everything already open
        // (operator, 2026-11-21: "clicking on something opens window behind
        // window - we must make sure all windows show at a level that makes
        // sense"). egui keeps each window's place in the stack between
        // showings, so a dialog shown once before came back under the one
        // open now - the stack-setup dialog flickered with the setup steps,
        // Messages and the top dialog each lifted themselves every frame and
        // covered whatever they opened. One rule instead: the newest is on top.
        ctx.memory_mut(|m| {
            let a = m.areas_mut();
            let fresh: Vec<egui::LayerId> = a.visible_layer_ids().into_iter().filter(|l| !a.visible_last_frame(l) && matches!(l.order, egui::Order::Middle | egui::Order::Foreground)).collect();
            for l in fresh {
                a.move_to_top(l);
            }
        });
        let covered = floating || tour_covers || matches!(self.mode, Mode::Atlas | Mode::Builds | Mode::Connect | Mode::Account) || ended || self.setup.covering() || self.drag.tab.is_some() || self.splitting;
        crate::ide::viewer_frame_end(covered);
        crate::paste::end_frame();
        #[cfg(not(target_arch = "wasm32"))]
        crate::webview::end_frame(ctx, frame, covered);
        #[cfg(target_arch = "wasm32")]
        let _ = frame;
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        // A one-tab page shares the browser's store with the IDE that opened
        // it: saving from here would put its one-window layout over the IDE's.
        if self.solo.is_some() {
            return;
        }
        let (domain, open) = self.ide.remember();
        self.saved.domain = domain;
        self.saved.open_by_domain = open;
        self.saved.agents_layout = self.ide.agents_layout;
        self.saved.agents_frac = Some(self.ide.agents_frac);
        self.saved.agent_all = self.ide.exec_on;
        self.saved.recent_domains = self.ide.recent_hosts.clone();
        self.saved.tabs = self.ide.tabs.clone();
        self.saved.tour = self.walk.prefs.clone();
        self.saved.live_off = self.ide.live_off.clone();
        self.saved.listen_secs = self.ide.listen_secs;
        self.saved.listen_minutes = (self.ide.listen_secs / 60).max(1) as u8;
        self.saved.speak = crate::voice::speak::on();
        eframe::set_value(storage, eframe::APP_KEY, &self.saved);
    }
}

/// Whether Cove fills the screen: the window's own state on a desktop, the
/// document's Fullscreen API state in a browser (egui's viewport commands do
/// nothing there).
/// The page's full-screen helper (web/index.html `window.coveFullscreen`):
/// the standard API, Safari's prefixed one, and what to say when neither is there.
#[cfg(target_arch = "wasm32")]
fn page_fullscreen(method: &str) -> Option<wasm_bindgen::JsValue> {
    use wasm_bindgen::JsCast;
    let w = web_sys::window()?;
    let fs = js_sys::Reflect::get(&w, &"coveFullscreen".into()).ok().filter(|v| !v.is_undefined())?;
    let f: js_sys::Function = js_sys::Reflect::get(&fs, &method.into()).ok()?.dyn_into().ok()?;
    f.call0(&fs).ok()
}

fn is_fullscreen(ctx: &egui::Context) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        match page_fullscreen("active") {
            Some(v) => v.as_bool().unwrap_or(false),
            None => web_sys::window().and_then(|w| w.document()).is_some_and(|d| d.fullscreen_element().is_some()),
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    ctx.input(|i| i.viewport().fullscreen.unwrap_or(false))
}

/// In a browser this runs a frame after the click or key, inside the
/// page's transient user activation, which is what requestFullscreen needs.
/// Err when it cannot be done here, with what to do instead.
fn toggle_fullscreen(ctx: &egui::Context) -> Result<(), &'static str> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        match page_fullscreen("toggle").and_then(|v| v.as_string()).as_deref() {
            Some("unsupported") => return Err("This browser cannot put a page full screen (iPhone Safari). Share › Add to Home Screen opens Cove full screen, without the browser's bars."),
            Some("standalone") => return Err("Cove is already running as an app, without the browser's bars."),
            Some(_) => return Ok(()),
            None => {}
        }
        if let Some(d) = web_sys::window().and_then(|w| w.document()) {
            if d.fullscreen_element().is_some() {
                d.exit_fullscreen();
            } else if let Some(el) = d.document_element() {
                if let Err(e) = el.request_fullscreen() {
                    web_sys::console::warn_1(&e);
                }
            }
        }
        Ok(())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!is_fullscreen(ctx)));
        Ok(())
    }
}

#[cfg(test)]
mod dock_tests {
    use super::*;

    #[test]
    fn a_software_request_says_what_is_asked_for_first() {
        let mut d = RequestDraft { software: true, ..Default::default() };
        assert!(!d.ready(), "a name first");
        d.name = " samtools ".into();
        d.version = "1.21".into();
        d.kind = 2;
        d.what = "for aligning reads in the variant pipeline".into();
        assert!(d.ready());
        assert_eq!(d.filed_what(), "Add software: samtools 1.21 (library or tool (EasyBuild))\n\nfor aligning reads in the variant pipeline");
        let plain = RequestDraft { what: "the App tab is blank".into(), ..Default::default() };
        assert_eq!(plain.filed_what(), "the App tab is blank");
    }

    #[test]
    fn a_run_never_covers_the_agent() {
        let mut g = Grid::default();
        // Cove opens on the agent: a run does not cover it.
        assert_eq!(g.win((0, 0)).unwrap().front, Some(Tab::Agents));
        g.bring(Tab::Console, true);
        assert_eq!(g.win((0, 0)).unwrap().front, Some(Tab::Agents));
        // A layout saved before 2026-10-19 (2x2 corners), with Terminal (a
        // tab no more) in front and Files on the right, opens on Agents,
        // Inputs, Files.
        let cells = [vec![Tab::Terminal, Tab::Console], vec![Tab::App, Tab::Agents, Tab::Files], vec![], vec![]];
        let mut old = Grid::from_corners(&cells, &[Some(Tab::Terminal), Some(Tab::Agents), None, None], None);
        old.agents_first();
        assert!(old.find(Tab::Terminal).is_none());
        assert_eq!(old.win((0, 0)).unwrap().tabs[..3], [Tab::Agents, Tab::Console, Tab::Files]);
        assert_eq!(old.win((0, 0)).unwrap().front, Some(Tab::Agents));
        assert_eq!(old.win((1, 0)).unwrap().front, Some(Tab::App));
    }

    #[test]
    fn windows_lay_out_by_their_shares() {
        let g = Grid::default().dropped(Tab::Plots, Drop::Col(1)).dropped(Tab::Viewer, Drop::Row(1, 1));
        let a = egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1000.0 + 2.0 * win_gap(), 800.0 + win_gap()));
        let (wins, gaps) = grid_rects(a, &g);
        assert_eq!(wins.len(), 4);
        let plots = wins.iter().find(|(at, _)| *at == g.find(Tab::Plots).unwrap()).unwrap().1;
        assert!((plots.height() - 400.0).abs() < 0.5 && (plots.width() - 1000.0 / 3.0).abs() < 0.5, "{plots:?}");
        assert_eq!(gaps.iter().filter(|g| g.1).count(), 2, "two column splitters");
        assert_eq!(gaps.iter().filter(|g| !g.1).count(), 1, "one row splitter");
    }
}

/// Who may open an app, in words (host.rs visible_domains' levels).
fn access_words(level: &str) -> (&'static str, &'static str) {
    match level {
        "public" => ("Public", "anyone, signed in or not"),
        "login" => ("Signed in", "anyone signed in to ndexr"),
        "admin" => ("Admins", "ndexr admins only"),
        "owner" => ("Only the owner", "nobody but its owner"),
        "shared" => ("Shared with you", "its owner shared it with you or a group you are in"),
        _ => ("", ""),
    }
}

/// Each row's height in Connect, fixed so only the rows on screen are drawn;
/// a phone's row is three short lines.
const CONNECT_ROW_H: f32 = 88.0;
const CONNECT_ROW_H_NARROW: f32 = 84.0;

/// The name an app goes by: its title, else its host's first label.
fn connect_name(d: &crate::api::DomainInfo) -> &str {
    if d.title.is_empty() {
        d.host.split('.').next().unwrap_or(&d.host)
    } else {
        d.title.as_str()
    }
}

/// One app in Connect, one row, as Posit Connect lists content: its mark
/// large on the left; its name, then Type · Owner · who may open it · Last
/// changed · its address; Open and, for the person's own, Edit in IDE on the
/// right. Returns the app when Edit was pressed. `narrow`: a phone's column,
/// where connect_row_narrow lays the same facts out in three lines.
fn connect_row(ui: &mut egui::Ui, d: &crate::api::DomainInfo, level: &str, w: f32, now: i64, narrow: bool) -> Option<crate::api::DomainInfo> {
    if narrow {
        return connect_row_narrow(ui, d, level, w, now);
    }
    let mut edit = None;
    let mine = !d.path.is_empty();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, CONNECT_ROW_H), egui::Sense::hover());
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(rect, 6.0, theme::surface().gamma_multiply(0.6));
    }
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, theme::pane_brd()));
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(14.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut row;
    ui.spacing_mut().item_spacing.x = 18.0;
    if crate::frameworks::icon(ui, &d.framework, 52.0).is_none() {
        ui.allocate_exact_size(egui::vec2(52.0, 52.0), egui::Sense::hover());
    }
    let actions_w = if mine { 220.0 } else { 100.0 };
    let text_w = (ui.available_width() - actions_w).max(120.0);
    ui.allocate_ui_with_layout(egui::vec2(text_w, CONNECT_ROW_H), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(text_w);
        ui.add_space(18.0);
        let title = ui.add(egui::Label::new(RichText::new(connect_name(d)).size(18.0).color(theme::text_strong())).truncate().sense(egui::Sense::click()));
        if title.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(format!("https://{}/", d.host)).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://{}/", d.host)));
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let k = |t: &str| RichText::new(t).size(13.5).color(theme::text());
            let v = |t: &str| RichText::new(t).size(13.5).color(theme::text_strong());
            ui.label(k("Type:"));
            ui.label(v(crate::frameworks::mark(&d.framework).map_or("Not detected", |m| m.name)));
            ui.add_space(10.0);
            let owner = if mine { "you".to_owned() } else { d.owner.clone() };
            if !owner.is_empty() {
                ui.label(k("Owner:"));
                ui.label(v(&owner));
                ui.add_space(10.0);
            }
            let (word, tip) = access_words(level);
            if !word.is_empty() {
                egui::Frame::new().fill(theme::primary().gamma_multiply(0.16)).corner_radius(9).inner_margin(egui::Margin::symmetric(8, 1)).show(ui, |ui| {
                    ui.label(RichText::new(word).size(13.0).strong().color(theme::primary()));
                }).response.on_hover_text(tip);
                ui.add_space(10.0);
            }
            // Created as a date, its age on hover (operator, 2026-10-28:
            // "content needs a clean created date or age"); Last changed as
            // an age, for the person's own.
            if d.created > 0 {
                ui.label(k("Created:"));
                ui.label(v(&date_words(d.created as i64))).on_hover_text(format!("{} old", age_words(now - d.created as i64)));
                ui.add_space(10.0);
            }
            if d.modified > 0 {
                ui.label(k("Last changed:"));
                ui.label(v(&format!("{} ago", age_words(now - d.modified as i64))));
                ui.add_space(10.0);
            }
            ui.add(egui::Label::new(RichText::new(&d.host).monospace().size(12.5).color(theme::text())).truncate());
        });
    });
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if mine && ui.add(egui::Button::new(RichText::new("Edit in IDE").size(14.0)).min_size(egui::vec2(0.0, 34.0))).on_hover_text("open its folder in the IDE").clicked() {
            edit = Some(d.clone());
        }
        if ui.add(egui::Button::new(RichText::new("Open ↗").size(14.0)).min_size(egui::vec2(0.0, 34.0))).on_hover_text(format!("https://{}/ in a new tab", d.host)).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://{}/", d.host)));
        }
    });
    edit
}

/// One app in Connect on a phone: its mark; its name, then what it is built
/// with, who may open it and whose it is, then how long since it changed and
/// its address; Open (↗) and, for the person's own, Edit in IDE (✎) as
/// thumb-sized buttons on the right.
fn connect_row_narrow(ui: &mut egui::Ui, d: &crate::api::DomainInfo, level: &str, w: f32, now: i64) -> Option<crate::api::DomainInfo> {
    let mut edit = None;
    let mine = !d.path.is_empty();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, CONNECT_ROW_H_NARROW), egui::Sense::hover());
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, theme::pane_brd()));
    let mut row = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(6.0, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut row;
    ui.spacing_mut().item_spacing.x = 10.0;
    if crate::frameworks::icon(ui, &d.framework, 36.0).is_none() {
        ui.allocate_exact_size(egui::vec2(36.0, 36.0), egui::Sense::hover());
    }
    let button = egui::vec2(42.0, 42.0);
    let actions_w = if mine { 2.0 * button.x + 18.0 } else { button.x + 10.0 };
    let text_w = (ui.available_width() - actions_w).max(80.0);
    ui.allocate_ui_with_layout(egui::vec2(text_w, CONNECT_ROW_H_NARROW), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(text_w);
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.add_space(12.0);
        ui.add(egui::Label::new(RichText::new(connect_name(d)).size(16.5).color(theme::text_strong())).truncate());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            ui.label(RichText::new(crate::frameworks::mark(&d.framework).map_or("Not detected", |m| m.name)).size(13.5).color(theme::text_strong()));
            let (word, tip) = access_words(level);
            if !word.is_empty() {
                egui::Frame::new().fill(theme::primary().gamma_multiply(0.16)).corner_radius(9).inner_margin(egui::Margin::symmetric(6, 0)).show(ui, |ui| {
                    ui.label(RichText::new(word).size(12.5).strong().color(theme::primary()));
                }).response.on_hover_text(tip);
            }
            let owner = if mine { "you".to_owned() } else { d.owner.clone() };
            if !owner.is_empty() {
                ui.add(egui::Label::new(RichText::new(owner).size(13.5).color(theme::text())).truncate());
            }
        });
        let when = if d.modified > 0 {
            format!("changed {} ago · ", age_words(now - d.modified as i64))
        } else if d.created > 0 {
            format!("created {} · ", date_words(d.created as i64))
        } else {
            String::new()
        };
        ui.add(egui::Label::new(RichText::new(format!("{when}{}", d.host)).size(12.5).color(theme::text())).truncate());
    });
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        if ui.add(egui::Button::new(RichText::new("↗").size(18.0)).min_size(button)).on_hover_text(format!("open https://{}/ in a new tab", d.host)).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://{}/", d.host)));
        }
        if mine && ui.add(egui::Button::new(RichText::new("✎").size(18.0)).min_size(button)).on_hover_text("edit in the IDE").clicked() {
            edit = Some(d.clone());
        }
    });
    edit
}

/// An age as a person says it: "4 min", "3 h", "12 days", "5 months", "2 years".
pub(crate) fn age_words(secs: i64) -> String {
    let s = secs.max(0);
    let (n, unit) = match s {
        0..=59 => return "under a minute".to_owned(),
        60..=3599 => (s / 60, "min"),
        3600..=86399 => (s / 3600, "h"),
        86400..=5_183_999 => (s / 86400, "day"),
        5_184_000..=63_071_999 => (s / 2_592_000, "month"),
        _ => (s / 31_536_000, "year"),
    };
    match unit {
        "min" | "h" => format!("{n} {unit}"),
        _ => format!("{n} {unit}{}", if n == 1 { "" } else { "s" }),
    }
}

/// A unix time as a date, "Oct 20, 2026" (UTC), from the civil-from-days
/// algorithm: no date library in the browser build.
fn date_words(t: i64) -> String {
    let z = t.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    format!("{} {day}, {year}", M[(month - 1) as usize])
}

#[cfg(test)]
mod connect_tests {
    #[test]
    fn dates_and_ages_read_as_a_person_says_them() {
        assert_eq!(super::date_words(0), "Jan 1, 1970");
        assert_eq!(super::date_words(1_792_454_400), "Oct 20, 2026");
        assert_eq!(super::date_words(951_782_400), "Feb 29, 2000");
        assert_eq!(super::age_words(30), "under a minute");
        assert_eq!(super::age_words(3 * 3600), "3 h");
        assert_eq!(super::age_words(86_400), "1 day");
        assert_eq!(super::age_words(12 * 86_400), "12 days");
        assert_eq!(super::age_words(90 * 86_400), "3 months");
        assert_eq!(super::age_words(800 * 86_400), "2 years");
    }
}

/// A registry level as Connections names it.
fn level_word(l: &str) -> String {
    match l {
        "public" => "Public".into(),
        "login" => "Signed in".into(),
        "owner" => "Owner and shared".into(),
        "admin" => "Admins".into(),
        "password" => "Site password".into(),
        "shared" => "Shared with you".into(),
        "" => "Not in the registry".into(),
        other => other.to_owned(),
    }
}

/// What a site did for a stranger (host.rs site_gate), in words.
fn gate_word(g: &str) -> String {
    match g {
        "open" => "it opens for anyone".into(),
        "login" => "it asks to sign in".into(),
        "password" => "it asks for a site password".into(),
        "down" => "nothing answers".into(),
        other => format!("it answered {other}"),
    }
}

/// Whether what a stranger met (`gate`) is what the level promises: public
/// opens, the signed-in levels send to sign-in, a site password asks for it.
fn gate_fits(level: &str, gate: &str) -> bool {
    match level {
        "public" => gate == "open",
        "login" | "owner" | "admin" | "shared" => gate == "login",
        "password" => gate == "password",
        _ => gate != "down" && !gate.starts_with("unknown"),
    }
}
