//! The kernels: one long-lived R session and one Prefix shell per machine,
//! each started inside the Lmod module the person picked (Inlet
//! 0004 — `env/r4.6_py3.13-release` brings R, Python and a site library
//! together), or the R on PATH when none is picked. The shell is bash inside
//! the stack: its Gentoo Prefix's Lmod, `ml`, and the modules picked for it.
//! Native only.
//!
//! Python lives INSIDE the R session, through reticulate (operator,
//! 2026-10-13: "start our R session with a reticulate session working instead
//! of R and python … a multilingual single syntax ide through reticulate", and
//! not rpy2). R starts with RETICULATE_PYTHON at its own module's python3,
//! attaches reticulate (installing it into the person's library the first
//! time) and initialises Python before the first prompt, so `np <- import(
//! "numpy")`, `py$x` and `r_to_py()` work at once. A .py file or a {python}
//! chunk is not a second interpreter: `eval(Lang::Python, …)` runs it in that
//! same embedded Python (HELPER_PY's `run`), where `r.x` reaches R and `py$x`
//! reaches back. One session, one environment, one set of plots.
//!
//! Each kernel runs a small driver (DRIVER_R / DRIVER_SH below) that reads
//! requests on stdin and answers on stdout:
//!
//!   request    <n>\n then n lines of code - on R's request FIFO (COVE_REQ),
//!              not its stdin: stdin is R's real console, so browser(),
//!              debugonce(), readline(), menu() and Python's pdb read what is
//!              typed there, as in RStudio (operator, 2026-10-19: "make sure
//!              debugging through browser() and the python equivalent and step
//!              through works"). R runs --interactive --quiet so it says when
//!              it waits (`Browse[1]> `, `(Pdb) `); see `poll`'s prompt.
//!   answer     whatever the code prints, as it prints it, then tagged lines
//!              SENT TAG \t field … : BANNER, CWD, ERR, WARN, PLOT, ENVBEGIN,
//!              ENV name class value, and DONE once the request is finished.
//!
//! The pimp locks live here too (operator, 2026-10-12, "lock them", "remove
//! controls antithetical to science"): R never loads or saves .RData
//! (--no-restore --no-save), nothing writes a console history to disk, CRAN
//! is an https snapshot unless the module already pinned one, and plots come
//! back as files instead of a window - matplotlib's too, saved off reticulate's
//! Python after each request.
//!
//! Live modules (Inlet 0010): `ml("jq")` / `module("load", "jq")`
//! swap Lmod modules in the running R. The driver applies Lmod's own R output
//! to the live process and re-derives .libPaths(); a swap that changes R itself
//! sends RESTART, and the kernel comes back inside the new list (with that
//! list's python3 as RETICULATE_PYTHON - an initialised Python cannot change
//! in place). The list a kernel runs in survives Session › Restart; saving
//! Global Options goes back to the module picked there.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};

use crate::api::{shell_quote, EnvRow, KernelView, Lang, Line, LineKind, TableView};

/// Starts every tagged line. Code that prints it verbatim confuses only itself.
const SENT: &str = "\u{1}ndexr\u{2}";
/// Transcript lines kept per kernel; older ones are dropped from the front.
const KEEP: usize = 20_000;

#[derive(Default)]
struct State {
    /// Transcript; `dropped` lines came before lines[0].
    lines: Vec<Line>,
    dropped: usize,
    /// Requests written and not yet DONE (the startup counts as one).
    pending: usize,
    alive: bool,
    banner: String,
    cwd: String,
    /// The shell's environment as of its last command (DRIVER_SH save_env).
    env_file: String,
    env: Vec<EnvRow>,
    env_next: Vec<EnvRow>,
    plots: Vec<PathBuf>,
    /// Which process may write here: a killed kernel's reader goes quiet.
    generation: u64,
    /// The module list the driver last reported (MODS), and the one it asked
    /// to be restarted into (RESTART).
    mods: Option<String>,
    restart_into: Option<String>,
    /// The shell loaded another R (RLOADED): the R module, for the R console.
    r_loaded: Option<String>,
    /// The data viewer: a table being received (TABLE … TEND), the last one
    /// whole, and how many have arrived.
    table_next: Option<TableView>,
    table: Option<TableView>,
    tables: u64,
    /// What R last handed its viewer, as a Viewer address, and how many times.
    view: String,
    views: u64,
    /// The last completion answered (COMPL): its request id, where the word
    /// starts and the candidates.
    compl: Option<(u64, crate::api::Completion)>,
    /// Output after the last newline, and when it last grew: a prompt R or
    /// pdb is waiting at, once nothing more comes (`poll`).
    partial: String,
    partial_at: Option<std::time::Instant>,
    /// R's own top level before the driver is sourced (its "> " and the
    /// echoed source() line): not the person's transcript.
    quiet: bool,
    /// Ask the R debugger's frame for the Environment at the next
    /// `Browse[n]> ` (`frame_request`).
    frame_wanted: bool,
    /// Where R's debugger stopped (KernelView.debug_*): the line from R's own
    /// `debug at <file>#<n>:`, else the browser() call's (DBGFN); the file;
    /// the stopped function's source and its first line. A frame being
    /// received goes in `dbg_next` until ENVEND.
    dbg_line: usize,
    dbg_file: String,
    dbg_from: usize,
    dbg_src: Vec<String>,
    dbg_next: Option<(String, usize, usize, Vec<String>)>,
}

impl State {
    /// A request ends, or a new one starts: no debugger stop is current.
    fn forget_stop(&mut self) {
        self.dbg_line = 0;
        self.dbg_file.clear();
        self.dbg_from = 0;
        self.dbg_src.clear();
        self.dbg_next = None;
    }

    /// R's own word on where it is: `debug at /p/f.R#12: x <- 1` (a step
    /// with a source reference) or `Called from: f()` (browser() reached).
    fn note_stop(&mut self, text: &str) {
        let t = text.rsplit_once("]> ").map_or(text, |(p, rest)| if p.starts_with("Browse[") { rest } else { text });
        if t.starts_with("Called from: ") {
            self.dbg_line = 0;
        } else if let Some(rest) = t.strip_prefix("debug at ") {
            let loc = rest.split_once(": ").map_or(rest, |(l, _)| l);
            if let Some((file, n)) = loc.rsplit_once('#') {
                if let Ok(n) = n.trim().parse() {
                    self.dbg_line = n;
                    if file.starts_with('/') {
                        self.dbg_file = file.to_owned();
                    }
                }
            }
        }
    }
}

/// R's browser prompt, `Browse[1]> `.
fn is_browse(p: &str) -> bool {
    p.starts_with("Browse[") && p.trim_end().ends_with("]>")
}

/// pdb's prompt (Python's breakpoint(), pdb.set_trace()).
fn is_pdb(p: &str) -> bool {
    matches!(p.trim_end(), "(Pdb)" | "ipdb>")
}

impl State {
    fn push(&mut self, kind: LineKind, text: impl Into<String>) {
        self.lines.push(Line { kind, text: text.into() });
        if self.lines.len() > KEEP {
            let cut = self.lines.len() - KEEP;
            self.lines.drain(..cut);
            self.dropped += cut;
        }
    }

    /// One line of the kernel's stdout.
    fn take(&mut self, raw: &str) {
        let (text, tag) = match raw.find(SENT) {
            Some(i) => (&raw[..i], Some(&raw[i + SENT.len()..])),
            None => (raw, None),
        };
        if self.quiet {
            // R's top level, once: its "> " prompt and the echoed source().
            if tag.is_none() && (text.trim_end() == ">" || text.starts_with("> ")) {
                return;
            }
            if tag.is_some() {
                self.quiet = false;
            }
        }
        // The IDE asking the debugger's frame for the Environment: R echoes
        // it after the prompt; it is not the person's.
        if text.contains(".ndexr_frame(environment())") {
            return;
        }
        if tag.is_none() || !text.is_empty() {
            self.note_stop(text);
            self.push(LineKind::Output, text);
        }
        let Some(tag) = tag else { return };
        let (t, rest) = tag.split_once('\t').unwrap_or((tag, ""));
        match t {
            "DONE" => {
                self.pending = self.pending.saturating_sub(1);
                self.forget_stop();
                self.env = std::mem::take(&mut self.env_next);
            }
            "ERR" => self.push(LineKind::Error, rest),
            "WARN" => self.push(LineKind::Warning, rest),
            "BANNER" => self.banner = rest.to_owned(),
            "CWD" => self.cwd = rest.to_owned(),
            "ENVFILE" => self.env_file = rest.to_owned(),
            "PLOT" => self.plots.push(PathBuf::from(rest)),
            "VIEW" => match crate::host::view_address(rest) {
                Ok(a) => {
                    self.view = a;
                    self.views += 1;
                }
                Err(e) => self.push(LineKind::Error, format!("viewer: {e}")),
            },
            "MODS" => self.mods = Some(rest.to_owned()),
            "RESTART" => self.restart_into = Some(rest.to_owned()),
            "RLOADED" => self.r_loaded = Some(rest.to_owned()),
            "ENVBEGIN" => self.env_next.clear(),
            // The frame a debugger stopped in, shown at once (no DONE comes).
            "ENVEND" => {
                self.env = std::mem::take(&mut self.env_next);
                if let Some((file, from, at, src)) = self.dbg_next.take() {
                    if !file.is_empty() {
                        self.dbg_file = file;
                    }
                    self.dbg_from = from;
                    self.dbg_src = src;
                    if self.dbg_line == 0 {
                        self.dbg_line = at;
                    }
                }
            }
            // The stopped function (driver `where`): its file, first line, the
            // browser() call's line, then its source a line per DBGSRC.
            "DBGFN" => {
                let mut f = rest.split('\t');
                let file = f.next().unwrap_or("").to_owned();
                let from = f.next().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
                let at = f.next().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
                self.dbg_next = Some((file, from, at, Vec::new()));
            }
            "DBGSRC" => {
                if let Some(n) = self.dbg_next.as_mut() {
                    n.3.push(rest.to_owned());
                }
            }
            "COMPL" => {
                let mut f = rest.split('\t');
                let id = f.next().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
                let start = f.next().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
                let items = f.filter(|i| !i.is_empty()).map(str::to_owned).collect();
                self.compl = Some((id, crate::api::Completion { start, items, ..Default::default() }));
            }
            "TABLE" => {
                let mut f = rest.splitn(3, '\t');
                let name = f.next().unwrap_or("").to_owned();
                let nrow = f.next().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
                let ncol = f.next().and_then(|n| n.trim().parse().ok()).unwrap_or(0);
                self.table_next = Some(TableView { name, nrow, ncol, ..Default::default() });
            }
            "TCOLS" | "TTYPES" | "TROW" => {
                let Some(table) = self.table_next.as_mut() else { return };
                let fields: Vec<String> = rest.split('\t').map(str::to_owned).collect();
                if t == "TCOLS" {
                    table.cols = if rest.is_empty() { Vec::new() } else { fields };
                } else if t == "TTYPES" {
                    table.types = fields;
                } else if table.rows.len() < TableView::ROWS {
                    // One empty field and none look alike on the wire: a row
                    // is as wide as the header, whatever came.
                    let mut row = fields;
                    row.resize(table.cols.len().max(1), String::new());
                    table.rows.push(row);
                }
            }
            "TEND" => {
                if let Some(t) = self.table_next.take() {
                    self.table = Some(t);
                    self.tables += 1;
                }
            }
            "ENV" => {
                let mut f = rest.splitn(3, '\t');
                let mut next = || f.next().unwrap_or("").to_owned();
                self.env_next.push(EnvRow { name: next(), class: next(), value: next() });
            }
            _ => {}
        }
    }
}

struct Proc {
    child: Child,
    /// The console: what browser(), pdb and readline() read.
    stdin: ChildStdin,
    /// Requests (DRIVER_R reads COVE_REQ); the shell takes them on stdin.
    req: Option<std::fs::File>,
}

fn states() -> &'static Mutex<HashMap<Lang, Arc<Mutex<State>>>> {
    static S: std::sync::OnceLock<Mutex<HashMap<Lang, Arc<Mutex<State>>>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

fn procs() -> &'static Mutex<HashMap<Lang, Proc>> {
    static P: std::sync::OnceLock<Mutex<HashMap<Lang, Proc>>> = std::sync::OnceLock::new();
    P.get_or_init(Default::default)
}

/// The module list each kernel runs in after a live swap; absent, the one
/// picked in Global Options.
fn live() -> &'static Mutex<HashMap<Lang, String>> {
    static L: std::sync::OnceLock<Mutex<HashMap<Lang, String>>> = std::sync::OnceLock::new();
    L.get_or_init(Default::default)
}

