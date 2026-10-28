//! What the UI and the machine say to each other — the same types whether the
//! machine is in-process (the desktop app) or across HTTP (`cove serve`,
//! the UI running in a browser). backend.rs carries them; host.rs answers
//! them; server.rs puts them on /api.

use serde::{Deserialize, Serialize};

/// A typed question. The IDE runs code only by handing it to a kernel
/// (kernel.rs) in one of `Lang`'s languages: R, Python, or the Prefix shell.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "q", rename_all = "snake_case")]
pub enum Query {
    Probe,
    /// Where things live and which interpreters the kernels start in — asked,
    /// never assumed (host.rs keeps them in ~/.config/ndexr/cove.json there).
    Settings,
    SaveSettings { settings: Settings },
    /// The stack's Lmod modules that bring an R or a Python: env/, R/, Python/
    /// (Inlet 0004, the module picker).
    Modules,
    /// Hand code to a kernel; it starts on first use. Returns at once — the
    /// output arrives through `Poll`.
    Eval {
        lang: Lang,
        code: String,
        /// The file it came from and the line it starts on (the editor):
        /// R keeps them as the code's source references, so the debugger
        /// says - and the editor highlights - where it stopped in that file.
        #[serde(default)]
        at: Option<(String, usize)>,
    },
    /// Everything the kernel said after transcript line `since`, and its state.
    Poll { lang: Lang, since: usize },
    /// What may finish the word before `cursor` (characters) in `line`,
    /// asked of the live session so it knows the session's objects and the
    /// environment's commands (operator, 2026-10-15: "each language needs its
    /// autocomplete running"). Only while the kernel is idle.
    Complete { lang: Lang, line: String, cursor: usize },
    /// An ask in the R, Python or shell input - Agent ticked, or spoken (operator, 2026-10-18):
    /// code in `lang` for what was asked, written with the console's recent
    /// lines and environment as `context`. The language model is OpenAI's, not
    /// Claude - this is the shell's quick helper, not the Agents tab. The
    /// answer goes into the input for the person to read and run.
    Assist { lang: Lang, ask: String, context: String },
    Interrupt { lang: Lang },
    /// Kill and start clean, in the interpreter the settings name now.
    Restart { lang: Lang },
    /// Kill and leave stopped: a console the environment does not bring.
    Stop { lang: Lang },
    /// One of a kernel's plots, as base64 PNG.
    Plot { lang: Lang, n: usize },
    /// The interpreter's installed packages (R: installed.packages; Python: pip).
    Packages { lang: Lang },
    /// One object of a kernel's global environment as a table (the data
    /// viewer, R's `View()`); it arrives in `KernelView::table`.
    View { lang: Lang, name: String },
    /// The ndexr stack's CVMFS client on this machine: start setting it up
    /// (`ndexr install cvmfs`), push the person's proxy / cache settings to it,
    /// or try a proxy against our servers. Setup and apply run as jobs; their
    /// progress is `StackJob`.
    StackSetup,
    StackApply { cvmfs: CvmfsSettings },
    StackTest { proxy: String },
    StackJob,
    /// `quarto render` an .qmd / .Rmd / .ipynb in the picked modules, as a
    /// job; `RenderJob` reports it and names the HTML it wrote.
    Render { path: String },
    RenderJob,
    /// The ndexr domains the person can open: under the project folder (a
    /// cell starts there, in its domains/), one level of owner folders deep.
    ListDomains,
    /// The slim half of route.ndexr.io's provisioning: lay down
    /// `<parent>/<host>/` (module.r, CONTEXT.md, a git repo) for the person to
    /// work in. Publishing it - DNS, certificate, vhost - stays route's.
    /// `rust`: start it as a Rust app instead (templates/rust-app: a minimal
    /// Cove - egui UI, a small server, a media core over FFmpeg).
    /// `starter` names the template (starters.rs); empty is the older client's
    /// choice, read from `rust`.
    CreateDomain { host: String, parent: String, #[serde(default)] rust: bool, #[serde(default)] starter: String },
    /// Name `modules` as the environment of the folder `dir` (its
    /// `.ndexr-env`), or "" to stop naming one; returns what it names now.
    PinEnv { dir: String, modules: String },
    /// git.ndexr.io's per-owner sweep: `ndexr bulk-commit <root>`, which
    /// commits and pushes every repo under the person's domains folder, as a
    /// job; `PushJob` reports it.
    PushAll { root: String },
    PushJob,
    /// The build machines (boxes.rs), and one of them kicked off, its build
    /// stopped, shut down or terminated (`verb`: up, down, shutdown,
    /// terminate) - as n-box.ndexr.io does. Answered by `cove multiuser`
    /// for an ndexr admin only; every session and the desktop app refuse.
    Boxes,
    /// The machine Cove runs on: CPU, memory and disk (vitals.rs), as the
    /// ndexr bar shows admins. Answered by `cove multiuser` for an ndexr admin
    /// only.
    HostVitals,
    BoxAct { server: String, verb: String },
    BoxJob { server: String },
    /// git.ndexr.io's Archive (retire): prove the domain's whole history is in
    /// S3, then remove its working tree - as a job; `ArchiveJob` reports it.
    Archive { host: String },
    ArchiveJob,
    /// What stands in front of https://<host>/ for a visitor: "open", "login"
    /// (the ndexr sign-in, which a signed-in person passes), "password" (a
    /// site page-password, whose form will not render inside a frame), "down",
    /// or "http <code>".
    SiteGate { host: String },
    /// The person's own Claude (setup.rs, operator 2026-10-15: "a clear
    /// end-to-end setup on startup to get their claude credentials working
    /// with cove"): where they stand, connect a token or key, remove it. The
    /// same store and wording as console.ndexr.io's Claude tab
    /// (credentials.ndexr.io/connect.r). A desktop app asks api.ndexr.io
    /// instead (account.rs `claude`).
    ClaudeStatus,
    ClaudeConnect { token: String },
    ClaudeForget,
    /// The Account tab (2026-10-15, operator: port credentials.ndexr.io,
    /// api.ndexr.io's keys and box/my/recent into Cove for all users): the
    /// person's account, cell, domains, API keys, key events and Claude, from
    /// api.ndexr.io/account.r - through the platform R here, through
    /// api.ndexr.io by the app's key on a desktop (account.rs `overview`).
    Account,
    /// Revoke one of the person's API keys; answers the account again.
    RevokeKey { key_id: String },
    /// The domain's agent - exec.ndexr.io's workflow: one turn through the
    /// platform's own host_run_claude, in the domain's thread (_domain/<host>),
    /// resuming its last session. Returns at once; `AgentPoll` streams it.
    /// `cwd`: a folder inside the domain to run at instead of the domain's
    /// own (the Files tab's file asks run at the file's folder); such a run
    /// starts a fresh session, since a session belongs to the folder it ran in.
    AgentRun {
        domain: String,
        prompt: String,
        model: String,
        #[serde(default)]
        cwd: Option<String>,
        /// How the turn may act - "build" (Auto: it does the work), "plan"
        /// (thinks it through, changes nothing) or "ask" (answers, read-only);
        /// exec.ndexr.io's modes. Absent is "build".
        #[serde(default)]
        mode: Option<String>,
    },
    /// A run's stream from byte `offset`: what it said and did since.
    AgentPoll { id: String, offset: u64 },
    AgentStop { id: String },
    /// The domain thread's recent turns, as the in-app chat shows them.
    AgentHistory { domain: String },
    /// The Terminal tab (term.rs): a real terminal - bash on a pty, as the
    /// person, so vim, ranger, top and claude run in it. `id` 0 attaches to
    /// the session's terminal, starting one if there is none. Answers what it
    /// printed after byte `offset`, waiting up to a quarter second for some;
    /// `rows` × `cols` resizes it.
    Term { id: u32, offset: u64, rows: u16, cols: u16 },
    /// Keys typed into terminal `id`, as the bytes an xterm sends.
    TermWrite { id: u32, input: String },
    /// End terminal `id` and what runs in it; the next `Term` with id 0 starts afresh.
    TermKill { id: u32 },
    /// Help › Send a request: one request on complaints.ndexr.io, as the
    /// site's REQUEST dialog files it, with what was pasted, dropped or
    /// picked into it. Answers its id.
    SendRequest {
        domain: String,
        what: String,
        trying: String,
        blocked: bool,
        #[serde(default)]
        files: Vec<Attachment>,
    },
    /// Messages (inbox.rs): the person's requests and their threads (an
    /// admin's: everyone's, with `all`), who is live and public, and the chat
    /// with `chat` (a username, "" = none). Also stamps them as here.
    Inbox {
        #[serde(default)]
        all: bool,
        /// A username, or "#<id>" for a group's chat.
        #[serde(default)]
        chat: String,
        /// Chat's search: public people whose name holds it (InboxView.found).
        #[serde(default)]
        find: String,
    },
    /// Something done in Messages: a message on a request, its status, the
    /// public switch, a direct message, a thread read.
    InboxAct { act: InboxAct },
    /// A file sent in Chat, for its sender or the person it went to.
    DmFile { id: i64 },
    /// How the applications reach people (host.rs `serving`, the atlas's
    /// serving column): each apex's DNS and address, the router and its
    /// tunnel, this machine, nginx, and whether each upstream the domains'
    /// targets name answers. An ndexr admin only.
    Serving { apexes: Vec<String>, targets: Vec<String> },
    /// Packages (pkgflow.rs): every language set through the whole cycle -
    /// the kit's manifest, the build machines, S3, this host's library, the
    /// CVMFS module, the sessions loading it - and, for `set` ("" = none),
    /// each of its packages at each stage. Answered by the root server, which
    /// reads the kit and the sessions; the session counts are an admin's only.
    PackageFlow {
        set: String,
        /// A package of `set` to trace: its dependencies, what needs it, the
        /// toolchain under it, and where it is at each stage.
        #[serde(default)]
        package: String,
    },
    /// Go's Recap (recap.rs): the last `minutes` of work everywhere, as
    /// sections and as words to read aloud. An ndexr admin only.
    Recap { minutes: u32 },
    /// Go's Briefing (brief.rs): the recap as a spoken episode, written and
    /// voiced on the server. Answers with where it has got to; ask again
    /// until `state` is ready. `fresh` makes a new one. An ndexr admin only.
    RecapBrief { minutes: u32, #[serde(default)] fresh: bool },
    ListDir { path: String },
    ReadFile { path: String },
    /// Find in files (host.rs `grep`): every line under `root` that matches.
    Grep { root: String, query: String, case: bool, regex: bool },
    /// Put the person's Rust app live (app/assets/deploy-app.sh, run by `cove
    /// multiuser`): built and served as them, routed at https://<host>/.
    DeployApp { host: String },
    /// That deploy's progress: its lines, running, and how it ended.
    DeployJob { host: String },
    /// Build, Up or Down a project (apps.rs): its own build.sh / up.sh /
    /// down.sh, run in the person's session as them - the app contract
    /// (testy.ndexr.io/APP-CONTRACT.md). verb: "build" | "up" | "down".
    AppAction { host: String, verb: String },
    /// What the project is (its app.env, or inferred) and whether it answers.
    AppStatus { host: String },
    /// That project's Build / Up / Down job as far as it got.
    AppJob { host: String },
    /// A project's saved versions, newest first (Versions): what the autosave
    /// service and Save committed, mirrored to the private S3 git archive.
    Versions { host: String },
    /// Put a saved version back: what is there now is saved as a version
    /// first, then the project's files become that version's, as a new one.
    VersionRestore { host: String, sha: String },
    /// Route a published domain to its app's APP_PORT (app/assets/route-app.sh,
    /// run as root by `cove multiuser`): the one step of Up a person's session
    /// cannot take itself.
    AppRoute { host: String },
    /// Who may open one of my domains (Project › Access): its level, and,
    /// with `level` given, set it - "public" (everyone), "login" (anyone
    /// signed in), "owner" (me and the people and groups I share it with),
    /// "admin" for an admin only (app/assets/set-access.sh, run by `cove
    /// multiuser`). An empty `level` only reads.
    AppAccess {
        host: String,
        #[serde(default)]
        level: String,
    },
    /// Delete's last step, after Archive removed the folder: give up the
    /// name - registry row, ownership and vhost (assets/release-project.sh,
    /// run by `cove multiuser`).
    ProjectRelease { host: String },
    /// An EasyBuild module's easyconfig (host.rs `easyconfig`): the one it
    /// was built from when it is installed, else the suite's recipe for it.
    Easyconfig { name: String, version: String },
    /// Each path's fingerprint - modified time and size, "" when it is not
    /// there - asked every second for the open files and the Files folder, so
    /// what changes on disk shows without a refresh (operator, 2026-10-14).
    Stamps { paths: Vec<String> },
    /// One fingerprint of a domain's site sources (host.rs `site_stamp`):
    /// asked every few seconds while a domain is open, so a change made by
    /// the agent, the Terminal or anything else reloads App - not only a save
    /// in Cove's own editor (operator, 2026-10-16).
    SiteStamp { path: String },
    /// A star's exact source as a domain project of the person's own
    /// (names.rs, source.rs): `upstream/<version>` the pristine source,
    /// `work/<version>` checked out. `kind` is the catalog's (prefix,
    /// easybuild, R, Python); `sets` the sets that list it; `env` the
    /// environment it is opened from, "" for the session's own.
    SourceOpen {
        kind: String,
        name: String,
        version: String,
        #[serde(default)]
        sets: Vec<String>,
        #[serde(default)]
        env: String,
    },
    /// Register `hosts` (a paradigm parent, then its project) as the signed-in
    /// person's, each only when a domain above it is already theirs - the
    /// same rule the agent applies to a new subdomain (host.rs CLAIM_R).
    DomainClaim { hosts: Vec<String> },
    /// Put a domain of the person's on the web - registry row, DNS, certificate,
    /// nginx vhost - with the platform's own provisioner (site_provision.r
    /// provision_start), as route.ndexr.io does (operator, 2026-10-18: "new
    /// sites start w/ a 404, they need to start with something running").
    /// Its module.r is never overwritten. Starts the slow half in the
    /// background; the site answers within a minute or two.
    PublishDomain { host: String },
    /// A project of theirs that is not on this machine: clone its saved
    /// repository into their domains folder, every branch, and answer it as a
    /// domain to open.
    DomainRestore { host: String },
    /// The workspace a Files path is in: what changed against upstream, its
    /// checkpoints and candidates (source.rs `status`).
    SourceStatus { path: String },
    /// Commit the workspace's changes on `work`.
    SourceCheckpoint { path: String, message: String },
    /// Build the workspace's checkpoint, in the session's environment, as a
    /// candidate of its own (job "candidate").
    CandidateBuild { path: String },
    /// A snippet against the published package, then against the candidate.
    CandidateTest { dir: String, code: String },
    /// The candidate job: building or testing, its output so far.
    CandidateJob,
    /// A new environment revision from an accepted candidate (source.rs
    /// `revise`): a module of the person's own, written once.
    EnvRevise { dir: String },
    WriteFile { path: String, text: String },
    /// The Data tab (db.rs): a SQLite database's tables, views, columns and
    /// row counts. `path` "" is the person's own database (db.rs `own_db`),
    /// made on first open. Every Db* query reads only: the connection is
    /// opened read-only.
    DbSchema { path: String },
    /// One page of a table or view: `filter` matches any column as text,
    /// `sort` names a column. `count` asks for the matching total too (once
    /// per table / filter - it costs a scan).
    DbRows {
        path: String,
        table: String,
        offset: u64,
        limit: u64,
        #[serde(default)]
        filter: String,
        #[serde(default)]
        sort: Option<String>,
        #[serde(default)]
        desc: bool,
        #[serde(default)]
        count: bool,
    },
    /// One row in full (the grid cuts long values): by rowid.
    DbRow { path: String, table: String, rowid: i64 },
    /// A read-only statement of the person's own (SELECT, WITH, PRAGMA …).
    DbSql { path: String, sql: String },
    /// The person's storage against their plan (storage.rs): None where no
    /// `cove multiuser` measures it (a desktop, a cell). `refresh` asks for a
    /// new measure - after removing something - within ~10 s.
    Storage {
        #[serde(default)]
        refresh: bool,
    },
    /// Over the cap, "Let the agent clear it": removes only what rebuilds
    /// itself (storage.rs `clear`) and asks for a new measure.
    StorageClear,
    /// The env manifests (manifest.rs): every one with its counts and what is
    /// built here, and one with its records - from the ndexr CLI's kit, or
    /// repo.ndexr.io/env for a named set the kit lacks.
    Manifests,
    Manifest { name: String },
    /// Software › Install, the desktop app's: whether this machine can build
    /// the stack, start building the picked environments with the ndexr CLI
    /// (a job, kind "install"), its progress, Stop; and the person's builds
    /// as their account has them (api.ndexr.io/v1/builds, their key).
    InstallInfo,
    Install { install: crate::manifest::InstallRequest },
    InstallJob,
    InstallStop,
    MyBuilds,
}

/// What a person's files take on ndexr's machine against their plan, as
/// `cove multiuser` measured them (storage.rs).
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct StorageView {
    /// billing's tier: "free", "paid" or "admin".
    pub plan: String,
    /// Bytes the plan allows; 0 = no cap (paid, admin).
    pub cap: u64,
    pub used: u64,
    /// Unix seconds of the measure; 0 = not measured yet.
    pub measured: u64,
    /// The biggest folders and files, two levels down, biggest first.
    pub top: Vec<StorageItem>,
    /// Unix seconds the keeper first saw them over the cap; 0 = under.
    #[serde(default)]
    pub over_since: u64,
    /// Seconds of grace left to get back under before growth stops, as of
    /// the read (storage.rs `of`); 0 once it has run out.
    #[serde(default)]
    pub grace_left: u64,
}

/// What `StorageClear` removed.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct StorageCleared {
    pub freed: u64,
    pub removed: Vec<String>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct StorageItem {
    pub path: String,
    pub bytes: u64,
}

/// How long someone over the cap has to clear it before uploads, new domains
/// and ordinary agent turns stop (operator, 2026-10-18: "give user 5 minutes
/// to clear").
pub const STORAGE_GRACE_SECS: u64 = 300;

impl StorageView {
    /// Past the cap at all - in grace or not.
    pub fn exceeded(&self) -> bool {
        self.cap > 0 && self.used > self.cap
    }

    /// Past the cap and out of grace: growth is held back.
    pub fn over(&self) -> bool {
        self.exceeded() && self.grace_left == 0
    }
}

/// Where a person subscribes: billing's screens, mounted in console.
pub const SUBSCRIBE_URL: &str = "https://console.ndexr.io/#billing";

/// 5000000000 → "5.0 GB" (decimal, as billing states the allowance).
pub fn gb(bytes: u64) -> String {
    if bytes < 100_000_000 {
        format!("{} MB", bytes / 1_000_000)
    } else {
        format!("{:.1} GB", bytes as f64 / 1e9)
    }
}

/// A database as the Data tab lists it.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct DbSchema {
    pub path: String,
    pub bytes: u64,
    pub tables: Vec<DbTable>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct DbTable {
    pub name: String,
    /// "table" or "view".
    pub kind: String,
    /// None when it could not be counted (a view that fails, a locked file).
    pub rows: Option<i64>,
    pub cols: Vec<DbCol>,
    /// Rows have a rowid (a plain table), so one row can be asked for in full.
    pub rowid: bool,
    /// Its indexes: (name, CREATE INDEX …).
    pub indexes: Vec<(String, String)>,
    /// Its own CREATE statement.
    pub sql: String,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct DbCol {
    pub name: String,
    /// The declared type, as written ("" when none).
    pub ty: String,
    pub pk: bool,
    pub notnull: bool,
    pub default: Option<String>,
}

/// Rows, as text: None is NULL. Values longer than `DbPage::CUT` characters
/// come cut, ending in "…"; a blob comes as "‹blob N bytes›".
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct DbPage {
    pub cols: Vec<String>,
    pub types: Vec<String>,
    /// Each column holds only numbers on this page (they align right).
    pub nums: Vec<bool>,
    pub rows: Vec<Vec<Option<String>>>,
    /// Each row's rowid, when the table has them.
    pub rowids: Vec<i64>,
    /// Matching rows in all, when asked for (`count`).
    pub total: Option<i64>,
    /// More rows were there than came (a statement's results stop at `SQL_MAX`).
    pub more: bool,
    /// How long the database took, in milliseconds.
    pub ms: f64,
}

impl DbPage {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // native only (db.rs)
    pub const CUT: usize = 2000;
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub const SQL_MAX: usize = 5000;
}

/// A database file the Data tab opens rather than the editor, by extension.
pub fn is_sqlite(path: &str) -> bool {
    path.rsplit_once('.').is_some_and(|(_, e)| matches!(e.to_ascii_lowercase().as_str(), "sqlite" | "sqlite3" | "db" | "db3" | "s3db" | "sl3"))
}

/// The consoles: R and Python, the languages the IDE is for from the start,
/// and the Prefix shell - bash inside the stack's Gentoo Prefix with `ml`, the
/// pimp's Terminal for everything that is neither (2026-10-13).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    #[default]
    R,
    Python,
    Shell,
    /// Julia, a session of its own (operator, 2026-10-20: "cove needs julia
    /// support like we have for R and Py and Shell"): `ml Julia` from the
    /// stack, else julia on PATH.
    Julia,
}

impl Lang {
    /// The sessions that exist: R (with Python inside it, through reticulate)
    /// and the Prefix shell. Each has a console.
    pub const CONSOLES: [Lang; 3] = [Lang::R, Lang::Shell, Lang::Julia];

    /// The session code in this language runs in. Python is not a process of
    /// its own (operator, 2026-10-13: "a multilingual single syntax ide
    /// through reticulate"): it runs inside R.
    pub fn session(self) -> Lang {
        match self {
            Lang::Python => Lang::R,
            l => l,
        }
    }

    pub fn word(self) -> &'static str {
        match self {
            Lang::R => "R",
            Lang::Python => "Python",
            Lang::Shell => "Shell",
            Lang::Julia => "Julia",
        }
    }

    /// The console prompt, as each interpreter draws its own.
    pub fn prompt(self) -> &'static str {
        match self {
            Lang::R => ">",
            Lang::Python => ">>>",
            Lang::Shell => "$",
            Lang::Julia => "julia>",
        }
    }

    /// The prompt for a statement's second line on.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // native only (kernel.rs / stack.rs)
    pub fn continuation(self) -> &'static str {
        match self {
            Lang::R => "+",
            Lang::Python => "...",
            Lang::Shell => ">",
            Lang::Julia => "      ",
        }
    }

    /// From a file name: .R/.r → R, .py → Python, .sh → the shell; a
    /// document (.qmd/.Rmd) is R outside its chunks.
    pub fn of_path(p: &str) -> Option<Lang> {
        let ext = p.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
        match ext.as_str() {
            "r" | "rmd" | "qmd" | "rprofile" => Some(Lang::R),
            "py" | "pyw" => Some(Lang::Python),
            "sh" | "bash" => Some(Lang::Shell),
            "jl" => Some(Lang::Julia),
            _ => None,
        }
    }

    /// A chunk header's engine: ```{r}, ```{python}, ```{bash}.
    pub fn of_engine(e: &str) -> Option<Lang> {
        match e.to_ascii_lowercase().as_str() {
            "r" => Some(Lang::R),
            "python" | "py" => Some(Lang::Python),
            "bash" | "sh" | "shell" => Some(Lang::Shell),
            "julia" | "jl" => Some(Lang::Julia),
            _ => None,
        }
    }
}

/// A Quarto or R Markdown document: chunks of R, Python and bash in prose,
/// each chunk run in its own language's console.
pub fn is_document(p: &str) -> bool {
    p.rsplit_once('.').is_some_and(|(_, e)| matches!(e.to_ascii_lowercase().as_str(), "qmd" | "rmd"))
}