/// Global Options were saved: each kernel goes back to the module picked there.
pub fn forget_live() {
    live().lock().unwrap().clear();
}

fn state(lang: Lang) -> Arc<Mutex<State>> {
    states().lock().unwrap().entry(lang).or_default().clone()
}

/// ~/.cache/ndexr/cove — the drivers, and each kernel's plots.
pub fn cache_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(home).join(".cache")).join("ndexr/cove")
}

fn plots_dir(lang: Lang) -> PathBuf {
    cache_dir().join(format!("plots-{}", lang.word().to_lowercase()))
}

/// Lmod module names reach a shell: letters, digits and . _ + / - only.
pub fn valid_module(m: &str) -> bool {
    !m.is_empty() && m.len() < 200 && !m.starts_with(['/', '-', '.']) && m.chars().all(|c| c.is_ascii_alphanumeric() || "._+/-".contains(c))
}

/// The shell prefix that puts `module` (one name, or a live list separated by
/// spaces) in effect, or nothing for PATH's own. Shared with host.rs's package
/// listing and renders so all of them see the same interpreter. The load is
/// resolved once and replayed (envs.rs): 4.3 s a start became 0.45 s.
pub fn module_prefix(module: &str) -> Result<String, String> {
    crate::envs::prefix(module)
}

/// The interpreter command and the check that it exists.
fn interpreter(lang: Lang, driver: &str) -> String {
    let d = shell_quote(driver);
    // start() refuses without the stack, so R missing here is the module's.
    let why = "no R here — pick an R or env/ module in Tools › Global Options";
    match lang {
        Lang::R => format!(
            "command -v R >/dev/null || {{ echo '{why}'; exit 127; }}; \
             exec R --interactive --quiet --no-restore --no-save 2>&1"
        ),
        // Python is not a process of its own: it runs inside R (reticulate).
        Lang::Python => unreachable!("Python runs inside the R session"),
        // The host's bash: the driver itself enters the stack (use.sh) and
        // loads the shell's modules, so `ml` is a function in THIS shell.
        Lang::Shell => format!("exec bash --noprofile --norc {d} 2>&1"),
        // `ml Julia` came first (Settings::module): the stack's Julia, else
        // one on PATH. No startup file or history: the session is the driver's.
        // The packages are the stack's Julia set, beside the module until an
        // env/Julia module is published - the same fallback a starter's
        // env.sh takes, so a server.jl that runs from up.sh runs here too
        // (2026-10-26: testwetsa's `using HTTP` failed only in the console).
        Lang::Julia => format!(
            "command -v julia >/dev/null || {{ echo 'no Julia here: ml Julia did not load - mount or build the ndexr stack (Tools › Global Options), or put julia on PATH'; exit 127; }}; \
             if [ -z \"${{JULIA_LOAD_PATH:-}}\" ] && [ -n \"${{EBVERSIONJULIA:-}}\" ]; then \
               case \"$(uname -m)\" in aarch64|arm64) t=aarch64-unknown-linux-gnu ;; *) t=x86_64-pc-linux-gnu ;; esac; \
               s=/opt/ndexr/site-library/$t/Julia/$EBVERSIONJULIA/julia; \
               if [ -d \"$s\" ]; then export JULIA_DEPOT_PATH=\"$HOME/.julia:$s/depot:\" JULIA_LOAD_PATH=\"@:@v#.#:$s:@stdlib\" JULIA_CONDAPKG_BACKEND=Null; fi; \
             fi; \
             exec julia --startup-file=no --history-file=no --color=no --banner=no {d} 2>&1"
        ),
    }
}