/// What Render (Ctrl+Shift+K) turns into a page for the Viewer: a Quarto or R
/// Markdown document, a notebook, Markdown, a page already written, or the
/// file that names a whole site - a Quarto project, a Jupyter Book, MkDocs.
pub fn renders(p: &str) -> bool {
    let name = p.rsplit('/').next().unwrap_or(p).to_ascii_lowercase();
    is_document(p)
        || matches!(name.as_str(), "_quarto.yml" | "_quarto.yaml" | "_toc.yml" | "mkdocs.yml" | "mkdocs.yaml")
        || name.rsplit_once('.').is_some_and(|(_, e)| matches!(e, "ipynb" | "md" | "markdown" | "html" | "htm"))
}

/// The first local web address in `text` - what a server prints when it
/// starts (`quarto preview`, `shiny::runApp`, `jupyter-book`, `mkdocs serve`,
/// `python -m http.server` …), found the way Positron's app launcher and
/// RStudio's Quarto job find it. Given back as `http://127.0.0.1:<port>/<path>`.
pub fn local_url(text: &str) -> Option<String> {
    let mut rest = text;
    while let Some(i) = rest.find("http") {
        let tail = &rest[i..];
        let after = tail.strip_prefix("https://").or_else(|| tail.strip_prefix("http://"));
        rest = &rest[i + 4..];
        let Some(after) = after else { continue };
        let Some(hostport) = ["localhost:", "127.0.0.1:", "0.0.0.0:", "[::1]:", "[::]:"].iter().find_map(|h| after.strip_prefix(h)) else { continue };
        let digits = hostport.bytes().take_while(u8::is_ascii_digit).count();
        let Ok(port) = hostport[..digits].parse::<u16>() else { continue };
        if port == 0 {
            continue;
        }
        // The path runs to whitespace, a quote, a bracket or an escape.
        let path: String = hostport[digits..].chars().take_while(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | '<' | '>' | ')' | ']' | '\u{1b}' | '\u{7}')).collect();
        let path = path.trim_end_matches(['.', ',', ';']);
        let path = if path.starts_with('/') { path.to_owned() } else { "/".to_owned() };
        return Some(format!("http://127.0.0.1:{port}{path}"));
    }
    None
}

/// Where the Viewer shows `local` (a `local_url`) through Cove's server: its
/// /port/ proxy, RStudio Server's /p/ and Positron's /proxy/.
pub fn port_address(local: &str) -> Option<String> {
    let rest = local.strip_prefix("http://127.0.0.1:")?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let port: u16 = rest[..digits].parse().ok()?;
    let path = &rest[digits..];
    Some(format!("/port/{port}{}", if path.is_empty() { "/" } else { path }))
}

/// One line of a console transcript.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Line {
    pub kind: LineKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineKind {
    /// What was typed (or sent from the source pane), prompt included.
    Input,
    Output,
    Warning,
    Error,
    /// The IDE speaking: kernel started, restarted, exited.
    System,
}

/// One object in the kernel's global environment (the Environment pane).
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct EnvRow {
    pub name: String,
    pub class: String,
    pub value: String,
}

/// A kernel as the machine sees it after `since`.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct KernelView {
    pub lines: Vec<Line>,
    /// Index of the next transcript line — the next poll's `since`.
    pub next: usize,
    pub busy: bool,
    pub alive: bool,
    /// "R 4.6.1 · env/r4.6_py3.13-release", once the kernel has said.
    pub banner: String,
    pub env: Vec<EnvRow>,
    /// How many plots the kernel has drawn (fetch one with `Query::Plot`).
    pub plots: usize,
    /// The kernel's working directory.
    pub cwd: String,
    /// The last object shown in the data viewer, and how many have been shown
    /// (a new one opens a viewer tab).
    #[serde(default)]
    pub table: Option<TableView>,
    #[serde(default)]
    pub tables: u64,
    /// The last thing R handed its viewer (`options(viewer=)`, htmlwidgets,
    /// `shiny.launch.browser`) as a Viewer address, and how many it has handed.
    #[serde(default)]
    pub view: String,
    #[serde(default)]
    pub views: u64,
    /// The session is waiting for console input while running something: R's
    /// `Browse[1]> ` (browser(), debug(), debugonce()), pdb's `(Pdb) `, or a
    /// readline()/input() prompt. What is typed then goes to it, as in
    /// RStudio's console.
    #[serde(default)]
    pub prompt: String,
    /// Which debugger holds that prompt: "R", "Python" or "".
    #[serde(default)]
    pub debug: String,
    /// Where R's debugger is stopped: the file ("" for code typed in the
    /// console), the line, and the stopped function's source from line
    /// `debug_from` on - what the editor shows when there is no file.
    #[serde(default)]
    pub debug_file: String,
    #[serde(default)]
    pub debug_line: usize,
    #[serde(default)]
    pub debug_from: usize,
    #[serde(default)]
    pub debug_src: Vec<String>,
}

/// A data frame (R data.frame / tibble / matrix, pandas DataFrame, numpy
/// 2-D array) as the data viewer shows it: at most `ROWS` rows.
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct TableView {
    pub name: String,
    pub nrow: usize,
    pub ncol: usize,
    pub cols: Vec<String>,
    /// Each column's type (R class, pandas dtype).
    pub types: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl TableView {
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // native only (kernel.rs / stack.rs)
    pub const ROWS: usize = 1000;
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct PlotData {
    pub png_b64: String,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Package {
    pub name: String,
    pub version: String,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub dir: bool,
    pub bytes: u64,
    /// Last modified, seconds since the epoch (UTC); 0 if unknown.
    #[serde(default)]
    pub modified: i64,
}

/// One domain: a folder named for its host (`blog.ndexr.io`), whose
/// `module.r` the platform serves live at https://<host>/.
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct DomainInfo {
    pub host: String,
    pub path: String,
    /// CONTEXT.md's title line, if it has one.
    #[serde(default)]
    pub title: String,
    /// Has a module.r: there is a site to view.
    #[serde(default)]
    pub module: bool,
    /// What serves https://<host>/, from its nginx.conf: "module", "port:8001",
    /// "cell", "remote:…", … ("" = no nginx.conf). Set by the
    /// `# ndexr-target:` marker src/r/nginx/target.r writes, else read off
    /// the HTTPS `location /`.
    #[serde(default)]
    pub target: String,
    /// The environment its `.ndexr-env` names ("" = none): the one it runs in
    /// whoever opens it (envs.rs; RStudio's project `RVersion`).
    #[serde(default)]
    pub env: String,
    /// What kind of application answers https://<host>/ (host.rs app_kind):
    /// "shiny" (its module.r, on the platform or in the owner's cell),
    /// "rust" (its own Rust app - app/Cargo.toml - on a port), "process"
    /// (some other program on a port), "redirect", "" = not known. The atlas
    /// tints a domain's star by it.
    #[serde(default)]
    pub app: String,
    /// Whose it is: the owner's username, for a signed-in viewer (host.rs
    /// visible_domains); "" when not known or not signed in.
    #[serde(default)]
    pub owner: String,
    /// When its folder last changed (unix seconds): the newest modification
    /// among its top-level entries (host.rs domain_at). Orders the projects
    /// offered as recent before any has been opened here.
    #[serde(default)]
    pub modified: u64,
    /// Its language (apps.rs language: r, python, julia, rust, node; "" when
    /// nothing says).
    #[serde(default)]
    pub language: String,
    /// The environment its language runs in on this machine (host.rs
    /// language_env): env/R/…, env/Python/…, env/Julia/…, env/Rust/… or
    /// nodejs/…; "" for a language with none. A project's .ndexr-env (`env`)
    /// wins over it (operator, 2026-10-26: "make sure the environment is
    /// selected at the beginning automatically for the user").
    #[serde(default)]
    pub env_default: String,
    /// What it is built with, as the key of its mark (apps.rs framework:
    /// shiny, fastapi, react, wasm, … or its language; "" = not known).
    #[serde(default)]
    pub framework: String,
    /// When it was created (unix seconds): the registry's created_at, as
    /// host.rs visible_domains reads it; 0 = not known.
    #[serde(default)]
    pub created: u64,
}

/// An app kind as a person reads it.
pub fn app_kind_word(kind: &str) -> &'static str {
    match kind {
        "shiny" => "Shiny module",
        "rust" => "Rust app",
        "process" => "its own process",
        "redirect" => "a redirect",
        _ => "",
    }
}

/// One thing an agent run did: "text" (it said), "tool" (it ran a tool:
/// name and a one-line summary), "result" (its final answer), "error".
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentEvent {
    pub kind: String,
    pub text: String,
}

/// A run's stream since an offset.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct AgentChunk {
    pub events: Vec<AgentEvent>,
    /// Where the next poll continues.
    pub offset: u64,
    pub done: bool,
    /// The run's cost, once it reported one.
    #[serde(default)]
    pub cost: Option<f64>,
}

/// A terminal's output since an offset (`Query::Term`).
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct TermChunk {
    /// The terminal this is (the session's, when 0 was asked).
    pub id: u32,
    /// Where the next poll continues.
    pub offset: u64,
    /// Output to feed the screen: raw terminal bytes, whole characters.
    pub data: String,
    /// `data` is the whole screen as it stands: start a clean one first.
    pub reset: bool,
    pub alive: bool,
    /// Whose Claude `claude` runs on in it.
    pub note: String,
}

/// One turn of a domain thread: what was asked, and what came back.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct AgentTurn {
    pub id: String,
    pub prompt: String,
    pub events: Vec<AgentEvent>,
    pub done: bool,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Listing {
    /// The absolute folder listed (after resolving ~ and ..).
    pub path: String,
    pub entries: Vec<DirEntry>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct FileText {
    pub path: String,
    pub text: String,
}

/// The modules the picker offers, newest first within each family.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Modules {
    /// env/r4.6_py3.13-release — an R, a Python and a site library together.
    pub env: Vec<String>,
    pub r: Vec<String>,
    pub python: Vec<String>,
    /// Every other module, to layer on top of the env (GDAL, CmdStan …).
    #[serde(default)]
    pub other: Vec<String>,
    /// The use.sh they were listed through, or why there is none.
    pub source: String,
}

/// Completions for a word: where it starts in the line (characters) and
/// what may replace it.
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Completion {
    pub start: usize,
    pub items: Vec<String>,
}

/// The machine's settings. Empty means "not chosen yet": the app asks.
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Where the kernels start and the Files pane opens. Empty: home.
    #[serde(default)]
    pub project_dir: String,
    /// The ONE Lmod module the whole IDE runs in - R, Python and the shell
    /// alike ("" = the R / python3 on PATH, the shell on the bare stack).
    /// Operator, 2026-10-13: "a module switcher in the main top panel not
    /// hidden and one at a time for the whole ide not per language" - an env/
    /// module brings an R, a Python and a site library together, so one pick
    /// keeps R, reticulate's Python, the shell and a render on one stack.
    #[serde(default)]
    pub module: String,
    /// A stack built or copied to a folder of its own (one with use.sh, or
    /// <triple>/use.sh), used instead of the CVMFS mount. "" = /opt/ndexr/stack,
    /// else ~/.ndexr/stack, else none: R, Python and the shell from PATH
    /// (operator, 2026-10-19: "cove to work without a connection to ndexr as
    /// a standalone ide ... local with the languages without cvmfs, a local
    /// cvmfs build, or a connection to the server").
    #[serde(default)]
    pub stack_dir: String,
    /// Read from settings saved before the one module, never written: the
    /// first of them that is set becomes `module` (`one_module`).
    #[serde(default, skip_serializing)]
    pub r_module: String,
    #[serde(default, skip_serializing)]
    pub py_module: String,
    #[serde(default, skip_serializing)]
    pub sh_module: String,
    /// The person's own CVMFS client settings; the repository, servers and
    /// keys come from us and are not settings.
    #[serde(default)]
    pub cvmfs: CvmfsSettings,
    /// How often, in seconds (1-3), the open files and the Files folder are
    /// checked for changes on disk (operator, 2026-10-14: "up to every three
    /// seconds (refresh rate in settings)"). 0 - never set - is 3.
    #[serde(default)]
    pub refresh_secs: u8,
}

impl Settings {
    /// Seconds between checks for changed files (`refresh_secs`, 1-3; 3 unset).
    pub fn refresh_every(&self) -> f64 {
        match self.refresh_secs {
            n @ 1..=3 => n as f64,
            _ => 3.0,
        }
    }

    /// The module every kernel starts in; the same for each language.
    pub fn module(&self, lang: Lang) -> &str {
        // Julia's own environment when the project's is one (env/Julia/…:
        // Julia with its package set); an R/Python environment carries no
        // Julia, so then the stack's own module.
        if lang == Lang::Julia && !self.module.split_whitespace().next().is_some_and(|m| m.starts_with("env/Julia/")) {
            return "Julia";
        }
        &self.module
    }

    /// Settings saved per language (before 2026-10-13) as one module: R's
    /// pick, else Python's, else the shell's.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // native only (host.rs)
    pub fn one_module(mut self) -> Self {
        if self.module.trim().is_empty() {
            self.module = [&self.r_module, &self.py_module, &self.sh_module].into_iter().find(|m| !m.trim().is_empty()).cloned().unwrap_or_default();
        }
        self.r_module.clear();
        self.py_module.clear();
        self.sh_module.clear();
        self
    }
}

/// What the person may set on their CVMFS client (operator, 2026-10-13: "allow
/// them to set up settings for cvmfs ... the environment will come from us").
/// Empty = the client's own default. Written to the repository's .local file,
/// the client's standard override (config.d/stack.ndexr.io.local).
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CvmfsSettings {
    /// CVMFS_HTTP_PROXY: "DIRECT", or proxies in CVMFS's own syntax
    /// (`http://p1:3128|http://p2:3128;DIRECT`).
    #[serde(default)]
    pub proxy: String,
    /// CVMFS_CACHE_BASE: where the client caches what it read (absolute).
    #[serde(default)]
    pub cache_base: String,
    /// CVMFS_QUOTA_LIMIT in MB ("" = the client's default).
    #[serde(default)]
    pub quota_mb: String,
}