/// `s` as a Julia string literal: the escapes, and `$`, which Julia would
/// otherwise interpolate.
fn jl_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '$' => out.push_str("\\$"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The python3 reticulate binds the R session to: the one in the module list
/// R starts in (the env/ module's own Python, from its resolution - no shell
/// of its own any more), else python3 on PATH. A RETICULATE_PYTHON already in
/// the environment is the person's, and wins.
fn reticulate_python(mods: &str) -> Option<String> {
    if std::env::var_os("RETICULATE_PYTHON").is_some() {
        return None;
    }
    if !mods.trim().is_empty() {
        return crate::envs::resolve(mods).ok()?.python.as_ref().map(|p| p.exe.clone());
    }
    let found = crate::runner::capture_within("command -v python3", std::time::Duration::from_secs(20))?;
    let path = found.lines().last()?.trim().to_owned();
    path.starts_with('/').then_some(path)
}

/// `code` as an R string literal: what reaches the R driver is one call,
/// `.ndexr_py("…")`, whatever quotes, backslashes or newlines the Python had.
fn r_string(code: &str) -> String {
    let mut out = String::with_capacity(code.len() + 2);
    out.push('"');
    for c in code.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => out.push_str(&format!("\\x{:02x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── plots in the IDE's colours ──────────────────────────────────────────────
// A plot is shown on the Plots pane's own ground (theme::bg()). R's defaults —
// a transparent page with black ink — drew black axes and titles straight into
// the dark pane, where they vanished (operator screenshot, 2026-10-13). So
// every plotting system is themed at the source, from theme.rs, the one place
// the palette lives: base graphics through par() on the kernel's device, ggplot2
// through theme_set() when it loads, matplotlib through a matplotlibrc.

fn hex(c: eframe::egui::Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// Series colours, in order: the theme's accents.
fn plot_palette() -> Vec<String> {
    use crate::theme::*;
    [primary(), net(), amber(), ok(), alt(), muted()].into_iter().map(hex).collect()
}

/// What the R driver reads (COVE_PLOT_*).
fn plot_env() -> Vec<(&'static str, String)> {
    use crate::theme::*;
    vec![
        ("COVE_PLOT_BG", hex(bg())),
        ("COVE_PLOT_FG", hex(text())),
        ("COVE_PLOT_DIM", hex(muted())),
        ("COVE_PLOT_GRID", hex(moss())),
        ("COVE_PLOT_PALETTE", plot_palette().join(",")),
    ]
}

/// matplotlib's defaults, found through $MATPLOTLIBRC; seaborn or an explicit
/// style still override them.
fn matplotlibrc() -> String {
    use crate::theme::*;
    let h = |c| hex(c).trim_start_matches('#').to_owned();
    let cycle = plot_palette().iter().map(|c| format!("'{}'", c.trim_start_matches('#'))).collect::<Vec<_>>().join(", ");
    format!(
        "# written by Cove (kernel.rs) — plots in the IDE's colours\n\
         figure.facecolor: {bg}\naxes.facecolor: {bg}\nsavefig.facecolor: {bg}\n\
         axes.edgecolor: {hush}\naxes.labelcolor: {fg}\ntext.color: {fg}\n\
         xtick.color: {dim}\nytick.color: {dim}\ngrid.color: {grid}\n\
         legend.facecolor: {bg}\nlegend.edgecolor: {grid}\n\
         boxplot.boxprops.color: {fg}\nboxplot.whiskerprops.color: {fg}\nboxplot.capprops.color: {fg}\nboxplot.medianprops.color: {amber}\n\
         axes.prop_cycle: cycler('color', [{cycle}])\n",
        bg = h(bg()), fg = h(text()), dim = h(muted()), hush = h(hush()), grid = h(moss()), amber = h(amber()),
    )
}

/// The full launch line for a kernel under these settings.
/// The R slot runs Python itself (DRIVER_PY): the session's environment
/// brings Python and no R (env/Python/…). Set when that slot starts.
static PY_PRIMARY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn py_primary() -> bool {
    PY_PRIMARY.load(std::sync::atomic::Ordering::Relaxed)
}

/// An environment with Python and no R (ide.rs env_consoles).
fn python_only(mods: &str) -> bool {
    let c = crate::ide::env_consoles(mods);
    c.contains(&Lang::Python) && !c.contains(&Lang::R)
}

fn launch_line(lang: Lang, mods: &str, driver: &str, cwd: &str) -> Result<String, String> {
    let cd = format!("cd {} 2>/dev/null || cd; ", shell_quote(cwd));
    Ok(format!("{cd}{}{}", module_prefix(mods)?, interpreter(lang, driver)))
}

/// What a console says where the ndexr stack is not: no use.sh, so no `ml`.
pub const NO_STACK: &str = "The console runs on the ndexr stack, and this computer does not have it yet. Set it up with \"Set it up\" above the windows, or the stack in the status bar: mount it with CernVM-FS, or build it here. The Terminal works without it.";

fn start(lang: Lang) -> Result<(), String> {
    // Only on the ndexr stack (operator, 2026-11-14: "console should only
    // work if ml/source activate for the ndexr stack is available"). Without
    // use.sh R, Python and bash came from this computer's PATH - a host R
    // with none of the stack's packages, under a banner that named no module.
    if crate::host::use_sh().is_none() {
        return Err(NO_STACK.into());
    }
    // Windows is a remote front end (operator, 2026-10-13: "a remote ide going
    // forward"): its R, Python and shell are the cell's, never this machine's.
    if cfg!(windows) {
        return Err("on Windows the ide runs in your ndexr cell - sign in (toolbar, right) and it connects there".into());
    }
    let s = crate::host::saved_settings();
    let cwd = crate::host::project_dir(&s);
    let dir = cache_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let lang = lang.session();
    let mods = live().lock().unwrap().get(&lang).cloned().unwrap_or_else(|| s.module(lang).to_owned());
    let primary = lang == Lang::R && python_only(&mods);
    // An environment that brings neither R nor Python starts no R: R would
    // be whatever is on PATH - the host's /opt/R/4.6.0, without the project's
    // packages (2026-10-26: an env/Julia session's R console ran it, then
    // tried to install reticulate into ~/R). Refused here as well as in the
    // page, so an older page cannot start it either.
    if lang == Lang::R && !primary && !mods.trim().is_empty() && !crate::ide::env_consoles(&mods).contains(&Lang::R) {
        return Err(format!("{mods} has no R - its consoles are {}", crate::ide::env_consoles(&mods).iter().map(|l| l.word()).collect::<Vec<_>>().join(" and ")));
    }
    if lang == Lang::R {
        PY_PRIMARY.store(primary, std::sync::atomic::Ordering::Relaxed);
    }
    let (name, src) = match lang {
        Lang::Shell => ("driver.sh", DRIVER_SH),
        Lang::Julia => ("driver.jl", DRIVER_JL),
        _ if primary => ("driver.py", DRIVER_PY),
        _ => ("driver.R", DRIVER_R),
    };
    let driver = dir.join(name);
    std::fs::write(&driver, src).map_err(|e| format!("{}: {e}", driver.display()))?;
    // reticulate's side of the R session: imported from this folder by name.
    let helper = dir.join("cove.py");
    std::fs::write(&helper, HELPER_PY).map_err(|e| format!("{}: {e}", helper.display()))?;
    let plots = plots_dir(lang);
    let _ = std::fs::remove_dir_all(&plots);
    std::fs::create_dir_all(&plots).map_err(|e| e.to_string())?;

    let mplrc = dir.join("matplotlibrc");
    std::fs::write(&mplrc, matplotlibrc()).map_err(|e| format!("{}: {e}", mplrc.display()))?;

    let line = if lang == Lang::Shell {
        if let Some(bad) = mods.split_whitespace().find(|m| !valid_module(m)) {
            return Err(format!("not a module name: {bad}"));
        }
        format!("cd {} 2>/dev/null || cd; {}", shell_quote(&cwd), interpreter(lang, &driver.display().to_string()))
    } else if primary {
        let d = shell_quote(&driver.display().to_string());
        format!(
            "cd {} 2>/dev/null || cd; {}command -v python3 >/dev/null || {{ echo 'no Python here: {} did not load'; exit 127; }}; exec python3 -u {d} 2>&1",
            shell_quote(&cwd),
            module_prefix(&mods)?,
            mods
        )
    } else {
        launch_line(lang, &mods, &driver.display().to_string(), &cwd)?
    };
    let py = if lang == Lang::R && !primary { reticulate_python(&mods) } else { None };
    // R's and Julia's requests come on a FIFO of their own; stdin stays the
    // session's, so code that reads input (readline()) does not eat a request.
    let fifo = if matches!(lang, Lang::R | Lang::Julia) {
        let f = dir.join(format!("req-{}.fifo", std::process::id()));
        let _ = std::fs::remove_file(&f);
        let made = Command::new("mkfifo").arg("-m").arg("600").arg(&f).status().is_ok_and(|s| s.success());
        if !made {
            return Err(format!("could not make {}'s request pipe {}", lang.word(), f.display()));
        }
        Some(f)
    } else {
        None
    };
    let mut cmd = Command::new(crate::runner::login_shell());
    if let Some(f) = &fifo {
        cmd.env("COVE_REQ", f);
    }
    crate::runner::forget_loaded_modules(&mut cmd);
    if let Some(p) = &py {
        // Twice: COVE_RETICULATE_PYTHON is what the driver puts back
        // after R's startup profiles ran (see DRIVER_R py_setup).
        cmd.env("RETICULATE_PYTHON", p).env("COVE_RETICULATE_PYTHON", p);
    }
    cmd.envs(plot_env()).env("MATPLOTLIBRC", &mplrc);
    // The shell driver replays the resolved load too (DRIVER_SH), "" = load it.
    let script = if lang == Lang::Shell && !mods.trim().is_empty() { crate::envs::resolve(&mods).map(|r| r.script.display().to_string()).unwrap_or_default() } else { String::new() };
    cmd.args(["-lc", &crate::runner::with_path(&line)])
        .env("COVE_ENV_SCRIPT", script)
        .env("COVE_USE_SH", crate::host::use_sh().map(|p| p.display().to_string()).unwrap_or_default())
        .env("COVE_PREFIX", crate::stack::prefix())
        .env("COVE", "1")
        .env("COVE_PLOTS", &plots)
        .env("COVE_HELPERS", &dir)
        .env("COVE_CWD", &cwd)
        .env("COVE_MODULE", s.module(lang))
        .env("COVE_MODULES", mods.as_str())
        .env("COVE_ENV_LOCAL", crate::source::env_local_modules().map(|d| d.display().to_string()).unwrap_or_default())
        .env("MPLBACKEND", "Agg")
        .env("NO_COLOR", "1")
        .env("PYTHONUNBUFFERED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own group: ^C in the terminal that started the app is not the kernel's.
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(|e| format!("could not start {}: {e}", lang.word()))?;
    let mut stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    // Read and write, so it never blocks waiting for R to open it and R's end
    // never sees EOF while the IDE holds it.
    let req = match &fifo {
        Some(f) => Some(std::fs::OpenOptions::new().read(true).write(true).open(f).map_err(|e| format!("{}: {e}", f.display()))?),
        None => None,
    };
    if lang == Lang::R {
        // R's console starts the driver; from then on it reads only what a
        // debugger or readline() asks for.
        stdin
            .write_all(format!("source({}, echo = FALSE)\n", r_string(&driver.display().to_string())).as_bytes())
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("R did not start its driver: {e}"))?;
    }

    let st = state(lang);
    let generation = {
        let mut g = st.lock().unwrap();
        g.generation += 1;
        g.pending = 1;
        g.alive = true;
        g.banner.clear();
        g.env.clear();
        g.env_next.clear();
        g.plots.clear();
        g.mods = None;
        g.restart_into = None;
        // Nothing said here: the driver says one line once it is up (operator,
        // 2026-10-15: "too much text on startup" - "starting R · …", the
        // RETICULATE_PYTHON path and four banner lines, twice a restart). The
        // module is on the toolbar and the tab's banner already.
        let _ = &py;
        g.table_next = None;
        g.partial.clear();
        g.partial_at = None;
        g.quiet = lang == Lang::R;
        g.frame_wanted = false;
        g.generation
    };
    let reader_state = st.clone();
    std::thread::spawn(move || {
        // Raw reads, not lines: a prompt R or pdb waits at has no newline.
        let mut r = stdout;
        let mut chunk = [0u8; 16 * 1024];
        let mut acc: Vec<u8> = Vec::new();
        loop {
            let n = std::io::Read::read(&mut r, &mut chunk).unwrap_or(0);
            let mut g = reader_state.lock().unwrap();
            if g.generation != generation {
                return;
            }
            if n == 0 {
                if !acc.is_empty() {
                    let text = String::from_utf8_lossy(&acc).into_owned();
                    g.take(&text);
                }
                g.alive = false;
                g.pending = 0;
                g.partial.clear();
                g.push(LineKind::System, format!("{} session ended — the next command starts a new one", lang.word()));
                return;
            }
            acc.extend_from_slice(&chunk[..n]);
            while let Some(i) = acc.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = acc.drain(..=i).collect();
                let text = String::from_utf8_lossy(&line);
                g.take(text.trim_end_matches(['\n', '\r']));
                if let Some(m) = g.mods.take() {
                    live().lock().unwrap().insert(lang, m);
                }
                if let Some(r) = g.r_loaded.take() {
                    // Not under this shell's lock: it restarts another kernel.
                    std::thread::spawn(move || follow_shell_r(&r));
                }
                if let Some(m) = g.restart_into.take() {
                    live().lock().unwrap().insert(lang, m.clone());
                    g.push(LineKind::System, format!("{} itself changed - restarting {} in {m}", lang.word(), lang.word()));
                    drop(g);
                    // This reader's process is the one being replaced: its
                    // generation moves on, so it stops here.
                    if let Err(e) = restart(lang) {
                        reader_state.lock().unwrap().push(LineKind::Error, format!("restart failed: {e}"));
                    }
                    return;
                }
            }
            g.partial = String::from_utf8_lossy(&acc).into_owned();
            g.partial_at = Some(std::time::Instant::now());
            // Stopped in R's debugger: its frame's objects for the Environment.
            let ask = g.frame_wanted && g.pending > 0 && is_browse(&g.partial);
            if ask {
                g.frame_wanted = false;
            }
            drop(g);
            if ask {
                frame_request(lang);
            }
        }
    });
    procs().lock().unwrap().insert(lang, Proc { child, stdin, req });
    if lang == Lang::R && !mods.trim().is_empty() {
        tell_mismatch(st, generation, mods, cwd);
    }
    Ok(())
}

/// A project whose renv.lock or .python-version asks for another R or Python
/// than the one starting is told so, once, with the environment that matches
/// (RStudio's renv check). In the background: the module list can take seconds.
fn tell_mismatch(st: Arc<Mutex<State>>, generation: u64, mods: String, cwd: String) {
    std::thread::spawn(move || {
        let Ok(running) = crate::envs::resolve(&mods) else { return };
        let Some(said) = crate::envs::mismatch(std::path::Path::new(&cwd), &running, || crate::host::modules().env) else { return };
        let mut g = st.lock().unwrap();
        if g.generation == generation {
            g.push(LineKind::System, said);
        }
    });
}

/// The modules the R console runs in with `r` (an R/x module) in place of
/// whatever R it had: the environment first, then the R, as `ml("R/x")`
/// inside R gives ("env/r4.6_py3.13-release R/4.5.3-foss-2024a").
fn with_r(mods: &str, r: &str) -> String {
    let mut v: Vec<&str> = mods.split_whitespace().filter(|m| !m.starts_with("R/")).collect();
    v.push(r);
    v.join(" ")
}

/// The Shell console loaded another R (`ml R/4.5`): the R console follows
/// (operator, 2026-11-15: "opening up R goes to 4.6 and not 4.5 which we
/// loaded"). Running, it restarts in it and says so; not running, its next
/// start is in it. The same as `ml("R/4.5")` typed in R.
fn follow_shell_r(r: &str) {
    if !valid_module(r) || !r.starts_with("R/") {
        return;
    }
    let s = crate::host::saved_settings();
    let now = live().lock().unwrap().get(&Lang::R).cloned().unwrap_or_else(|| s.module(Lang::R).to_owned());
    let next = with_r(&now, r);
    if next == now {
        return;
    }
    live().lock().unwrap().insert(Lang::R, next.clone());
    if running(Lang::R) {
        state(Lang::R).lock().unwrap().push(LineKind::System, format!("{r} was loaded in the Shell - restarting R in {next}. Objects in the workspace are not carried over."));
        if let Err(e) = restart(Lang::R) {
            state(Lang::R).lock().unwrap().push(LineKind::Error, format!("restart failed: {e}"));
        }
    }
}

fn running(lang: Lang) -> bool {
    let mut p = procs().lock().unwrap();
    match p.get_mut(&lang) {
        Some(k) => match k.child.try_wait() {
            Ok(None) => true,
            _ => {
                p.remove(&lang);
                false
            }
        },
        None => false,
    }
}

/// Send code to `lang`'s kernel, starting it first if it is not running.
/// Empty code only makes sure it runs (the app starts both at launch).
/// Python goes to the R session, wrapped for reticulate and echoed as the
/// Python it is.
/// `eval` with where the code came from: an R file's path and the line it
/// starts on, kept as its source references (DRIVER_R `.ndexr_src`), so
/// browser() and debug() name that file and line.
pub fn eval_at(lang: Lang, code: &str, at: Option<&(String, usize)>) -> Result<(), String> {
    match at {
        Some((path, line)) if lang == Lang::R && !waiting(Lang::R) && !code.trim().is_empty() && path.starts_with('/') => {
            let code = code.trim_end_matches(['\n', '\r', ' ', '\t']);
            send(Lang::R, &format!("{TOOLS}.ndexr_src({}, {}L, {})", r_string(path), (*line).max(1), r_string(code)), Some((Lang::R, code)))
        }
        _ => eval(lang, code),
    }
}

pub fn eval(lang: Lang, code: &str) -> Result<(), String> {
    // The R slot runs Python itself (an env/Python project): Python's code
    // goes to it as written; there is no R to send to.
    if py_primary() && running(Lang::R) {
        match lang {
            Lang::Python => return send(Lang::R, code, Some((Lang::Python, code))),
            Lang::R => return Err("this project's environment has Python and no R".into()),
            _ => {}
        }
    }
    // At a debugger or readline() prompt, Python's console types into it too
    // (pdb's n, s, c, p x), unwrapped.
    if lang == Lang::Python && waiting(Lang::R) {
        return send(Lang::R, code, None);
    }
    if lang == Lang::Python && !code.trim().is_empty() {
        let code = code.trim_end_matches(['\n', '\r', ' ', '\t']);
        return send(Lang::R, &format!("{TOOLS}.ndexr_py({})", r_string(code)), Some((Lang::Python, code)));
    }
    send(lang.session(), code, Some((lang, code)))
}

/// The data viewer: `name` in `lang`'s global environment, as a table
/// (KernelView::table). Not echoed: it is the IDE asking, not the person.
pub fn view(lang: Lang, name: &str) -> Result<(), String> {
    // `py$x`: an object in reticulate's Python, as the Environment lists it.
    let (py, name) = match name.strip_prefix("py$") {
        Some(n) => (true, n),
        None => (lang == Lang::Python, name),
    };
    let ok = !name.is_empty() && name.len() < 200 && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.') && !name.starts_with(|c: char| c.is_ascii_digit());
    if !ok {
        return Err(format!("not a name the viewer can show: {name}"));
    }
    if py_primary() && running(Lang::R) {
        return Err("the data viewer needs R in the environment".into());
    }
    let code = match lang {
        Lang::Shell => return Err("the shell has no data frames".into()),
        _ if py => format!("{TOOLS}.ndexr_view(\"py${name}\")"),
        _ => format!("{TOOLS}.ndexr_view({name:?})"),
    };
    send(Lang::R, &code, None)
}

/// How long output must stand without a newline to be a prompt: a long line
/// still being written is not one.
const PROMPT_SETTLE: std::time::Duration = std::time::Duration::from_millis(120);

/// Something is running and waiting for console input (`poll`'s prompt).
fn waiting_in(g: &State) -> bool {
    g.pending > 0 && !g.partial.is_empty() && g.partial_at.is_some_and(|t| t.elapsed() >= PROMPT_SETTLE)
}

fn waiting(lang: Lang) -> bool {
    waiting_in(&state(lang.session()).lock().unwrap())
}

/// Where Cove's helpers live (DRIVER_R attaches them as "tools:ndexr"), named
/// in full on every call Cove sends. Paused in browser() inside a box module,
/// R looks names up from the module, whose scope ends at base R and never
/// reaches the search path: `.ndexr_frame(...)` there was "could not find
/// function" after every Next (box.biomarker.ndexr.io, 2026-11-17).
/// as.environment() is base, which every module sees.
pub const TOOLS: &str = "as.environment(\"tools:ndexr\")$";

/// The debugger's frame, for the Environment: R evaluates what the browser
/// reads in the frame it stopped in, so `environment()` is that frame.
fn frame_request(lang: Lang) {
    if let Some(k) = procs().lock().unwrap().get_mut(&lang) {
        let _ = k.stdin.write_all(format!("{TOOLS}.ndexr_frame(environment())\n").as_bytes()).and_then(|_| k.stdin.flush());
    }
}

/// `echo`: the code to show in the transcript, with that language's prompts.
fn send(lang: Lang, code: &str, echo: Option<(Lang, &str)>) -> Result<(), String> {
    if !running(lang) {
        // A kernel that cannot start says why in its own console, not only
        // in a toast that fades (2026-10-13: the stack mid-publish).
        if let Err(e) = start(lang) {
            let st = state(lang);
            let mut g = st.lock().unwrap();
            g.pending = 0;
            g.push(LineKind::Error, e.clone());
            return Ok(());
        }
    }
    let code = code.trim_end_matches(['\n', '\r', ' ', '\t']);
    if code.trim().is_empty() {
        return Ok(());
    }
    let lines: Vec<&str> = code.split('\n').map(|l| l.trim_end_matches('\r')).collect();
    // A prompt is waiting (browser(), pdb, readline()): the lines answer it
    // on the console. R echoes them after its prompt, so no echo here.
    {
        let st = state(lang);
        let mut g = st.lock().unwrap();
        if waiting_in(&g) {
            g.frame_wanted = is_browse(&g.partial);
            g.partial.clear();
            g.partial_at = None;
            drop(g);
            let mut p = procs().lock().unwrap();
            let k = p.get_mut(&lang).ok_or("the kernel is not running")?;
            let mut text = lines.join("\n");
            text.push('\n');
            return k.stdin.write_all(text.as_bytes()).and_then(|_| k.stdin.flush()).map_err(|e| format!("{} did not take the input: {e}", lang.word()));
        }
    }
    {
        let st = state(lang);
        let mut g = st.lock().unwrap();
        // A request that stops in browser() shows that frame's objects.
        g.frame_wanted = true;
        g.forget_stop();
        if let Some((shown, text)) = echo {
            for (i, l) in text.split('\n').map(|l| l.trim_end_matches('\r')).enumerate() {
                g.push(LineKind::Input, format!("{} {l}", if i == 0 { shown.prompt() } else { shown.continuation() }));
            }
        }
        g.pending += 1;
    }
    let mut req = format!("{}\n", lines.len());
    for l in &lines {
        req.push_str(l);
        req.push('\n');
    }
    let mut p = procs().lock().unwrap();
    let k = p.get_mut(&lang).ok_or("the kernel is not running")?;
    let sent = match k.req.as_mut() {
        Some(r) => r.write_all(req.as_bytes()).and_then(|_| r.flush()),
        None => k.stdin.write_all(req.as_bytes()).and_then(|_| k.stdin.flush()),
    };
    sent.map_err(|e| format!("{} did not take the code: {e}", lang.word()))
}

/// Completions for the word before `cursor` in `line`, from the live
/// session: R's own completion engine (utils' .completeToken, as R's console
/// and RStudio use), Python's rlcompleter over reticulate's __main__, the
/// shell's compgen - each inside the environment's modules, so what the env
/// loaded is what completes. Hidden: nothing reaches the transcript. A busy
/// session is not interrupted for it.
pub fn complete(lang: Lang, line: &str, cursor: usize) -> Result<crate::api::Completion, String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let session = lang.session();
    if !running(session) {
        return Err("not running".into());
    }
    if state(session).lock().unwrap().pending > 0 {
        return Err("busy".into());
    }
    if line.len() > 4000 || line.contains('\n') {
        return Err("one line at a time".into());
    }
    let cursor = cursor.min(line.chars().count());
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let code = match lang {
        Lang::Shell => format!("__cove_complete {id} {} {cursor}", shell_quote(line)),
        Lang::Python if py_primary() => format!("__cove_complete({id}, {}, {cursor})", serde_json::to_string(line).unwrap_or_default()),
        Lang::Python => format!("{TOOLS}.ndexr_complete({id}, {}, {cursor}, TRUE)", r_string(line)),
        Lang::R => format!("{TOOLS}.ndexr_complete({id}, {}, {cursor}, FALSE)", r_string(line)),
        Lang::Julia => format!("__cove_complete({id}, {}, {cursor})", jl_string(line)),
    };
    send(session, &code, None)?;
    let until = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while std::time::Instant::now() < until {
        if let Some((got, c)) = state(session).lock().unwrap().compl.clone() {
            if got == id {
                return Ok(c);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    Err("no answer".into())
}

/// The transcript after `since`, and the kernel's state.
pub fn poll(lang: Lang, since: usize) -> KernelView {
    let lang = lang.session();
    let alive = running(lang);
    let st = state(lang);
    let g = st.lock().unwrap();
    let from = since.max(g.dropped) - g.dropped;
    KernelView {
        lines: g.lines.get(from.min(g.lines.len())..).unwrap_or_default().to_vec(),
        next: g.dropped + g.lines.len(),
        busy: alive && g.pending > 0,
        alive: alive && g.alive,
        banner: g.banner.clone(),
        env: g.env.clone(),
        plots: g.plots.len(),
        cwd: g.cwd.clone(),
        env_file: g.env_file.clone(),
        table: g.table.clone(),
        tables: g.tables,
        view: g.view.clone(),
        views: g.views,
        prompt: if waiting_in(&g) { g.partial.clone() } else { String::new() },
        debug: match &g.partial {
            p if waiting_in(&g) && is_browse(p) => "R".into(),
            p if waiting_in(&g) && is_pdb(p) => "Python".into(),
            _ => String::new(),
        },
        debug_file: g.dbg_file.clone(),
        debug_line: g.dbg_line,
        debug_from: g.dbg_from,
        debug_src: g.dbg_src.clone(),
    }
}

/// SIGINT, as Esc / the stop button in RStudio: R signals an `interrupt`
/// condition (reticulate turns it into KeyboardInterrupt inside Python); the
/// driver catches it.
pub fn interrupt(lang: Lang) -> Result<(), String> {
    let lang = lang.session();
    let pid = procs().lock().unwrap().get(&lang).map(|k| k.child.id()).ok_or("not running")?;
    let busy = state(lang).lock().unwrap().pending > 0;
    if !busy {
        return Ok(());
    }
    // The shell's work is its children: the whole group hears it, as a
    // terminal's ^C does; the driver traps it and keeps the session.
    let target = if lang == Lang::Shell { format!("-{pid}") } else { pid.to_string() };
    Command::new("kill").args(["-INT", &target]).status().map_err(|e| e.to_string()).map(|_| ())
}

/// Kill and start clean: a new process, an empty environment, no plots —
/// in whatever interpreter the settings name now.
/// Ended and not started again: a console the session's environment no
/// longer brings (an R project's R, when the project is now Julia's).
pub fn stop(lang: Lang) {
    let lang = lang.session();
    {
        let shared = state(lang);
        let mut st = shared.lock().unwrap();
        st.generation += 1;
        let n = st.lines.len();
        st.lines.clear();
        st.dropped += n;
    }
    if let Some(mut k) = procs().lock().unwrap().remove(&lang) {
        let _ = Command::new("kill").args(["-KILL", &format!("-{}", k.child.id())]).status();
        let _ = k.child.kill();
        let _ = k.child.wait();
    }
}

pub fn restart(lang: Lang) -> Result<(), String> {
    stop(lang);
    let lang = lang.session();
    start(lang)
}

/// Every kernel, and everything it started, ended: the server is going down.
pub fn shutdown_all() {
    for st in states().lock().unwrap().values() {
        st.lock().unwrap().generation += 1;
    }
    for (_, mut k) in procs().lock().unwrap().drain() {
        let _ = Command::new("kill").args(["-KILL", &format!("-{}", k.child.id())]).status();
        let _ = k.child.kill();
        let _ = k.child.wait();
    }
}

/// Plot `n` (0-based) as PNG bytes.
pub fn plot(lang: Lang, n: usize) -> Result<Vec<u8>, String> {
    let lang = lang.session();
    let p = state(lang).lock().unwrap().plots.get(n).cloned().ok_or("no such plot")?;
    // Only ever a file the driver wrote into this kernel's own plots folder.
    if p.parent() != Some(plots_dir(lang).as_path()) {
        return Err("not a plot".into());
    }
    std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))
}

const DRIVER_R: &str = r#"# Cove - the R kernel driver (app/src/kernel.rs writes this file).
local({
  S <- "\001ndexr\002"
  tag <- function(t, ...) {
    f <- gsub("[\t\r\n]+", " ", as.character(c(...)))
    cat(S, paste(c(t, f), collapse = "\t"), "\n", sep = "")
  }
  say <- function(t, msg) for (l in strsplit(paste(msg, collapse = "\n"), "\n", fixed = TRUE)[[1L]]) tag(t, l)

  # pimp lock: https only, and an https CRAN snapshot unless the module set one.
  r <- getOption("repos")
  if (is.null(r) || !length(r) || identical(unname(r["CRAN"]), "@CRAN@"))
    r <- c(CRAN = "https://packagemanager.posit.co/cran/latest")
  r[] <- sub("^http://", "https://", r)
  options(repos = r)

  # The Viewer, as RStudio's options(viewer=) (Options.R) and its Shiny hook
  # (SessionShinyViewer.R): htmlwidgets, gt, leaflet, plotly … hand it a page
  # in tempdir, shiny::runApp its address. The IDE shows either in its Viewer.
  # (Its own name: the data viewer below defines `view` too.)
  to_viewer <- function(url, ...) {
    tag("VIEW", url)
    invisible(url)
  }
  options(viewer = to_viewer, page_viewer = to_viewer, shiny.launch.browser = to_viewer)

  # Plots draw on an off-screen device that keeps its display list; after each
  # request a changed plot is replayed into a PNG the IDE shows.
  plots <- Sys.getenv("COVE_PLOTS", tempdir())
  # The IDE's colours (kernel.rs plot_env, from theme.rs): plots sit on the
  # Plots pane's own ground instead of a transparent page with black ink.
  bg   <- Sys.getenv("COVE_PLOT_BG", "white")
  fg   <- Sys.getenv("COVE_PLOT_FG", "black")
  dim  <- Sys.getenv("COVE_PLOT_DIM", "gray30")
  grid <- Sys.getenv("COVE_PLOT_GRID", "gray90")
  pal  <- Filter(nzchar, strsplit(Sys.getenv("COVE_PLOT_PALETTE"), ",", fixed = TRUE)[[1L]])
  if (length(pal)) grDevices::palette(c(fg, pal))
  options(device = function(...) {
    grDevices::pdf(NULL, width = 8, height = 5.5, bg = bg, fg = fg)
    grDevices::dev.control("enable")
    graphics::par(bg = bg, fg = dim, col = fg, col.axis = dim, col.lab = fg, col.main = fg, col.sub = dim)
  })
  # ggplot2 draws its own background, so it gets the same colours as a theme,
  # set the moment it loads; ggplot2 >= 3.5.2 also takes ink/paper for geoms.
  setHook(packageEvent("ggplot2", "onLoad"), function(...) try({
    gg <- asNamespace("ggplot2")
    base <- if ("ink" %in% names(formals(gg$theme_minimal))) gg$theme_minimal(base_size = 12, ink = fg, paper = bg)
            else gg$theme_minimal(base_size = 12)
    gg$theme_set(base + gg$theme(
      plot.background   = gg$element_rect(fill = bg, colour = NA),
      panel.background  = gg$element_rect(fill = bg, colour = NA),
      legend.background = gg$element_rect(fill = bg, colour = NA),
      legend.key        = gg$element_rect(fill = bg, colour = NA),
      panel.grid        = gg$element_line(colour = grid),
      text              = gg$element_text(colour = fg),
      axis.text         = gg$element_text(colour = dim),
      strip.text        = gg$element_text(colour = fg)))
    if (!"ink" %in% names(formals(gg$theme_minimal)))
      for (g in c("point", "line", "path", "text", "segment", "step", "errorbar", "linerange",
                  "pointrange", "hline", "vline", "abline", "rug"))
        try(gg$update_geom_defaults(g, list(colour = fg)), silent = TRUE)
    if (length(pal)) options(ggplot2.discrete.colour = pal, ggplot2.discrete.fill = pal)
  }, silent = TRUE))
  n <- 0L
  last <- NULL
  png_dev <- function(f) {
    if (capabilities("cairo")) grDevices::png(f, width = 8, height = 5.5, units = "in", res = 110, type = "cairo", bg = bg)
    else grDevices::png(f, width = 8, height = 5.5, units = "in", res = 110, bg = bg)
  }
  snap <- function() {
    if (grDevices::dev.cur() == 1L) return(invisible())
    p <- tryCatch(grDevices::recordPlot(), error = function(e) NULL)
    if (is.null(p) || !length(p[[1L]])) return(invisible())
    if (!is.null(last) && identical(p[[1L]], last[[1L]])) return(invisible())
    last <<- p
    n <<- n + 1L
    f <- file.path(plots, sprintf("plot-%04d.png", n))
    cur <- grDevices::dev.cur()
    ok <- tryCatch({ png_dev(f); grDevices::replayPlot(p); grDevices::dev.off(); TRUE },
                   error = function(e) { say("WARN", paste("plot not saved:", conditionMessage(e))); FALSE })
    if (cur %in% grDevices::dev.list()) grDevices::dev.set(cur)
    if (ok) tag("PLOT", f)
  }

  describe <- function(x) {
    if (is.data.frame(x)) return(sprintf("%d obs. of %d variable%s", nrow(x), ncol(x), if (ncol(x) == 1L) "" else "s"))
    if (is.function(x)) {
      a <- tryCatch(deparse(args(x))[1L], error = function(e) "function")
      return(sub("\\s+$", "", if (is.na(a) || a == "NULL") "function" else a))
    }
    paste(utils::capture.output(utils::str(x, give.attr = FALSE, vec.len = 2, max.level = 0)), collapse = " ")
  }
  env <- function(e = globalenv()) {
    tag("ENVBEGIN")
    for (nm in ls(e)) {
      x <- get(nm, envir = e)
      v <- tryCatch(describe(x), error = function(e) "?")
      tag("ENV", nm, class(x)[1L], substr(trimws(v), 1L, 200L))
    }
  }

  # The function frame `e` belongs to, by its source reference (kernel.rs
  # DBGFN): its file (a real one, else ""), first line, the line of its
  # browser() call, then its source - so the editor can show where R stopped
  # even before the first step, which says only "Called from:".
  where <- function(e) {
    fn <- NULL
    for (i in rev(seq_len(sys.nframe()))) if (identical(sys.frame(i), e)) { fn <- sys.function(i); break }
    sr <- if (!is.null(fn)) attr(fn, "srcref")
    if (is.null(sr)) return(invisible())
    sf <- attr(sr, "srcfile")
    lines <- getSrcLines(sf, sr[1L], sr[3L])
    file <- if (isTRUE(sf$isFile) && file.exists(sf$filename)) sf$filename else ""
    b <- grep("browser\\s*\\(", lines)[1L]
    tag("DBGFN", file, sr[1L], if (is.na(b)) sr[1L] else sr[1L] + b - 1L)
    for (l in lines) tag("DBGSRC", if (nzchar(l)) l else " ")
  }

  # Live modules (Inlet 0010): ml() / module() swap Lmod modules in
  # this running R. Lmod writes R for an R shell (Sys.setenv / Sys.unsetenv,
  # then `mlstatus <- TRUE`), evaluated in a scratch env so mlstatus never
  # lands in the workspace. `mods` is the list the person asked for - what
  # the kernel restarts into - not every dependency Lmod pulled in.
  words <- function(x) { x <- unlist(strsplit(x, "[[:space:]]+")); x[nzchar(x)] }
  base <- words(Sys.getenv("COVE_MODULE"))
  mods <- words(Sys.getenv("COVE_MODULES"))
  label <- function() {
    lead <- if (length(base)) base[1L] else if (length(mods)) mods[1L] else ""
    extra <- length(setdiff(mods, base))
    paste0(R.version.string, if (nzchar(lead)) paste0(" · ", lead) else "", if (extra) paste0(" +", extra) else "",
           if (nzchar(py_version)) paste0(" · Python ", py_version) else "")
  }
  verbs <- c("av", "avail", "spider", "list", "show", "help", "whatis", "keyword", "describe", "savelist",
             "overview", "is-loaded", "is-avail", "load", "try-load", "add", "unload", "del", "rm", "purge",
             "swap", "sw", "switch", "restore", "reset", "refresh")
  libs_env <- function(v) { p <- unlist(strsplit(Sys.getenv(v), ":", fixed = TRUE)); unique(p[nzchar(p)]) }
  lmod <- function(args) {
    cmd <- Sys.getenv("LMOD_CMD")
    if (!nzchar(cmd)) stop("no Lmod in this R - it was not started from a module (Tools > Global Options)", call. = FALSE)
    err <- tempfile()
    on.exit(unlink(err))
    out <- suppressWarnings(system2(cmd, c("R", shQuote(args)), stdout = TRUE, stderr = err))
    msg <- readLines(err, warn = FALSE)
    if (length(msg)) cat(msg, sep = "\n")
    e <- new.env()
    if (length(out)) eval(parse(text = out), e)
    !identical(e$mlstatus, FALSE)
  }
  drop_mod <- function(have, gone) have[!(have %in% gone | sub("/.*", "", have) %in% gone)]
  ml <- function(...) {
    a <- words(as.character(c(...)))
    if (!length(a)) a <- "list"
    bad <- a[!grepl("^-{0,2}[A-Za-z0-9._+/][A-Za-z0-9._+/-]*$", a)]
    if (length(bad)) stop("not a module name: ", bad[1L], call. = FALSE)
    verb <- if (a[1L] %in% verbs) a[1L] else "ml"
    rest <- if (verb == "ml") a else a[-1L]
    un <- sub("^-", "", rest[startsWith(rest, "-")])
    ld <- rest[!startsWith(rest, "-")]
    steps <- switch(verb,
      ml = list(if (length(un)) c("unload", un), if (length(ld)) c("load", ld)),
      reset = list("purge", if (length(base)) c("load", base)),
      list(a))
    r0 <- Sys.getenv("EBROOTR"); site0 <- libs_env("R_LIBS_SITE"); own0 <- c(libs_env("R_LIBS"), libs_env("R_LIBS_USER"))
    ok <- TRUE
    for (s in Filter(Negate(is.null), steps)) if (!(ok <- lmod(s))) break
    # A step Lmod refused (unknown name, conflict) leaves the list as it was.
    if (ok) mods <<- switch(verb,
      ml = unique(c(drop_mod(mods, un), ld)),
      load = , `try-load` = , add = unique(c(mods, rest)),
      unload = , del = , rm = drop_mod(mods, rest),
      purge = character(),
      reset = base,
      swap = , sw = , switch = unique(c(drop_mod(mods, rest[1L]), rest[length(rest)])),
      mods)
    if (!verb %in% c("ml", "load", "try-load", "add", "unload", "del", "rm", "purge", "reset", "swap", "sw", "switch", "restore"))
      return(invisible())
    r1 <- Sys.getenv("EBROOTR")
    if (!identical(r1, r0)) {
      if (!nzchar(r1)) {
        cat("This list has no R in it: R keeps running as it was. ml(\"reset\") goes back to ",
            if (length(base)) paste(base, collapse = " ") else "the start", ".\n", sep = "")
      } else {
        tag("MODS", paste(mods, collapse = " "))
        tag("RESTART", paste(mods, collapse = " "))
        cat("R itself changes (", r1, ") - restarting R in: ", paste(mods, collapse = " "),
            "\nObjects in the workspace are not carried over.\n", sep = "")
        return(invisible(mods))
      }
    }
    # The library path follows the modules: site libraries in .Library.site
    # (R adds those to every .libPaths() call), R_LIBS / R_LIBS_USER in front,
    # and anything the person added with .libPaths() kept.
    site1 <- libs_env("R_LIBS_SITE")
    site1 <- site1[dir.exists(site1)]
    b <- baseenv()
    unlockBinding(".Library.site", b)
    assign(".Library.site", site1, envir = b)
    lockBinding(".Library.site", b)
    own1 <- c(libs_env("R_LIBS"), libs_env("R_LIBS_USER"))
    keep <- setdiff(.libPaths(), normalizePath(c(own0, site0, .Library), mustWork = FALSE))
    .libPaths(unique(c(own1[dir.exists(own1)], keep)))
    tag("MODS", paste(mods, collapse = " "))
    tag("BANNER", label())
    invisible(mods)
  }
  # The data viewer (RStudio's View()): a data frame, tibble or matrix as a
  # table the IDE opens in the source pane - the first 1000 rows and 200
  # columns, each value as print() would write it.
  view <- function(x, title) {
    if (missing(title)) title <- paste(deparse(substitute(x), nlines = 1L), collapse = "")
    if (is.matrix(x) || (is.array(x) && length(dim(x)) == 2L)) x <- as.data.frame(x, stringsAsFactors = FALSE)
    if (!is.data.frame(x)) x <- tryCatch(as.data.frame(x, stringsAsFactors = FALSE), error = function(e)
      stop("View() shows data frames, tibbles and matrices, not ", class(x)[1L], call. = FALSE))
    nr <- nrow(x); nc <- ncol(x)
    x <- x[seq_len(min(nr, 1000L)), seq_len(min(nc, 200L)), drop = FALSE]
    cols <- lapply(x, function(v) {
      f <- tryCatch(format(v, trim = TRUE, justify = "none"), error = function(e) as.character(v))
      f <- as.character(f)
      na <- tryCatch(is.na(v), error = function(e) rep(FALSE, length(f)))
      if (length(na) == length(f)) f[na] <- "NA"
      f
    })
    tag("TABLE", title, nr, nc)
    tag("TCOLS", names(x))
    tag("TTYPES", vapply(x, function(v) class(v)[1L], ""))
    for (i in seq_len(nrow(x))) tag("TROW", vapply(cols, function(v) v[[i]], ""))
    tag("TEND")
    invisible(NULL)
  }
  # reticulate (operator, 2026-10-13): Python lives inside this R session -
  # R syntax for both (import(), py$x, r_to_py()), never rpy2. Attached and
  # initialised before the first prompt; installed into the person's own
  # library the first time (no env/ module carries it yet, measured
  # 2026-10-13). pyh is the cove helper module (kernel.rs HELPER_PY):
  # .py files and {python} chunks run through pyh$run in __main__, which is
  # reticulate's `py`.
  pyh <- NULL
  py_version <- ""
  py_why <- "it has not started"
  # Where reticulate goes when no library has it: R_LIBS_USER when it can be
  # written, else ~/.ndexr/R/<x.y>-<platform> - in a cell R_LIBS_USER is the
  # read-only shared library and only /root/.ndexr outlives a restart (it is the
  # account's ~/.ndexr/state), so an install anywhere else is lost each start.
  # That library joins .libPaths() at every start, not only after an install.
  ndexr_lib <- path.expand(file.path("~", ".ndexr", "R",
    paste0(R.version$major, ".", sub("[.].*", "", R.version$minor), "-", R.version$platform)))
  # In a cell /root is scratch (only /root/.ndexr survives a restart) and the
  # env module points R_LIBS_USER at ~/R/ndexr/..., i.e. under /root: so the
  # person's own installs go to ndexr_lib, first in .libPaths().
  in_cell <- identical(Sys.getenv("NDEXR_POOL"), "cell")
  if (in_cell) dir.create(ndexr_lib, recursive = TRUE, showWarnings = FALSE)
  if (dir.exists(ndexr_lib)) .libPaths(c(ndexr_lib, .libPaths()))
  # The shared library is the env module's own (R_LIBS_SITE): the console,
  # every cell and every person load the same env and its site library
  # (operator, 2026-10-18). Until then this added the console's private
  # /eblib here; that library is gone.
  own_lib <- function() {
    if (in_cell) return(ndexr_lib)
    lib <- path.expand(strsplit(Sys.getenv("R_LIBS_USER"), ":", fixed = TRUE)[[1L]][1L])
    ok <- !is.na(lib) && nzchar(lib) &&
      (dir.exists(lib) || dir.create(lib, recursive = TRUE, showWarnings = FALSE)) && file.access(lib, 2L) == 0L
    if (ok) lib else ndexr_lib
  }
  py_setup <- function() {
    # The IDE's pick wins over a startup profile's: in a cell, cell-entry.sh
    # exports the platform console's R_PROFILE_USER (console-ml.Rprofile),
    # which sets RETICULATE_PYTHON=/opt/venv/bin/python - a path no cell has
    # (2026-10-13). Unset when the person exported their own; theirs stands.
    pick <- Sys.getenv("COVE_RETICULATE_PYTHON")
    if (nzchar(pick)) Sys.setenv(RETICULATE_PYTHON = pick)
    # The environment asked for did not load (a stack remount mid-start,
    # 2026-10-19): this R is not its R, and installing reticulate here built it
    # into the account's shared ~/R from the system R, where two consoles'
    # installs collided on 00LOCK. Say so; a restart loads the environment.
    want <- Sys.getenv("COVE_MODULE")
    loaded <- unlist(strsplit(Sys.getenv("LOADEDMODULES"), ":", fixed = TRUE))
    if (nzchar(want) && !want %in% loaded)
      stop(want, " did not load, so this is ", R.version.string, " from ", R.home(),
           " - restart R to load it (the stack may have been updating)", call. = FALSE)
    if (!requireNamespace("reticulate", quietly = TRUE)) {
      lib <- own_lib()
      dir.create(lib, recursive = TRUE, showWarnings = FALSE)
      .libPaths(c(lib, .libPaths()))
      cat("reticulate is not installed for this R - installing it into ", lib,
          " (once; built from source, it takes a few minutes)\n", sep = "")
      utils::flush.console()
      cores <- suppressWarnings(parallel::detectCores())
      utils::install.packages("reticulate", lib = lib, quiet = TRUE,
                              Ncpus = if (is.na(cores)) 1L else max(1L, min(4L, cores)))
    }
    if (!requireNamespace("reticulate", quietly = TRUE))
      stop("reticulate did not install - see the messages above", call. = FALSE)
    suppressPackageStartupMessages(library(reticulate))
    cfg <- reticulate::py_config()
    pyh <<- reticulate::import_from_path("cove", path = Sys.getenv("COVE_HELPERS"), convert = TRUE)
    # `r` in Python, as repl_python() and knitr's python engine have it.
    inject <- get0("py_inject_r", envir = asNamespace("reticulate"), inherits = FALSE)
    if (is.function(inject)) try(inject(), silent = TRUE)
    py_version <<- reticulate::py_eval("__import__('platform').python_version()")
    invisible(cfg)
  }
  py_run <- function(code) {
    if (is.null(pyh)) stop("Python is not running in this R session: ", py_why, call. = FALSE)
    flush(stdout())
    err <- pyh$run(code)
    if (nzchar(err)) say("ERR", sub("\n+$", "", err))
    invisible()
  }
  py_snap <- function() {
    if (is.null(pyh)) return(invisible())
    for (f in unlist(pyh$figs(plots, n))) {
      n <<- n + 1L
      tag("PLOT", f)
    }
  }
  py_env <- function() {
    if (is.null(pyh)) return(invisible())
    for (row in pyh$env()) tag("ENV", paste0("py$", row[[1L]]), row[[2L]], substr(trimws(row[[3L]]), 1L, 200L))
  }

  local({
    t <- new.env()
    t$ml <- ml
    t$module <- ml
    t$View <- view
    t$.ndexr_view <- function(name) {
      if (startsWith(name, "py$")) view(reticulate::py_eval(substring(name, 4L)), name)
      else view(get(name, envir = globalenv()), name)
    }
    t$.ndexr_py <- py_run
    # The frame browser() stopped in (kernel.rs frame_request): its objects
    # for the Environment, as RStudio shows the function's environment.
    t$.ndexr_frame <- function(e) {
      tryCatch(env(e), error = function(err) NULL)
      tryCatch(where(e), error = function(err) NULL)
      tag("ENVEND")
      invisible()
    }
    t$.ndexr_src <- function(path, line, code) {
      run(code, path, line)
      invisible()
    }
    # Tab in the Inputs (kernel.rs `complete`): what may finish the word before
    # `cursor`, R's way or - py = TRUE - Python's, answered as one COMPL tag.
    t$.ndexr_complete <- function(id, line, cursor, py = FALSE) {
      start <- 0L
      items <- character()
      try(silent = TRUE, {
        if (py) {
          if (!is.null(pyh)) {
            got <- pyh$complete(line, as.integer(cursor))
            start <- as.integer(got[[1L]])
            items <- as.character(unlist(got[[2L]]))
          }
        } else {
          ce <- utils:::.CompletionEnv
          utils:::.assignLinebuffer(line)
          utils:::.assignEnd(cursor)
          utils:::.guessTokenFromLine()
          utils:::.completeToken()
          items <- utils:::.retrieveCompletions()
          start <- as.integer(ce[["start"]])
        }
      })
      tag("COMPL", id, start, utils::head(unique(items), 200L))
      invisible()
    }
    # Session › Clear Workspace: R's objects and Python's alike.
    t$.ndexr_clear <- function() {
      rm(list = ls(globalenv(), all.names = FALSE), envir = globalenv())
      if (!is.null(pyh)) pyh$clear()
      invisible()
    }
    attach(t, name = "tools:ndexr", warn.conflicts = FALSE)
  })

  # Source references kept, as RStudio's console keeps them: browser() and
  # debug() then say where they are. Code from an editor file (`.ndexr_src`)
  # is parsed as that file, padded to its own line numbers.
  run <- function(code, path = "", line = 1L) {
    src <- c(rep("", max(0L, line - 1L)), strsplit(code, "\n", fixed = TRUE)[[1L]])
    sf <- srcfilecopy(path, src, isFile = nzchar(path))
    exprs <- tryCatch(parse(text = src, srcfile = sf, keep.source = TRUE),
                      error = function(e) { say("ERR", paste("Error:", conditionMessage(e))); NULL })
    for (e in exprs) {
      ok <- withCallingHandlers(
        tryCatch({
          r <- withVisible(eval(e, globalenv()))
          if (r$visible) print(r$value)
          TRUE
        }, error = function(err) {
          call <- conditionCall(err)
          # a top-level stop() names the driver's own eval(); R's console says "Error: msg"
          if (identical(call, quote(eval(e, globalenv())))) call <- NULL
          msg <- conditionMessage(err)
          say("ERR", if (is.null(call)) paste("Error:", msg)
                     else paste0("Error in ", deparse(call, nlines = 1L)[1L], " : ", msg))
          FALSE
        }, interrupt = function(i) { say("ERR", "Interrupted"); FALSE }),
        warning = function(w) {
          say("WARN", paste("Warning message:", conditionMessage(w)))
          invokeRestart("muffleWarning")
        },
        message = function(m) {
          say("WARN", sub("\n$", "", conditionMessage(m)))
          invokeRestart("muffleMessage")
        })
      if (!ok) break
    }
  }

  finish <- function() {
    utils::flush.console()
    tryCatch(snap(), error = function(e) NULL)
    tryCatch(py_snap(), error = function(e) say("WARN", paste("matplotlib plots:", conditionMessage(e))))
    tag("CWD", getwd())
    tryCatch({ env(); py_env() }, error = function(e) NULL)
    tag("DONE")
    flush(stdout())
  }

  try(setwd(Sys.getenv("COVE_CWD", getwd())), silent = TRUE)
  tag("BANNER", label())
  tryCatch(py_setup(), error = function(e) {
    py_why <<- conditionMessage(e)
    say("WARN", paste0("Python is not running in this session: ", py_why,
                       "\nR works as ever; pick an env/ module with a Python in Tools > Global Options, then restart R."))
  }, interrupt = function(i) {
    py_why <<- "its start was interrupted - restart R to try again"
    say("WARN", "Python start interrupted - restart R to try again")
  })
  # The one line a start says.
  cat("R ", R.version$major, ".", R.version$minor, if (!is.null(pyh)) paste0(" · Python ", py_version) else "", "\n", sep = "")
  tag("BANNER", label())
  finish()

  # Requests on their own FIFO; stdin stays R's console (kernel.rs header).
  req <- Sys.getenv("COVE_REQ")
  con <- if (nzchar(req)) file(req, open = "r", raw = TRUE) else file("stdin", open = "r")
  repeat {
    h <- tryCatch(readLines(con, n = 1L), interrupt = function(i) character())
    if (!length(h)) { if (isIncomplete(con) || nzchar(req)) { Sys.sleep(0.02); next } else break }
    k <- suppressWarnings(as.integer(h))
    code <- if (is.na(k) || k < 1L) "" else paste(readLines(con, n = k), collapse = "\n")
    tryCatch(run(code), interrupt = function(i) say("ERR", "Interrupted"))
    tryCatch(finish(), interrupt = function(i) tag("DONE"))
  }
})
"#;

/// reticulate's side of the R session, imported by the R driver as the module
/// `cove` (from COVE_HELPERS). Python code from a .py file, a
/// {python} chunk or the Packages pane runs here, in `__main__` - the module
/// reticulate's `py` is - so R and Python see each other's objects.
const HELPER_PY: &str = r#"# Cove - reticulate's side of the R session (app/src/kernel.rs writes this file).
import ast, os, sys, traceback


def complete(line, cursor):
    """The names that may finish the word before `cursor`: jedi over the live
    objects in __main__ (reticulate's `py`) where the environment has it -
    attributes after a dot, a call's parameters as `name=` inside it - else
    Python's own REPL completer, rlcompleter: [where the word starts,
    candidates]."""
    import re, rlcompleter
    head = line[:cursor]
    try:
        import jedi
        got = jedi.Interpreter(head, [sys.modules["__main__"].__dict__]).complete()
        if got:
            word = re.search(r"\w*$", head).group(0)
            out = []
            for c in got[:300]:
                if c.name_with_symbols not in out:
                    out.append(c.name_with_symbols)
            return [cursor - len(word), out]
    except Exception:
        pass
    m = re.search(r"[A-Za-z_][\w.]*$", head)
    word = m.group(0) if m else ""
    c = rlcompleter.Completer(sys.modules["__main__"].__dict__)
    out = []
    for i in range(300):
        s = c.complete(word, i)
        if s is None:
            break
        if s not in out:
            out.append(s)
    return [cursor - len(word), out]


def run(src):
    """Python as a console runs it: every statement, then the last expression
    printed. Returns "" or the error, which the R driver shows as one."""
    g = sys.modules["__main__"].__dict__
    try:
        tree = ast.parse(src, "<python>", "exec")
        last = None
        if tree.body and isinstance(tree.body[-1], ast.Expr):
            last = ast.Expression(tree.body.pop().value)
        exec(compile(tree, "<python>", "exec"), g)
        if last is not None:
            v = eval(compile(last, "<python>", "eval"), g)
            if v is not None:
                g["_"] = v
                print(repr(v))
        return ""
    except SystemExit:
        return "SystemExit ignored - restart R for a clean session"
    except KeyboardInterrupt:
        return "KeyboardInterrupt"
    except SyntaxError:
        return "".join(traceback.format_exception_only(*sys.exc_info()[:2]))
    except BaseException:
        et, ev, tb = sys.exc_info()
        return "".join(traceback.format_exception(et, ev, tb.tb_next))
    finally:
        sys.stdout.flush()
        sys.stderr.flush()


def figs(folder, n):
    """matplotlib figures, saved and closed after each request (inline, as in
    Jupyter). Numbered on from R's own plots, `n`."""
    plt = sys.modules.get("matplotlib.pyplot")
    saved = []
    if plt is None:
        return saved
    for num in plt.get_fignums():
        n += 1
        f = os.path.join(folder, "plot-%04d.png" % n)
        try:
            plt.figure(num).savefig(f, dpi=110, bbox_inches="tight")
            saved.append(f)
        except Exception as e:
            print("plot not saved: %s" % e, file=sys.stderr)
    plt.close("all")
    return saved


def describe(v):
    t = type(v).__name__
    if t == "module":
        return getattr(v, "__name__", "")
    shape = getattr(v, "shape", None)
    if isinstance(shape, tuple):
        return "%s %s" % (t, " x ".join(map(str, shape)))
    if callable(v):
        try:
            import inspect
            return "%s%s" % (getattr(v, "__name__", t), inspect.signature(v))
        except Exception:
            return t
    r = repr(v)
    if hasattr(v, "__len__") and not isinstance(v, (str, bytes)):
        try:
            r = "[%d] %s" % (len(v), r)
        except Exception:
            pass
    return r[:200]


def env():
    """__main__'s objects as [name, type, value] rows; `r` (reticulate's
    window onto R) and _names are the plumbing, not the person's."""
    g = sys.modules["__main__"].__dict__
    rows = []
    for k in sorted(g):
        if k.startswith("_") or k == "r":
            continue
        v = g[k]
        try:
            d = describe(v)
        except Exception:
            d = "?"
        rows.append([k, type(v).__name__, d])
    return rows


def clear():
    g = sys.modules["__main__"].__dict__
    for k in [k for k in g if not k.startswith("_") and k != "r"]:
        del g[k]
"#;

const DRIVER_PY: &str = r#"# Cove - the Python kernel driver, for an environment with no R (app/src/kernel.rs
# writes this file). An env/Python project's console is Python itself, not
# Python inside R through reticulate (operator, 2026-10-26: "make sure the
# environment is selected ... so that we can prevent having an R, Py, Julia,
# etc shell"). Requests come on COVE_REQ as R's and Julia's do: a line with the
# number of code lines, then the lines. Each runs as a console runs it (cove.py
# `run`, the same as reticulate's side uses); then the same tags the other
# drivers end with - PLOT, ENVBEGIN/ENV/ENVEND, CWD, BANNER, DONE, ERR, COMPL.
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import cove

_S = "\x01ndexr\x02"


def _tag(*fields):
    print(_S + "\t".join(str(f).replace("\t", " ").replace("\n", " ") for f in fields), flush=True)


try:
    os.chdir(os.environ.get("COVE_CWD") or os.path.expanduser("~"))
except OSError:
    os.chdir(os.path.expanduser("~"))
_mods = os.environ.get("COVE_MODULES", "")
_label = "python %s · %s" % (sys.version.split()[0], _mods or "host")
_plots = [0]


def _finish():
    for f in cove.figs(os.environ.get("COVE_PLOTS", "/tmp"), _plots[0]):
        _plots[0] += 1
        _tag("PLOT", f)
    _tag("ENVBEGIN")
    for row in cove.env():
        _tag("ENV", *row)
    _tag("ENVEND")
    _tag("CWD", os.getcwd())
    _tag("BANNER", _label)
    _tag("DONE")


def __cove_complete(id, line, cursor):
    at, items = cove.complete(line, cursor)
    _tag("COMPL", id, at, *items[:200])


_req = open(os.environ.get("COVE_REQ", "/dev/stdin"))
print("Python", sys.version.split()[0], flush=True)
_finish()
while True:
    try:
        head = _req.readline()
    except KeyboardInterrupt:
        continue
    if not head:
        break
    try:
        n = int(head.strip() or 0)
    except ValueError:
        n = 0
    code = "\n".join(_req.readline().rstrip("\n") for _ in range(n))
    err = cove.run(code)
    if err:
        sys.stdout.write(err if err.endswith("\n") else err + "\n")
        _tag("ERR", err.strip().splitlines()[-1] if err.strip() else err)
    _finish()
"#;

const DRIVER_JL: &str = r#"# Cove - the Julia kernel driver (app/src/kernel.rs writes this file).
# Requests come on COVE_REQ: a line with the number of code lines, then the
# lines. Each runs in Main; what it returns is shown as the REPL would, then
# the same tags the shell driver ends with (CWD, BANNER, DONE, ERR, COMPL).
import REPL
const __COVE_S = "\x01ndexr\x02"
function __cove_tag(f, args...)
    parts = String[f; [replace(string(a), r"[\t\n]" => " ") for a in args]]
    print(stdout, __COVE_S, join(parts, "\t"), "\n")
    flush(stdout)
end
# ^C stops the code that is running, not the session (kernel.rs `interrupt`).
Base.exit_on_sigint(false)
try
    cd(get(ENV, "COVE_CWD", homedir()))
catch
    cd(homedir())
end
__cove_label() = "julia $(VERSION)" * (isempty(get(ENV, "COVE_MODULES", "")) ? " · host" : " · " * ENV["COVE_MODULES"])
__cove_finish() = (__cove_tag("CWD", pwd()); __cove_tag("BANNER", __cove_label()); __cove_tag("DONE"))
# Tab in the Inputs: Julia's own REPL completions for the line up to the cursor
# (characters); the offset is where the completed word starts, in characters.
function __cove_complete(id, line, cursor)
    prefix = first(line, cursor)
    got = String[]
    at = length(prefix)
    try
        cs, range, _ = REPL.REPLCompletions.completions(prefix, lastindex(prefix))
        got = unique(String[REPL.REPLCompletions.completion_text(c) for c in cs])[1:min(end, 200)]
        at = first(range) > 1 ? length(prefix[1:prevind(prefix, first(range))]) : 0
    catch
    end
    __cove_tag("COMPL", id, at, got...)
    nothing
end
# Plots come back as PNG files, as R's and matplotlib's do (a PLOT tag per
# file in COVE_PLOTS). GR draws off-screen ("100" = no window). A value that
# renders to PNG (a Plots.jl plot, a Makie figure, an image) is drawn instead
# of printed, and display() lands here too - through our display, which is
# kept on top of the stack because `using Plots` pushes its own.
get!(ENV, "GKSwstype", "100")
const __COVE_PLOTS = get(ENV, "COVE_PLOTS", tempdir())
const __cove_nplot = Ref(0)
__cove_drawable(x) = !(x isa AbstractString || x isa Number || x isa Symbol) &&
    Base.invokelatest(showable, MIME"image/png"(), x)
function __cove_png(x)
    f = joinpath(__COVE_PLOTS, "plot-" * lpad(__cove_nplot[] + 1, 4, '0') * ".png")
    try
        open(io -> Base.invokelatest(show, io, MIME"image/png"(), x), f, "w")
        __cove_nplot[] += 1
        __cove_tag("PLOT", f)
    catch e
        rm(f; force = true)
        __cove_tag("WARN", "plot not saved: " * sprint(showerror, e))
    end
    nothing
end
struct __CoveDisplay <: AbstractDisplay end
Base.display(::__CoveDisplay, x) = __cove_drawable(x) ? __cove_png(x) : throw(MethodError(display, (x,)))
function __cove_on_top()
    ds = Base.Multimedia.displays
    isempty(ds) || last(ds) isa __CoveDisplay && return
    filter!(d -> !(d isa __CoveDisplay), ds)
    pushdisplay(__CoveDisplay())
end
# Plots.jl in the IDE's colours (kernel.rs plot_env), set the moment it loads,
# so the first plot() after `using Plots` already has them.
function __cove_theme(pkg)
    pkg.name == "Plots" || return
    P = Base.loaded_modules[pkg]
    bg, fg = get(ENV, "COVE_PLOT_BG", "white"), get(ENV, "COVE_PLOT_FG", "black")
    pal = filter(!isempty, split(get(ENV, "COVE_PLOT_PALETTE", ""), ","))
    try
        Base.invokelatest(P.default; background_color = bg, foreground_color = fg)
        isempty(pal) || Base.invokelatest(P.default; palette = Base.invokelatest(P.palette, pal))
    catch e
        __cove_tag("WARN", "Plots colours not set: " * sprint(showerror, e))
    end
end
push!(Base.package_callbacks, __cove_theme)
function __cove_run(code)
    __cove_on_top()
    try
        val = include_string(Main, code, "In")
        __cove_on_top()
        if val !== nothing && !endswith(rstrip(code), ';')
            # invokelatest: `code` may have loaded the methods that show val
            # (`using Plots` then plot(...) - "method too new to be called
            # from this world context", 2026-10-31).
            if __cove_drawable(val)
                __cove_png(val)
            else
                Base.invokelatest(show, stdout, MIME"text/plain"(), val)
                println()
            end
        end
    catch e
        e isa LoadError && (e = e.error)
        if e isa InterruptException
            println("^C")
        else
            showerror(stdout, e)
            println()
            __cove_tag("ERR", sprint(showerror, e))
        end
    end
    flush(stdout)
end
const __COVE_REQ = open(get(ENV, "COVE_REQ", "/dev/stdin"))
println("julia ", VERSION)
__cove_finish()
while true
    local h
    try
        eof(__COVE_REQ) && break
        h = something(tryparse(Int, strip(readline(__COVE_REQ))), 0)
    catch e
        e isa InterruptException && continue
        rethrow()
    end
    code = join([readline(__COVE_REQ) for _ in 1:h], "\n")
    __cove_run(code)
    __cove_finish()
end
"#;

const DRIVER_SH: &str = r#"# Cove - the Prefix shell driver (app/src/kernel.rs writes this file).
# bash inside the ndexr stack: the Gentoo Prefix's Lmod and `ml`, the modules
# picked for the shell, and the person's working directory. Line mode: each
# request runs to the end, so full-screen programs (vim, top, less) do not.
S=$'\001ndexr\002'
tag() {
  local f=$1 a
  shift
  for a in "$@"; do a=${a//$'\t'/ }; a=${a//$'\n'/ }; f+=$'\t'$a; done
  printf '%s%s\n' "$S" "$f"
}
exec 3<&0 </dev/null
unset HISTFILE
export PAGER=cat GIT_PAGER=cat MANPAGER=cat TERM=dumb
if [ -n "${COVE_USE_SH:-}" ] && [ -r "$COVE_USE_SH" ]; then
  . "$COVE_USE_SH" >/dev/null 2>&1 || echo "could not enter the ndexr stack through $COVE_USE_SH"
  # The host's single-language env/ modules, so `ml av` and `ml env/R/...` here see them.
  [ -n "${COVE_ENV_LOCAL:-}" ] && [ -d "$COVE_ENV_LOCAL" ] && module use --append "$COVE_ENV_LOCAL" >/dev/null 2>&1
  if [ -n "${COVE_ENV_SCRIPT:-}" ] && [ -r "$COVE_ENV_SCRIPT" ]; then
    # The load, resolved once (envs.rs): `ml` still layers on top of it.
    . "$COVE_ENV_SCRIPT" || echo "could not load $COVE_MODULES"
  elif [ -n "${COVE_MODULES:-}" ]; then
    # shellcheck disable=SC2086
    ml $COVE_MODULES 2>&1 || echo "could not load $COVE_MODULES"
  fi
else
  echo "the ndexr stack is not on this machine yet - this shell has the host's programs only"
fi
cd "${COVE_CWD:-$HOME}" 2>/dev/null || cd
label() {
  local m=${COVE_MODULES:-}
  printf 'bash %s · %s%s' "${BASH_VERSION%%(*}" "${COVE_PREFIX:+Prefix }${COVE_PREFIX:-host}" "${m:+ · $m}"
}
# ^C ends the whole request, as at a terminal: the trap returns from run(),
# so `sleep 30; echo next` stops at the sleep - and cd and variables, set in
# this shell, stay.
trap 'printf "^C\n"; return 130' INT
run() { eval "$1"; }
# Tab in the Inputs (kernel.rs `complete`): commands for a line's first word
# (or after | ; && $( ), files and folders otherwise - from this shell's PATH
# and folder, so the env's modules are what completes. Folders end in /.
__cove_complete() {
  local id=$1 line=${2:0:$3} w before i
  local -a got out
  w=${line##*[[:space:]=\'\"\(\<\>]}
  before=${line:0:$((${#line} - ${#w}))}
  if [[ -z ${before//[[:space:]]/} || $before =~ (\||;|\&|\$\()[[:space:]]*$ ]]; then
    mapfile -t got < <(compgen -c -- "$w" 2>/dev/null | sort -u | head -200)
  else
    mapfile -t got < <(compgen -f -- "$w" 2>/dev/null | sort | head -200)
  fi
  for i in "${got[@]}"; do
    [[ -d $i ]] && i+=/
    out+=("$i")
  done
  tag COMPL "$id" "$(( ${#line} - ${#w} ))" "${out[@]}"
}
# The shell's environment after each command, for the Terminal: a full-screen
# program typed here runs there, and sources this first, so it gets the same
# modules, PATH and variables - `ml R/4.5` then `R` started R 4.6 in the
# Terminal's own shell (operator, 2026-11-15). Lmod keeps its state in the
# environment, so `ml` there carries on from it. Read-only variables and the
# ones this driver sets for line mode (TERM=dumb, the pagers) stay out. Only
# for this user (it may hold tokens), written whole then moved in. ~9 ms.
__cove_envfile="${COVE_HELPERS:-$HOME/.cache}/shell-env.sh"
save_env() {
  ( umask 077
    declare -px | grep -vE '^declare -[a-zA-Z]*r[a-zA-Z]* |^declare -x (TERM|PAGER|GIT_PAGER|MANPAGER|PWD|OLDPWD|SHLVL|_|COLUMNS|LINES|HISTFILE|S)=' > "$__cove_envfile.part" &&
      mv -f "$__cove_envfile.part" "$__cove_envfile" ) 2>/dev/null
}
# The R the shell has loaded: when a command changes it (`ml R/4.5`), say
# which R module it is, and the R console follows (kernel.rs follow_shell_r).
__cove_r=${EBROOTR:-}
r_loaded() {
  [ "${EBROOTR:-}" = "$__cove_r" ] && return
  __cove_r=${EBROOTR:-}
  [ -n "$__cove_r" ] || return
  local m
  for m in ${LOADEDMODULES//:/ }; do
    case $m in R/*) tag RLOADED "$m"; return ;; esac
  done
}
finish() { tag CWD "$PWD"; save_env && tag ENVFILE "$__cove_envfile"; r_loaded; tag BANNER "$(label)"; tag DONE; }
# The one line a start says (ml av lists the stack, ml <name> loads it).
printf 'bash %s\n' "${BASH_VERSION%%(*}"
finish
while IFS= read -r h <&3; do
  case $h in '' | *[!0-9]*) h=0 ;; esac
  code=
  for ((i = 0; i < h; i++)); do IFS= read -r l <&3; code+=$l$'\n'; done
  run "$code" 2>&1
  rc=$?
  [ "$rc" -eq 0 ] || tag ERR "exit status $rc"
  finish
done
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn julia_strings_escape_quotes_dollars_and_newlines() {
        assert_eq!(jl_string(r#"say "hi" $x\n"#), r#""say \"hi\" \$x\\n""#);
        assert_eq!(jl_string("a\tb\nc"), r#""a\tb\nc""#);
    }

    #[test]
    fn a_table_arrives_whole_and_rows_keep_the_header_width() {
        let mut s = State { pending: 1, ..Default::default() };
        s.take(&format!("{SENT}TABLE\tdf\t3\t2"));
        s.take(&format!("{SENT}TCOLS\tx\ty"));
        s.take(&format!("{SENT}TTYPES\tnumeric\tcharacter"));
        s.take(&format!("{SENT}TROW\t1\ta"));
        s.take(&format!("{SENT}TROW\t2"));
        assert!(s.table.is_none(), "not shown before TEND");
        s.take(&format!("{SENT}TEND"));
        let t = s.table.clone().unwrap();
        assert_eq!((t.name.as_str(), t.nrow, t.ncol), ("df", 3, 2));
        assert_eq!(t.cols, ["x", "y"]);
        assert_eq!(t.rows, [vec!["1".to_owned(), "a".into()], vec!["2".into(), String::new()]]);
        assert_eq!(s.tables, 1);
        assert!(s.lines.is_empty(), "a table is not transcript");
    }

    #[test]
    fn python_reaches_r_as_one_string_literal() {
        assert_eq!(r_string("print(\"a\\nb\")\nx = 'q'\t#"), r#""print(\"a\\nb\")\nx = 'q'\t#""#);
        assert_eq!(r_string("\u{1}é"), "\"\\x01é\"");
        assert!(!r_string("a\nb\r\nc").contains(['\n', '\r']), "one request line");
    }

    #[test]
    fn python_runs_in_the_r_session() {
        assert_eq!(Lang::Python.session(), Lang::R);
        assert_eq!(Lang::Shell.session(), Lang::Shell);
    }

    #[test]
    fn the_viewer_takes_names_not_code() {
        assert!(view(Lang::R, "x; system('id')").is_err());
        assert!(view(Lang::Python, "__import__('os')").is_err());
        assert!(view(Lang::Shell, "x").is_err());
        assert!(view(Lang::R, "py$os.system('id')").is_err());
    }

    #[test]
    fn the_shell_driver_runs_requests_and_keeps_its_directory() {
        let d = std::env::temp_dir().join(format!("cove-sh-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let drv = d.join("driver.sh");
        std::fs::write(&drv, DRIVER_SH).unwrap();
        let out = Command::new("bash")
            .arg(&drv)
            .env("COVE_USE_SH", "")
            .env("COVE_CWD", &d)
            .env("COVE_HELPERS", &d)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .and_then(|mut c| {
                c.stdin.take().unwrap().write_all(b"2\nmkdir -p sub && cd sub\necho here\n1\nfalse\n1\nexport COVE_SEEN=from-the-console\n1\nexport EBROOTR=/s/R/4.5.3 LOADEDMODULES=GCCcore/13.3.0:R/4.5.3-foss-2024a:env/x\n")?;
                c.wait_with_output()
            })
            .unwrap();
        let mut st = State::default();
        for l in String::from_utf8_lossy(&out.stdout).lines() {
            st.take(l);
        }
        let text: Vec<&str> = st.lines.iter().map(|l| l.text.as_str()).collect();
        assert!(text.contains(&"here"), "{text:?}");
        assert!(st.lines.iter().any(|l| l.kind == LineKind::Error && l.text == "exit status 1"), "{text:?}");
        assert!(st.cwd.ends_with("/sub"), "cd persists across requests: {}", st.cwd);
        // The Terminal sources its environment and gets what it set, and the
        // line mode's TERM=dumb stays behind.
        assert_eq!(st.env_file, d.join("shell-env.sh").display().to_string());
        // Another R loaded in the shell is said, for the R console to follow.
        assert_eq!(st.r_loaded.as_deref(), Some("R/4.5.3-foss-2024a"));
        assert_eq!(with_r("env/r4.6_py3.13-release R/4.6.1-foss-2024a", "R/4.5.3-foss-2024a"), "env/r4.6_py3.13-release R/4.5.3-foss-2024a");
        assert_eq!(with_r("env/r4.6_py3.13-release", "R/4.5.3-foss-2024a"), "env/r4.6_py3.13-release R/4.5.3-foss-2024a");
        let got = Command::new("bash").args(["--noprofile", "--norc", "-c", &format!(". {} && printf '%s|%s' \"$COVE_SEEN\" \"$TERM\"", st.env_file)]).env("TERM", "xterm-256color").output().unwrap();
        assert_eq!(String::from_utf8_lossy(&got.stdout), "from-the-console|xterm-256color", "{}", String::from_utf8_lossy(&got.stderr));
        assert!(st.banner.starts_with("bash "), "{}", st.banner);
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn tagged_lines_split_from_output() {
        let mut s = State { pending: 1, ..Default::default() };
        s.take("hello");
        s.take(&format!("partial{SENT}ERR\tError: boom"));
        s.take(&format!("{SENT}ENVBEGIN"));
        s.take(&format!("{SENT}ENV\tx\tnumeric\tnum [1:3] 1 2 3"));
        s.take(&format!("{SENT}PLOT\t/tmp/p.png"));
        s.take(&format!("{SENT}DONE"));
        assert_eq!(s.lines.len(), 3);
        assert_eq!(s.lines[1], Line { kind: LineKind::Output, text: "partial".into() });
        assert_eq!(s.lines[2].kind, LineKind::Error);
        assert_eq!(s.env, vec![EnvRow { name: "x".into(), class: "numeric".into(), value: "num [1:3] 1 2 3".into() }]);
        assert_eq!(s.plots.len(), 1);
        assert_eq!(s.pending, 0);
    }

    #[test]
    fn module_names_cannot_reach_the_shell() {
        assert!(valid_module("env/r4.6_py3.13-release"));
        assert!(valid_module("R/4.6.1-foss-2024a"));
        assert!(valid_module("RStudio-Server/2026.04.0+526-foss-2024a-R-4.6.1"));
        assert!(!valid_module("R; rm -rf ~"));
        assert!(!valid_module("$(id)"));
        assert!(!valid_module("--force"));
        assert!(!valid_module(""));
        assert!(module_prefix("x; id").is_err());
        assert_eq!(module_prefix("").unwrap(), "");
        // a live list is checked name by name
        assert!(module_prefix("env/r4.6_py3.13-release jq $(id)").is_err());
        assert!(module_prefix("jq --force").is_err());
        assert_eq!(module_prefix("  ").unwrap(), "");
    }

    #[test]
    fn live_module_tags_are_kept_for_the_reader() {
        let mut st = State::default();
        st.take(&format!("{SENT}MODS\tenv/r4.6_py3.13-release jq"));
        st.take(&format!("{SENT}RESTART\tenv/r4.6_py3.13-release R/4.5.2-foss-2024a"));
        st.take(&format!("{SENT}BANNER\tR version 4.6.1 · env/r4.6_py3.13-release +1"));
        assert_eq!(st.mods.as_deref(), Some("env/r4.6_py3.13-release jq"));
        assert_eq!(st.restart_into.as_deref(), Some("env/r4.6_py3.13-release R/4.5.2-foss-2024a"));
        assert!(st.banner.ends_with("+1"));
        assert!(st.lines.is_empty(), "tags are not transcript");
    }

    /// Tab completion against a live R (`cargo test completion_live --
    /// --ignored`), after the kind of lines a person runs (operator,
    /// 2026-10-25: "autocomple in the Consoles seems to have stopped").
    #[test]
    #[ignore]
    fn completion_live() {
        use super::*;
        let wait = |what: &str, f: &dyn Fn(&KernelView) -> bool| {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(90);
            loop {
                let v = poll(Lang::R, 0);
                if f(&v) {
                    return v;
                }
                assert!(std::time::Instant::now() < until, "waited for {what}");
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        };
        eval(Lang::R, "").unwrap();
        wait("R to start", &|v| v.alive && !v.busy && !v.banner.is_empty());
        for line in ["ls()", "library(stats)", "mtcars"] {
            eval(Lang::R, line).unwrap();
            wait(line, &|v| !v.busy);
        }
        let c = complete(Lang::R, "mtc", 3).expect("an answer");
        assert!(c.items.iter().any(|i| i == "mtcars"), "{:?}", c.items);
        let c = complete(Lang::R, "x <- libr", 9).expect("an answer");
        assert!(c.items.iter().any(|i| i.starts_with("library")), "{:?}", c.items);
        // And again: a completion must leave the session ready for the next.
        let c = complete(Lang::R, "mtc", 3).expect("a second answer");
        assert!(!c.items.is_empty());
    }

    /// The live R kernel (`cargo test debugger_live -- --ignored`): browser()
    /// stops with R's prompt, steps, shows the frame, continues; readline()
    /// takes its answer from the console (operator, 2026-10-19).
    #[test]
    #[ignore]
    fn debugger_live() {
        use super::*;
        let wait = |what: &str, f: &dyn Fn(&KernelView) -> bool| {
            let until = std::time::Instant::now() + std::time::Duration::from_secs(90);
            loop {
                let v = poll(Lang::R, 0);
                if f(&v) {
                    return v;
                }
                assert!(std::time::Instant::now() < until, "waited for {what}: {:?}", v.lines.iter().rev().take(8).collect::<Vec<_>>());
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
        };
        eval(Lang::R, "").unwrap();
        wait("R to start", &|v| v.alive && !v.busy && !v.banner.is_empty());
        eval(Lang::R, "f <- function() { x <- 1; browser(); y <- x + 1; y * 10 }").unwrap();
        wait("the definition", &|v| !v.busy);
        eval(Lang::R, "f()").unwrap();
        let v = wait("Browse[1]>", &|v| v.debug == "R");
        assert!(v.prompt.starts_with("Browse["), "{}", v.prompt);
        let v = wait("the frame", &|v| v.env.iter().any(|e| e.name == "x"));
        assert!(!v.env.iter().any(|e| e.name == "f"), "the frame, not the globals");
        // Typed in the console: no file, but the function's own source and the browser() line.
        let v = wait("where it stopped", &|v| v.debug_line > 0 && !v.debug_src.is_empty());
        assert!(v.debug_file.is_empty());
        assert!(v.debug_src[v.debug_line - v.debug_from].contains("browser()"), "{:?} at {}", v.debug_src, v.debug_line);
        eval(Lang::R, "n").unwrap();
        wait("the next stop", &|v| v.debug == "R" && v.lines.iter().any(|l| l.text.contains("y <- x + 1")));
        eval(Lang::R, "c").unwrap();
        let v = wait("f() to finish", &|v| !v.busy);
        assert!(v.lines.iter().any(|l| l.text.contains("[1] 20")), "{:?}", v.lines.iter().rev().take(6).collect::<Vec<_>>());
        assert!(v.env.iter().any(|e| e.name == "f"), "the globals again");
        eval(Lang::R, "who <- readline('name? ')").unwrap();
        wait("readline's prompt", &|v| v.prompt.contains("name?") && v.debug.is_empty());
        eval(Lang::R, "fred").unwrap();
        wait("the answer", &|v| !v.busy);
        eval(Lang::R, "cat(who, '\\n')").unwrap();
        let v = wait("it printed", &|v| !v.busy && v.lines.iter().any(|l| l.text.trim() == "fred"));
        assert!(v.prompt.is_empty());
        // From a file in the editor: R names the file, and the line moves with each step.
        let dir = cache_dir().join("debug-test");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("steps.R");
        let src = "# a file\nh <- function(n) {\n  browser()\n  m <- n * 3\n  m + 1\n}\n";
        std::fs::write(&file, src).unwrap();
        let path = file.display().to_string();
        eval_at(Lang::R, src, Some(&(path.clone(), 1))).unwrap();
        wait("the file's function", &|v| !v.busy);
        eval(Lang::R, "h(2)").unwrap();
        let v = wait("the stop in the file", &|v| v.debug == "R" && v.debug_line > 0 && !v.debug_file.is_empty());
        assert_eq!((v.debug_file.as_str(), v.debug_line), (path.as_str(), 3));
        eval(Lang::R, "n").unwrap();
        wait("line 4", &|v| v.debug == "R" && v.debug_line == 4 && v.debug_file == path);
        eval(Lang::R, "c").unwrap();
        let v = wait("h(2) to finish", &|v| !v.busy);
        assert!(v.lines.iter().any(|l| l.text.contains("[1] 7")));
        assert_eq!(v.debug_line, 0, "no stop once it finished");
        // Python's debugger, inside the same session through reticulate.
        eval(Lang::Python, "def g():\n    a = 41\n    breakpoint()\n    return a + 1\nprint('got', g())").unwrap();
        wait("(Pdb)", &|v| v.debug == "Python");
        eval(Lang::Python, "p a").unwrap();
        wait("pdb printed a", &|v| v.debug == "Python" && v.lines.iter().any(|l| l.text.trim_end().ends_with("41")));
        eval(Lang::Python, "c").unwrap();
        wait("g() to return", &|v| !v.busy && v.lines.iter().any(|l| l.text.contains("got 42")));
        shutdown_all();
    }
}