impl CvmfsSettings {
    /// Each value reaches a root-owned config file, so only what CVMFS itself
    /// accepts goes through.
    pub fn check(&self) -> Result<(), String> {
        let p = self.proxy.trim();
        if !p.is_empty() && !p.chars().all(|c| c.is_ascii_alphanumeric() || ".:/_-|;@[]".contains(c)) {
            return Err(format!("not a CVMFS proxy list: {p}"));
        }
        let c = self.cache_base.trim();
        if !c.is_empty() && (!c.starts_with('/') || c.contains("..") || !c.chars().all(|ch| ch.is_ascii_alphanumeric() || "/._-".contains(ch))) {
            return Err(format!("the cache folder must be a plain absolute path: {c}"));
        }
        let q = self.quota_mb.trim();
        if !q.is_empty() && q.parse::<u64>().map_or(true, |n| !(1000..=100_000_000).contains(&n)) {
            return Err(format!("the cache size is in MB, 1000 or more: {q}"));
        }
        Ok(())
    }

    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // native only (kernel.rs / stack.rs)
    pub fn is_empty(&self) -> bool {
        self.proxy.trim().is_empty() && self.cache_base.trim().is_empty() && self.quota_mb.trim().is_empty()
    }
}

/// The settings, whether the person has chosen them, and good choices to offer.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct SettingsView {
    pub settings: Settings,
    /// False until saved once — the app asks before it acts.
    pub chosen: bool,
    /// The stack every module comes from (operator, 2026-10-13: the IDE is
    /// coupled to our CVMFS distribution - no stack folder to choose).
    #[serde(default)]
    pub stack: StackView,
}

/// The ndexr stack as this machine sees it: stack.ndexr.io on CVMFS, mounted
/// at /opt/ndexr/stack. `revision` is the catalog revision the client is on;
/// the client follows new ones by itself.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct StackView {
    pub root: String,
    pub repo: String,
    pub revision: String,
    /// The client's last "switched to catalog revision N" line.
    pub updated: String,
    /// Platforms published under the root, and whether this machine's is there.
    pub triples: Vec<String>,
    pub mine: String,
    pub ok: bool,
    /// The Gentoo Prefix of this machine's platform, the ground every module
    /// stands on (`<root>/<triple>/prefix/<triple>`).
    #[serde(default)]
    pub prefix: String,
    /// The CVMFS client on this machine: installed, and its effective
    /// configuration for the repository (`cvmfs_config showconfig`).
    #[serde(default)]
    pub client: bool,
    #[serde(default)]
    pub config: Vec<ConfigLine>,
    /// The values the person's settings last wrote to the client, if any.
    #[serde(default)]
    pub local: CvmfsSettings,
    /// How a setup or apply would get root here: "sudo" (no password),
    /// "pkexec" (one desktop prompt), or "" (it cannot - the command to run
    /// by hand is shown instead).
    #[serde(default)]
    pub root_by: String,
    /// The command that sets the stack up, for someone doing it by hand.
    #[serde(default)]
    pub setup_cmd: String,
    /// Why the stack cannot come to this machine at all (macOS, Windows).
    #[serde(default)]
    pub unsupported: String,
}

/// One effective CVMFS parameter and the file it came from.
#[derive(Clone, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigLine {
    pub key: String,
    pub value: String,
    pub from: String,
}

/// Where a Start has got to, in words, read off its job's output and the
/// machine's state (operator, 2026-10-18: "it also isnt very gratifying when
/// i click on start for build machines, not sure exactly what is happening or
/// if they started"). Start is `ndexr aws server start`, then ssh once it has
/// an address, wait for its bootstrap, `ndexr update`, then `ndexr stack up`
/// (boxes.rs). Returns (what it is doing, finished, failed).
pub fn start_phase(job: &JobView, b: Option<&BoxView>) -> (String, bool, bool) {
    let has = |s: &str| job.lines.iter().any(|l| l.contains(s));
    let last = job.lines.iter().rev().map(|l| l.trim()).find(|l| !l.is_empty()).unwrap_or("").chars().take(120).collect::<String>();
    // Not a start (boxes.rs `act` names it "<server>: <verb>"): its own words.
    let verb = job.what.rsplit(": ").next().unwrap_or("");
    if !matches!(verb, "kick off" | "up") {
        let (doing, did) = match verb {
            "stop build" | "down" => ("Stopping the build", "Build stopped"),
            "shut down" | "shutdown" => ("Shutting the machine down", "Machine shut down - its disk is kept"),
            _ => ("Terminating the machine", "Machine terminated"),
        };
        return match job.ok {
            Some(false) => (format!("Did not work: {last}"), true, true),
            Some(true) => (did.into(), true, false),
            None => (doing.into(), false, false),
        };
    }
    if job.ok == Some(false) {
        return (format!("Did not start: {last}"), true, true);
    }
    if let Some(b) = b.filter(|b| b.state == "running") {
        let step = if b.steps > 0 && !b.step.is_empty() { format!(" · step {} of {}, {}", b.step_index, b.steps, b.step) } else { String::new() };
        return (format!("Building{step}"), true, false);
    }
    if has("[ndexr up]") {
        return ("The build has begun on the machine".into(), false, false);
    }
    if has("is now ndexr") || has("ndexr update") || has("==> installed") {
        return ("Updating ndexr on the machine".into(), false, false);
    }
    let ip = job.lines.iter().find_map(|l| l.split_whitespace().find(|w| {
        let p: Vec<&str> = w.trim_matches(|c: char| !c.is_ascii_digit() && c != '.').split('.').collect();
        p.len() == 4 && p.iter().all(|x| !x.is_empty() && x.parse::<u8>().is_ok())
    }).map(|w| w.trim_matches(|c: char| !c.is_ascii_digit() && c != '.').to_owned()));
    if let Some(ip) = ip {
        return (format!("Machine is up at {ip} - waiting for it to finish booting"), false, false);
    }
    if job.running {
        return ("Asking AWS to start the machine".into(), false, false);
    }
    (if last.is_empty() { "Sent".into() } else { last }, job.ok.is_some(), false)
}

/// The machine Cove runs on, for an admin's header chip.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Vitals {
    /// Busy share of every core since the last ask, 0-100.
    pub cpu_pct: f64,
    /// Memory in use (total less available), 0-100.
    pub mem_pct: f64,
    /// The root filesystem: bytes free and in all.
    pub disk_free: u64,
    pub disk_total: u64,
    pub cores: u32,
}

/// One build machine as Builds shows it to an admin.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BoxView {
    pub server: String,
    pub role: String,
    pub stem: String,
    pub arch: String,
    pub instance: String,
    /// running · stale · done · failed · off · terminated · starting · unknown
    pub state: String,
    /// The `ndexr stack up` step it is on, and where in the run.
    pub step: String,
    pub step_index: i64,
    pub steps: i64,
    /// It has an address: on, and billing.
    pub up: bool,
    /// Seconds since its log last grew.
    pub quiet_s: i64,
}

/// One saved version of a project: its commit, when (unix seconds), whose,
/// and what changed in a line.
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Version {
    pub sha: String,
    pub when: i64,
    pub author: String,
    pub message: String,
}

/// A project by the app contract (apps.rs): its app.env's language, kind
/// (port | module), port and health path; which of build.sh / up.sh / down.sh
/// it has; whether APP_HEALTH answers on APP_PORT; and whether app.env was
/// missing and this was inferred from its files.
#[derive(Clone, Default, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppStatus {
    pub language: String,
    pub kind: String,
    pub port: String,
    pub health: String,
    pub running: bool,
    pub verbs: Vec<String>,
    pub inferred: bool,
    /// A build running in the folder (build.sh, cargo), by anyone: Cove's
    /// button, an agent, a terminal. Seconds it has run; None = none.
    #[serde(default)]
    pub compiling: Option<u64>,
    /// The project has build outputs to start from (target/release/app for
    /// Rust); false = Up has nothing to run until Build. Always true for
    /// languages that compile on start or not at all.
    #[serde(default)]
    pub built: bool,
    /// The person's share of the machine, from their own cgroup (limits.rs);
    /// None where nobody is limited.
    #[serde(default)]
    pub quota: Option<Quota>,
}

/// What a person may use and uses, read from their cgroup (limits.rs `mine`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Quota {
    /// Bytes; 0 = no limit.
    pub memory_max: u64,
    pub memory_now: u64,
    /// Cores; 0.0 = no limit.
    pub cpus: f64,
    /// Times the kernel killed something of theirs for memory.
    pub oom_kills: u64,
}

/// A background job (stack setup, settings apply, a render) as far as it got.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct JobView {
    /// What it is: "setup", "apply", "render <file>".
    pub what: String,
    pub running: bool,
    /// None while running; then whether it worked.
    pub ok: Option<bool>,
    pub lines: Vec<String>,
    /// A render's HTML, once written.
    #[serde(default)]
    pub output: String,
    /// Where the Viewer shows it: the server's /site/<id>/<page> (host.rs
    /// `view_address`), which serves the page with everything beside it.
    #[serde(default)]
    pub view: String,
}

/// One proxy tried against each of our servers.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct ProxyCheck {
    pub proxy: String,
    pub server: String,
    pub ok: bool,
    /// "200 in 0.21 s", or what went wrong.
    pub said: String,
}

/// The machine the UI is driving: what is installed and who it runs as.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
pub struct Probe {
    pub ndexr: Option<String>,
    pub version: Option<String>,
    pub home: String,
    pub host: String,
    pub os: String,
}

/// One shell word, single-quoted. Shared: the UI builds the commands it shows
/// (in a browser too), and the machine quotes its paths.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// What can be done in Messages (Query::InboxAct).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum InboxAct {
    /// Write on request `id`'s thread: its sender or an admin.
    Post { id: i64, body: String },
    /// Move request `id`: an admin to any status; its sender to "resolved"
    /// or back to "open".
    Status { id: i64, status: String },
    /// The sender has read request `id`'s thread.
    Seen { id: i64 },
    /// Others may see that I am here, and message me first.
    Public { on: bool },
    /// Who can find me and write to me first: "private", "org" (people with
    /// my email's domain) or "public" (anyone). Public above is the old
    /// two-way switch: on = "public", off = "private".
    Findable { level: String },
    /// A direct message to `to` (a username, or "#<id>" for a group's chat): `kind` "text" (or ""), "file"
    /// (`file` carries it; body is its name) or "site" (body is a host).
    Dm {
        to: String,
        body: String,
        #[serde(default)]
        kind: String,
        #[serde(default)]
        file: Option<Attachment>,
    },
    /// Ask `to` (a username) to connect; accepts theirs if they asked first.
    Connect { to: String },
    /// Their ask to connect: accept it, or (false) decline it - or end an
    /// accepted connection.
    Connection { with: String, accept: bool },
    /// A new group, made by me, with me in it.
    GroupNew { name: String },
    /// Invite `who`, someone I am connected with, into group `id`.
    GroupInvite { id: i64, who: String },
    /// My invitation to group `id`: take the place, or (false) turn it down.
    GroupJoin { id: i64, accept: bool },
    GroupLeave { id: i64 },
    /// Share my `domain` with a person I am connected with (`to`), or a group
    /// I am in (`group`, when not 0).
    Share {
        domain: String,
        #[serde(default)]
        to: String,
        #[serde(default)]
        group: i64,
    },
    /// A domain offered to me: take it, or (false) turn it down.
    ShareDecide { id: i64, accept: bool },
    /// Take back a share of mine.
    ShareRevoke { id: i64 },
}

/// Who may open a domain (Query::AppAccess).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AppAccessView {
    /// public | login | owner | admin.
    pub level: String,
    /// Served by its own process on a port: the gate is in its vhost, so a
    /// change needs the app up.
    pub port: bool,
    /// What the change did, said in words ("" on a read).
    pub said: String,
}

/// One message on a request's thread.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThreadMsg {
    /// Their username (an admin sees emails for people with none).
    pub who: String,
    /// "message", or "status" (body is the new status).
    pub kind: String,
    pub body: String,
    pub at: String,
    pub mine: bool,
    /// Written by an admin: the ndexr team answering.
    pub team: bool,
}

/// A request, as Messages lists it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestView {
    pub id: i64,
    /// Who sent it: username, and for an admin the email.
    pub username: String,
    pub email: String,
    pub domain: String,
    pub what: String,
    pub trying: String,
    /// open, working, resolved, wontfix.
    pub status: String,
    pub urgency: String,
    pub created: String,
    pub n_uploads: i64,
    pub mine: bool,
    /// Something new on it for whoever is looking: for the sender, a team
    /// message since they last read it; for an admin, the sender wrote last.
    pub unread: bool,
    pub messages: Vec<ThreadMsg>,
}

/// Someone to talk to: live and public, or already in a conversation.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Person {
    pub username: String,
    pub live: bool,
    pub public: bool,
    /// Their messages to me I have not read.
    pub unread: i64,
    /// The last message between us, as the list shows it, and when.
    pub last: String,
    pub last_at: String,
    /// On the ndexr team: a person's requests show in their chat with them.
    pub admin: bool,
}

/// One direct message.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DmView {
    pub id: i64,
    pub mine: bool,
    /// "text", "file" (body is its name, `size` its bytes) or "site" (body is a host).
    pub kind: String,
    pub body: String,
    pub size: i64,
    pub at: String,
    /// They have opened the chat since: the sender's ✓✓.
    pub read: bool,
    /// Who sent it (a group's chat has many).
    pub who: String,
}

/// Go's Recap: what changed, by kind, and all of it as one thing to say.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recap {
    pub minutes: u32,
    pub parts: Vec<RecapPart>,
    pub spoken: String,
}

/// Go's Briefing: `state` writing · recording · ready · failed; `script` from
/// recording on; `audio` (base64 mp3) only when ready.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecapBrief {
    pub minutes: u32,
    pub state: String,
    pub script: String,
    pub audio: String,
    pub error: String,
    pub made_at: i64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecapPart {
    pub title: String,
    pub lines: Vec<String>,
}

/// An easyconfig's text and where it was read; `built` when it is the
/// installed module's own copy.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EasyconfigView {
    pub path: String,
    pub text: String,
    pub built: bool,
}

/// Where every package set is in the cycle that makes it (Query::PackageFlow,
/// pkgflow.rs): planned in the kit's manifest, built on a build machine,
/// published to S3 (repo.ndexr.io/env), synced into this host's library,
/// reached by an `env/<set>` module on CVMFS, loaded by sessions.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PackageFlow {
    /// This host's triple, and the public repo the sets are published to.
    pub triple: String,
    pub repo: String,
    /// This machine's name, which a publish record names as `host` when the
    /// set was built and published here - on the machine that serves people.
    #[serde(default)]
    pub host: String,
    /// The CVMFS repository this host mounts: its revision, and when the kit
    /// on it was last synced - the last promotion into what `ml` loads.
    #[serde(default)]
    pub stack_revision: String,
    #[serde(default)]
    pub stack_synced: String,
    pub sets: Vec<PfSet>,
    /// The build machines and their last run (catalog.json).
    pub machines: Vec<PfMachine>,
    /// Live Cove sessions per module (an admin's view; empty otherwise).
    pub sessions: Vec<PfUse>,
    /// The asked set's packages on this host's triple.
    pub rows: Vec<PfRow>,
    /// The asked package, traced.
    #[serde(default)]
    pub trace: Option<PfTrace>,
    /// What is running now: ndexr operations on this host, and each build
    /// machine's state (builds.json).
    #[serde(default)]
    pub live: Vec<PfLive>,
    #[serde(default)]
    pub live_machines: Vec<PfLiveMachine>,
    /// Unix seconds this was read.
    pub at: i64,
}

/// One stage of one set (pkgflow.rs `stages`): `key` is planned, built,
/// published, synced, installed or cvmfs; `state` ok, behind (there, but
/// older or fewer than the stage before), gap (missing) or none (not
/// expected - a combined set has no manifest); `fix` the operation that
/// closes it, when there is one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PfStage {
    pub key: String,
    pub state: String,
    pub value: String,
    pub note: String,
    pub fix: String,
}

/// A publish record (a current.json's head): who wrote it, when, from
/// which manifest, holding how many packages.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PfRecord {
    pub host: String,
    pub at: String,
    pub manifest_sha256: String,
    pub packages: u32,
    pub failed: u32,
}

/// An ndexr operation running on this host now: the stage it moves
/// (built, published, synced, cvmfs), the set ("" = every set), what it is,
/// and since when (unix seconds).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PfLive {
    pub stage: String,
    pub set: String,
    pub what: String,
    pub pid: u32,
    pub since: i64,
}

/// A build machine as builds.json has it now.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PfLiveMachine {
    pub name: String,
    pub kind: String,
    pub arch: String,
    pub active: bool,
    /// What it is building, when it is.
    pub running: String,
    pub done: u32,
    pub failed: u32,
    pub total: u32,
    pub remaining_s: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PfTrace {
    pub name: String,
    /// What it needs, each with the version the set holds ("" = not in it).
    pub deps: Vec<(String, String)>,
    /// What in the set needs it.
    pub needed_by: Vec<String>,
    /// The modules under the set: its language, toolchain, libraries.
    pub toolchain: Vec<String>,
    /// Its build step, when the set's timeline has one: status and when.
    pub built: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PfSet {
    /// `R/4.6.1-bioc3.23`, or a combined set (`r4.6_py3.13-release`) that the
    /// stack's modules still load.
    pub set: String,
    pub language: String,
    pub module: String,
    /// The kit's manifest: its records and sha256 now ("" = no manifest).
    pub records: u32,
    pub manifest_sha256: String,
    /// What S3 serves for it, per triple.
    pub published: Vec<PfPublished>,
    /// This host's library: where, how many installed, last changed.
    pub library: String,
    pub local: u32,
    pub local_at: i64,
    /// The module file on this triple's CVMFS tree, and the variable and
    /// path through which the language finds the library at run time.
    pub module_on_stack: bool,
    pub load: String,
    /// The newest build of it (a build machine's staged publish, or a build
    /// here), and the publish this host last synchronized from.
    #[serde(default)]
    pub built: PfRecord,
    #[serde(default)]
    pub synced: PfRecord,
    /// Its six stages, judged (pkgflow.rs `stages`).
    #[serde(default)]
    pub stages: Vec<PfStage>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PfPublished {
    pub triple: String,
    /// False when the repo answered nothing (not published, or unreachable).
    pub found: bool,
    pub packages: u32,
    pub published_at: String,
    pub host: String,
    pub cli_version: String,
    /// The manifest it was built from; differs from the kit's when the
    /// manifest changed after this publish.
    pub manifest_sha256: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PfMachine {
    pub name: String,
    pub arch: String,
    pub itype: String,
    pub last_end: i64,
    pub last_built: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PfUse {
    pub module: String,
    pub sessions: u32,
}

/// One package of a set at each stage: the version the manifest asks for
/// ("" = pulled in as a dependency), the version S3 publishes with its
/// sha256 and size, and the version installed on this host.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct PfRow {
    pub name: String,
    pub requested: String,
    pub source: String,
    pub published: String,
    pub file: String,
    pub sha256: String,
    pub size: u64,
    pub local: String,
    /// The version in the publish this host synchronized from.
    #[serde(default)]
    pub synced: String,
}

/// The path from a person's browser to an application, as this machine sees
/// it now (Query::Serving): DNS, the edge router, the on-prem host, nginx,
/// and the upstreams behind it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Serving {
    pub host: String,
    pub kernel: String,
    pub arch: String,
    pub cpus: u32,
    pub mem_total_kb: u64,
    pub mem_avail_kb: u64,
    pub load1: f32,
    /// nginx's enabled sites, and whether it answers on :443 here.
    pub vhosts: u32,
    pub nginx_up: bool,
    /// The edge router public traffic comes in through, and the state of the
    /// reverse tunnel to it (systemd's word: active, failed ...).
    pub router: String,
    pub tunnel: String,
    pub zones: Vec<Zone>,
    pub upstreams: Vec<Upstream>,
}

/// An apex domain: who answers its DNS and the address it resolves to.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Zone {
    pub apex: String,
    /// "Route 53", or the name servers' own domain; "" when none answered.
    pub dns: String,
    pub address: String,
}

/// One thing nginx hands a request to (a domain's `target`): the platform's
/// Shiny router (`module`, its replicas), a port, a cell, a remote, a
/// redirect. `up`/`of`: how many of its listeners answered here.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Upstream {
    pub target: String,
    pub up: u32,
    pub of: u32,
}

/// A module of the stack built with patches applied (patches.rs,
/// /view/patches.json): every patch its install recorded.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Patched {
    pub name: String,
    pub version: String,
    pub patches: Vec<Patch>,
}

/// What one install was built from, read from its own `easybuild/` record,
/// with the public copies of each file (ndexr stack promote --records): the
/// upstream sources and the sha256 the build checked, the recipe as built,
/// every patch, the build log. Served at /view/record?name=<module>.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BuildRecord {
    pub name: String,
    pub version: String,
    pub triple: String,
    /// EasyBuild's log stamp, the build's id (20261013.052031).
    pub build: String,
    pub easybuild: String,
    pub homepage: String,
    /// The public page over every build of this version.
    pub records: String,
    pub recipe: RecordFile,
    pub log: RecordFile,
    pub sources: Vec<RecordFile>,
    pub patches: Vec<RecordFile>,
}

/// One file of a build: its name, public URL ("" when withheld) and sha256;
/// `note` says why a source is not mirrored.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RecordFile {
    pub file: String,
    pub url: String,
    pub sha256: String,
    pub note: String,
}

/// One applied patch: its file, whether it is ndexr's own (in the kit) or
/// came with EasyBuild's upstream recipe, why (its own header) and its size.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Patch {
    pub file: String,
    pub ours: bool,
    pub why: String,
    pub added: u32,
    pub removed: u32,
}

/// A package name as the catalog spells it: lower case, `_` and `.` as `-`
/// (torch, nvidia-cublas, polars-runtime-32).
pub fn pkg_key(name: &str) -> String {
    name.to_ascii_lowercase().replace(['_', '.'], "-")
}

/// Where an installed package's machine code came from, read off the site
/// libraries themselves (host.rs `provenance`, /view/provenance.json):
/// compiled here, no machine code at all, or a binary built somewhere else
/// and downloaded - a manylinux wheel from PyPI, an R binary from a package
/// manager (operator, 2026-10-21: "catching and understanding the use of
/// external binaries in a regulatory setting").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Provenance {
    /// The language (R, Python), the package as the catalog names it, how it
    /// came, what says so (a wheel's tag, a DESCRIPTION field), and the sets
    /// it came that way in.
    pub language: String,
    pub name: String,
    pub version: String,
    /// "external" (a binary built elsewhere), "built" (compiled here),
    /// "pure" (no compiled code).
    pub how: String,
    pub evidence: String,
    pub sets: Vec<String>,
    /// For a downloaded binary: the reason the stack kit's
    /// `wheel-exceptions.tsv` gives for taking it ("" = not on that list - a
    /// download nobody accounted for).
    #[serde(default)]
    pub reason: String,
    /// For a downloaded binary: what the next release does about it, from the
    /// stack kit's `binary-plan.tsv` - "source", "remove" or "keep" ("" = no
    /// decision yet) - and the line saying how or why.
    #[serde(default)]
    pub fate: String,
    #[serde(default)]
    pub plan: String,
}

/// What an env set's module loads, read from its easyconfig's
/// `dependencies` (host.rs `env_uses`, /view/envdeps.json): the EasyBuild
/// modules the set is built on, each as EasyBuild names its module version
/// (R 4.6.1-foss-2024a). The catalog itself does not carry these.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvUses {
    /// The set's module: env/R/4.6.1-bioc3.24.
    pub module: String,
    /// The easyconfig it was read from; empty with `error` when none was found.
    pub path: String,
    pub uses: Vec<EnvUse>,
    pub error: String,
}

/// The stack kit's record sets and their combinations (host.rs `env_sets`,
/// /view/envsets.json): every language set the kit defines, whether or not
/// it is built yet, and each env/<combo> as the two sets it joins.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvSets {
    pub sets: Vec<KitSet>,
    pub combos: Vec<KitCombo>,
    /// Each installed Julia package's repository, from the General registry
    /// in the set's depot (host.rs `julia_repos`): the catalog carries no
    /// homepage for them, and an owner is read from it (atlas/owners.rs).
    #[serde(default)]
    pub repos: std::collections::HashMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KitSet {
    /// R/4.6.1-bioc3.24, Julia/1.11.7: the module is env/<set>.
    pub set: String,
    pub language: String,
    pub records: u32,
    /// Its manifest's packages - only for the sets asked for (a set the
    /// catalog does not have yet).
    pub packages: Vec<KitPackage>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KitPackage {
    pub name: String,
    pub version: String,
    pub group: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct KitCombo {
    /// r4.6_py3.13-release: the module is env/<name>.
    pub name: String,
    pub sets: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct EnvUse {
    pub name: String,
    pub version: String,
}

/// Find in files' answer: each matching line, how many files were read,
/// and whether it stopped at the limit.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrepView {
    pub root: String,
    pub hits: Vec<GrepHit>,
    pub files: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GrepHit {
    pub path: String,
    /// 1-based.
    pub line: usize,
    pub text: String,
}

/// What a file sent in Chat may weigh.
pub const DM_FILE_MAX: usize = 10 << 20;

/// Messages, all at once (Query::Inbox).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InboxView {
    pub me: String,
    pub admin: bool,
    /// Others may see me and message me first.
    pub public: bool,
    /// Who can find me: "private", "org" or "public" (InboxAct::Findable).
    pub findable: String,
    /// My email's domain is one an organization owns (not gmail.com and the
    /// like), so "org" means something for me.
    pub has_org: bool,
    pub requests: Vec<RequestView>,
    pub people: Vec<Person>,
    /// Public people whose username holds what is typed in Chat's "with",
    /// here now or not (operator, 2026-10-18: "if a user has allowed themselves
    /// to be public ... they're searchable through chat").
    pub found: Vec<Person>,
    /// The chat with the person asked for, oldest first.
    pub chat: Vec<DmView>,
    /// For an admin: requests open or being worked on, everyone's.
    pub open: i64,
    /// Who I am connected with, and who asked to be.
    pub connections: Vec<Connection>,
    /// Groups I am in or invited to.
    pub groups: Vec<GroupView>,
    /// Domains shared with me, and the ones I shared.
    pub shares: Vec<ShareView>,
}

/// A connection (src/sql/tables/people_sharing.sql): two people who may
/// message, invite and share with each other once one accepted the other.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    pub username: String,
    /// "pending" or "accepted".
    pub status: String,
    /// They asked me: mine to accept.
    pub incoming: bool,
    pub live: bool,
}

/// A group people made, as one of its members (or an invitee) sees it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupView {
    pub id: i64,
    pub name: String,
    /// I made it.
    pub mine: bool,
    /// I accepted my place; false while I am only invited.
    pub member: bool,
    pub members: Vec<String>,
    pub invited: Vec<String>,
    /// The last message in its chat, and when.
    pub last: String,
    pub last_at: String,
}

/// A domain shared: with me, or by me.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShareView {
    pub id: i64,
    pub domain: String,
    /// "use" (open it through its gate).
    pub role: String,
    /// "offered", "accepted" or "declined".
    pub status: String,
    /// Its owner's username.
    pub owner: String,
    /// I own it.
    pub mine: bool,
    /// The person (username) or group (name) it is shared with.
    pub to: String,
    /// The group it is shared with; 0 = a person.
    pub group: i64,
    pub at: String,
}

impl InboxView {
    /// What the toolbar's Messages badge counts.
    pub fn unread(&self) -> i64 {
        self.requests.iter().filter(|r| r.unread).count() as i64
            + self.people.iter().map(|p| p.unread).sum::<i64>()
            + self.connections.iter().filter(|c| c.incoming).count() as i64
            + self.groups.iter().filter(|g| !g.member).count() as i64
            + self.shares.iter().filter(|s| !s.mine && s.status == "offered").count() as i64
    }
}

/// What a request may carry: a few screenshots, a recording, a document.
pub const REQUEST_FILES_MAX: usize = 24 << 20;

/// A file sent with a request: its name and its bytes as a data URL
/// (`data:image/png;base64,…`) or bare base64.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Attachment {
    pub name: String,
    pub data: String,
}

impl Attachment {
    /// The bytes, whichever way they were written.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    pub fn bytes(&self) -> Option<Vec<u8>> {
        b64_decode(self.data.split_once(";base64,").map_or(self.data.as_str(), |(_, b)| b))
    }

    /// About how many bytes it carries (base64 is 4 characters for 3).
    pub fn size(&self) -> usize {
        self.data.split_once(',').map_or(self.data.len(), |(_, b)| b.len()) / 4 * 3
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Plots cross the wire as base64 (JSON has no bytes).
// Also the web's dropped files for Send a request (app.rs).
pub fn b64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn b64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let (mut acc, mut bits) = (0u32, 0);
    for ch in s.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=') {
        let v = B64.iter().position(|&b| b == ch)? as u32;
        acc = acc << 6 | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_start_says_where_it_has_got_to() {
        use super::{start_phase, BoxView, JobView};
        let job = |lines: &[&str], running: bool, ok: Option<bool>| JobView { what: "four-box-gpu: kick off".into(), running, ok, lines: lines.iter().map(|l| l.to_string()).collect(), ..Default::default() };
        assert_eq!(start_phase(&job(&[], true, None), None), ("Asking AWS to start the machine".into(), false, false));
        assert!(start_phase(&job(&["started i-0abc at 54.12.3.4"], true, None), None).0.contains("54.12.3.4"));
        assert_eq!(start_phase(&job(&["54.12.3.4", "ndexr update"], true, None), None).0, "Updating ndexr on the machine");
        assert_eq!(start_phase(&job(&["[ndexr up] stack up"], true, None), None).0, "The build has begun on the machine");
        let running = BoxView { state: "running".into(), step: "R".into(), step_index: 3, steps: 9, ..Default::default() };
        assert_eq!(start_phase(&job(&["[ndexr up]"], false, Some(true)), Some(&running)), ("Building · step 3 of 9, R".into(), true, false));
        let (t, done, failed) = start_phase(&job(&["no capacity"], false, Some(false)), None);
        assert!(t.ends_with("no capacity") && done && failed);
        let shut = JobView { what: "four-box-gpu: shut down".into(), running: true, ..Default::default() };
        assert_eq!(start_phase(&shut, Some(&running)).0, "Shutting the machine down");
    }

    #[test]
    fn local_urls_are_found_where_servers_print_them() {
        use super::{local_url, port_address};
        assert_eq!(local_url("Listening on http://127.0.0.1:4321").as_deref(), Some("http://127.0.0.1:4321/"));
        assert_eq!(local_url("Browse at http://localhost:4200/docs/index.html\x1b[0m").as_deref(), Some("http://127.0.0.1:4200/docs/index.html"));
        assert_eq!(local_url("Serving HTTP on 0.0.0.0 port 8000 (http://0.0.0.0:8000/) ...").as_deref(), Some("http://127.0.0.1:8000/"));
        assert_eq!(local_url("see https://ndexr.io and http://example.com:80/"), None);
        assert_eq!(port_address("http://127.0.0.1:4200/a/b.html").as_deref(), Some("/port/4200/a/b.html"));
        assert!(super::renders("/x/_quarto.yml") && super::renders("a.md") && super::renders("n.ipynb") && !super::renders("a.R"));
    }

    use super::*;

    #[test]
    fn base64_round_trips() {
        for s in ["", "f", "fo", "foo", "foob", "fooba", "foobar"] {
            let e = b64_encode(s.as_bytes());
            assert_eq!(b64_decode(&e).unwrap(), s.as_bytes());
        }
        assert_eq!(b64_encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(b64_encode(b"fo"), "Zm8=");
    }

    #[test]
    fn languages_come_from_file_names() {
        assert_eq!(Lang::of_path("analysis.R"), Some(Lang::R));
        assert_eq!(Lang::of_path("report.Rmd"), Some(Lang::R));
        assert_eq!(Lang::of_path("/x/y/model.py"), Some(Lang::Python));
        assert_eq!(Lang::of_path("README.md"), None);
        assert_eq!(Lang::of_path("Makefile"), None);
    }

    #[test]
    fn a_language_outside_the_four_is_refused_at_the_boundary() {
        assert!(serde_json::from_str::<Query>(r#"{"q":"eval","lang":"bash","code":"id"}"#).is_err());
        assert!(serde_json::from_str::<Query>(r#"{"q":"eval","lang":"julia","code":"1+1"}"#).is_ok());
        assert!(serde_json::from_str::<Query>(r#"{"q":"eval","lang":"fortran","code":"x"}"#).is_err());
        assert!(serde_json::from_str::<Query>(r#"{"q":"eval","lang":"python","code":"1+1"}"#).is_ok());
        assert!(serde_json::from_str::<Query>(r#"{"q":"eval","lang":"shell","code":"ml av"}"#).is_ok());
    }

    #[test]
    fn per_language_settings_become_one_module() {
        let old: Settings = serde_json::from_str(r#"{"project_dir":"","r_module":"env/r4.1_py3.11-release","py_module":"env/r4.6_py3.13-release"}"#).unwrap();
        let s = old.one_module();
        assert_eq!(s.module, "env/r4.1_py3.11-release");
        assert_eq!(s.module(Lang::Python), s.module(Lang::Shell));
        let saved = serde_json::to_string(&s).unwrap();
        assert!(saved.contains("\"module\"") && !saved.contains("r_module"), "{saved}");
        let new: Settings = serde_json::from_str(r#"{"module":"R/4.6.1-foss-2024a","r_module":"env/x"}"#).unwrap();
        assert_eq!(new.one_module().module, "R/4.6.1-foss-2024a", "a saved module wins over leftovers");
    }

    #[test]
    fn documents_and_engines() {
        assert!(is_document("/x/report.qmd") && is_document("a.Rmd") && !is_document("a.R"));
        assert_eq!(Lang::of_engine("python"), Some(Lang::Python));
        assert_eq!(Lang::of_engine("R"), Some(Lang::R));
        assert_eq!(Lang::of_engine("bash"), Some(Lang::Shell));
        assert_eq!(Lang::of_engine("julia"), Some(Lang::Julia));
        assert_eq!(Lang::of_path("fit.jl"), Some(Lang::Julia));
        assert_eq!(Lang::of_path("run.sh"), Some(Lang::Shell));
    }

    #[test]
    fn cvmfs_settings_are_checked_before_they_reach_a_root_file() {
        let ok = CvmfsSettings { proxy: "http://squid.site:3128|http://s2:3128;DIRECT".into(), cache_base: "/scratch/cvmfs".into(), quota_mb: "20000".into() };
        assert!(ok.check().is_ok());
        assert!(CvmfsSettings::default().check().is_ok());
        for bad in [
            CvmfsSettings { proxy: "DIRECT\nCVMFS_SERVER_URL=http://evil".into(), ..Default::default() },
            CvmfsSettings { proxy: "$(id)".into(), ..Default::default() },
            CvmfsSettings { cache_base: "relative/cache".into(), ..Default::default() },
            CvmfsSettings { cache_base: "/a/../etc".into(), ..Default::default() },
            CvmfsSettings { quota_mb: "10".into(), ..Default::default() },
            CvmfsSettings { quota_mb: "lots".into(), ..Default::default() },
        ] {
            assert!(bad.check().is_err(), "{bad:?}");
        }
    }
}

/// What the Files tab shows in place rather than as text (operator,
/// 2026-10-14: "images, pdfs, etc when clicked render in place"), by
/// extension, with the type it is served as.
pub fn preview_type(path: &str) -> Option<&'static str> {
    let ext = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" | "oga" => "audio/ogg",
        "m4a" => "audio/mp4",
        _ => return None,
    })
}

/// Where the person stands with Claude - credentials.ndexr.io/connect.r's
/// `summary`: claude_access's verdict and words, their own credential if one
/// is connected, and the shared seat's weekly allowance (what someone with no
/// Claude of their own runs the sub-* models on).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ClaudeView {
    /// "operator" | "own" | "plan" | "none" | "unknown"
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub detail: String,
    /// e.g. "Your Claude · subscription", "House Claude".
    #[serde(default)]
    pub label: String,
    /// "oauth_token" | "api_key" when their own is connected.
    #[serde(default)]
    pub cred_kind: Option<String>,
    #[serde(default)]
    pub last4: Option<String>,
    #[serde(default)]
    pub seat_left: Option<i64>,
    #[serde(default)]
    pub seat_limit: Option<i64>,
    #[serde(default)]
    pub seat_allowed: Option<bool>,
    #[serde(default)]
    pub seat_message: Option<String>,
    #[serde(default)]
    pub seat_resets: Option<String>,
}

impl ClaudeView {
    /// Their own Claude is connected.
    pub fn own(&self) -> bool {
        self.kind == "own"
    }
}

/// One account as api.ndexr.io/account.r's `overview` has it - the Account tab.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountView {
    #[serde(default)]
    pub email: String,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub personal_domain: Option<String>,
    #[serde(default)]
    pub provision_status: Option<String>,
    #[serde(default)]
    pub is_admin: bool,
    /// https://code.<personal domain>, when the account has a username.
    #[serde(default)]
    pub cell: Option<String>,
    #[serde(default)]
    pub cell_enabled: bool,
    #[serde(default)]
    pub cell_last_seen: Option<String>,
    #[serde(default)]
    pub idle_timeout_min: Option<i64>,
    /// A username, uid, ports and personal domain exist.
    #[serde(default)]
    pub ready: bool,
    /// The cell's app and code ports listening; None = could not be asked.
    #[serde(default)]
    pub up_app: Option<bool>,
    #[serde(default)]
    pub up_code: Option<bool>,
    #[serde(default)]
    pub domains: Vec<AccountDomain>,
    #[serde(default)]
    pub keys: Vec<AccountKey>,
    #[serde(default)]
    pub events: Vec<AccountEvent>,
    #[serde(default)]
    pub claude: Option<ClaudeView>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountDomain {
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub access_level: String,
    #[serde(default)]
    pub published: bool,
    /// What serves it now (accounts.r served_by): "cell", "platform", …
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub target: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountKey {
    #[serde(default)]
    pub key_id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub scopes: Option<String>,
    #[serde(default)]
    pub created_at: Option<String>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub last_used_at: Option<String>,
    #[serde(default)]
    pub use_count: i64,
    #[serde(default)]
    pub revoked_at: Option<String>,
    /// The key this app is signed in with (desktop only).
    #[serde(default)]
    pub this: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AccountEvent {
    #[serde(default)]
    pub at: Option<String>,
    #[serde(default)]
    pub key_id: Option<String>,
    #[serde(default)]
    pub event: Option<String>,
    #[serde(default)]
    pub audience: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

/// What `Query::Assist` wrote: code for the input, nothing else.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AssistReply {
    pub code: String,
    #[serde(default)]
    pub model: String,
}
