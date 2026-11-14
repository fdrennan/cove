//! The IDE's panes, in RStudio's places (PaneConfig.createDefault()). app.rs
//! lays out the quadrants and the tab strips of the two tab sets; each pane
//! here draws its own toolbar row and content:
//!
//!   source        (top-left)      tabs of .R / .py / .sh files and .qmd / .Rmd
//!                                 documents; Ctrl+Enter runs the line or
//!                                 selection, Ctrl+Shift+Enter the file - or,
//!                                 in a document, the chunk, in its language.
//!                                 View() opens a data frame here too.
//!   console       (bottom-left)   R, Python and the Prefix shell, one tab each
//!   environment · history         (top-right)
//!   app · plots · software · viewer   (right; software.rs)
//!
//! R and Python are equals from the start: both kernels start with the app,
//! and every pane follows the language of the console in front (the shell
//! has no environment: the panes stay on the last of R or Python). Everything
//! goes through `Backend`, so the same panes run in a browser against `serve`.
//! The editor is drawn in Inlet's "ndexr Dark" editor theme.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, text::LayoutJob, Color32, FontId, Key, RichText, TextFormat};

use crate::api::{self, is_document, DomainInfo, FileText, JobView, KernelView, Lang, LineKind, Listing, Package, PlotData, Query, TableView};
use crate::backend::Backend;
use crate::theme;

type Slot<T> = Arc<Mutex<Option<T>>>;

/// How many recent projects the ☰ menu keeps.
const RECENT_MAX: usize = 8;

/// The projects to offer as recent: the ones opened here, newest first,
/// then - so the list is never empty on a new browser - the rest of the
/// person's domains by when their files last changed (operator, 2026-10-26:
/// "it gives no known domain we can go to"). At most RECENT_MAX.
/// Every project in the order ← and → step through them in the project
/// menu: the opened ones in the order opened, then the rest, newest
/// changed first - recent_projects without its limit.
fn project_ring(opened: &[String], all: &[DomainInfo]) -> Vec<DomainInfo> {
    let mut out: Vec<DomainInfo> = opened.iter().filter_map(|h| all.iter().find(|d| d.host == *h).cloned()).collect();
    out.dedup_by(|a, b| a.host == b.host);
    let mut rest: Vec<&DomainInfo> = all.iter().filter(|d| !out.iter().any(|o| o.host == d.host)).collect();
    rest.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.host.cmp(&b.host)));
    out.extend(rest.into_iter().cloned());
    out
}

/// One project in the project menu: its framework's mark in a fixed column,
/// the host, then its title in the quieter colour, cut to the menu's width
/// (the full name is the hover). Every row the same height and width, so the
/// list reads as a column rather than a ragged run of buttons.
fn project_row(ui: &mut egui::Ui, d: &DomainInfo, on: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 30.0), egui::Sense::click());
    let p = ui.painter().clone();
    if on {
        p.rect_filled(rect, 6.0, theme::primary().gamma_multiply(0.16));
    } else if resp.hovered() {
        p.rect_filled(rect, 6.0, theme::surface_hi());
    }
    let mark = egui::Rect::from_center_size(egui::pos2(rect.left() + 16.0, rect.center().y), egui::vec2(20.0, 20.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(mark), |ui| {
        if crate::frameworks::icon(ui, &d.framework, 20.0).is_none() {
            ui.label(RichText::new("⌂").size(14.0));
        }
    });
    let x = rect.left() + 34.0;
    let mut job = egui::text::LayoutJob::default();
    job.append(&d.host, 0.0, egui::TextFormat { font_id: egui::FontId::monospace(12.5), color: theme::text_strong(), ..Default::default() });
    if !d.title.is_empty() {
        job.append(&format!("  {}", d.title), 0.0, egui::TextFormat { font_id: egui::FontId::proportional(12.5), color: theme::text(), ..Default::default() });
    }
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - x - 8.0).max(40.0));
    let galley = p.layout_job(job);
    p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, theme::text());
    let full = if d.title.is_empty() { d.host.clone() } else { format!("{} · {}", d.host, d.title) };
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(if on { format!("{full} - open now") } else { full })
}

/// What a click in the tab strip asks for (Ide::tab_act).
enum TabAct {
    Switch(String),
    New,
    Reopen,
    Reload,
    Outside(String),
    Copy(String),
    Pin(String),
    Live(String),
    Move(String, bool),
    Close(Vec<String>),
}

struct TabResp {
    resp: egui::Response,
    close: bool,
}

/// One project tab: its framework's mark, its short name (only the mark when
/// pinned), ⏸ when its live reload is paused, and × to close (on the tab in
/// front, or under the pointer).
#[allow(clippy::too_many_arguments)]
fn project_tab(ui: &mut egui::Ui, host: &str, info: Option<&DomainInfo>, on: bool, pinned: bool, paused: bool, w: f32, small: bool) -> TabResp {
    let h = if small { 30.0 } else { 26.0 };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let hovered = resp.hovered();
    let fill = if on { theme::surface() } else if hovered { theme::surface().gamma_multiply(0.6) } else { egui::Color32::TRANSPARENT };
    let p = ui.painter_at(rect);
    p.rect_filled(rect, egui::CornerRadius { nw: 7, ne: 7, sw: 0, se: 0 }, fill);
    if on {
        p.hline(rect.x_range(), rect.top() + 1.0, egui::Stroke::new(2.0, theme::primary()));
    }
    let mut x = rect.left() + 8.0;
    let mark = info.map_or("", |d| d.framework.as_str());
    let icon_rect = egui::Rect::from_center_size(egui::pos2(x + 8.0, rect.center().y), egui::vec2(16.0, 16.0));
    let marked = crate::frameworks::mark(mark).is_some() && {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(icon_rect));
        crate::frameworks::icon(&mut child, mark, 16.0).is_some()
    };
    if !marked {
        let first = crate::tabs::Tabs::short(host).chars().next().unwrap_or('?').to_ascii_uppercase();
        p.text(icon_rect.center(), egui::Align2::CENTER_CENTER, first, egui::FontId::proportional(12.0), theme::muted());
    }
    x += 22.0;
    let show_x = !pinned && (on || hovered);
    let x_rect = egui::Rect::from_center_size(egui::pos2(rect.right() - 13.0, rect.center().y), egui::vec2(18.0, 18.0));
    if !pinned {
        let right = if show_x { x_rect.left() - 4.0 } else { rect.right() - 6.0 };
        let name = format!("{}{}", if paused { "⏸ " } else { "" }, crate::tabs::Tabs::short(host));
        let mut job = egui::text::LayoutJob::single_section(name, egui::TextFormat { font_id: egui::FontId::proportional(12.5), color: if on { theme::text_strong() } else { theme::text() }, ..Default::default() });
        job.wrap = egui::text::TextWrapping::truncate_at_width((right - x).max(10.0));
        let galley = p.layout_job(job);
        p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, theme::text());
    }
    let mut close = false;
    if show_x {
        let xr = ui.interact(x_rect, resp.id.with("close"), egui::Sense::click());
        if xr.hovered() {
            p.rect_filled(x_rect, 4.0, theme::surface_hi());
        }
        p.text(x_rect.center(), egui::Align2::CENTER_CENTER, "×", egui::FontId::proportional(14.0), if xr.hovered() { theme::text_strong() } else { theme::muted() });
        close = xr.on_hover_text("close tab").clicked();
    }
    TabResp { resp, close }
}

/// What the project menu's field finds for `q`: the name itself first, then
/// names that start with it, then names and titles that hold it, each in ring
/// order. Typed whole, the project is the first row and Enter opens it: a
/// typed "ndexr.io" was told "already yours - open it below" over a list that
/// did not show it (operator's screenshot, 2026-11-14).
fn find_projects(q: &str, ring: &[DomainInfo]) -> Vec<DomainInfo> {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut starts: Vec<&DomainInfo> = ring.iter().filter(|d| d.host.to_lowercase().starts_with(&q)).collect();
    starts.sort_by_key(|d| d.host.to_lowercase() != q);
    let holds = ring.iter().filter(|d| !d.host.to_lowercase().starts_with(&q) && (d.host.to_lowercase().contains(&q) || d.title.to_lowercase().contains(&q)));
    starts.into_iter().chain(holds).take(12).cloned().collect()
}

fn recent_projects(opened: &[String], all: &[DomainInfo]) -> Vec<DomainInfo> {
    let mut out: Vec<DomainInfo> = opened.iter().filter_map(|h| all.iter().find(|d| d.host == *h).cloned()).take(RECENT_MAX).collect();
    let mut rest: Vec<&DomainInfo> = all.iter().filter(|d| !out.iter().any(|o| o.host == d.host)).collect();
    rest.sort_by(|a, b| b.modified.cmp(&a.modified).then_with(|| a.host.cmp(&b.host)));
    out.extend(rest.into_iter().take(RECENT_MAX.saturating_sub(out.len())).cloned());
    out
}

/// A new project being brought live (operator, 2026-10-19: "those projects
/// need to start live ... routing and deployment of the app needs to go all
/// of the way through for the user and until its complete the attention of
/// the user needs to be held"): a window over the IDE from the moment it is
/// created until its address answers.
struct Launch {
    /// The person's first site (Ide `first_site`): no window, because they
    /// are saying what to build. App shows it coming up; a failure still opens
    /// the window, which says why.
    quiet: bool,
    host: String,
    rust: bool,
    started: f64,
    /// The provisioner (Query::PublishDomain): registry, DNS, certificate, vhost.
    publish: Slot<Result<String, String>>,
    /// A Rust app's deploy (Query::DeployApp / DeployJob): build, serve, route.
    deploy: Slot<Result<JobView, String>>,
    deploy_sent: bool,
    deploy_asked: f64,
    /// What the project is (Query::AppStatus, its app.env), asked once. Any
    /// other app on a port - Julia, Python, a Rust API - is started by its own
    /// ./up.sh as the person (Query::AppAction, followed through AppJob),
    /// waiting out the first build create_domain started, then routed
    /// (Query::AppRoute). Before 2026-10-26 those were treated as Shiny
    /// modules: published, never started, and the domain showed a Shiny page.
    app: Slot<Result<crate::api::AppStatus, String>>,
    app_asked: bool,
    up: Slot<Result<JobView, String>>,
    up_sent: bool,
    up_asked: f64,
    route: Slot<Result<String, String>>,
    route_sent: bool,
    route_asked: f64,
    /// What answers at https://<host>/ (Query::SiteGate), asked every few seconds.
    gate: Slot<String>,
    gate_asked: f64,
}

/// Archive or Delete of a project (operator, 2026-10-26: "need option to
/// delete or archive project, move out of ui and into hamburger menu"). Both
/// stop the app and run git.ndexr.io's Archive (history proven in S3, then the
/// folder removed); Delete then gives up the name (Query::ProjectRelease).
struct Removing {
    host: String,
    delete: bool,
    /// Delete asks for the name typed back.
    typed: String,
    stage: RemoveStage,
    /// The app's Down answered (or there was none to stop).
    stopped: Slot<bool>,
    release: Slot<Result<String, String>>,
}

#[derive(Clone, PartialEq)]
enum RemoveStage {
    Confirm,
    Stopping,
    Archiving,
    Releasing,
    Done(Result<String, String>),
}

/// The editor's find bar (Ctrl/Cmd+F, as RStudio's): what is looked for, how,
/// and which match is current.
#[derive(Default)]
struct FindBar {
    open: bool,
    query: String,
    case: bool,
    regex: bool,
    at: usize,
    /// Put the cursor in the query box next frame.
    focus: bool,
}

/// Find in files (Ctrl/Cmd+Shift+F): every matching line under the open
/// domain, each a click from its file at that line.
#[derive(Default)]
struct FindInFiles {
    open: bool,
    query: String,
    case: bool,
    regex: bool,
    focus: bool,
    result: Slot<Result<api::GrepView, String>>,
    busy: Arc<AtomicBool>,
}

/// Code colours: the palette's (theme.rs `PALETTES`) - ndexr Regolith,
/// ndexr Light, or whichever look the person picked.
mod ace {
    use crate::theme::{is_light, pal, rgb};
    use eframe::egui::Color32;
    /// Code on a ~90% ground on the dark looks, the backdrop faint behind it;
    /// solid on the light ones.
    pub fn bg() -> Color32 {
        let c = rgb(pal().code);
        Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), if is_light() { 0xff } else { 0xe6 })
    }
    pub fn text() -> Color32 {
        rgb(pal().text)
    }
    pub fn keyword() -> Color32 {
        rgb(pal().keyword)
    }
    pub fn operator() -> Color32 {
        rgb(pal().operator)
    }
    pub fn number() -> Color32 {
        rgb(pal().number)
    }
    pub fn language() -> Color32 {
        rgb(pal().number)
    }
    pub fn function() -> Color32 {
        rgb(pal().function)
    }
    pub fn string() -> Color32 {
        rgb(pal().string)
    }
    pub fn comment() -> Color32 {
        rgb(pal().comment)
    }
    pub fn variable() -> Color32 {
        rgb(pal().text)
    }
    pub fn error() -> Color32 {
        rgb(pal().alt)
    }
    pub fn warn() -> Color32 {
        rgb(pal().warn)
    }
}

const R_KEYWORDS: &[&str] = &["if", "else", "repeat", "while", "function", "for", "in", "next", "break"];
const R_CONSTANTS: &[&str] = &["TRUE", "FALSE", "NULL", "Inf", "NaN", "NA", "NA_integer_", "NA_real_", "NA_character_", "T", "F"];
const PY_KEYWORDS: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif", "else", "except", "finally", "for",
    "from", "global", "if", "import", "in", "is", "lambda", "nonlocal", "not", "or", "pass", "raise", "return", "try", "while",
    "with", "yield", "match", "case",
];
const PY_CONSTANTS: &[&str] = &["True", "False", "None", "self"];
const SH_KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac", "in", "function", "select", "return",
    "local", "export", "unset", "readonly", "declare", "source", "time",
];
const SH_CONSTANTS: &[&str] = &["true", "false", "ml", "module"];
const JL_KEYWORDS: &[&str] = &[
    "function", "end", "if", "elseif", "else", "for", "while", "in", "return", "begin", "let", "do", "try", "catch", "finally",
    "module", "using", "import", "export", "struct", "mutable", "abstract", "primitive", "type", "const", "local", "global",
    "macro", "quote", "break", "continue", "where",
];
const JL_CONSTANTS: &[&str] = &["true", "false", "nothing", "missing", "Inf", "NaN", "pi"];

const EDITOR_FONT: f32 = 13.0;

/// The source pane colours code by tokens; anything it does not know stays plain.
/// A file the consoles do not run, by its name or first line (syntax.rs):
/// coloured, completed and typed into by its kind.
fn file_kind(doc: &Doc) -> Option<crate::syntax::Kind> {
    if doc.lang.is_some() || doc.document {
        return None;
    }
    crate::syntax::of_path(&doc.path).or_else(|| crate::syntax::of_text(&doc.text))
}

/// The colours of a file syntax.rs knows.
pub fn highlight_kind(src: &str, kind: crate::syntax::Kind) -> LayoutJob {
    use crate::syntax::Tok;
    let mut job = LayoutJob::default();
    let font = FontId::monospace(EDITOR_FONT);
    for (a, b, t) in crate::syntax::tokens(src, kind) {
        let (color, italics) = match t {
            Tok::Text => (ace::text(), false),
            Tok::Keyword | Tok::Tag | Tok::Heading => (ace::keyword(), false),
            Tok::Constant | Tok::Variable => (ace::language(), false),
            Tok::Number => (ace::number(), false),
            Tok::String => (ace::string(), false),
            Tok::Comment => (ace::comment(), true),
            Tok::Function | Tok::Attr | Tok::Property => (ace::function(), false),
            Tok::Operator => (ace::operator(), false),
        };
        job.append(&src[a..b], 0.0, TextFormat { font_id: font.clone(), color, italics, ..Default::default() });
    }
    job
}

pub fn highlight(src: &str, lang: Option<Lang>) -> LayoutJob {
    let mut job = LayoutJob::default();
    highlight_into(&mut job, src, lang);
    job
}

/// A document: prose plain (headings stand out), fences quiet, and each
/// chunk's code coloured as its own language.
pub fn highlight_doc(src: &str) -> LayoutJob {
    let mut job = LayoutJob::default();
    let font = FontId::monospace(EDITOR_FONT);
    let fmt = |color: Color32, italics: bool| TextFormat { font_id: font.clone(), color, italics, ..Default::default() };
    let chunks = chunks(src);
    let mut line = 0;
    let mut at = 0;
    let mut pieces = src.split_inclusive('\n').peekable();
    while pieces.peek().is_some() {
        match chunks.iter().find(|c| c.header == line) {
            Some(c) => {
                // header, body, fence - the body as one run of its language
                let mut take = |n: usize| -> String { (0..n).filter_map(|_| pieces.next()).collect() };
                let head = take(1);
                job.append(&head, 0.0, fmt(ace::comment(), false));
                let body = take(c.end - c.body);
                highlight_into(&mut job, &body, c.lang);
                let fence = take(if c.closed { 1 } else { 0 });
                job.append(&fence, 0.0, fmt(ace::comment(), false));
                at += head.len() + body.len() + fence.len();
                line = c.end + usize::from(c.closed);
            }
            None => {
                let l = pieces.next().unwrap_or("");
                let t = l.trim_start();
                let color = if t.starts_with('#') { ace::function() } else if t.starts_with("---") || t.starts_with("```") { ace::comment() } else { ace::text() };
                job.append(l, 0.0, fmt(color, false));
                at += l.len();
                line += 1;
            }
        }
    }
    debug_assert_eq!(at, src.len());
    job
}

fn highlight_into(job: &mut LayoutJob, src: &str, lang: Option<Lang>) {
    let font = FontId::monospace(EDITOR_FONT);
    let fmt = |color: Color32, italics: bool| TextFormat { font_id: font.clone(), color, italics, ..Default::default() };
    let Some(lang) = lang else {
        job.append(src, 0.0, fmt(ace::text(), false));
        return;
    };
    let b = src.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let start = i;
        let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_' || (lang == Lang::R && c == b'.');
        let (end, color, italics) = if c == b'#' && (lang != Lang::Shell || i == 0 || b[i - 1].is_ascii_whitespace()) {
            (src[i..].find('\n').map_or(b.len(), |n| i + n), ace::comment(), true)
        } else if lang == Lang::Shell && c == b'$' && b.get(i + 1).is_some_and(|n| n.is_ascii_alphanumeric() || b"_{({@#?*!".contains(n)) {
            let mut j = i + 1;
            if b[j] == b'{' {
                while j < b.len() && b[j] != b'}' && b[j] != b'\n' {
                    j += 1;
                }
                j = (j + 1).min(b.len());
            } else {
                j += 1;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
            }
            (j, ace::language(), false)
        } else if c == b'"' || c == b'\'' || (lang == Lang::R && c == b'`') {
            let triple = lang == Lang::Python && b.get(i + 1) == Some(&c) && b.get(i + 2) == Some(&c);
            let mut j = i + if triple { 3 } else { 1 };
            loop {
                match b.get(j) {
                    None => break,
                    Some(b'\\') => j += 2,
                    Some(&q) if q == c && (!triple || (b.get(j + 1) == Some(&c) && b.get(j + 2) == Some(&c))) => {
                        j += if triple { 3 } else { 1 };
                        break;
                    }
                    Some(b'\n') if !triple && lang == Lang::Python => break,
                    Some(_) => j += 1,
                }
            }
            (j.min(b.len()), if c == b'`' { ace::variable() } else { ace::string() }, false)
        } else if c.is_ascii_digit() || (c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit)) {
            let mut j = i + 1;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'.' || b[j] == b'_') {
                j += 1;
            }
            (j, ace::number(), false)
        } else if c.is_ascii_alphabetic() || c == b'_' || (lang == Lang::R && c == b'.') {
            let mut j = i + 1;
            while j < b.len() && ident(b[j]) {
                j += 1;
            }
            let w = &src[i..j];
            let (kw, konst) = match lang {
                Lang::R => (R_KEYWORDS, R_CONSTANTS),
                Lang::Python => (PY_KEYWORDS, PY_CONSTANTS),
                Lang::Shell => (SH_KEYWORDS, SH_CONSTANTS),
                Lang::Julia => (JL_KEYWORDS, JL_CONSTANTS),
            };
            let called = src[j..].trim_start_matches([' ', '\t']).starts_with('(');
            let color = if kw.contains(&w) {
                ace::keyword()
            } else if konst.contains(&w) {
                ace::language()
            } else if called {
                ace::function()
            } else {
                ace::text()
            };
            (j, color, false)
        } else if b"<-=+*/^%&|!~$@:>?".contains(&c) {
            (i + 1, ace::operator(), false)
        } else {
            (i + src[i..].chars().next().map_or(1, char::len_utf8), ace::text(), false)
        };
        job.append(&src[start..end], 0.0, fmt(color, italics));
        i = end;
    }
}

/// One executable chunk of a document: ```{r}, ```{python label}, ```{bash}.
/// Lines are 0-based: the header, the body's first line, and `end` (the
/// closing fence, or the line count when it never closes).
#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub engine: String,
    pub lang: Option<Lang>,
    pub header: usize,
    pub body: usize,
    pub end: usize,
    pub closed: bool,
    /// `eval=FALSE` in the header, or `#| eval: false` in the body: Run All
    /// leaves it out, as render does.
    pub eval: bool,
}

/// Every executable chunk, in order. A plain ``` block (no braces) is code to
/// show, not to run, and is not a chunk.
pub fn chunks(text: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = Vec::new();
    let mut n = 0;
    while n < lines.len() {
        let t = lines[n].trim_start();
        let Some(spec) = t.strip_prefix("```{").and_then(|r| r.split('}').next()) else {
            // skip over a plain code block whole: a ```{r} written inside one
            // is an example, not a chunk
            if t.starts_with("```") {
                n += 1;
                while n < lines.len() && !lines[n].trim_start().starts_with("```") {
                    n += 1;
                }
            }
            n += 1;
            continue;
        };
        let engine = spec.split([' ', ',', '}']).next().unwrap_or("").trim().to_owned();
        let mut end = n + 1;
        while end < lines.len() && lines[end].trim() != "```" {
            end += 1;
        }
        let closed = end < lines.len();
        let squash = spec.replace(' ', "").to_ascii_lowercase();
        let off_in_body = lines[n + 1..end].iter().take_while(|l| l.trim_start().starts_with("#|")).any(|l| {
            let o = l.trim_start().trim_start_matches("#|").replace(' ', "").to_ascii_lowercase();
            o == "eval:false"
        });
        let eval = !(squash.contains("eval=false") || squash.contains("eval=f,") || squash.ends_with("eval=f") || off_in_body);
        out.push(Chunk { lang: Lang::of_engine(&engine), engine, header: n, body: n + 1, end, closed, eval });
        n = end + 1;
    }
    out
}

/// A chunk's code, by its lines.
fn chunk_code(text: &str, c: &Chunk) -> String {
    text.split('\n').skip(c.body).take(c.end - c.body).collect::<Vec<_>>().join("\n")
}

/// The 0-based line holding char index `cursor`.
fn line_of(text: &str, cursor: usize) -> usize {
    text.chars().take(cursor).filter(|&c| c == '\n').count()
}

/// The char index where line `n` starts.
fn line_start(text: &str, n: usize) -> usize {
    if n == 0 {
        return 0;
    }
    let mut seen = 0;
    for (i, c) in text.chars().enumerate() {
        if c == '\n' {
            seen += 1;
            if seen == n {
                return i + 1;
            }
        }
    }
    text.chars().count()
}

/// From the cursor's line, the code Ctrl+Enter runs: the line, extended while
/// brackets stay open (R and Python), and over an indented block after a
/// line ending in ':' (Python). Returns (code, char index of the next line).
pub fn statement_at(text: &str, cursor: usize, lang: Lang) -> (String, usize) {
    let lines: Vec<&str> = text.split('\n').collect();
    // Which line holds the cursor (char indices).
    let mut at = 0;
    let mut first = 0;
    for (n, l) in lines.iter().enumerate() {
        let len = l.chars().count();
        if cursor <= at + len {
            first = n;
            break;
        }
        at += len + 1;
        first = n + 1;
    }
    let first = first.min(lines.len().saturating_sub(1));
    let mut last = first;
    let mut depth = bracket_depth(lines[first]);
    while depth > 0 && last + 1 < lines.len() {
        last += 1;
        depth += bracket_depth(lines[last]);
    }
    if lang == Lang::Python && lines[last].trim_end().ends_with(':') {
        while last + 1 < lines.len() {
            let next = lines[last + 1];
            if next.trim().is_empty() || next.starts_with([' ', '\t']) {
                last += 1;
            } else {
                break;
            }
        }
    }
    let code = lines[first..=last].join("\n");
    let next: usize = lines[..=last].iter().map(|l| l.chars().count() + 1).sum();
    (code, next.min(text.chars().count()))
}

/// Opened minus closed brackets on a line, outside strings and comments.
fn bracket_depth(line: &str) -> i32 {
    let mut depth = 0;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for c in line.chars() {
        if let Some(q) = quote {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == q {
                quote = None;
            }
            continue;
        }
        match c {
            '#' => break,
            '"' | '\'' | '`' => quote = Some(c),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// Take a key press out of this frame's events before a widget sees it.
fn take_key(ui: &egui::Ui, key: Key, want: impl Fn(egui::Modifiers) -> bool) -> bool {
    ui.input_mut(|i| {
        let mut hit = false;
        i.events.retain(|e| match e {
            egui::Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && want(*modifiers) && !hit => {
                hit = true;
                false
            }
            _ => true,
        });
        hit
    })
}

fn ix(l: Lang) -> usize {
    match l {
        Lang::R => 0,
        Lang::Python => 1,
        Lang::Shell => 2,
        Lang::Julia => 3,
    }
}

/// Listen's list in words: each piece heard and what became of it, then
/// what it is doing now (ide.rs listen_status).
struct ListenStatus {
    heard: Vec<(String, String)>,
    doing: Option<String>,
}

fn draw_listen(ui: &mut egui::Ui, s: &ListenStatus, salt: &str) {
    egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
        egui::ScrollArea::vertical().id_salt(("listen-heard", salt)).max_height(140.0).stick_to_bottom(true).show(ui, |ui| {
            for (t, fate) in &s.heard {
                ui.label(RichText::new(t).size(12.5).color(theme::text()));
                let c = if fate.starts_with("sent") { theme::ok() } else { theme::text() };
                ui.label(RichText::new(format!("↳ {fate}")).size(12.0).color(c));
                ui.add_space(4.0);
            }
            if let Some(d) = &s.doing {
                ui.label(RichText::new(d).size(12.0).color(theme::primary()));
            }
        });
    });
}

/// Where Listen's "act as I talk" sends what it hears.
#[derive(Clone, Copy, PartialEq, Debug)]
enum ListenTo {
    /// The site's agent or Exec, whichever is chosen: the Agents tab, Go and
    /// the console share them.
    Agent,
    /// The Files agent, about the open file or the tree's folder.
    File,
}

pub struct Doc {
    pub path: String,
    pub text: String,
    saved: String,
    pub lang: Option<Lang>,
    /// A .qmd / .Rmd: run by chunk, each in its own language, and rendered.
    pub document: bool,
    /// A data viewer tab (View()): a table, not text.
    pub table: Option<(Lang, TableView)>,
    /// An image, PDF, video or sound: shown in place, never as text.
    pub preview: bool,
    filter: String,
    id: egui::Id,
    /// The file as it is on disk now, when it changed there while this tab
    /// had edits not saved: the strip over the editor asks which to keep.
    disk: Option<String>,
}

impl Doc {
    fn name(&self) -> String {
        self.path.rsplit('/').next().unwrap_or(&self.path).to_owned()
    }
    fn dirty(&self) -> bool {
        self.text != self.saved
    }
    fn untitled(&self) -> bool {
        !self.path.starts_with('/')
    }
    /// Lines the source pane can run: a script in its language, a document
    /// by its chunks.
    fn runnable(&self) -> bool {
        self.table.is_none() && !self.preview && (self.lang.is_some() || self.document)
    }
}

#[derive(Default)]
struct Console {
    view: Arc<Mutex<KernelView>>,
    input: String,
    /// This session's commands (console, source and panes alike): Up/Down
    /// and the History pane. Never written to disk (pimp lock).
    history: Vec<String>,
    hist: Option<usize>,
    inflight: Arc<AtomicBool>,
    asked: f64,
}

/// Where the Inputs page sends what is typed (operator, 2026-10-15: "Agents
/// comes first, then R, Py, and Shell, agents treated like the others"): the
/// domain's agent, or a language - R, Python (in R, through reticulate) or the
/// line-mode shell. Pushing right goes round: Agent, R, Py, $, Agent …
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    Agent,
    Code(Lang),
}

/// How the Agents tab lays out Site and Exec.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum AgentsLayout {
    #[default]
    One,
    SideBySide,
    Stacked,
}

/// The ring, left to right: the languages only (operator, 2026-10-18: "the
/// agent in the console is different from the agents - exec and should be
/// separate"). Claude is the Agents tab and Exec; here, with Agent ticked,
/// what is typed in any of them is an ask that writes code for it
/// (`Ide::ask_assist`).
const CHANNELS: [Channel; 4] = [Channel::Code(Lang::R), Channel::Code(Lang::Python), Channel::Code(Lang::Shell), Channel::Code(Lang::Julia)];

/// The consoles an environment brings (operator, 2026-10-26: "match the UI
/// with the project env which shows what consoles are displayed ... where envs
/// combine R and Py then we see R and Py in the console switcher but not
/// before then"). The shell always; R for an R set, Python for a Python set,
/// both for a combined env/r*_py*, Julia for Julia; Rust and Node work in the
/// shell. Read from the environment's name, never from PATH: the host has an R
/// of its own at /usr/bin/R that no environment brought. No environment at all
/// (a laptop's own programs) offers every console, as before.
pub fn env_consoles(module: &str) -> Vec<Lang> {
    // A layered list ("env/R/4.6.1-bioc3.23 GDAL/...") is its first module's;
    // a person's revision (cove-env/R-4.6.1-bioc3.23/2) is its base's.
    let first = module.split_whitespace().next().unwrap_or("");
    let base = first.strip_prefix("cove-env/").map(|r| r.split('/').next().unwrap_or("")).unwrap_or(first);
    let name = base.trim_start_matches("env/");
    let mut out = Vec::new();
    let combined = name.starts_with('r') && name.contains("_py");
    if first.is_empty() {
        out.extend([Lang::R, Lang::Python]);
    } else if combined {
        out.extend([Lang::R, Lang::Python]);
    } else if name.starts_with("R/") || name.starts_with("R-") {
        out.push(Lang::R);
    } else if name.starts_with("Python/") || name.starts_with("Python-") {
        out.push(Lang::Python);
    } else if name.starts_with("Julia/") || name.starts_with("Julia-") {
        out.push(Lang::Julia);
    }
    out.push(Lang::Shell);
    if first.is_empty() {
        out.push(Lang::Julia);
    }
    out
}

impl Channel {
    pub fn word(self) -> &'static str {
        match self {
            Channel::Agent => "Agent",
            Channel::Code(l) => l.word(),
        }
    }
    /// The switcher's label.
    fn pill(self) -> &'static str {
        match self {
            Channel::Agent => "Agent",
            Channel::Code(Lang::R) => "R",
            Channel::Code(Lang::Python) => "Py",
            Channel::Code(Lang::Shell) => "$",
            Channel::Code(Lang::Julia) => "Jl",
        }
    }
    fn prompt(self) -> &'static str {
        match self {
            Channel::Agent => "»",
            Channel::Code(l) => l.prompt(),
        }
    }
    /// Each channel's colour in its bands and on the switcher.
    fn color(self) -> Color32 {
        match self {
            Channel::Agent => theme::primary(),
            Channel::Code(Lang::R) => theme::net(),
            Channel::Code(Lang::Python) => ace::keyword(),
            Channel::Code(Lang::Shell) => theme::ok(),
            Channel::Code(Lang::Julia) => theme::warn(),
        }
    }
    /// Its slot in per-channel arrays (history).
    fn slot(self) -> usize {
        match self {
            Channel::Agent => 4,
            Channel::Code(l) => ix(l),
        }
    }
    /// The kernel whose transcript holds its lines: R's = 0 (Python's too),
    /// the shell's = 1, Julia's = 2.
    fn kernel(self) -> Option<usize> {
        match self {
            Channel::Agent => None,
            Channel::Code(Lang::R | Lang::Python) => Some(0),
            Channel::Code(Lang::Shell) => Some(1),
            Channel::Code(Lang::Julia) => Some(2),
        }
    }
    /// The next one round `ring` (the environment's consoles, in CHANNELS
    /// order), `step` places on (negative goes back).
    fn step(self, step: i32, ring: &[Channel]) -> Channel {
        if ring.is_empty() {
            return self;
        }
        let at = ring.iter().position(|&c| c == self).unwrap_or(0) as i32;
        ring[(at + step).rem_euclid(ring.len() as i32) as usize]
    }
}

/// One run in the Inputs page (operator, 2026-10-15: "comfortably shift
/// between language chunks and keep them consistent"): where it went, what
/// was sent, where its kernel's transcript stood then (an absolute line
/// number) and how many lines its echo takes. R and Python share the R
/// session, so a Python chunk is a stretch of R's transcript. `moved` is
/// shell code that went to the Terminal (vim, top, claude …); `agent` the
/// key of a turn sent to the agent; `quiet` the IDE's own (a domain's setwd),
/// left out of the recent strip.
struct Run {
    chan: Channel,
    code: String,
    from: usize,
    echo: usize,
    moved: bool,
    agent: Option<String>,
    quiet: bool,
}

impl Run {
    fn code(chan: Channel, code: &str, from: usize) -> Run {
        let echo = code.trim_end_matches(['\n', '\r', ' ', '\t']).split('\n').count();
        Run { chan, code: code.to_owned(), from, echo, moved: false, agent: None, quiet: false }
    }
}

/// An Inputs row: a band's head, a kernel's transcript line (kernel, position
/// in its `lines`) in a run's channel, or a line of the page's own.
enum Row {
    Head(Channel),
    Line(Channel, usize, usize),
    Note(Channel, LineKind, String),
}

/// The agent's side of a run, looked up by its key: what was asked, the reply
/// so far, finished.
type AgentText<'a> = &'a dyn Fn(&str) -> Option<(String, String, bool)>;

/// The Inputs page, as rows: R's start (its banner) and then every run in the
/// order it was sent, a head wherever the channel changes. Each code run owns
/// its kernel's lines from its echo to the next run's on that kernel, so
/// output that arrives late (a long R call while a shell chunk ran) stays in
/// the run that printed it; an agent run shows the prompt and the reply as it
/// streams. The shell's banner is the tab's tooltip, not transcript.
fn console_rows(runs: &[Run], views: [&KernelView; 3], agent: AgentText) -> Vec<Row> {
    // Where each run's lines begin in its kernel's `lines`: the first echo
    // line at or after where the transcript stood, past the one before's echo.
    let mut starts = vec![0usize; runs.len()];
    let mut seek = [0usize; 3];
    for (i, c) in runs.iter().enumerate() {
        let Some(k) = c.chan.kernel().filter(|_| !c.moved) else { continue };
        let v = views[k];
        let base = v.next.saturating_sub(v.lines.len());
        let from = c.from.saturating_sub(base).max(seek[k]).min(v.lines.len());
        let at = v.lines[from..].iter().position(|l| matches!(l.kind, LineKind::Input)).map_or(v.lines.len(), |p| from + p);
        starts[i] = at;
        seek[k] = (at + c.echo.max(1)).min(v.lines.len());
    }
    let in_kernel = |r: &Run, k: usize| !r.moved && r.chan.kernel() == Some(k);
    let mut rows = Vec::new();
    let first_r = runs.iter().zip(&starts).find(|(c, _)| in_kernel(c, 0)).map_or(views[0].lines.len(), |(_, s)| *s);
    let r = Channel::Code(Lang::R);
    if first_r > 0 {
        rows.push(Row::Head(r));
        rows.extend((0..first_r).map(|p| Row::Line(r, 0, p)));
    }
    let mut prev = (first_r > 0).then_some(r);
    for (i, c) in runs.iter().enumerate() {
        if prev != Some(c.chan) {
            rows.push(Row::Head(c.chan));
            prev = Some(c.chan);
        }
        if c.moved {
            for (j, l) in c.code.trim_end().lines().enumerate() {
                rows.push(Row::Note(c.chan, LineKind::Input, format!("{} {l}", if j == 0 { "$" } else { ">" })));
            }
            rows.push(Row::Note(c.chan, LineKind::System, "↗ a full-screen program: it runs in the Terminal tab".to_owned()));
            continue;
        }
        if let Some(key) = &c.agent {
            let (prompt, reply, done) = agent(key).unwrap_or_else(|| (c.code.clone(), String::new(), false));
            for (j, l) in prompt.trim_end().lines().enumerate() {
                rows.push(Row::Note(c.chan, LineKind::Input, format!("{} {l}", if j == 0 { "»" } else { " " })));
            }
            rows.extend(reply.trim_end().lines().map(|l| Row::Note(c.chan, LineKind::Output, l.to_owned())));
            if !done {
                rows.push(Row::Note(c.chan, LineKind::System, "working… (the whole turn is in the Agents tab)".to_owned()));
            }
            continue;
        }
        let Some(k) = c.chan.kernel() else { continue };
        let end = runs[i + 1..].iter().zip(&starts[i + 1..]).find(|(n, _)| in_kernel(n, k)).map_or(views[k].lines.len(), |(_, s)| *s);
        rows.extend((starts[i]..end.max(starts[i])).map(|p| Row::Line(c.chan, k, p)));
    }
    rows
}

/// The longest start every candidate shares: what Tab can type at once.
fn common_prefix(items: &[String]) -> String {
    let Some(first) = items.first() else { return String::new() };
    let mut n = first.chars().count();
    for it in &items[1..] {
        n = n.min(first.chars().zip(it.chars()).take_while(|(a, b)| a == b).count());
    }
    first.chars().take(n).collect()
}

/// Tab in the Inputs: the request out, and what it answered. `line_at` is
/// where the cursor's line starts in the input and `cursor` the cursor, both
/// in characters; `snapshot` the input when asked (an edit since drops it).
#[derive(Default)]
struct Completing {
    slot: Arc<Mutex<Option<Result<api::Completion, String>>>>,
    snapshot: String,
    line_at: usize,
    cursor: usize,
    /// The candidates on show, and the one picked.
    items: Vec<String>,
    start: usize,
    sel: usize,
}

/// Shell code that needs a terminal: a full-screen or interactive program
/// as any command's first word (operator, 2026-10-15: "if a gui terminal app
/// opens it needs to get pushed into opening in Terminal just like a shiny
/// app would go into viewer"). The Console's shell is line mode - no TTY,
/// stdin closed - where these hang, fail or draw garbage. Everything else
/// stays in the Console, where it is chunked with the R and Python around it.
fn wants_terminal(code: &str) -> bool {
    const TUI: &[&str] = &[
        "vi", "vim", "nvim", "view", "vimdiff", "nano", "pico", "micro", "hx", "helix", "kak", "emacs", "joe", "mcedit",
        "top", "htop", "btop", "atop", "nvtop", "glances", "iotop", "nmon", "watch",
        "less", "more", "most", "man", "info",
        "ranger", "nnn", "lf", "mc", "vifm", "yazi", "broot", "ncdu",
        "tmux", "screen", "zellij", "byobu", "mosh",
        "tig", "lazygit", "gitui", "k9s", "fzf", "dialog", "whiptail", "nmtui", "alsamixer", "cmatrix",
        "claude", "ipython", "radian", "julia", "ghci", "irb", "node", "sudo", "su", "passwd", "visudo",
    ];
    // Interactive only with nothing to run: `python3` is a REPL, `python3 x.py` is not.
    const REPL: &[&str] = &["R", "python", "python3", "bash", "sh", "zsh", "fish", "psql", "sqlite3", "mysql", "duckdb", "ssh", "ftp", "sftp"];
    code.lines()
        .flat_map(|l| l.split(['|', ';', '&', '(', ')']))
        .any(|cmd| {
            let mut words = cmd.split_whitespace().skip_while(|w| w.contains('=') && !w.starts_with('-') || matches!(*w, "exec" | "command" | "time" | "nohup" | "env" | "then" | "do" | "else" | "!"));
            let Some(first) = words.next() else { return false };
            if first.starts_with('#') {
                return false;
            }
            let prog = first.rsplit('/').next().unwrap_or(first);
            let rest: Vec<&str> = words.take_while(|w| !w.starts_with('#')).collect();
            match prog {
                // `claude -p "…"` prints and ends; `emacs --batch` draws nothing.
                "claude" => !rest.iter().any(|w| matches!(*w, "-p" | "--print" | "-v" | "--version" | "-h" | "--help" | "update" | "doctor" | "config" | "mcp")),
                "emacs" => !rest.iter().any(|w| w.starts_with("--batch") || *w == "-batch"),
                "git" => matches!(rest.first(), Some(&"commit")) && !rest.iter().any(|w| w.starts_with("-m") || w.starts_with("-F") || *w == "--no-edit" || w.starts_with("--message") || w.starts_with("--file"))
                    || rest.iter().any(|w| matches!(*w, "-i" | "--interactive" | "-p" | "--patch")) && matches!(rest.first(), Some(&("rebase" | "add" | "checkout" | "restore" | "reset" | "stash"))),
                "crontab" => rest.contains(&"-e"),
                // `ssh host` is a login, `ssh host cmd` runs one thing.
                "ssh" => rest.iter().filter(|w| !w.starts_with('-')).count() <= 1,
                p if REPL.contains(&p) => rest.iter().all(|w| w.starts_with('-') && !matches!(*w, "-c" | "-e" | "-f" | "-m" | "--command" | "--file")),
                p => TUI.contains(&p),
            }
        })
}

/// The bottom-right pane's tabs (RStudio's TabSet2).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Lower {
    /// The domain this session is in: pick or create it, see its site live,
    /// Push all, Archive (operator, 2026-10-13: "app, which just allows the
    /// user to have one app or one module open at a time").
    App,
    Plots,
    /// The Viewer tab (back 2026-10-15): renders, htmlwidgets, local servers.
    Viewer,
    /// The project's files and the files open in the editor, one tab (moved
    /// here from the left window, operator 2026-10-15: "files and data can go
    /// to the right after App, Software, Plots").
    Files,
    /// The person's SQLite database, or one opened from Files (db.rs).
    Data,
}

/// The Viewer: the address it shows (`Backend::viewer_url` makes it a
/// URL), back and forward, a reload count, the servers seen, and what it
/// already took from each console, the last render and the log toggle.
#[derive(Default)]
struct ViewerState {
    cur: String,
    back: Vec<String>,
    fwd: Vec<String>,
    seq: u64,
    servers: Vec<String>,
    seen: [u64; 4],
    rendered: String,
    log: bool,
}

/// A Viewer address as a person reads it: `localhost:4200/docs/` for a
/// server, the page's name for a served file.
fn server_label(addr: &str) -> String {
    if let Some(rest) = addr.strip_prefix("/port/") {
        return format!("localhost:{rest}");
    }
    if addr.starts_with("/site/") {
        let page = addr.rsplit('/').next().unwrap_or(addr);
        return if page.is_empty() { "index.html".to_owned() } else { page.to_owned() };
    }
    addr.to_owned()
}

pub struct Ide {
    backend: Backend,
    docs: Vec<Doc>,
    cur: usize,
    serial: u64,
    save_as: Option<String>,
    memo: HashMap<egui::Id, (u64, LayoutJob)>,
    consoles: [Console; 4],
    /// The console in front: R (Python inside it) or the shell.
    pub lang: Lang,
    /// Where the Inputs page's input goes: the agent, R, Python (in R,
    /// through reticulate) or the line-mode shell, switched round the ring by
    /// a push left or right.
    pub chan: Channel,
    /// Every run, in order, for the page's bands.
    chunks: Vec<Run>,
    /// Agent ticked in the Console: what is typed is an ask that writes code
    /// into the input; not ticked (the default), lines run as typed.
    /// the answer on its way, which language asked and what was typed, and
    /// the last word on it (an error, or which model wrote it).
    pub assist_on: bool,
    assist: Arc<Mutex<Option<Result<api::AssistReply, String>>>>,
    assisting: Option<(Lang, String)>,
    assist_note: Option<String>,
    /// Say it instead of typing it (operator, 2026-10-18: "add a audio input
    /// that will automatically use agents ... so that we can communicate our
    /// Python and shell commands and then have them placed into the console").
    /// What is heard is an ask for the channel in use, and the code
    /// lands in the input to read and run - never run unread.
    console_mic: crate::voice::Mic,
    /// What was sent on each channel (`Channel::slot`), for ↑ in the input,
    /// and where ↑ has got to.
    lang_hist: [Vec<String>; 5],
    lang_hist_at: Option<usize>,
    /// Tab completion, while asked and while its list is up.
    completing: Completing,
    /// The recent strip (the last five sent) is open, and which of its rows
    /// show their whole text.
    recent_open: bool,
    recent_wide: std::collections::HashSet<usize>,
    /// A sideways push on the input not yet a step, and when the last step was.
    swipe: f32,
    swiped_at: f64,
    pub lower: Lower,
    files_path: String,
    /// How many files the agents' running turns had changed when Files last
    /// listed, and whether any was running: a change lists the folder again,
    /// so what the agent writes shows as it lands (request #165).
    agent_marks: (usize, bool),
    /// Each ListDir asked is numbered; only the newest's answer is shown.
    list_seq: Arc<std::sync::atomic::AtomicU64>,
    listing: Slot<Result<Listing, String>>,
    opened: Arc<Mutex<Vec<FileText>>>,
    plot_sel: [Option<usize>; 4],
    /// The file tab last scrolled into view (a new one in front scrolls again).
    tab_follow: Option<usize>,
    seen_plots: [usize; 4],
    seen_tables: [u64; 4],
    plot_tex: HashMap<(Lang, usize), egui::TextureHandle>,
    plot_img: Arc<Mutex<HashMap<(Lang, usize), Result<egui::ColorImage, String>>>>,
    plot_asked: HashMap<(Lang, usize), bool>,
    packages: [Slot<Result<Vec<Package>, String>>; 4],
    pkg_asked: [bool; 4],
    /// The Software tab: the catalog, fetched at session start.
    software: crate::software::SoftwarePane,
    /// The Account tab, asked the first time it shows.
    account: crate::account_tab::AccountPane,
    /// The open project's app settings (settings_tab.rs), in the tab that was
    /// Account's.
    settings: crate::settings_tab::SettingsPane,
    /// Files' Copy glimpse (request #164): the folder asked for, and its
    /// `ndexr glimpse` when it comes back - copied as it arrives.
    glimpse: Arc<Mutex<Option<(String, Result<String, String>)>>>,
    glimpse_said: Option<(f64, String)>,
    /// The last render, polled while it runs.
    render: Arc<Mutex<JobView>>,
    /// The Viewer: what it shows and has shown.
    view: ViewerState,
    /// Local web addresses the consoles printed (api::local_url), for the Viewer.
    printed_urls: Arc<Mutex<Vec<String>>>,
    /// The file and line the next run comes from (run_current): R keeps them
    /// as the code's source references.
    run_at: Option<(String, usize)>,
    /// Where R's debugger is stopped, as the editor marks it: (path, line).
    /// The path is the file's, or DEBUG_DOC's for code with no file.
    debug_mark: Option<(String, usize)>,
    /// The stop last followed, so a stop opens and scrolls once.
    debug_seen: Option<(String, usize)>,
    /// The person's own domain has been opened for a first visit (once).
    own_opened: bool,
    /// A first visit in a browser (app.rs): when the person's own site opens
    /// and still holds the page signup laid, it becomes a React site.
    pub first_site: bool,
    /// Whether the person's own site is still waiting to be told what it is:
    /// its CONTEXT.md says "Your site" and its thread is empty. It goes first,
    /// before the project they were in last, once a session. Measured, not a
    /// flag, so a site reset for someone asks them too (operator, 2026-10-30:
    /// "each person on next log in should go through the first time set up").
    own_new_asked: bool,
    /// Their site went first: its folder, for app.rs to put the rest of the
    /// window back to its default, whatever this browser had saved.
    pub start_fresh: Option<String>,
    own_new_done: bool,
    own_new: Slot<Option<DomainInfo>>,
    /// The host that swap was asked for, until create_domain answers.
    first_site_for: Option<String>,
    find: FindBar,
    grep: FindInFiles,
    launch: Option<Launch>,
    /// A new domain waiting for what it starts as (kind_chooser, starters.rs).
    choosing: Option<Choosing>,
    /// The New project dialog's name, while it is open (new_project_dialog).
    naming: Option<String>,
    /// The open project's Build / Up / Down (app_strip, Query::AppAction):
    /// what it is and whether it runs, asked every 10 s and after each job;
    /// the job in its domain's slot; and whether its log is open.
    app_status: Arc<Mutex<Option<Result<crate::api::AppStatus, String>>>>,
    /// The Logs tab: the project folder it was asked for, the answer, when.
    run_log: Arc<Mutex<Option<(String, Result<FileText, String>)>>>,
    run_log_asked: f64,
    /// Logs shows Build's output (.cove/run/build.log) instead of the app's.
    run_log_build: bool,
    app_status_at: f64,
    app_host: String,
    app_job: Arc<Mutex<Option<Result<JobView, String>>>>,
    app_job_at: f64,
    app_log: bool,
    /// The open project's saved versions (Query::Versions), the window's
    /// state, the version waiting on "restore?", and the last restore's word.
    versions: Arc<Mutex<Option<Result<Vec<crate::api::Version>, String>>>>,
    versions_open: bool,
    version_said: Arc<Mutex<Option<Result<String, String>>>>,
    /// The Git window's panes (gitview.rs): the picked version, its files and
    /// the picked file's diff.
    git: crate::gitview::GitView,
    /// Completion in the editor (edcomplete.rs): the list at the cursor.
    edc: crate::edcomplete::EdComplete,
    /// On a phone, Files shows the tree or the open file, not both: whether
    /// the tree is the one showing.
    phone_tree: bool,
    /// A snippet being filled in (an Emmet expansion): its file's editor id
    /// and placeholders, which Tab and Shift+Tab walk (editkeys.rs).
    snippet: Option<(egui::Id, crate::editkeys::Snippet)>,
    /// The verb last run, whether its Up has been routed, and the route's word.
    app_verb: String,
    app_routed: bool,
    /// Up jobs that ended well this session, and whether the job in hand
    /// was counted (tour.rs: "run it again" is a new Up that worked).
    app_ups: u64,
    app_counted: bool,
    /// A walkthrough asked for from a control's own "?" (app.rs starts it).
    pub want_tour: Option<&'static str>,
    /// A site just made: its host, when, and how many settle-reloads are
    /// done (site_view). Its vhost and nginx's reload land seconds after the
    /// launch window, and a frame loaded in between showed nginx's 502 for
    /// the name's old upstream (operator screenshot, 2026-10-28).
    site_fresh: Option<(String, f64, usize)>,
    app_route: Arc<Mutex<Option<String>>>,
    /// Domains being created as Rust apps (create_domain), so their launch deploys.
    creating_rust: std::collections::HashSet<String>,
    /// A file to show at a line once it is open (a Find in Files hit).
    goto: Option<(String, usize)>,
    /// The editor scrolls to this line (1-based) on its next frame.
    scroll_line: Option<usize>,
    debug_scrolled: Option<(String, usize)>,
    render_asked: f64,
    render_inflight: Arc<AtomicBool>,
    errors: Arc<Mutex<Vec<String>>>,
    /// A transfer finished: the Files pane lists its folder again.
    files_stale: Arc<AtomicBool>,
    /// The domains this session can open (Query::ListDomains), and the picker's filter.
    domains: Slot<Result<Vec<DomainInfo>, String>>,
    /// How many domains md.rs was last told to link (set_domains).
    linked_domains: usize,
    domains_asked: bool,
    domain_filter: String,
    /// Parents opened in the picker's tree (`domain_tree`); folded otherwise.
    domains_unfolded: std::collections::HashSet<String>,
    /// The domain this session is in, once picked: the Viewer shows its site.
    pub domain: Option<DomainInfo>,
    /// Files open per domain (host -> paths): stepping back into a domain
    /// reopens them. The app keeps it across restarts (app.rs Saved).
    pub open_by_domain: HashMap<String, Vec<String>>,
    /// The environment the consoles run in, and the consoles it brings
    /// (env_consoles): the switcher offers only these.
    env_module: Option<String>,
    env_langs: Vec<Lang>,
    /// The projects stepped into, newest first (at most RECENT_MAX): the ☰
    /// menu offers them (operator, 2026-10-26: "the hamburger menu needs to show
    /// recent projects or a selector to create a new one"). Kept by app.rs Saved.
    pub recent_hosts: Vec<String>,
    /// The open projects as a browser's tabs (tabs.rs; tab_strip draws them).
    /// Kept by app.rs Saved.
    pub tabs: crate::tabs::Tabs,
    /// The sites whose App does not reload itself when their files change
    /// (the ● at the strip's right; operator, 2026-11-05). Live is the default.
    pub live_off: std::collections::BTreeSet<String>,
    /// The Projects dialog (project_dialog): which hit Enter opens, whether
    /// it was open last frame (its field is focused the moment it opens,
    /// request #168), and the frame domain_bar last drew it in, so the IDE's
    /// own call does not draw it twice.
    project_pick: usize,
    project_dialog_open: bool,
    bar_frame: u64,
    /// What ndexr says about the names in the search (Query::ProjectNames):
    /// shares waiting for me, and names someone else holds. With the names
    /// last asked about, so a frame does not ask again.
    names: Arc<Mutex<Option<api::ProjectNames>>>,
    names_asked: Option<String>,
    /// A share just accepted: opened as soon as its folder is mounted
    /// (shares.rs reconcile), asking for the projects again until then.
    pending_open: Option<(String, f64, f64)>,
    /// The tabs of domains stepped out of, kept whole: stepping back shows
    /// them at once and reads each again behind the scenes (`reread`), so a
    /// swap costs no wait for the files and loses no scroll or cursor.
    parked: HashMap<String, Vec<Doc>>,
    /// A domain to step into as soon as the list arrives (the last one, at start).
    pub restore: Option<String>,
    /// A folder the page was opened on (?folder=), whose domain opens once the
    /// list is here - ahead of `restore` and of the person's own domain.
    folder_domain: Option<String>,
    /// The person's home (the app's probe), for their own ndexr at ~/ndexr.
    pub home: String,
    /// A folder the Domain menu asked to open as the project (app.rs takes
    /// it: project dir, Files, the agent of all sites).
    pub want_folder: Option<String>,
    /// The project menu's Inlet: its recipe and patches as a project of the
    /// person's own (app.rs opens it the way the atlas opens a star's source).
    pub want_inlet: bool,
    /// Project › Access was chosen for this domain (app.rs opens access.rs).
    pub want_access: Option<String>,
    /// The project open is the person's own ndexr (a folder, not a domain).
    pub ndexr_open: bool,
    /// Push all: the job, when it was last asked, and its log dialog.
    push: Slot<JobView>,
    push_asked: f64,
    push_log: bool,
    /// A domain CreateDomain just made, to step into on the next frame.
    created: Slot<DomainInfo>,
    /// The Viewer shows the domain's live site (else the last render).
    /// Bumped by every save inside the domain: the site reloads.
    site_seq: u64,
    /// Archive: the host waiting for its confirm, and the job once started.
    /// Archive or Delete of the open project, from ☰ › Project: its dialog
    /// and how far it got (remove_dialog).
    removing: Option<Removing>,
    archive: Slot<JobView>,
    archive_asked: f64,
    /// The source workspace Files is in (source.rs): its status, for which
    /// root, when asked; the checkpoint message and test snippet being typed;
    /// the candidate job (build or test) and when it was last asked.
    ws: Slot<Result<crate::source::SourceStatus, String>>,
    ws_for: String,
    ws_asked: f64,
    ws_msg: String,
    ws_code: String,
    ws_job: Slot<JobView>,
    ws_job_asked: f64,
    /// A revision just made: the module the app switches the session to.
    switch_env: Slot<String>,
    /// What stands in front of the open domain's site (Query::SiteGate): (host, state).
    gate: Slot<(String, String)>,
    gate_asked: String,
    /// Domains asked to publish, and when (the App tab says so while it lands).
    publishing: HashMap<String, f64>,
    /// The Agents tab beside the consoles (agent.rs).
    agent: crate::agent::AgentPane,
    /// Exec: the same pane over the person's whole account - its own thread,
    /// never the open domain's. Shown inside Agents when `exec_on`.
    exec: crate::agent::AgentPane,
    /// Who the person is talking to: the agent of all their sites (Exec)
    /// rather than this site's. Chosen beside the composer's mic (agent.rs
    /// scope_toggle), saved across restarts, settable by ?agent=all|site.
    pub exec_on: bool,
    /// Site and Exec at once (operator, 2026-10-18: "split them in two up
    /// down or left right but within its agents tab ... so site and exec can
    /// be looked at and go between side by side"): one at a time, side by
    /// side, or one above the other; and the first pane's share of the room.
    pub agents_layout: AgentsLayout,
    pub agents_frac: f32,
    /// The Terminal tab: a real terminal, bash on a pty (term.rs).
    term: crate::term::TermPane,
    /// The Viewer shows the terminal (a full-screen program run from the
    /// Inputs shell - claude, ranger, vim …) rather than its page. There is no
    /// Terminal tab (operator, 2026-10-18: "we dont need a terminal tab when
    /// we have the shell in the console").
    view_term: bool,
    /// The Data tab: the person's SQLite database, or one opened from Files (db.rs).
    data: crate::db::DataPane,
    /// Tabs asked forward since the dock last looked (`take_focus`), each
    /// with whether it only swaps Console and Terminal (a run in the other).
    focus: Vec<(Tab, bool)>,
    /// The bar under an open file: ask its agent for a change to it.
    ask: crate::agent::FileAsk,
    /// Files to show in place (images, PDFs …) once the poll picks them up.
    to_preview: Arc<Mutex<Vec<String>>>,
    /// A file read again because it changed on disk (or a file ask works on it).
    reread: Arc<Mutex<Vec<FileText>>>,
    /// Live files (operator, 2026-10-14: "cove should know if files on the
    /// page have changed and if so it should update the view ... we should
    /// never have to hit refresh"): the last fingerprint of each open file and
    /// of the Files folder, and the answer to the latest ask.
    stamps: std::collections::HashMap<String, String>,
    stamp_slot: Arc<Mutex<Option<Vec<(String, String)>>>>,
    stamp_inflight: Arc<AtomicBool>,
    stamp_asked: f64,
    /// The open domain's site fingerprint (Query::SiteStamp): (folder, stamp),
    /// the answer in flight, when it was asked, and when a save in Cove's
    /// own editor last reloaded the site (a stamp change right after it is
    /// that same save, not a second one).
    site_stamp: Option<(String, String)>,
    site_stamp_slot: Slot<(String, String)>,
    site_stamp_inflight: Arc<AtomicBool>,
    site_stamp_asked: f64,
    site_saved_at: f64,
    /// Bumped when a previewed file changes, so its frame loads it again.
    preview_seq: std::collections::HashMap<String, u64>,
    /// Seconds between checks for changed files (Global Options, 1-3).
    pub refresh_every: f64,
    /// Help › Send a request is open: pastes and drops go to it, not to the
    /// agent or the Files folder (app.rs request_dialog).
    pub hold_shots: bool,
    /// Listen: the open mic, whether what it hears goes to an agent as it
    /// comes, and what it has heard so far this time - one for the whole IDE
    /// (listen_frame), whichever composer started it.
    listen: crate::voice::Listen,
    listen_act: bool,
    /// Seconds between Listen's pieces (voice.rs PIECES), kept across
    /// restarts (app.rs Saved), picked beside any agent's mode.
    pub listen_secs: u32,
    /// The agent "act as I talk" sends to: the composer whose Listen was
    /// clicked last.
    listen_to: ListenTo,
    /// Each piece heard, and what became of it ("sent to the agent", ...).
    heard: Vec<(String, String)>,
    /// The desktop app's decoded images, by path (the browser shows them in a frame).
    #[cfg(not(target_arch = "wasm32"))]
    images: std::collections::HashMap<String, Arc<Mutex<Option<Result<egui::ColorImage, String>>>>>,
    #[cfg(not(target_arch = "wasm32"))]
    textures: std::collections::HashMap<String, egui::TextureHandle>,
}

/// Every tab a window can hold. app.rs's dock says which window each is in
/// (operator, 2026-10-15: drag Console, Terminal, Agents, App, Software,
/// Plots … to a corner and it opens a window there, "similar to how pycharm
/// and vscode can do but rstudio cannot"). Agents is what a domain opens on
/// (operator: "Agents should be the first thing that opens not files when a
/// domain is selected").
#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Tab {
    Console,
    Terminal,
    Agents,
    /// The person's executive agent: every domain they own in one thread, as
    /// exec.ndexr.io is for the operator (operator, 2026-10-15: "we need an
    /// executive agent for all of the users domains ... in the tab those two
    /// are separate but always accessible"). Agents stays the open domain's.
    Exec,
    Files,
    App,
    /// R's global environment and reticulate's Python's, as RStudio's
    /// Environment pane: Data (a click opens View()), Values, Functions; while
    /// R's debugger is stopped, that frame's objects. Back 2026-10-19
    /// (operator: "make sure rstudio users feel comfortable using cove ...
    /// variable and document viewers, csv viewer etc brought out through the
    /// environment panel"), after going 2026-10-14.
    Environment,
    /// The stack's software, its builds, and installing it here: one tab each
    /// beside App and Plots, each with its own views beneath (operator,
    /// 2026-10-17: "packages, builds and install should be in the same lane as
    /// app software plots etc and then their sub groups below"). `Software`
    /// is Packages - the name a saved layout knows it by.
    Software,
    Builds,
    Install,
    Plots,
    Viewer,
    Data,
    /// The open project's app settings - info, access, visibility, variables
    /// (settings_tab.rs). Named Account until 2026-10-28, when the account
    /// moved to the Files panel; the variant keeps saved layouts loading.
    Account,
    /// The app's own output: run.sh's stdout and stderr as up.sh logs them
    /// (.cove/run/app.log), followed live, after App (operator, 2026-11-08).
    Logs,
}

impl Tab {
    /// Exec is not among them: it is a toggle inside Agents (2026-10-18). The
    /// variant stays so a layout saved with it still loads (Dock::mend drops it).
    /// Nor Data (removed 2026-10-18): its variant stays for saved layouts too.
    /// Builds is a mode of its own now (app.rs builds_view, operator
    /// 2026-10-19: "remove builds from IDE"): its variant stays so a saved
    /// layout still loads, and Grid::mend drops it.
    /// Packages (`Software`) went to Builds as well (operator, 2026-10-23:
    /// "the users then can focus in their tab exclusively on their site or
    /// environment"): the stack's catalog is Builds' Find, Tree and
    /// Manifests. Install stays - it is the person's own environment.
    pub const ALL: [Tab; 10] = [Tab::Agents, Tab::Console, Tab::Files, Tab::App, Tab::Logs, Tab::Environment, Tab::Install, Tab::Plots, Tab::Viewer, Tab::Account];

    pub fn name(self) -> &'static str {
        match self {
            // "Inputs" from 2026-10-15; back to "Console" (operator, 2026-10-20:
            // "the inputs window which we should rename console").
            Tab::Console => "Console",
            Tab::Terminal => "Terminal",
            Tab::Agents => "Agents",
            Tab::Exec => "Exec",
            Tab::Files => "Files",
            Tab::App => "App",
            Tab::Environment => "Environment",
            Tab::Software => "Packages",
            Tab::Builds => "Builds",
            Tab::Install => "Install",
            Tab::Plots => "Plots",
            Tab::Viewer => "Viewer",
            Tab::Data => "Data",
            Tab::Account => "Settings",
            Tab::Logs => "Logs",
        }
    }

    /// The tab a kernel's transcript is in.
    pub fn of_lang(lang: Lang) -> Tab {
        let _ = lang;
        // The shell is a channel of Inputs; its full-screen programs, the Viewer.
        Tab::Console
    }

    fn of_lower(l: Lower) -> Tab {
        match l {
            Lower::App => Tab::App,
            Lower::Plots => Tab::Plots,
            Lower::Viewer => Tab::Viewer,
            Lower::Files => Tab::Files,
            Lower::Data => Tab::Data,
        }
    }
}

/// What the + menu makes: a script in a language, or a document (Quarto
/// when true, else R Markdown).
#[derive(Clone, Copy)]
enum NewFile {
    Script(Lang),
    Document(bool),
}

/// The + menu of new files, shared by the editor's tabs and a phone's tree.
fn new_file_menu(ui: &mut egui::Ui, label: RichText) -> Option<NewFile> {
    let mut made = None;
    ui.menu_button(label, |ui| {
        for (word, n) in [("R Script", NewFile::Script(Lang::R)), ("Python Script", NewFile::Script(Lang::Python)), ("Shell Script", NewFile::Script(Lang::Shell))] {
            if ui.button(word).clicked() {
                made = Some(n);
            }
        }
        ui.separator();
        for (word, n) in [("Quarto Document", NewFile::Document(true)), ("R Markdown", NewFile::Document(false))] {
            if ui.button(word).clicked() {
                made = Some(n);
            }
        }
    })
    .response
    .on_hover_text("new file");
    made
}

/// A .py file runs in the R session's own Python (reticulate): `r.x` is R's x,
/// and what it defines is `py$name` back in R.
const STARTER_JL: &str = "# Cove · Julia\n# Julia from the ndexr stack (ml Julia); packages go to your own depot (~/.julia)\n\nprintln(VERSION)\n";
const STARTER_SH: &str = "# Cove · Prefix shell\n# bash in the ndexr stack: ml av lists every module, ml <name> loads one\n\nml av R/ Python/ 2>&1 | head -20\n";
/// A new document has an R and a Python chunk: both run in the one R session,
/// the Python through reticulate, as knitr renders them.
const STARTER_QMD: &str = "---\ntitle: \"Untitled\"\nformat: html\n---\n\n## R\n\n```{r}\nx <- rnorm(200)\nsummary(x)\nhist(x)\n```\n\n## Python\n\n```{python}\nimport statistics\nstatistics.mean([1.5, 2.5, 4.0])\n```\n";
const STARTER_RMD: &str = "---\ntitle: \"Untitled\"\noutput: html_document\n---\n\n```{r setup}\nknitr::opts_chunk$set(echo = TRUE)\n```\n\n## R\n\n```{r}\nsummary(cars)\n```\n\n## Python\n\n```{python}\nimport statistics\nstatistics.mean([1.5, 2.5, 4.0])\n```\n";


impl Ide {
    pub fn new(backend: Backend, ctx: &egui::Context) -> Self {
        let mut ide = Ide {
            backend,
            docs: Vec::new(),
            cur: 0,
            serial: 0,
            save_as: None,
            memo: HashMap::new(),
            consoles: Default::default(),
            lang: Lang::R,
            chan: Channel::Code(Lang::R),
            chunks: Vec::new(),
            // Off: a line runs as typed until the person ticks Agent.
            assist_on: false,
            assist: Default::default(),
            assisting: None,
            assist_note: None,
            console_mic: Default::default(),
            completing: Default::default(),
            recent_open: true,
            recent_wide: Default::default(),
            lang_hist: Default::default(),
            lang_hist_at: None,
            swipe: 0.0,
            swiped_at: f64::NEG_INFINITY,
            lower: Lower::App,
            files_path: String::new(),
            agent_marks: (0, false),
            list_seq: Default::default(),
            listing: Default::default(),
            opened: Default::default(),
            plot_sel: [None; 4],
            tab_follow: None,
            seen_plots: [0; 4],
            seen_tables: [0; 4],
            plot_tex: HashMap::new(),
            plot_img: Default::default(),
            plot_asked: HashMap::new(),
            packages: Default::default(),
            pkg_asked: [false; 4],
            software: Default::default(),
            account: Default::default(),
            settings: Default::default(),
            glimpse: Default::default(),
            glimpse_said: None,
            render: Default::default(),
            view: Default::default(),
            printed_urls: Default::default(),
            run_at: None,
            debug_mark: None,
            debug_seen: None,
            own_opened: false,
            first_site: false,
            own_new_asked: false,
            start_fresh: None,
            own_new_done: false,
            own_new: Default::default(),
            first_site_for: None,
            find: FindBar::default(),
            grep: FindInFiles::default(),
            launch: None,
            choosing: None,
            naming: None,
            app_status: Default::default(),
            run_log: Default::default(),
            run_log_asked: 0.0,
            run_log_build: false,
            app_status_at: f64::NEG_INFINITY,
            app_host: String::new(),
            app_job: Default::default(),
            app_job_at: f64::NEG_INFINITY,
            app_log: false,
            versions: Default::default(),
            versions_open: false,
            version_said: Default::default(),
            git: Default::default(),
            edc: Default::default(),
            phone_tree: false,
            snippet: None,
            app_verb: String::new(),
            app_routed: false,
            app_ups: 0,
            app_counted: false,
            want_tour: None,
            site_fresh: None,
            app_route: Default::default(),
            creating_rust: Default::default(),
            goto: None,
            scroll_line: None,
            debug_scrolled: None,
            render_asked: f64::NEG_INFINITY,
            render_inflight: Default::default(),
            errors: Default::default(),
            files_stale: Default::default(),
            domains: Default::default(),
            linked_domains: 0,
            domains_asked: false,
            domain_filter: String::new(),
            domains_unfolded: Default::default(),
            domain: None,
            open_by_domain: HashMap::new(),
            tabs: Default::default(),
            recent_hosts: Vec::new(),
            project_pick: 0,
            project_dialog_open: false,
            bar_frame: 0,
            names: Arc::new(Mutex::new(None)),
            names_asked: None,
            pending_open: None,
            env_module: None,
            env_langs: vec![Lang::R, Lang::Python, Lang::Shell, Lang::Julia],
            parked: HashMap::new(),
            restore: None,
            folder_domain: None,
            home: String::new(),
            want_folder: None,
            want_inlet: false,
            want_access: None,
            ndexr_open: false,
            push: Default::default(),
            push_asked: f64::NEG_INFINITY,
            push_log: false,
            created: Default::default(),
            site_seq: 0,
            removing: None,
            archive: Default::default(),
            archive_asked: f64::NEG_INFINITY,
            ws: Default::default(),
            ws_for: String::new(),
            ws_asked: f64::NEG_INFINITY,
            ws_msg: String::new(),
            ws_code: String::new(),
            ws_job: Default::default(),
            ws_job_asked: f64::NEG_INFINITY,
            switch_env: Default::default(),
            gate: Default::default(),
            gate_asked: String::new(),
            publishing: HashMap::new(),
            agent: Default::default(),
            exec: crate::agent::AgentPane::exec(),
            exec_on: false,
            agents_layout: AgentsLayout::One,
            agents_frac: 0.5,
            term: Default::default(),
            view_term: false,
            focus: Vec::new(),
            ask: Default::default(),
            to_preview: Default::default(),
            data: Default::default(),
            reread: Default::default(),
            stamps: Default::default(),
            stamp_slot: Default::default(),
            stamp_inflight: Default::default(),
            stamp_asked: f64::NEG_INFINITY,
            site_stamp: None,
            site_stamp_slot: Default::default(),
            site_stamp_inflight: Default::default(),
            site_stamp_asked: f64::NEG_INFINITY,
            site_saved_at: f64::NEG_INFINITY,
            live_off: Default::default(),
            preview_seq: Default::default(),
            refresh_every: 3.0,
            hold_shots: false,
            listen: Default::default(),
            listen_act: false,
            listen_secs: 60,
            listen_to: ListenTo::Agent,
            heard: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            images: Default::default(),
            #[cfg(not(target_arch = "wasm32"))]
            textures: Default::default(),
        };
        // No Untitled1.R / Untitled2.py: a session starts in a domain, picked
        // in the App tab (operator, 2026-10-13: "users will always start in a
        // domain").
        // Every session starts with the app: empty code only starts one.
        for lang in Lang::CONSOLES {
            ide.send(lang, String::new(), ctx);
        }
        ide.list("", ctx);
        // The Software catalog comes now, so the tab opens on it at once.
        ide.software.load(&ide.backend, ctx);
        ide
    }

    fn new_doc(&mut self, path: &str, text: &str, lang: Option<Lang>) {
        self.serial += 1;
        let document = is_document(path);
        self.docs.push(Doc {
            path: path.to_owned(),
            text: text.to_owned(),
            saved: if path.starts_with('/') { text.to_owned() } else { String::new() },
            lang: if document { None } else { lang },
            document,
            table: None,
            preview: false,
            filter: String::new(),
            id: egui::Id::new(("doc", self.serial)),
            disk: None,
        });
        self.cur = self.docs.len() - 1;
    }

    /// The session the Environment and Plots panes show: R's, which holds
    /// reticulate's Python too; in an environment without R (env/Julia,
    /// env/Python), its own language's - an env/Julia project's plots were
    /// drawn and never shown (2026-10-31).
    fn data_lang(&self) -> Lang {
        if self.env_langs.contains(&Lang::R) {
            return Lang::R;
        }
        self.env_langs.iter().copied().find(|l| matches!(l, Lang::Julia | Lang::Python)).unwrap_or(Lang::R)
    }

    /// Errors for the app's toast.
    pub fn take_errors(&self) -> Vec<String> {
        std::mem::take(&mut *self.errors.lock().unwrap())
    }

    pub fn banner(&self, lang: Lang) -> String {
        self.consoles[ix(lang)].view.lock().unwrap().banner.clone()
    }

    /// Send `text` to Exec as a turn of its own, with Agents · Exec forward
    /// (Messages' Hand to Exec). Returns the turn's key.
    pub fn exec_ask(&mut self, ctx: &egui::Context, text: &str) -> String {
        self.exec_on = true;
        self.bring(Tab::Agents);
        let host = self.exec_scope().host;
        let Ide { exec, backend, .. } = self;
        exec.ask(backend, ctx, &host, text, "")
    }

    /// An Exec turn by its key: its reply so far, and whether it finished.
    pub fn exec_turn(&self, key: &str) -> Option<(String, bool)> {
        self.exec.turn_text(key).map(|(_, reply, done)| (reply, done))
    }

    /// The Agents tab forward, with `text` typed into its prompt (not sent).
    pub fn ask_agent(&mut self, text: &str) {
        self.agent.prefill(text);
        self.bring(Tab::Agents);
    }

    /// Any console computing (on a phone, a run brings the console forward).
    pub fn busy(&self) -> bool {
        self.consoles.iter().any(|c| c.view.lock().unwrap().busy)
    }

    fn fail(errors: &Arc<Mutex<Vec<String>>>, e: String) {
        errors.lock().unwrap().push(e);
    }

    // ── kernels ────────────────────────────────────────────────────────────

    /// Hand code to a kernel. The answer is only "taken"; the transcript
    /// arrives by polling.
    /// Python goes to the R session (the server wraps it for reticulate).
    pub fn send(&mut self, lang: Lang, code: String, ctx: &egui::Context) {
        self.send_as(lang, code, ctx, false);
    }

    fn send_as(&mut self, lang: Lang, code: String, ctx: &egui::Context, quiet: bool) {
        // A console the environment does not bring is never started by a
        // chunk or an agent's code block: it would run whatever that
        // language is on PATH (the host's /usr/bin/R), not the project's.
        if !self.has_console(lang) {
            let env = self.env_module.clone().unwrap_or_default();
            Ide::fail(&self.errors, format!("{} is not in this project's environment ({env}) - pin one with it in ☰ › Project › Settings", lang.word()));
            return;
        }
        let ran = !code.trim().is_empty();
        let chan = Channel::Code(lang);
        if ran && !quiet {
            let h = &mut self.lang_hist[chan.slot()];
            if h.last() != Some(&code) {
                h.push(code.clone());
            }
            self.lang_hist_at = None;
        }
        // A full-screen program goes to the Terminal tab, typed in as if by
        // hand where the page's shell is; the page keeps a note of it.
        if lang == Lang::Shell && ran && wants_terminal(&code) {
            let cwd = self.consoles[ix(Lang::Shell)].view.lock().unwrap().cwd.clone();
            if !cwd.is_empty() {
                self.term.type_line(&format!("cd {}", api::shell_quote(&cwd)));
            }
            self.term.type_line(&code);
            self.chunks.push(Run { moved: true, quiet, ..Run::code(chan, &code, 0) });
            self.view_term = true;
            self.focus.push((Tab::Viewer, false));
            return;
        }
        let session = lang.session();
        let c = &mut self.consoles[ix(session)];
        if ran {
            let mut v = c.view.lock().unwrap();
            v.busy = true;
            self.chunks.push(Run { quiet, ..Run::code(chan, &code, v.next) });
            drop(v);
            // History holds everything run, from wherever, as RStudio's does.
            if c.history.last() != Some(&code) {
                c.history.push(code.clone());
            }
            c.hist = None;
            if session != Lang::Shell {
                self.lang = session;
            }
            // The Console shows the command where it ran (operator, 2026-10-20:
            // "when clicking and executing a command from agents, it should
            // open up the command in the inputs window"): on its own channel,
            // so the page and its input are R / Py / $ - not left on Agent,
            // which hid the run behind the agent's thread - and ↑ recalls it.
            if !quiet {
                self.chan = chan;
            }
            self.focus.push((Tab::Console, true));
        }
        c.asked = f64::NEG_INFINITY;
        let errors = self.errors.clone();
        let at = self.run_at.take();
        self.backend.query(Query::Eval { lang, code, at }, ctx, move |r: Result<KernelView, String>| {
            if let Err(e) = r {
                Ide::fail(&errors, format!("{}: {e}", lang.word()));
            }
        });
    }

    /// Restart `lang`'s session. The shell is two: the Console's line-mode
    /// shell and the Terminal's bash, and both start again (a new environment
    /// reaches both).
    /// End a console's kernel and leave it stopped (a console the
    /// environment does not bring).
    pub fn stop_console(&mut self, lang: Lang, ctx: &egui::Context) {
        self.backend.query(Query::Stop { lang }, ctx, |_: Result<KernelView, String>| {});
    }

    pub fn restart(&mut self, lang: Lang, ctx: &egui::Context) {
        let lang = lang.session();
        if lang == Lang::Shell {
            self.restart_terminal(ctx);
        }
        if lang == Lang::R {
            // reticulate's Python restarts with it: its packages may change too.
            *self.packages[ix(Lang::Python)].lock().unwrap() = None;
            self.pkg_asked[ix(Lang::Python)] = false;
        }
        self.plot_sel[ix(lang)] = None;
        self.seen_plots[ix(lang)] = 0;
        self.plot_tex.retain(|(l, _), _| *l != lang);
        self.plot_asked.retain(|(l, _), _| *l != lang);
        self.plot_img.lock().unwrap().retain(|(l, _), _| *l != lang);
        *self.packages[ix(lang)].lock().unwrap() = None;
        self.pkg_asked[ix(lang)] = false;
        self.consoles[ix(lang)].asked = f64::NEG_INFINITY;
        let errors = self.errors.clone();
        self.backend.query(Query::Restart { lang }, ctx, move |r: Result<KernelView, String>| {
            if let Err(e) = r {
                Ide::fail(&errors, format!("restart {}: {e}", lang.word()));
            }
        });
    }

    /// Typed on the Agent channel: the domain's agent takes it as the
    /// person's next turn - the same thread as the Agents tab - and the page
    /// shows the prompt and the reply as a band like any other.
    /// An ask in a language's input (Agent ticked, or spoken): code in that language
    /// with what the console shows - its recent lines, the session's objects,
    /// where it is and which environment - and put the answer in the input
    /// to read, change and run (`poll_assist`). It never runs by itself.
    fn ask_assist(&mut self, lang: Lang, typed: &str, ask: &str, ctx: &egui::Context) {
        if self.assisting.is_some() {
            self.consoles[ix(Lang::R)].input = typed.to_owned();
            return;
        }
        let context = {
            let v = self.consoles[ix(lang.session())].view.lock().unwrap();
            let lines: Vec<&str> = v.lines.iter().rev().take(60).map(|l| l.text.as_str()).collect();
            let objs: Vec<String> = v.env.iter().take(40).map(|e| format!("{} ({})", e.name, e.class)).collect();
            format!(
                "language: {}\nsession: {}\nworking directory: {}\nobjects: {}\n--- last lines ---\n{}",
                lang.word(),
                v.banner,
                v.cwd,
                if objs.is_empty() { "none listed".to_owned() } else { objs.join(", ") },
                lines.into_iter().rev().collect::<Vec<_>>().join("\n")
            )
        };
        let h = &mut self.lang_hist[Channel::Code(lang).slot()];
        if h.last().map(String::as_str) != Some(typed) {
            h.push(typed.to_owned());
        }
        self.lang_hist_at = None;
        self.assisting = Some((lang, typed.to_owned()));
        self.assist_note = None;
        self.consoles[ix(Lang::R)].input = typed.to_owned();
        let slot = self.assist.clone();
        *slot.lock().unwrap() = None;
        self.backend.query(Query::Assist { lang, ask: ask.to_owned(), context }, ctx, move |r: Result<api::AssistReply, String>| {
            *slot.lock().unwrap() = Some(r);
        });
    }

    /// The agent's answer, once here: the code replaces what was typed, in
    /// the language that asked, with the cursor at its end.
    fn poll_assist(&mut self, ctx: &egui::Context, input_id: egui::Id) {
        let Some((lang, typed)) = self.assisting.clone() else { return };
        let Some(r) = self.assist.lock().unwrap().take() else {
            ctx.request_repaint_after(std::time::Duration::from_millis(150));
            return;
        };
        self.assisting = None;
        let input = &mut self.consoles[ix(Lang::R)].input;
        match r {
            Ok(a) if !a.code.trim().is_empty() => {
                // Only if the input still holds the ask: typing on meanwhile wins.
                if input.trim() == typed.trim() || input.trim().is_empty() {
                    *input = a.code.clone();
                    self.chan = Channel::Code(lang);
                    // The code is in: Agent turns itself off, so Enter runs it
                    // instead of asking again (operator, 2026-10-31).
                    self.assist_on = false;
                    set_cursor(ctx, input_id, input.chars().count());
                    ctx.memory_mut(|m| m.request_focus(input_id));
                }
                self.assist_note = Some(format!("{} · written by {} - Enter runs it, edit it first if you like", lang.word(), if a.model.is_empty() { "the agent" } else { a.model.as_str() }));
            }
            Ok(_) => self.assist_note = Some("the agent had nothing to write for that - ask another way".into()),
            Err(e) => self.assist_note = Some(format!("Agent: {e}")),
        }
        ctx.request_repaint();
    }

    fn send_agent(&mut self, text: String, ctx: &egui::Context) {
        let Some(host) = self.domain.as_ref().map(|d| d.host.clone()) else {
            Ide::fail(&self.errors, "open a domain first - its agent works in that domain's thread".into());
            return;
        };
        let tail = self.console_tail(Lang::R, 60);
        let Ide { agent, backend, .. } = self;
        let key = agent.ask(backend, ctx, &host, &text, &tail);
        let h = &mut self.lang_hist[Channel::Agent.slot()];
        if h.last() != Some(&text) {
            h.push(text.clone());
        }
        self.lang_hist_at = None;
        self.chunks.push(Run { agent: Some(key), ..Run::code(Channel::Agent, &text, 0) });
    }

    /// The Terminal tab's bash only: end it and start a new one.
    pub fn restart_terminal(&mut self, ctx: &egui::Context) {
        let Ide { term, backend, .. } = self;
        term.restart(backend, ctx);
    }

    pub fn interrupt(&self, lang: Lang, ctx: &egui::Context) {
        let errors = self.errors.clone();
        self.backend.query(Query::Interrupt { lang }, ctx, move |r: Result<serde_json::Value, String>| {
            if let Err(e) = r {
                Ide::fail(&errors, format!("interrupt: {e}"));
            }
        });
    }

    /// Each frame: ask each kernel what it said — every 150 ms while busy,
    /// every 1.5 s otherwise.
    pub fn poll(&mut self, ctx: &egui::Context) {
        self.follow_debugger(ctx);
        self.listen_frame(ctx);
        if ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, Key::F))) {
            self.grep.open = true;
            self.grep.focus = true;
        }
        self.find_in_files(ctx);
        // A domain named in a reply and clicked (md.rs): step into it. The
        // links are the person's own domains, kept current from the list -
        // asked for at start (once) so a fresh session has them too.
        self.ask_domains(ctx);
        if let Some(Ok(list)) = self.domains.lock().unwrap().as_ref() {
            if self.linked_domains != list.len() {
                self.linked_domains = list.len();
                crate::md::set_domains(list.iter().map(|d| d.host.clone()));
            }
        }
        if let Some(host) = crate::md::take_domain() {
            let hit = self.domains.lock().unwrap().as_ref().and_then(|r| r.as_ref().ok()).and_then(|l| l.iter().find(|d| d.host == host).cloned());
            match hit {
                Some(d) if self.domain.as_ref().is_none_or(|c| c.host != d.host) => self.open_domain(d, ctx),
                Some(_) => {}
                None => Ide::fail(&self.errors, format!("{host} is not one of your domains here")),
            }
        }
        // The agent's runs are followed here too: the Inputs page sends to it.
        {
            let scope = self.exec_scope();
            let Ide { agent, exec, backend, domain, .. } = self;
            agent.tick(backend, ctx, domain.as_ref());
            exec.tick(backend, ctx, Some(&scope));
        }
        // A domain just created: step into it.
        let made = self.created.lock().unwrap().take();
        if let Some(d) = made {
            // A new site goes up at once: published as it is made, so it
            // answers with its starter page rather than a 404 - and the person
            // watches it go live (launch_window).
            let rust = self.creating_rust.remove(&d.host);
            self.site_fresh = Some((d.host.clone(), ctx.input(|i| i.time), 0));
            let quiet = self.first_site_for.take().is_some_and(|h| h == d.host);
            if quiet {
                // Already open as the page it replaced: its tab goes, and what
                // the project now is reads afresh.
                let gone = format!("{}/module.r", d.path);
                self.docs.retain(|x| x.path != gone);
                self.cur = self.cur.min(self.docs.len().saturating_sub(1));
                self.domain = Some(d.clone());
                self.app_status_at = f64::NEG_INFINITY;
                self.open(format!("{}/static/app.js", d.path), ctx);
            }
            self.launch = Some(Launch {
                quiet,
                host: d.host.clone(),
                rust,
                started: ctx.input(|i| i.time),
                // Signup registered it and chose who may open it; publishing
                // again would set that back to login-only.
                publish: Arc::new(Mutex::new(quiet.then(|| Ok(d.host.clone())))),
                deploy: Default::default(),
                deploy_sent: false,
                deploy_asked: f64::NEG_INFINITY,
                app: Default::default(),
                app_asked: false,
                up: Default::default(),
                up_sent: false,
                up_asked: f64::NEG_INFINITY,
                route: Default::default(),
                route_sent: false,
                route_asked: f64::NEG_INFINITY,
                gate: Default::default(),
                gate_asked: f64::NEG_INFINITY,
            });
            if !quiet {
                self.publish_domain(d.host.clone(), ctx);
            }
            self.open_domain(d, ctx);
        }
        if self.first_site {
            if let Some(d) = self.domain.clone() {
                self.first_site = false;
                if own_domain(std::slice::from_ref(&d)).is_some() && d.framework == "shiny" {
                    // The server decides: anything but a new, untouched signup
                    // page is refused, and the site stays as it is.
                    self.first_site_for = Some(d.host.clone());
                    self.create_domain(d.host, crate::starters::NODE_SITE, Some(Default::default()), ctx);
                }
            }
        }
        self.launch_window(ctx);
        self.project_dialog(ctx, None::<fn(&mut egui::Ui)>);
        self.kind_chooser(ctx);
        self.remove_dialog(ctx);
        self.own_site_first(ctx);
        // Opened on a folder (?folder=, a site's "open in Cove" link): the
        // domain that folder is in, rather than the one this session was in
        // last (operator, 2026-10-23: recipes.ndexr.io's Cove button opened
        // its folder while the project, the agent and App stayed on
        // domains.ndexr.io). The deepest domain containing it wins; a folder
        // in no domain leaves the session as it was.
        if let Some(f) = self.folder_domain.clone().filter(|_| self.own_new_done) {
            self.ask_domains(ctx);
            let found = match self.domains.lock().unwrap().as_ref() {
                None => None,
                Some(Ok(v)) => Some(domain_of_folder(v, &f)),
                Some(Err(_)) => Some(None),
            };
            if let Some(d) = found {
                self.folder_domain = None;
                if let Some(d) = d {
                    self.restore = None;
                    self.own_opened = true;
                    if self.domain.as_ref().is_none_or(|c| c.host != d.host) {
                        self.open_domain(d, ctx);
                    }
                }
            }
        }
        // Nothing to go back to (a first visit): the person's own domain,
        // <username>.ndexr.io, open at once (operator, 2026-10-19: "their
        // domain should open up immediately").
        if self.own_new_done && self.restore.is_none() && self.domain.is_none() && !self.own_opened {
            self.ask_domains(ctx);
            let own = match self.domains.lock().unwrap().as_ref() {
                Some(Ok(v)) => Some(own_domain(v)),
                Some(Err(_)) => Some(None),
                None => None,
            };
            if let Some(d) = own {
                self.own_opened = true;
                if let Some(d) = d {
                    self.open_domain(d, ctx);
                }
            }
        }
        // At start: back into the domain this person was in last time, with
        // the files they had open there.
        if let Some(host) = self.restore.clone().filter(|_| self.own_new_done) {
            self.ask_domains(ctx);
            let found = match self.domains.lock().unwrap().as_ref() {
                None => None,
                Some(Ok(v)) => Some(v.iter().find(|d| d.host == host).cloned()),
                Some(Err(_)) => Some(None),
            };
            if let Some(d) = found {
                self.restore = None;
                if let Some(d) = d {
                    self.open_domain(d, ctx);
                }
            }
        }
        let now = ctx.input(|i| i.time);
        for lang in Lang::CONSOLES {
            let c = &mut self.consoles[ix(lang)];
            let busy = c.view.lock().unwrap().busy;
            let every = if busy { 0.15 } else { 1.5 };
            if c.inflight.load(Ordering::Relaxed) || now - c.asked < every {
                continue;
            }
            c.asked = now;
            c.inflight.store(true, Ordering::Relaxed);
            let since = c.view.lock().unwrap().next;
            let (view, inflight, printed) = (c.view.clone(), c.inflight.clone(), self.printed_urls.clone());
            self.backend.query(Query::Poll { lang, since }, ctx, move |r: Result<KernelView, String>| {
                if let Ok(new) = r {
                    // A server started from a console (quarto::quarto_preview,
                    // plumber, a system() call …): its address, for the Viewer.
                    printed.lock().unwrap().extend(new.lines.iter().filter_map(|l| api::local_url(&l.text)));
                    let mut v = view.lock().unwrap();
                    if new.next >= v.next {
                        v.lines.extend(new.lines);
                        v.next = new.next;
                    }
                    v.busy = new.busy;
                    v.prompt = new.prompt;
                    v.debug = new.debug;
                    v.debug_file = new.debug_file;
                    v.debug_line = new.debug_line;
                    v.debug_from = new.debug_from;
                    v.debug_src = new.debug_src;
                    v.alive = new.alive;
                    v.banner = new.banner;
                    v.env = new.env;
                    v.plots = new.plots;
                    v.cwd = new.cwd;
                    v.table = new.table;
                    v.tables = new.tables;
                    v.view = new.view;
                    v.views = new.views;
                    let keep = 20_000;
                    if v.lines.len() > keep {
                        let cut = v.lines.len() - keep;
                        v.lines.drain(..cut);
                    }
                }
                inflight.store(false, Ordering::Relaxed);
            });
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(every));
        }
        // A new plot brings the Plots pane forward, as in RStudio.
        for lang in Lang::CONSOLES {
            let n = self.consoles[ix(lang)].view.lock().unwrap().plots;
            if n > self.seen_plots[ix(lang)] {
                self.seen_plots[ix(lang)] = n;
                self.plot_sel[ix(lang)] = None;
                self.show_lower(Lower::Plots);
                self.lang = lang;
            } else if n < self.seen_plots[ix(lang)] {
                self.seen_plots[ix(lang)] = n;
            }
        }
        // View() opened a table: a viewer tab in the source pane, replacing
        // the one already showing that object.
        for lang in Lang::CONSOLES {
            let (n, table) = {
                let v = self.consoles[ix(lang)].view.lock().unwrap();
                (v.tables, v.table.clone())
            };
            if n > self.seen_tables[ix(lang)] {
                self.seen_tables[ix(lang)] = n;
                if let Some(t) = table {
                    self.show_table(lang, t);
                }
            } else if n < self.seen_tables[ix(lang)] {
                self.seen_tables[ix(lang)] = n;
            }
        }
        // The Viewer: what R handed its viewer, a finished render, a server
        // a console or the terminal printed.
        for lang in Lang::CONSOLES {
            let (n, addr) = {
                let v = self.consoles[ix(lang)].view.lock().unwrap();
                (v.views, v.view.clone())
            };
            if n > self.view.seen[ix(lang)] {
                self.view.seen[ix(lang)] = n;
                self.show_view(addr);
            } else if n < self.view.seen[ix(lang)] {
                self.view.seen[ix(lang)] = n;
            }
        }
        let done = {
            let j = self.render.lock().unwrap();
            (!j.running && j.ok == Some(true) && !j.view.is_empty()).then(|| j.view.clone())
        };
        if let Some(addr) = done.filter(|a| *a != self.view.rendered) {
            self.view.rendered = addr.clone();
            self.view.log = false;
            self.show_view(addr);
        }
        let printed: Vec<String> = std::mem::take(&mut *self.printed_urls.lock().unwrap()).into_iter().chain(self.term.take_urls()).collect();
        for u in printed {
            if let Some(addr) = api::port_address(&u) {
                if !self.view.servers.contains(&addr) {
                    self.view.servers.push(addr.clone());
                    self.show_view(addr);
                }
            }
        }
        // A render running: ask how far it got, every second.
        let rendering = self.render.lock().unwrap().running;
        if rendering && !self.render_inflight.load(Ordering::Relaxed) && now - self.render_asked > 1.0 {
            self.render_asked = now;
            self.render_inflight.store(true, Ordering::Relaxed);
            let (slot, inflight) = (self.render.clone(), self.render_inflight.clone());
            self.backend.query(Query::RenderJob, ctx, move |r: Result<JobView, String>| {
                if let Ok(v) = r {
                    *slot.lock().unwrap() = v;
                }
                inflight.store(false, Ordering::Relaxed);
            });
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        // Images, PDFs …: a tab each, shown in place.
        let previews: Vec<String> = std::mem::take(&mut *self.to_preview.lock().unwrap());
        for path in previews {
            match self.docs.iter().position(|d| d.path == path) {
                Some(i) => self.cur = i,
                None => {
                    self.new_doc(&path, "", None);
                    self.docs.last_mut().unwrap().preview = true;
                }
            }
        }
        // A file ask at work: read its file again, and show what the agent
        // wrote - unless the person has typed into it since (their edits win;
        // the file on disk is still there to reload).
        let now = ctx.input(|i| i.time);
        if let Some(path) = self.ask.refresh_due(now) {
            if self.docs.iter().any(|d| d.path == path && !d.preview) {
                let (slot, errors) = (self.reread.clone(), self.errors.clone());
                self.backend.query(Query::ReadFile { path }, ctx, move |r: Result<FileText, String>| match r {
                    Ok(f) => slot.lock().unwrap().push(f),
                    Err(e) => Ide::fail(&errors, e),
                });
            }
        }
        self.watch_files(ctx, now);
        let reread: Vec<FileText> = std::mem::take(&mut *self.reread.lock().unwrap());
        // What changed on disk: shown at once where nothing here is unsaved;
        // where something is, kept aside for the strip to ask about - the
        // agent or a terminal wrote the file while it was being edited, and
        // the next save would have overwritten that without a word.
        for f in reread {
            let Some(d) = self.docs.iter_mut().find(|d| d.path == f.path) else { continue };
            if f.text == d.saved {
                continue;
            }
            if !d.dirty() {
                d.text = f.text.clone();
                d.saved = f.text;
            } else if f.text == d.text {
                d.saved = f.text;
                d.disk = None;
            } else {
                d.disk = Some(f.text);
            }
        }
        // Files read for the editor become tabs.
        let opened: Vec<FileText> = std::mem::take(&mut *self.opened.lock().unwrap());
        // A file opened on a phone is shown, not the tree it was picked in.
        if !opened.is_empty() {
            self.phone_tree = false;
        }
        for f in opened {
            match self.docs.iter().position(|d| d.path == f.path) {
                Some(i) => self.cur = i,
                None => {
                    let lang = Lang::of_path(&f.path);
                    self.new_doc(&f.path, &f.text, lang);
                }
            }
        }
    }

    // ── source ─────────────────────────────────────────────────────────────

    pub fn new_script(&mut self, lang: Lang) {
        let n = self.serial + 1;
        let (ext, text) = match lang {
            Lang::R => ("R", ""),
            Lang::Python => ("py", ""),
            Lang::Shell => ("sh", STARTER_SH),
            Lang::Julia => ("jl", STARTER_JL),
        };
        self.new_doc(&format!("Untitled{n}.{ext}"), text, Some(lang));
        self.show_lower(Lower::Files);
    }

    /// File › New File › Quarto Document (`quarto`) or R Markdown.
    pub fn new_document(&mut self, quarto: bool) {
        let n = self.serial + 1;
        let (ext, text) = if quarto { ("qmd", STARTER_QMD) } else { ("Rmd", STARTER_RMD) };
        self.new_doc(&format!("Untitled{n}.{ext}"), text, None);
        self.show_lower(Lower::Files);
    }

    /// A data viewer tab for `t`, in place of one already showing it.
    fn show_table(&mut self, lang: Lang, t: TableView) {
        self.show_lower(Lower::Files);
        let path = format!("View: {} ({})", t.name, lang.word());
        match self.docs.iter().position(|d| d.path == path) {
            Some(i) => {
                self.docs[i].table = Some((lang, t));
                self.cur = i;
            }
            None => {
                self.new_doc(&path, "", None);
                let d = self.docs.last_mut().unwrap();
                d.table = Some((lang, t));
            }
        }
    }

    /// The data viewer: ask `lang` for `name` as a table.
    pub fn view(&self, lang: Lang, name: &str, ctx: &egui::Context) {
        let errors = self.errors.clone();
        self.backend.query(Query::View { lang, name: name.to_owned() }, ctx, move |r: Result<KernelView, String>| {
            if let Err(e) = r {
                Ide::fail(&errors, format!("View {e}"));
            }
        });
    }

    pub fn close_current(&mut self) {
        if !self.docs.is_empty() {
            self.close(self.cur);
        }
    }

    fn close(&mut self, i: usize) {
        self.docs.remove(i);
        if i < self.cur {
            self.cur -= 1;
        }
        self.cur = self.cur.min(self.docs.len().saturating_sub(1));
        self.save_as = None;
    }

    /// Code › Run Selected Line(s) (`all` = false) or Source File (true). In a
    /// document `all` runs the chunk at the cursor, in its own language.
    pub fn run_current(&mut self, ctx: &egui::Context, all: bool) {
        let Some(doc) = self.docs.get(self.cur) else { return };
        if doc.table.is_some() {
            return;
        }
        if doc.document {
            return self.run_in_document(ctx, all);
        }
        let Some(lang) = doc.lang else {
            Ide::fail(&self.errors, "this file is not R, Python or shell — save it as .R, .py or .sh to run it".into());
            return;
        };
        let path = (!doc.untitled()).then(|| doc.path.clone());
        let (code, line) = if all { (doc.text.clone(), 1) } else { self.code_at_cursor(ctx, lang) };
        if !code.trim().is_empty() {
            // From a saved R file: R keeps the file and its lines (kernel.rs
            // eval_at), so its debugger stops are shown in this file.
            self.run_at = path.filter(|_| lang == Lang::R).map(|p| (p, line));
            self.send(lang, code, ctx);
        }
    }

    /// Ctrl/Cmd+F: the find bar open, its box focused, with what is selected
    /// (one line of it) to look for.
    fn open_find(&mut self, ctx: &egui::Context) {
        let doc = &self.docs[self.cur];
        if let Some(r) = egui::text_edit::TextEditState::load(ctx, doc.id).and_then(|s| s.cursor.char_range()) {
            let [a, b] = r.sorted_cursors();
            let (a, b) = (usize::from(a.index), usize::from(b.index));
            let sel: String = doc.text.chars().skip(a).take(b - a).collect();
            if !sel.is_empty() && !sel.contains('\n') {
                self.find.query = sel;
            }
        }
        self.find.open = true;
        self.find.focus = true;
    }

    /// The find bar over the editor: the query, case and regex, "n of m",
    /// previous and next (Enter, Shift+Enter), Esc closes. Returns the
    /// matches as (line, column, length) in characters, 1-based lines, for
    /// the editor to mark; the current one is selected and scrolled to.
    fn find_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) -> Vec<(usize, usize, usize)> {
        let doc_id = self.docs[self.cur].id;
        let text = self.docs[self.cur].text.clone();
        let matcher = crate::find::Matcher::new(&self.find.query, self.find.case, self.find.regex);
        let spans = matcher.as_ref().map(|m| m.find_all(&text, 5000)).unwrap_or_default();
        let (mut step, mut close) = (0i32, false);
        let box_id = egui::Id::new("editor-find");
        egui::Frame::new().fill(theme::surface()).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.find.query).id(box_id).hint_text("find").desired_width(220.0));
                if self.find.focus {
                    r.request_focus();
                    self.find.focus = false;
                }
                if r.changed() {
                    self.find.at = 0;
                    step = 0;
                }
                if r.has_focus() || r.lost_focus() {
                    if ui.input(|i| i.key_pressed(Key::Enter)) {
                        step = if ui.input(|i| i.modifiers.shift) { -1 } else { 1 };
                        r.request_focus();
                    }
                    if ui.input(|i| i.key_pressed(Key::Escape)) {
                        close = true;
                    }
                }
                ui.toggle_value(&mut self.find.case, RichText::new("Aa").monospace().size(11.5)).on_hover_text("match case");
                ui.toggle_value(&mut self.find.regex, RichText::new(".*").monospace().size(11.5)).on_hover_text("regular expression");
                let count = match &matcher {
                    Err(e) if !self.find.query.is_empty() => RichText::new(e).size(11.0).color(theme::alt()),
                    _ if spans.is_empty() => RichText::new(if self.find.query.is_empty() { "" } else { "no matches" }).size(11.0).color(theme::muted()),
                    _ => RichText::new(format!("{} of {}{}", self.find.at.min(spans.len() - 1) + 1, spans.len(), if spans.len() >= 5000 { "+" } else { "" })).size(11.0).color(theme::muted()),
                };
                ui.label(count);
                if ui.small_button("↑").on_hover_text("previous · Shift+Enter").clicked() {
                    step = -1;
                }
                if ui.small_button("↓").on_hover_text("next · Enter").clicked() {
                    step = 1;
                }
                if ui.small_button("✕").on_hover_text("close · Esc").clicked() {
                    close = true;
                }
            });
        });
        if close {
            self.find.open = false;
            ctx.memory_mut(|m| m.request_focus(doc_id));
            return Vec::new();
        }
        if spans.is_empty() {
            return Vec::new();
        }
        let n = spans.len();
        self.find.at = self.find.at.min(n - 1);
        if step != 0 {
            self.find.at = (self.find.at as i64 + step as i64).rem_euclid(n as i64) as usize;
            // The match selected in the editor, and in view.
            let (a, b) = spans[self.find.at];
            let ca = text[..a].chars().count();
            let cb = ca + text[a..b].chars().count();
            let mut st = egui::text_edit::TextEditState::load(ctx, doc_id).unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(ca), egui::text::CCursor::new(cb))));
            st.store(ctx, doc_id);
            self.scroll_line = Some(text[..a].matches('\n').count() + 1);
        }
        spans
            .iter()
            .map(|&(a, b)| {
                let line_start = text[..a].rfind('\n').map_or(0, |p| p + 1);
                (text[..a].matches('\n').count() + 1, text[line_start..a].chars().count(), text[a..b].chars().count())
            })
            .collect()
    }

    /// Find in files: a window over the IDE - the query, case and regex, and
    /// every matching line under the open domain (or the Files folder), as
    /// path:line text; a click opens the file at that line.
    fn find_in_files(&mut self, ctx: &egui::Context) {
        if !self.grep.open {
            return;
        }
        let root = self.domain.as_ref().map(|d| d.path.clone()).filter(|p| !p.is_empty()).unwrap_or_else(|| self.files_path.clone());
        let mut open = true;
        let mut ask = false;
        let mut hit: Option<(String, usize)> = None;
        let result = self.grep.result.lock().unwrap().clone();
        let busy = self.grep.busy.load(Ordering::Relaxed);
        egui::Window::new("Find in files").id(egui::Id::new("find-in-files")).open(&mut open).default_size([640.0, 420.0]).resizable(true).collapsible(false).show(ctx, |ui| {
            ui.horizontal(|ui| {
                let r = ui.add(egui::TextEdit::singleline(&mut self.grep.query).hint_text("find in files").desired_width(260.0));
                if self.grep.focus {
                    r.request_focus();
                    self.grep.focus = false;
                }
                ask |= r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.toggle_value(&mut self.grep.case, RichText::new("Aa").monospace().size(11.5)).on_hover_text("match case");
                ui.toggle_value(&mut self.grep.regex, RichText::new(".*").monospace().size(11.5)).on_hover_text("regular expression");
                ask |= ui.add_enabled(!busy && !self.grep.query.is_empty(), theme::primary_button("Find")).clicked();
                if busy {
                    ui.spinner();
                }
            });
            ui.label(RichText::new(if root.is_empty() { "open a domain, or a folder in Files, to search it".to_owned() } else { format!("in {root}") }).size(11.0).color(theme::hush()));
            ui.separator();
            match &result {
                None => {}
                Some(Err(e)) => {
                    ui.label(RichText::new(e).size(12.0).color(theme::alt()));
                }
                Some(Ok(v)) => {
                    let files: std::collections::BTreeSet<&str> = v.hits.iter().map(|h| h.path.as_str()).collect();
                    ui.label(RichText::new(format!("{} lines in {} files · {} files read{}", v.hits.len(), files.len(), v.files, if v.truncated { " · stopped at the limit" } else { "" })).size(11.0).color(theme::muted()));
                    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                        let mut last = "";
                        for h in &v.hits {
                            if h.path != last {
                                last = &h.path;
                                let shown = h.path.strip_prefix(&format!("{}/", v.root)).unwrap_or(&h.path);
                                ui.label(RichText::new(shown).monospace().size(11.5).strong().color(theme::text_strong()));
                            }
                            let row = format!("{:>5}  {}", h.line, h.text.trim_end());
                            let r = ui.add(egui::Label::new(RichText::new(row).monospace().size(11.5).color(theme::text())).truncate().sense(egui::Sense::click()));
                            if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                hit = Some((h.path.clone(), h.line));
                            }
                        }
                    });
                }
            }
        });
        self.grep.open = open;
        if ask && !root.is_empty() && !self.grep.query.is_empty() {
            self.grep.busy.store(true, Ordering::Relaxed);
            let (slot, flag) = (self.grep.result.clone(), self.grep.busy.clone());
            let q = Query::Grep { root, query: self.grep.query.clone(), case: self.grep.case, regex: self.grep.regex };
            self.backend.query(q, ctx, move |r: Result<api::GrepView, String>| {
                *slot.lock().unwrap() = Some(r);
                flag.store(false, Ordering::Relaxed);
            });
        }
        if let Some((path, line)) = hit {
            self.open(path.clone(), ctx);
            self.goto = Some((path, line));
            self.show_lower(Lower::Files);
        }
    }

    /// R's debugger stopped (KernelView.debug_*): its file opens in the
    /// editor with the line marked, as RStudio does (operator, 2026-10-19:
    /// "browser needs to be sure to be able to access the file and highlight
    /// the location being debugged"). Code typed in the console has no file:
    /// the stopped function's own source opens instead, read-only, marked.
    fn follow_debugger(&mut self, ctx: &egui::Context) {
        let v = self.consoles[ix(Lang::R)].view.lock().unwrap().clone_debug();
        if v.debug != "R" || v.debug_line == 0 {
            self.debug_mark = None;
            self.debug_seen = None;
            return;
        }
        let (path, line) = if !v.debug_file.is_empty() {
            (v.debug_file.clone(), v.debug_line)
        } else if !v.debug_src.is_empty() && v.debug_line >= v.debug_from {
            (DEBUG_DOC.to_owned(), v.debug_line - v.debug_from + 1)
        } else {
            self.debug_mark = None;
            return;
        };
        self.debug_mark = Some((path.clone(), line));
        if self.debug_seen.as_ref() == Some(&(path.clone(), line)) {
            return;
        }
        let first_stop = self.debug_seen.as_ref().is_none_or(|(p, _)| *p != path);
        self.debug_seen = Some((path.clone(), line));
        if path == DEBUG_DOC {
            let text = v.debug_src.join("\n");
            match self.docs.iter().position(|d| d.path == DEBUG_DOC) {
                Some(i) => {
                    self.docs[i].text = text.clone();
                    self.docs[i].saved = text;
                    self.cur = i;
                }
                None => {
                    self.new_doc(DEBUG_DOC, &text, Some(Lang::R));
                    let i = self.docs.len() - 1;
                    self.docs[i].saved = text;
                }
            }
        } else {
            self.open(path, ctx);
        }
        if first_stop {
            self.show_lower(Lower::Files);
        }
    }

    /// The chunk the cursor is in: its line or selection (`chunk` = false) or
    /// the whole chunk, sent to that chunk's language.
    fn run_in_document(&mut self, ctx: &egui::Context, chunk: bool) {
        use egui::text::{CCursor, CCursorRange};
        let doc = &self.docs[self.cur];
        let mut state = egui::text_edit::TextEditState::load(ctx, doc.id).unwrap_or_default();
        let range = state.cursor.char_range().unwrap_or_else(|| CCursorRange::one(CCursor::new(0)));
        let [a, b] = range.sorted_cursors();
        let (a, b) = (usize::from(a.index), usize::from(b.index));
        let line = line_of(&doc.text, a);
        let all = chunks(&doc.text);
        let Some(c) = all.iter().find(|c| (c.body..c.end.max(c.body + 1)).contains(&line) || c.header == line) else {
            Ide::fail(&self.errors, "the cursor is not in a code chunk — Run All runs every chunk".into());
            return;
        };
        let Some(lang) = c.lang else {
            Ide::fail(&self.errors, format!("{} chunks have no console here — R, Python and bash run", c.engine));
            return;
        };
        let code = if chunk {
            chunk_code(&doc.text, c)
        } else if a != b {
            doc.text.chars().skip(a).take(b - a).collect()
        } else {
            // the statement at the cursor, within the chunk
            let body = chunk_code(&doc.text, c);
            let base = line_start(&doc.text, c.body);
            let (code, next) = statement_at(&body, a.saturating_sub(base).min(body.chars().count()), lang);
            state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(base + next))));
            state.store(ctx, doc.id);
            code
        };
        if !code.trim().is_empty() {
            self.send(lang, code, ctx);
        }
    }

    /// Every chunk of the open document, top to bottom, each in its language
    /// (`eval: false` ones left out, as render leaves them).
    pub fn run_all_chunks(&mut self, ctx: &egui::Context) {
        let Some(doc) = self.docs.get(self.cur).filter(|d| d.document) else { return };
        let text = doc.text.clone();
        let runs: Vec<(Lang, String)> = chunks(&text).into_iter().filter(|c| c.eval).filter_map(|c| Some((c.lang?, chunk_code(&text, &c)))).collect();
        if runs.is_empty() {
            Ide::fail(&self.errors, "no R, Python or bash chunks to run".into());
        }
        for (lang, code) in runs {
            if !code.trim().is_empty() {
                self.send(lang, code, ctx);
            }
        }
    }

    /// Session › Clear Workspace.
    pub fn clear_workspace(&mut self, ctx: &egui::Context) {
        let lang = self.data_lang();
        // R's objects and reticulate's Python's: one session, one workspace.
        let code = match lang {
            Lang::R | Lang::Python => ".ndexr_clear()",
            Lang::Shell | Lang::Julia => return,
        };
        self.send(lang, code.into(), ctx);
    }

    /// quarto render the open document (saved first), shown in the Viewer.
    pub fn render_current(&mut self, ctx: &egui::Context) {
        let Some(doc) = self.docs.get(self.cur) else { return };
        if !api::renders(&doc.path) {
            Ide::fail(&self.errors, "Render takes .qmd, .Rmd, .ipynb, .md, .html, _quarto.yml, _toc.yml or mkdocs.yml".into());
            return;
        }
        if doc.untitled() {
            self.save(ctx);
            Ide::fail(&self.errors, "save the document first - then Render".into());
            return;
        }
        let (path, text, dirty) = (doc.path.clone(), doc.text.clone(), doc.dirty());
        self.show_lower(Lower::Viewer);
        let (slot, errors) = (self.render.clone(), self.errors.clone());
        *slot.lock().unwrap() = JobView { what: format!("render {}", path.rsplit('/').next().unwrap_or(&path)), running: true, ..Default::default() };
        self.render_asked = f64::NEG_INFINITY;
        let failed = |slot: &Arc<Mutex<JobView>>, errors: &Arc<Mutex<Vec<String>>>, e: String| {
            let mut v = slot.lock().unwrap();
            v.running = false;
            v.ok = Some(false);
            v.lines.push(e.clone());
            Ide::fail(errors, format!("render: {e}"));
        };
        let (backend, ctx2) = (self.backend.clone(), ctx.clone());
        let go = move |slot: Arc<Mutex<JobView>>, errors: Arc<Mutex<Vec<String>>>| {
            backend.query(Query::Render { path }, &ctx2, move |r: Result<JobView, String>| match r {
                Ok(v) => *slot.lock().unwrap() = v,
                Err(e) => failed(&slot, &errors, e),
            });
        };
        if !dirty {
            return go(slot, errors);
        }
        // Saved first, and only then rendered: the two would otherwise race.
        self.docs[self.cur].saved = text.clone();
        let p = self.docs[self.cur].path.clone();
        self.backend.query(Query::WriteFile { path: p, text }, ctx, move |r: Result<FileText, String>| match r {
            Ok(_) => go(slot, errors),
            Err(e) => failed(&slot, &errors, format!("not saved: {e}")),
        });
    }

    pub fn save_current(&mut self, ctx: &egui::Context) {
        if !self.docs.is_empty() {
            self.save(ctx);
        }
    }

    /// The Source pane, file tabs included. Returns true when its zoom
    /// control was clicked.
    /// `back`: a phone's files tab, where the tree and the file take turns -
    /// a "‹" at the start of the tab row goes back to the tree (it had a row
    /// of its own, "← files": operator screenshot, 2026-11-07).
    pub fn source(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, zoomed: bool, back: bool) -> bool {
        let (mut zoom, mut pick, mut close, mut new, mut new_doc) = (false, None, None, None, None);
        // The open files scroll sideways when they outrun the strip, the one
        // in front kept in view; + and zoom stay pinned at the right. A
        // narrow pane cuts names shorter. Each tab's light: its kind's
        // colour, amber while it has unsaved edits.
        let narrow = ui.available_width() < 480.0;
        let cur = self.cur;
        let follow = self.tab_follow != Some(cur);
        self.tab_follow = Some(cur);
        theme::strip(ui, theme::TAB_H, theme::pane_hdr(), |ui| {
            if back && ui.add(egui::Button::new(RichText::new("‹").size(18.0)).min_size(egui::vec2(32.0, 0.0))).on_hover_text("the files").clicked() {
                self.phone_tree = true;
            }
            let room = (ui.available_width() - 56.0).max(40.0);
            egui::ScrollArea::horizontal().id_salt("file-tabs").max_width(room).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for (i, d) in self.docs.iter().enumerate() {
                        let name = d.name();
                        let label = middle_cut(&name, if narrow { 14 } else { 28 });
                        let dot = if d.dirty() { theme::amber() } else if d.table.is_some() { theme::amber().gamma_multiply(0.6) } else { file_glyph(&name, false).1 };
                        let (r, closed) = theme::tab(ui, &label, i == cur, Some(dot), true);
                        if i == cur && follow {
                            r.scroll_to_me(None);
                        }
                        let tip = if d.dirty() { format!("{} · unsaved", d.path) } else { d.path.clone() };
                        if r.on_hover_text(tip).clicked() {
                            pick = Some(i);
                        }
                        if closed {
                            close = Some(i);
                        }
                    }
                });
            });
            ui.add_space(4.0);
            if let Some(n) = new_file_menu(ui, RichText::new("+").size(14.0).color(theme::muted())) {
                match n {
                    NewFile::Script(l) => new = Some(l),
                    NewFile::Document(quarto) => new_doc = Some(quarto),
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| zoom = theme::zoom_button(ui, zoomed));
        });
        if let Some(i) = pick {
            self.cur = i;
            self.save_as = None;
        }
        if let Some(i) = close {
            self.close(i);
        }
        if let Some(l) = new {
            self.new_script(l);
        }
        if let Some(q) = new_doc {
            self.new_document(q);
        }
        if self.docs.is_empty() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                let hint = if self.domain.is_some() { "No file open — open one from Files, or File › New File." } else { "Pick your domain in App (bottom right) — its files open here." };
                ui.label(RichText::new(hint).color(theme::hush()))
            });
            return zoom;
        }

        if self.docs[self.cur].table.is_some() {
            self.table_view(ui, ctx);
            return zoom;
        }
        // The agent is Files' third section, under the tree and the file
        // (files_tab, request #166), no longer a bar inside the editor.
        if self.docs[self.cur].preview {
            self.preview_view(ui, ctx);
            return zoom;
        }

        let (mut run_line, mut run_all, mut save, mut run_chunks, mut render) = (false, false, false, false, false);
        let id = self.docs[self.cur].id;
        // Ctrl/Cmd+F: the find bar, with the selection in it.
        if ui.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::F))) {
            self.open_find(ctx);
        }
        // A Find in Files hit: the cursor to its line once its file is in front.
        if let Some((p, line)) = self.goto.clone() {
            if p == self.docs[self.cur].path {
                self.goto = None;
                let text = &self.docs[self.cur].text;
                let at = text.split('\n').take(line.saturating_sub(1)).map(|l| l.chars().count() + 1).sum::<usize>();
                let mut st = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
                st.store(ctx, id);
                ctx.memory_mut(|m| m.request_focus(id));
                self.scroll_line = Some(line);
            }
        }
        let mut ask_complete = false;
        if ui.memory(|m| m.has_focus(id)) {
            // The completion list's keys first: Enter takes a choice there.
            ask_complete = self.edc.keys(ui, id);
            self.smart_keys(ui, ctx, id);
            run_all = take_key(ui, Key::Enter, |m| m.command && m.shift);
            run_line = !run_all && take_key(ui, Key::Enter, |m| m.command);
            save = take_key(ui, Key::S, |m| m.command);
            // RStudio: Ctrl+Alt+R runs every chunk, Ctrl+Shift+K renders.
            run_chunks = take_key(ui, Key::R, |m| m.command && m.alt);
            render = take_key(ui, Key::K, |m| m.command && m.shift);
        }

        let lang = self.docs[self.cur].lang;
        let document = self.docs[self.cur].document;
        let renders = api::renders(&self.docs[self.cur].path);
        let can_run = self.docs[self.cur].runnable();
        let rendering = self.render.lock().unwrap().running;
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(4.0);
            save |= theme::tool(ui, "Save", "save the file · Ctrl+S").clicked();
            theme::tool_sep(ui);
            if theme::tool(ui, "Find", "find in this file · Ctrl+F").clicked() {
                self.find.open = !self.find.open;
                self.find.focus = self.find.open;
            }
            if theme::tool(ui, "in files", "find in every file of the domain · Ctrl+Shift+F").clicked() {
                self.grep.open = true;
                self.grep.focus = true;
            }
            if renders {
                theme::tool_sep(ui);
                render |= theme::tool_if(ui, !rendering, "Render", "render it (quarto, jupyter-book, mkdocs), shown in the Viewer · Ctrl+Shift+K").clicked();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                if document {
                    run_chunks |= theme::tool(ui, "Run All", "every chunk, top to bottom, each in its own console · Ctrl+Alt+R").clicked();
                    run_all |= theme::tool(ui, "Run Chunk", "the chunk at the cursor · Ctrl+Shift+Enter").clicked();
                    run_line |= theme::tool(ui, "▶ Run", "the line or selection, in the chunk's language · Ctrl+Enter").clicked();
                } else {
                    run_all |= theme::tool_if(ui, can_run, "Source", "run the whole file · Ctrl+Shift+Enter").clicked();
                    run_line |= theme::tool_if(ui, can_run, "▶ Run", "run the line or selection · Ctrl+Enter").clicked();
                }
                // The kind of file, where there is room for it: on a phone it
                // was drawn over Render.
                if ui.available_width() < 120.0 {
                    return;
                }
                theme::tool_sep(ui);
                let kind = if document {
                    if self.docs[self.cur].path.to_ascii_lowercase().ends_with(".qmd") { "Quarto" } else { "R Markdown" }
                } else {
                    lang.map(|l| l.word()).or_else(|| file_kind(&self.docs[self.cur]).map(crate::syntax::Kind::label)).unwrap_or("Text")
                };
                ui.label(RichText::new(kind).size(11.5).color(theme::hush()));
            });
        });

        if let Some(path) = &mut self.save_as {
            let (mut go, mut cancel) = (false, false);
            theme::strip(ui, theme::TOOL_H + 4.0, theme::pane_hdr(), |ui| {
                ui.add_space(6.0);
                ui.label(RichText::new("Save as").size(11.5).color(theme::muted()));
                let r = ui.add(egui::TextEdit::singleline(path).font(egui::TextStyle::Monospace).desired_width(ui.available_width() - 110.0));
                go = r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                go |= ui.add(theme::primary_button("Save")).clicked();
                cancel = ui.small_button("Cancel").clicked();
            });
            if cancel {
                self.save_as = None;
            } else if go {
                let p = path.trim().to_owned();
                self.save_as = None;
                let d = &mut self.docs[self.cur];
                d.path = p;
                d.document = is_document(&d.path);
                d.lang = if d.document { None } else { Lang::of_path(&d.path).or(lang) };
                save = true;
            }
        }

        // Stopped in R's debugger: its buttons over the file too, so stepping
        // and the marked line are side by side.
        self.debug_bar(ui, ctx);
        self.disk_bar(ui);
        let found = if self.find.open { self.find_bar(ui, ctx) } else { Vec::new() };
        let current = self.find.at;
        let scroll_line = self.scroll_line.take();
        let mark = self.debug_mark.clone().filter(|(p, _)| *p == self.docs[self.cur].path).map(|(_, l)| l);
        let scroll_to_mark = mark.is_some() && self.debug_scrolled != self.debug_mark;
        if scroll_to_mark {
            self.debug_scrolled = self.debug_mark.clone();
        }

        // The editor: line numbers, then the text, in ndexr Dark. No soft
        // wrap (RStudio's default), so the gutter stays in step with the rows.
        let Ide { docs, cur, memo, edc, backend, .. } = self;
        let doc = &mut docs[*cur];
        let doc_lang = doc.lang;
        let doc_document = doc.document;
        let doc_kind = file_kind(doc);
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| {
            let text = buf.as_str();
            let key = {
                use std::hash::{Hash, Hasher};
                let mut h = std::collections::hash_map::DefaultHasher::new();
                text.hash(&mut h);
                doc_lang.map(ix).hash(&mut h);
                doc_document.hash(&mut h);
                doc_kind.hash(&mut h);
                h.finish()
            };
            let entry = memo.entry(doc.id).or_insert_with(|| (key.wrapping_add(1), LayoutJob::default()));
            if entry.0 != key {
                *entry = (key, match doc_kind {
                    _ if doc_document => highlight_doc(text),
                    Some(k) => highlight_kind(text, k),
                    None => highlight(text, doc_lang),
                });
            }
            let mut job = entry.1.clone();
            job.wrap.max_width = f32::INFINITY;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let rows = doc.text.split('\n').count();
        let width = rows.to_string().len().max(2);
        let gutter = (1..=rows).map(|n| format!("{n:>width$}")).collect::<Vec<_>>().join("\n");
        egui::Frame::new().fill(ace::bg()).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::both().id_salt(("editor", doc.id)).auto_shrink([false, false]).show(ui, |ui| {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    egui::Frame::new().inner_margin(egui::Margin { left: 8, right: 4, top: 4, bottom: 4 }).show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(gutter).font(FontId::monospace(EDITOR_FONT)).color(ace::comment())).selectable(false).wrap_mode(egui::TextWrapMode::Extend));
                    });
                    let te = ui.add(
                        egui::TextEdit::multiline(&mut doc.text)
                            .id(doc.id)
                            .code_editor()
                            .frame(egui::Frame::NONE)
                            .margin(egui::Margin::symmetric(6, 4))
                            .background_color(ace::bg())
                            .desired_width(f32::INFINITY)
                            .desired_rows(30)
                            .lock_focus(true)
                            .layouter(&mut layouter),
                    );
                    let row_h = ui.fonts_mut(|f| f.row_height(&FontId::monospace(EDITOR_FONT)));
                    let char_w = ui.fonts_mut(|f| f.glyph_width(&FontId::monospace(EDITOR_FONT), 'x'));
                    let here = crate::edcomplete::Here {
                        id: doc.id,
                        path: &doc.path,
                        text: &mut doc.text,
                        document: doc.document,
                        lang: doc.lang,
                        origin: egui::pos2(te.rect.left() + 6.0, te.rect.top() + 4.0),
                        char_w,
                        row_h,
                    };
                    edc.after(ui, backend, here, ask_complete);
                    let row_rect = |line: usize| egui::Rect::from_min_size(egui::pos2(te.rect.left(), te.rect.top() + 4.0 + row_h * line.saturating_sub(1) as f32), egui::vec2(te.rect.width(), row_h));
                    // Find's matches: each one marked, the current one strong.
                    for (k, (line, col, len)) in found.iter().enumerate() {
                        let r = egui::Rect::from_min_size(egui::pos2(te.rect.left() + 6.0 + char_w * *col as f32, te.rect.top() + 4.0 + row_h * (*line - 1) as f32), egui::vec2((char_w * *len as f32).max(2.0), row_h));
                        let on = k == current;
                        ui.painter().rect_filled(r, 2.0, theme::primary().gamma_multiply(if on { 0.45 } else { 0.18 }));
                        if on {
                            ui.painter().rect_stroke(r, 2.0, egui::Stroke::new(1.0, theme::primary()), egui::StrokeKind::Outside);
                        }
                    }
                    if let Some(line) = scroll_line {
                        ui.scroll_to_rect(row_rect(line), Some(egui::Align::Center));
                    }
                    // The line R's debugger is stopped on: RStudio's yellow
                    // band and arrow, scrolled into view once per stop.
                    if let Some(line) = mark {
                        let row_h = ui.fonts_mut(|f| f.row_height(&FontId::monospace(EDITOR_FONT)));
                        let y = te.rect.top() + 4.0 + row_h * (line.saturating_sub(1)) as f32;
                        let band = egui::Rect::from_min_size(egui::pos2(te.rect.left() - 40.0, y), egui::vec2(te.rect.width() + 40.0, row_h));
                        ui.painter().rect_filled(band, 0.0, theme::amber().gamma_multiply(0.22));
                        ui.painter().text(egui::pos2(te.rect.left() - 4.0, y + row_h / 2.0), egui::Align2::RIGHT_CENTER, "▶", FontId::monospace(EDITOR_FONT * 0.8), theme::amber());
                        if scroll_to_mark {
                            ui.scroll_to_rect(band, Some(egui::Align::Center));
                        }
                    }
                });
            });
        });

        if save {
            self.save(ctx);
        }
        if run_line || run_all {
            self.run_current(ctx, run_all);
        }
        if run_chunks {
            self.run_all_chunks(ctx);
        }
        if render {
            self.render_current(ctx);
        }
        zoom
    }

    /// The open file changed on disk while it had edits here: which to keep.
    /// Theirs replaces the tab's text; mine stays, and the next save writes
    /// it over what is on disk.
    fn disk_bar(&mut self, ui: &mut egui::Ui) {
        let Some(disk) = self.docs[self.cur].disk.clone() else { return };
        let (mut theirs, mut mine) = (false, false);
        theme::strip(ui, theme::TOOL_H + 6.0, theme::amber().gamma_multiply(0.18), |ui| {
            ui.add_space(8.0);
            ui.label(RichText::new("This file changed on disk while you were editing it.").size(13.0).color(theme::text_strong()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                mine = ui.button("Keep mine").on_hover_text("keep your edits; saving writes them over the file on disk").clicked();
                theirs = ui.add(theme::primary_button("Use the file on disk")).on_hover_text("drop your unsaved edits and show what is on disk now").clicked();
            });
        });
        let d = &mut self.docs[self.cur];
        if theirs {
            d.text = disk.clone();
            d.saved = disk;
            d.disk = None;
        } else if mine {
            d.saved = disk;
            d.disk = None;
        }
    }

    /// The file ask bar at the foot of the open file (agent.rs FileAsk). An
    /// ask saves the file first when it has unsaved edits, so the agent
    /// starts from what is on screen.
    /// The keys that depend on what is being written (editkeys.rs): Tab and
    /// Shift+Tab, Enter, the pairs, closing tags, Backspace in a pair. Taken
    /// before the editor sees them; the completion list has already had its
    /// Tab and Enter when it is open (edcomplete.rs `keys`), so with a list up
    /// Tab takes the choice and never indents, and without one it never
    /// takes a suggestion.
    fn smart_keys(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, id: egui::Id) {
        use crate::editkeys as ek;
        let list = self.edc.open();
        let doc = &self.docs[self.cur];
        if doc.table.is_some() || doc.preview {
            return;
        }
        let Some(range) = egui::text_edit::TextEditState::load(ctx, id).and_then(|s| s.cursor.char_range()) else { return };
        let (anchor, cursor): (usize, usize) = (range.secondary.index.into(), range.primary.index.into());
        let kind = file_kind(doc);
        let unit = ek::indent_unit(&doc.text, kind);
        // The live snippet follows what is typed into its placeholder.
        if let Some((sid, s)) = &mut self.snippet {
            if *sid == id {
                s.follow(&doc.text);
            } else {
                self.snippet = None;
            }
        }
        let text = doc.text.clone();
        let snippet = &mut self.snippet;
        let mut out: Option<ek::Edit> = None;
        let mut jump: Option<usize> = None;
        ui.input_mut(|i| {
            i.events.retain(|e| {
                if out.is_some() || jump.is_some() {
                    return true;
                }
                match e {
                    egui::Event::Key { key: Key::Escape, pressed: true, .. } => {
                        *snippet = None;
                        true
                    }
                    egui::Event::Key { key: Key::Tab, pressed: true, modifiers, .. } if !list && !modifiers.command && !modifiers.alt => {
                        if let Some((_, s)) = snippet.as_mut() {
                            match s.step(!modifiers.shift) {
                                Some(m) => {
                                    jump = Some(m);
                                    return false;
                                }
                                None => *snippet = None,
                            }
                        }
                        out = Some(if modifiers.shift {
                            ek::shift_lines(&text, anchor, cursor, &unit, false)
                        } else {
                            kind.and_then(|k| ek::emmet(&text, anchor, cursor, k, &unit)).unwrap_or_else(|| ek::tab(&text, anchor, cursor, &unit))
                        });
                        false
                    }
                    egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if !list && modifiers.is_none() => {
                        out = Some(ek::enter(&text, anchor, cursor, &unit, kind));
                        false
                    }
                    egui::Event::Key { key: Key::Backspace, pressed: true, modifiers, .. } if modifiers.is_none() => match ek::backspace(&text, anchor, cursor) {
                        Some(e) => {
                            out = Some(e);
                            false
                        }
                        None => true,
                    },
                    egui::Event::Text(t) if t.chars().count() == 1 => match ek::typed(&text, anchor, cursor, t.chars().next().unwrap_or(' '), kind) {
                        Some(e) => {
                            out = Some(e);
                            false
                        }
                        None => true,
                    },
                    _ => true,
                }
            });
        });
        let set = |a: usize, c: usize| {
            let mut st = egui::text_edit::TextEditState::load(ctx, id).unwrap_or_default();
            let (lo, hi) = (a.min(c), a.max(c));
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(lo), egui::text::CCursor::new(hi))));
            st.store(ctx, id);
        };
        if let Some(m) = jump {
            set(m, m);
            ctx.request_repaint();
        }
        if let Some(e) = out {
            self.docs[self.cur].text = e.text.clone();
            set(e.anchor, e.cursor);
            if let Some(s) = ek::Snippet::new(e.marks.clone(), &e.text) {
                self.snippet = Some((id, s));
            }
            ctx.request_repaint();
        }
    }

    /// Files' agent section: about the open file - a real one, not an
    /// untitled script or a View() table - or the folder the tree is in.
    fn ask_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (file, folder) = self.ask_where();
        // What Listen has heard, over this composer as over Agents' and Go's.
        let heard = self.listen_status(ctx.input(|i| i.time));
        let mut asked = false;
        {
            let Ide { ask, backend, domain, .. } = self;
            egui::Panel::bottom(egui::Id::new("file-ask-bar"))
                .resizable(false)
                .show_separator_line(true)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    if let Some(h) = &heard {
                        draw_listen(ui, h, "files");
                    }
                    asked = ask.show(ui, ctx, backend, domain.as_ref(), file.as_deref(), &folder);
                });
        }
        if !asked {
            return;
        }
        if file.is_some() && self.docs[self.cur].dirty() {
            self.save(ctx);
        }
        if let Some(host) = self.domain.as_ref().map(|d| d.host.clone()) {
            let Ide { ask, backend, .. } = self;
            ask.start(backend, ctx, &host, file.as_deref(), &folder);
        }
    }

    /// An image, PDF, video or sound, shown in place (operator, 2026-10-14:
    /// "images, pdfs, etc when clicked render in place to show their
    /// content"). In a browser it is a frame over the editor's rectangle on
    /// the server's /preview, which the browser draws as it draws any such
    /// file; the desktop app decodes images itself and opens the rest.
    fn preview_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let path = self.docs[self.cur].path.clone();
        let kind = crate::api::preview_type(&path).unwrap_or("");
        // Changed on disk since it was first shown: a new address, so the
        // frame loads it again (watch_files).
        let base = self.backend.preview_url(&path);
        let url = match self.preview_seq.get(&path) {
            Some(n) if *n > 0 && base.contains('?') => format!("{base}&v={n}"),
            _ => base,
        };
        let download = self.backend.download_url(&path);
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new(kind).size(11.5).color(theme::hush()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                if theme::tool(ui, "⤓", "download this file").clicked() {
                    ctx.open_url(egui::OpenUrl::same_tab(&download));
                }
                if theme::tool(ui, "↗", "open it in a tab of its own").clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(&url));
                }
            });
        });
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, egui::Sense::hover());
        #[cfg(target_arch = "wasm32")]
        if !ctx.any_popup_open() {
            site_frame::place_preview(ctx, rect, &url);
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.preview_native(ui, ctx, rect, &path, kind, &url);
    }

    /// The desktop app's in-place view: images decoded here (fitted, aspect
    /// kept); a PDF, video or sound opens in the system's viewer.
    #[cfg(not(target_arch = "wasm32"))]
    fn preview_native(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, rect: egui::Rect, path: &str, kind: &str, url: &str) {
        let raster = kind.starts_with("image/") && kind != "image/svg+xml";
        if !raster {
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new(format!("{kind} - shown in place in the browser build (cove.ndexr.io).")).color(theme::muted()));
                    if ui.add(theme::primary_button("Open it")).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(url));
                    }
                });
            });
            return;
        }
        if !self.textures.contains_key(path) {
            let slot = self.images.entry(path.to_owned()).or_insert_with(|| {
                let slot: Arc<Mutex<Option<Result<egui::ColorImage, String>>>> = Default::default();
                let (s, c) = (slot.clone(), ctx.clone());
                self.backend.download(path, move |r| {
                    let img = r.and_then(|(_, bytes)| {
                        let i = image::load_from_memory(&bytes).map_err(|e| e.to_string())?.to_rgba8();
                        Ok(egui::ColorImage::from_rgba_unmultiplied([i.width() as usize, i.height() as usize], i.as_raw()))
                    });
                    *s.lock().unwrap() = Some(img);
                    c.request_repaint();
                });
                slot
            });
            let got = slot.lock().unwrap().take();
            match got {
                Some(Ok(img)) => {
                    let tex = ctx.load_texture(path, img, egui::TextureOptions::LINEAR);
                    self.textures.insert(path.to_owned(), tex);
                }
                Some(Err(e)) => {
                    self.images.remove(path);
                    ui.put(rect, egui::Label::new(RichText::new(e).color(theme::alt())));
                    return;
                }
                None => {
                    ui.put(rect, egui::Spinner::new());
                    return;
                }
            }
        }
        if let Some(tex) = self.textures.get(path) {
            ui.put(rect.shrink(8.0), egui::Image::new(tex).maintain_aspect_ratio(true).max_size(rect.shrink(8.0).size()));
        }
    }

    /// A View() tab: the table, its size, and a filter over its rows.
    fn table_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let doc = &mut self.docs[self.cur];
        let Some((lang, t)) = doc.table.clone() else { return };
        let mut refresh = false;
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(6.0);
            let shown = if t.nrow > t.rows.len() || t.ncol > t.cols.len() { format!(" · showing {} × {}", t.rows.len(), t.cols.len()) } else { String::new() };
            ui.label(RichText::new(format!("{} · {} obs. of {} variable{}{shown}", t.name, t.nrow, t.ncol, if t.ncol == 1 { "" } else { "s" })).size(11.5).color(theme::muted()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                refresh = theme::tool(ui, "⟳", &format!("ask {} again", lang.word())).clicked();
                ui.add(egui::TextEdit::singleline(&mut doc.filter).hint_text("filter rows").desired_width(140.0));
            });
        });
        if refresh {
            let name = t.name.clone();
            self.view(lang, &name, ctx);
        }
        let filter = self.docs[self.cur].filter.to_lowercase();
        let rows: Vec<&Vec<String>> = t.rows.iter().filter(|r| filter.is_empty() || r.iter().any(|v| v.to_lowercase().contains(&filter))).collect();
        // Each column as wide as its longest value, within reason.
        let cw = ui.fonts_mut(|f| f.glyph_width(&FontId::monospace(12.0), '0')).max(6.0);
        let widths: Vec<f32> = (0..t.cols.len().max(1))
            .map(|i| {
                let longest = t.rows.iter().take(200).map(|r| r.get(i).map_or(0, |v| v.chars().count())).max().unwrap_or(0);
                let head = t.cols.get(i).map_or(0, |c| c.chars().count()).max(t.types.get(i).map_or(0, |c| c.chars().count()));
                (longest.max(head).clamp(3, 40) as f32) * cw + 16.0
            })
            .collect();
        let num_w = ((rows.len().max(1)).to_string().len() as f32) * cw + 14.0;
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
        let cell = |ui: &mut egui::Ui, w: f32, text: RichText| {
            ui.add_sized([w, row_h], egui::Label::new(text).truncate());
        };
        egui::Frame::new().fill(ace::bg()).show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            egui::ScrollArea::horizontal().id_salt(("table-h", self.docs[self.cur].id)).auto_shrink([false, false]).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        cell(ui, num_w, RichText::new(""));
                        for (i, c) in t.cols.iter().enumerate() {
                            let ty = t.types.get(i).cloned().unwrap_or_default();
                            ui.add_sized([widths[i], row_h * 1.6], egui::Label::new(RichText::new(format!("{c}\n{ty}")).monospace().size(11.5).color(theme::text_strong())).truncate()).on_hover_text(format!("{c} · {ty}"));
                        }
                    });
                    ui.separator();
                    egui::ScrollArea::vertical().id_salt(("table-v", self.docs[self.cur].id)).auto_shrink([false, false]).show_rows(ui, row_h, rows.len(), |ui, range| {
                        for n in range {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                cell(ui, num_w, RichText::new((n + 1).to_string()).monospace().size(11.0).color(theme::hush()));
                                for (i, v) in rows[n].iter().enumerate() {
                                    let color = if v == "NA" || v == "nan" || v == "None" { theme::hush() } else { ace::text() };
                                    cell(ui, widths.get(i).copied().unwrap_or(80.0), RichText::new(v).monospace().size(12.0).color(color));
                                }
                            });
                        }
                    });
                });
            });
        });
    }

    /// The selection, or the statement at the cursor — then the cursor moves
    /// to the line after it, as RStudio's Ctrl+Enter does.
    /// The selection, or the statement at the cursor, and the line it
    /// starts on in the file (1-based).
    fn code_at_cursor(&mut self, ctx: &egui::Context, lang: Lang) -> (String, usize) {
        use egui::text::{CCursor, CCursorRange};
        let doc = &self.docs[self.cur];
        let mut state = egui::text_edit::TextEditState::load(ctx, doc.id).unwrap_or_default();
        let range = state.cursor.char_range().unwrap_or_else(|| CCursorRange::one(CCursor::new(0)));
        let [a, b] = range.sorted_cursors();
        let (a, b) = (usize::from(a.index), usize::from(b.index));
        if a != b {
            return (doc.text.chars().skip(a).take(b - a).collect(), line_of(&doc.text, a) + 1);
        }
        let (code, next) = statement_at(&doc.text, a, lang);
        // Where the statement starts: its last occurrence ending by `next`.
        let upto: String = doc.text.chars().take(next.max(a)).collect();
        let start = upto.rfind(code.trim_end()).map_or(a, |byte| upto[..byte].chars().count());
        state.cursor.set_char_range(Some(CCursorRange::one(CCursor::new(next))));
        state.store(ctx, doc.id);
        (code, line_of(&doc.text, start) + 1)
    }

    fn save(&mut self, ctx: &egui::Context) {
        let doc = &self.docs[self.cur];
        if doc.table.is_some() {
            return;
        }
        if doc.untitled() {
            let cwd = self.consoles[ix(self.lang)].view.lock().unwrap().cwd.clone();
            let dir = if self.files_path.is_empty() { cwd } else { self.files_path.clone() };
            self.save_as = Some(format!("{}/{}", dir.trim_end_matches('/'), doc.name()));
            return;
        }
        let (path, text) = (doc.path.clone(), doc.text.clone());
        self.docs[self.cur].saved = text.clone();
        self.docs[self.cur].disk = None;
        // The platform reloads a domain's .r files on the next request: a
        // save inside the open domain reloads its site in the Viewer.
        if self.domain.as_ref().is_some_and(|d| path.starts_with(&format!("{}/", d.path)) && !self.live_off.contains(&d.host)) {
            self.site_seq += 1;
            self.site_saved_at = ctx.input(|i| i.time);
        }
        let errors = self.errors.clone();
        let listing = self.listing.clone();
        self.backend.query(Query::WriteFile { path: path.clone(), text }, ctx, move |r: Result<FileText, String>| match r {
            Ok(_) => *listing.lock().unwrap() = None,
            Err(e) => Ide::fail(&errors, format!("not saved: {e}")),
        });
        if self.listing.lock().unwrap().is_none() {
            let p = self.files_path.clone();
            self.list(&p, ctx);
        }
    }

    // ── console ────────────────────────────────────────────────────────────

    /// Clear the Console: R's and the shell's transcripts and their bands
    /// together, since they are one page. The Terminal is not touched.
    /// Everything the consoles remember on this page: their lines, the
    /// Inputs page's runs, each language's ↑ history and the recent strip.
    /// The kernels drop their own transcripts when they restart (kernel.rs).
    fn forget_consoles(&mut self) {
        self.clear_console(Lang::R);
        for c in self.consoles.iter_mut() {
            c.history.clear();
            c.hist = None;
            c.input.clear();
        }
        for h in self.lang_hist.iter_mut() {
            h.clear();
        }
        self.lang_hist_at = None;
        self.recent_wide.clear();
        self.completing.items.clear();
        self.assist_note = None;
    }

    pub fn clear_console(&mut self, _lang: Lang) {
        for l in Lang::CONSOLES {
            self.consoles[ix(l)].view.lock().unwrap().lines.clear();
        }
        self.chunks.clear();
    }

    /// The right window's Files tab, with its unsaved count.
    pub fn files_label(&self) -> String {
        match self.docs.iter().filter(|d| d.dirty()).count() {
            0 => "Files".to_owned(),
            n => format!("Files · {n} unsaved"),
        }
    }

    /// A database opened from Files (or the agent) brings Data forward.
    pub fn take_data_open(&mut self) {
        if self.data.take_open() {
            self.show_lower(Lower::Data);
        }
    }

    /// The Files tab: the browser on the left of the tab, the
    /// editor (and its ask bar) beside it. True when its zoom was clicked.
    pub fn files_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, zoomed: bool) -> bool {
        // Three sections: the tree, the open file, and the agent the full
        // width under both (request #166, 2026-11-01: "takes up full width
        // under the file names and the file itself ... more space and the
        // context"). Reserved first, so the tree and the file fill the rest.
        self.ask_bar(ui, ctx);
        let w = ui.available_width();
        // A phone (operator, 2026-11-01, screenshot: the tree took 40% and
        // every line of the file was cut): the tree or the file, each the
        // whole width; opening a file shows it, the tab row's "‹" goes back.
        if w < 600.0 {
            let mut zoom = false;
            if self.phone_tree || self.docs.is_empty() {
                let mut made = None;
                let tree = egui::CentralPanel::no_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if !self.docs.is_empty() && ui.add(egui::Button::new(RichText::new(format!("{} →", self.docs[self.cur].name())).size(14.0)).min_size(egui::vec2(0.0, 32.0))).on_hover_text("back to the open file").clicked() {
                            self.phone_tree = false;
                        }
                        // The editor's + is not on screen with the tree.
                        made = new_file_menu(ui, RichText::new("+ New file").size(14.0));
                    });
                    self.files(ui, ctx);
                });
                match made {
                    Some(NewFile::Script(l)) => self.new_script(l),
                    Some(NewFile::Document(quarto)) => self.new_document(quarto),
                    None => {}
                }
                if made.is_some() {
                    self.phone_tree = false;
                }
                crate::tour::mark_at(ctx, "files-tree", tree.response.rect);
            } else {
                let ed = egui::CentralPanel::no_frame().show(ui, |ui| zoom = self.source(ui, ctx, zoomed, true));
                crate::tour::mark_at(ctx, "editor", ed.response.rect);
            }
            return zoom;
        }
        let tree = egui::Panel::left("right-files-tree")
            .resizable(true)
            .show_separator_line(true)
            .default_size((w * 0.3).clamp(160.0, 320.0))
            .size_range(120.0..=(w - 200.0).max(121.0))
            .frame(egui::Frame::NONE)
            .show(ui, |ui| self.files(ui, ctx));
        crate::tour::mark_at(ctx, "files-tree", tree.response.rect);
        let mut zoom = false;
        let ed = egui::CentralPanel::no_frame().show(ui, |ui| zoom = self.source(ui, ctx, zoomed, false));
        crate::tour::mark_at(ctx, "editor", ed.response.rect);
        zoom
    }

    /// The right window's Data tab.
    pub fn data_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Ide { data, backend, .. } = self;
        data.show(ui, ctx, backend);
    }

    pub fn account_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let status = match &*self.app_status.lock().unwrap() {
            Some(Ok(s)) if self.domain.as_ref().is_some_and(|d| d.host == self.app_host) => Some(s.clone()),
            _ => None,
        };
        let Ide { settings, backend, domain, .. } = self;
        match settings.show(ui, ctx, backend, domain.as_ref(), status.as_ref()) {
            Some(crate::settings_tab::Act::Access(h)) => self.want_access = Some(h),
            Some(crate::settings_tab::Act::Publish(h)) => self.publish_domain(h, ctx),
            Some(crate::settings_tab::Act::Run(verb)) => self.app_action(&verb, ctx),
            Some(crate::settings_tab::Act::SetType(lang)) => self.set_app_type(lang, ctx),
            None => {}
        }
    }

    /// Settings › Info's Type menu (dotcove.rs set_type), then the status
    /// again, so the strip and Settings show the new type at once.
    fn set_app_type(&mut self, language: String, ctx: &egui::Context) {
        let Some(d) = &self.domain else { return };
        let (slot, errors, backend, host, c) = (self.app_status.clone(), self.errors.clone(), self.backend.clone(), d.host.clone(), ctx.clone());
        self.backend.query(Query::AppType { host: host.clone(), language }, ctx, move |r: Result<String, String>| match r {
            Err(e) => Ide::fail(&errors, format!("type not changed: {e}")),
            Ok(_) => backend.query(Query::AppStatus { host }, &c, move |r: Result<crate::api::AppStatus, String>| {
                *slot.lock().unwrap() = Some(r);
            }),
        });
    }

    /// Type a command into the Terminal and bring it forward (setup's
    /// `claude setup-token`).
    pub fn run_in_terminal(&mut self, line: &str) {
        self.term.interrupt_then_line(line);
        self.view_term = true;
        self.bring(Tab::Viewer);
    }

    /// The token `claude setup-token` printed in the Terminal, if it is on screen.
    pub fn terminal_oauth_token(&self) -> Option<String> {
        let (rows, cols) = self.term.screen_rows();
        crate::term::oauth_token_in(&rows, cols)
    }

    /// The sign-in link `claude setup-token` printed in the Terminal.
    pub fn terminal_signin_link(&self) -> Option<String> {
        let (rows, cols) = self.term.screen_rows();
        crate::term::signin_link_in(&rows, cols)
    }

    /// Bring `t` forward in whichever window holds it.
    pub fn bring(&mut self, t: Tab) {
        match t {
            Tab::Console => self.lang = Lang::R,
            Tab::Terminal => self.lang = Lang::Shell,
            _ => {}
        }
        self.focus.push((t, false));
    }

    /// The start of the session has decided what it opens on (own_site_first,
    /// then the first visit's own domain).
    pub fn own_site_settled(&self) -> bool {
        self.own_new_done && (self.own_opened || self.restore.is_some())
    }

    /// Open the person's own site first when it is waiting for its first
    /// answer (`own_new_asked`); otherwise let the session open as it would.
    fn own_site_first(&mut self, ctx: &egui::Context) {
        if self.own_new_done {
            return;
        }
        if self.own_new_asked {
            let answer = self.own_new.lock().unwrap().take();
            if let Some(found) = answer {
                self.own_new_done = true;
                if let Some(d) = found {
                    self.restore = None;
                    self.folder_domain = None;
                    self.own_opened = true;
                    self.exec_on = false;
                    self.agents_layout = AgentsLayout::default();
                    self.start_fresh = Some(d.path.clone());
                    self.open_domain(d, ctx);
                }
            }
            return;
        }
        self.ask_domains(ctx);
        let own = match self.domains.lock().unwrap().as_ref() {
            None => return,
            Some(Ok(v)) => own_domain(v),
            Some(Err(_)) => None,
        };
        match own.filter(|d| d.title == crate::agent::SITE_TITLE) {
            None => self.own_new_done = true,
            Some(d) => {
                self.own_new_asked = true;
                let slot = self.own_new.clone();
                self.backend.query(Query::AgentHistory { domain: d.host.clone() }, ctx, move |r: Result<Vec<crate::api::AgentTurn>, String>| {
                    *slot.lock().unwrap() = Some(Some(d).filter(|_| r.is_ok_and(|t| t.is_empty())));
                });
            }
        }
    }

    /// A right-hand view forward: its tab, and which of Plots' two views.
    fn show_lower(&mut self, l: Lower) {
        // A page for the Viewer (a render, a widget, a server) takes it back
        // from the terminal; the terminal runs on behind it.
        if l == Lower::Viewer {
            self.view_term = false;
        }
        self.lower = l;
        self.focus.push((Tab::of_lower(l), false));
    }

    /// The tabs asked forward since the last frame, for the dock: (tab, only
    /// in place of Console or Terminal).
    pub fn take_focus(&mut self) -> Vec<(Tab, bool)> {
        self.take_data_open();
        std::mem::take(&mut self.focus)
    }

    /// A tab clicked: the console in front follows it.
    pub fn clicked(&mut self, t: Tab) {
        match t {
            Tab::Console => self.lang = Lang::R,
            Tab::Terminal => self.lang = Lang::Shell,
            _ => {}
        }
    }

    /// A tab's label, its status light and what hovering it says.
    pub fn tab_head(&self, t: Tab) -> (String, Option<Color32>, String) {
        match t {
            Tab::Console => {
                let (r, sh) = (self.consoles[ix(Lang::R)].view.lock().unwrap(), self.consoles[ix(Lang::Shell)].view.lock().unwrap());
                let dot = if r.busy || sh.busy { theme::warn() } else if r.alive { theme::ok() } else { theme::hush() };
                let or = |b: &str| if b.is_empty() { "starting…".to_owned() } else { b.to_owned() };
                let tip = format!("{}\n{}\nAgent · R · Python · Shell · Julia: push the input left or right, round and round (or ← → when it is empty); Tab completes", or(&r.banner), or(&sh.banner));
                let dot = if self.agent.busy() && self.chan == Channel::Agent { theme::warn() } else { dot };
                (format!("Console · {}", self.chan.word()), Some(dot), tip)
            }
            Tab::Terminal => ("Terminal".to_owned(), Some(if self.term.alive() { theme::ok() } else { theme::hush() }), self.term.note()),
            Tab::Agents => (
                // Just "Agents" (operator, 2026-10-20); Exec is a toggle inside it.
                "Agents".to_owned(),
                Some(if self.agent.busy() || self.exec.busy() { theme::warn() } else { theme::primary() }),
                "Site: this domain's agent, the same thread as the in-app chat · Exec: every domain on your account, in one thread".to_owned(),
            ),
            Tab::Exec => (
                "Exec".to_owned(),
                Some(if self.exec.busy() { theme::warn() } else { theme::primary() }),
                "your executive agent - every domain you own, in one thread; Agents stays this domain's".to_owned(),
            ),
            Tab::Files => {
                let unsaved = self.docs.iter().any(|d| d.dirty());
                (self.files_label(), unsaved.then_some(theme::amber()), if unsaved { "files with edits not saved".to_owned() } else { String::new() })
            }
            // The open domain's site: up, behind a sign-in or password, or down.
            Tab::App => {
                let Some(d) = &self.domain else { return ("App".to_owned(), None, "no domain open".to_owned()) };
                let state = self.gate.lock().unwrap().clone().filter(|(h, _)| *h == d.host).map(|(_, s)| s);
                let dot = match state.as_deref() {
                    Some("open") => theme::ok(),
                    Some("login" | "password") => theme::primary(),
                    Some("down") => theme::alt(),
                    _ => theme::hush(),
                };
                ("App".to_owned(), Some(dot), format!("{} · {}", d.host, state.unwrap_or_else(|| "checking…".to_owned())))
            }
            t => (t.name().to_owned(), None, String::new()),
        }
    }

    /// The tools at the right of a window's tab strip for the tab in front,
    /// laid out right to left after the zoom control: a console's Clear,
    /// restart and interrupt, the terminal's restart.
    pub fn tab_tools(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, t: Tab) {
        match t {
            Tab::Console => {
                // The tools act on the session the input is in: R (and its
                // Python) or the page's shell; on the Agent channel, R's.
                let session = match self.chan {
                    Channel::Code(l) => l.session(),
                    Channel::Agent => Lang::R,
                };
                let v = self.consoles[ix(session)].view.lock().unwrap().clone_head();
                theme::tool_sep(ui);
                if theme::tool(ui, "Clear", "clear the console · Ctrl+L").clicked() {
                    self.clear_console(session);
                }
                let what = if session == Lang::Shell { "the Console's shell — and the Terminal's bash".to_owned() } else { "R — a clean session: empty environment, no plots, Python with it".to_owned() };
                if theme::tool(ui, "⟳", &format!("restart {what}")).clicked() {
                    self.restart(session, ctx);
                }
                if ui.add_enabled(v.busy, egui::Button::new(RichText::new("■").size(12.0).color(ace::error())).frame(false)).on_hover_text("interrupt · Esc").clicked() {
                    self.interrupt(session, ctx);
                }
                ui.add_space(4.0);
                let where_ = [v.banner.as_str(), v.cwd.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
                ui.add(egui::Label::new(RichText::new(where_).monospace().size(11.0).color(theme::hush())).truncate());
            }
            Tab::Terminal => {
                theme::tool_sep(ui);
                if theme::tool(ui, "⟳", "end this terminal and start a new one").clicked() {
                    self.restart_terminal(ctx);
                }
                ui.add_space(4.0);
                ui.add(egui::Label::new(crate::term::strip_label(&self.term)).truncate());
            }
            _ => {}
        }
    }

    /// A tab's content, under its window's strip. True when a zoom control
    /// inside it (the editor's) was clicked.
    pub fn tab_body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, t: Tab, zoomed: bool) -> bool {
        match t {
            Tab::Console => self.lang_body(ui, ctx, Lang::R),
            Tab::Terminal => self.lang_body(ui, ctx, Lang::Shell),
            Tab::Agents => self.agents_tab(ui, ctx),
            Tab::Exec => self.exec_tab(ui, ctx),
            Tab::Files => return self.files_tab(ui, ctx, zoomed),
            Tab::App => self.app(ui, ctx),
            Tab::Environment => self.environment(ui, ctx),
            Tab::Software => self.software(ui, ctx, 0),
            Tab::Builds => self.software(ui, ctx, 1),
            Tab::Install => self.software(ui, ctx, 2),
            Tab::Plots => self.plots(ui, ctx),
            Tab::Viewer => self.viewer(ui, ctx),
            Tab::Data => self.data_tab(ui, ctx),
            Tab::Account => self.account_tab(ui, ctx),
            Tab::Logs => self.logs_tab(ui, ctx),
        }
        false
    }

    /// The Agents tab: the open domain's agent (Site) and the account's
    /// executive (Exec) - one at a time, or both, side by side or stacked,
    /// with a divider to drag between them.
    fn agents_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // A paste lands here whether or not the App pane is drawn.
        self.take_shots(ctx);
        // Site or Exec, one tab (operator, 2026-10-18: "Exec goes into agents
        // as a toggle there instead of its own tab"). Exec is the account's
        // executive for everyone: exec.ndexr.io for the operator, the
        // person's own `owner:<username>` thread for anyone else (host.rs
        // AGENT_R) - no access to exec.ndexr.io itself needed.
        // Which agent is chosen beside the mic (agent.rs scope_toggle); here
        // only how they are laid out. Each pane's header says whose it is.
        // A phone shows one at a time and spends no row on the choice
        // (operator, 2026-11-07: a row of three small icons and nothing else
        // was "poor use of white space"); the saved layout returns on a wider
        // screen.
        let phone = ui.available_width() < 640.0;
        let layout = if phone { AgentsLayout::One } else { self.agents_layout };
        if !phone {
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (l, icon, tip) in [
                    (AgentsLayout::Stacked, "⬒", "this site above, all sites below - drag the line between them"),
                    (AgentsLayout::SideBySide, "◫", "this site and all sites side by side - drag the line between them"),
                    (AgentsLayout::One, "▢", "one at a time - this site or Exec, chosen beside the mic"),
                ] {
                    if theme::tool_if(ui, self.agents_layout == l, icon, tip).clicked() {
                        self.agents_layout = l;
                    }
                }
            });
        });
        }
        // What Listen has heard, as in Go and Files.
        self.listen_bar(ui, "agents");
        // One at a time: the switch is beside the mic. Side by side both are
        // on screen, so neither composer carries one.
        let one = layout == AgentsLayout::One;
        self.set_scope(one);
        match layout {
            AgentsLayout::One if self.exec_on || self.domain.is_none() => self.exec_tab(ui, ctx),
            AgentsLayout::One => self.site_agent(ui, ctx),
            layout => {
                let across = layout == AgentsLayout::SideBySide;
                let r = ui.available_rect_before_wrap();
                let gap = 7.0;
                let span = if across { r.width() } else { r.height() };
                let min = 120.0f32.min(span / 3.0);
                let first = (span * self.agents_frac).clamp(min, (span - gap - min).max(min));
                let (a, bar, b) = if across {
                    (
                        egui::Rect::from_min_size(r.min, egui::vec2(first, r.height())),
                        egui::Rect::from_min_size(egui::pos2(r.left() + first, r.top()), egui::vec2(gap, r.height())),
                        egui::Rect::from_min_max(egui::pos2(r.left() + first + gap, r.top()), r.max),
                    )
                } else {
                    (
                        egui::Rect::from_min_size(r.min, egui::vec2(r.width(), first)),
                        egui::Rect::from_min_size(egui::pos2(r.left(), r.top() + first), egui::vec2(r.width(), gap)),
                        egui::Rect::from_min_max(egui::pos2(r.left(), r.top() + first + gap), r.max),
                    )
                };
                let pane = |ui: &mut egui::Ui, rect: egui::Rect, exec: bool, me: &mut Self| {
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
                    child.set_clip_rect(rect.intersect(ui.clip_rect()));
                    if exec {
                        me.exec_tab(&mut child, ctx);
                    } else {
                        me.site_agent(&mut child, ctx);
                    }
                };
                pane(ui, a, false, self);
                pane(ui, b, true, self);
                // The divider: drag it to give either agent more room.
                let resp = ui.interact(bar, ui.id().with("agents-split"), egui::Sense::drag());
                let lit = resp.hovered() || resp.dragged();
                if lit {
                    ctx.set_cursor_icon(if across { egui::CursorIcon::ResizeHorizontal } else { egui::CursorIcon::ResizeVertical });
                }
                if resp.dragged() {
                    let d = if across { resp.drag_delta().x } else { resp.drag_delta().y };
                    self.agents_frac = ((first + d) / span).clamp(0.15, 0.85);
                }
                let c = bar.center();
                let (p, q) = if across { (egui::pos2(c.x, c.y - 14.0), egui::pos2(c.x, c.y + 14.0)) } else { (egui::pos2(c.x - 14.0, c.y), egui::pos2(c.x + 14.0, c.y)) };
                ui.painter().line_segment([p, q], egui::Stroke::new(if lit { 3.0 } else { 2.0 }, if lit { theme::primary() } else { theme::hush() }));
                ui.allocate_rect(r, egui::Sense::hover());
            }
        }
        self.take_scope();
    }

    /// The switch beside each composer's mic (agent.rs scope_toggle): shown,
    /// or not, with what it should say this frame.
    fn set_scope(&mut self, shown: bool) {
        // No project open means Exec for now, never a choice kept: this used to
        // set exec_on, and it was saved, so a first visit (no domain yet on the
        // first frame) stayed on Exec instead of its own site's thread.
        let exec_on = self.exec_on || self.domain.is_none();
        let sc = shown.then(|| crate::agent::Scope { site: self.domain.as_ref().map(|d| d.host.clone()), exec_on, exec_busy: self.exec.busy() });
        self.agent.scope = sc.clone();
        self.exec.scope = sc;
    }

    /// A click on that switch, from either composer.
    fn take_scope(&mut self) {
        for all in [self.agent.scope_pick.take(), self.exec.scope_pick.take()].into_iter().flatten() {
            self.exec_on = all;
        }
    }

    /// The open domain's agent, and - split - the console it sends code to, under it.
    fn site_agent(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        // A picture pasted while the agent is in front lands in its box.
        self.take_shots(ctx);
        let tail = self.console_tail(Lang::R, 60);
        let split = self.agent.split && self.domain.is_some();
        let h = if split { (ui.available_height() * 0.58).max(160.0) } else { ui.available_height() };
        let w = ui.available_width();
        let Ide { agent, backend, domain, .. } = self;
        let run = ui.allocate_ui(egui::vec2(w, h), |ui| agent.show(ui, ctx, backend, domain.as_ref(), &tail)).inner;
        for (lang, code) in run {
            self.send(lang, code, ctx);
        }
        // A doc or a changed file the agent pane points at: Files.
        for p in std::mem::take(&mut self.agent.open) {
            self.open(p, ctx);
            self.show_lower(Lower::Files);
        }
        if split {
            ui.separator();
            let lang = self.lang;
            self.lang_body(ui, ctx, lang);
        }
    }

    /// What the Exec pane runs against. Always the scope `exec.ndexr.io`: the
    /// platform decides whose thread that is (host.rs AGENT_R) - the operator's
    /// exec.ndexr.io, anyone else's own account (`owner:<username>`). Its
    /// folder, for attachments and the docs the agent works from: exec's own
    /// when it is in the list (the operator), else the person's domains folder.
    fn exec_scope(&self) -> DomainInfo {
        let list = self.domains.lock().unwrap().as_ref().and_then(|r| r.as_ref().ok()).cloned().unwrap_or_default();
        let path = list
            .iter()
            .find(|d| d.host == "exec.ndexr.io")
            .map(|d| d.path.clone())
            .or_else(|| list.first().or(self.domain.as_ref()).and_then(|d| std::path::Path::new(&d.path).parent()).map(|p| p.display().to_string()))
            .unwrap_or_default();
        DomainInfo { host: "exec.ndexr.io".to_owned(), path, title: "Exec".to_owned(), module: false, ..Default::default() }
    }

    /// The Exec tab: the person's executive agent. Code it gives runs in the
    /// same consoles the Agents tab sends to.
    /// Go mode's agent (app.rs go_view): the Site / Exec switch a thumb can
    /// hit, the thread, and the big record-and-send bar under it.
    /// `min_h`: the least height the thread and composer get, scrolled to
    /// when the screen is shorter (app.rs go_view).
    pub fn go_agent(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, min_h: f32) {
        self.take_shots(ctx);
        // Site or Exec: the switch beside the mic, as on the Agents tab.
        self.set_scope(true);
        ui.add_space(4.0);
        self.listen_bar(ui, "go");
        let (w, h) = (ui.available_width(), ui.available_height().max(min_h));
        // One composer, its mic and Send big - no second bar under it. Not on
        // a short screen (a laptop zoomed in): there the thread needs the room.
        let big = ctx.content_rect().height() >= 480.0;
        self.agent.big = big;
        self.exec.big = big;
        let tail = self.console_tail(Lang::R, 60);
        let run = if self.exec_on || self.domain.is_none() {
            let scope = self.exec_scope();
            let Ide { exec, backend, .. } = self;
            ui.allocate_ui(egui::vec2(w, h), |ui| exec.show(ui, ctx, backend, Some(&scope), &tail)).inner
        } else {
            let Ide { agent, backend, domain, .. } = self;
            ui.allocate_ui(egui::vec2(w, h), |ui| agent.show(ui, ctx, backend, domain.as_ref(), &tail)).inner
        };
        self.take_scope();
        for (lang, code) in run {
            self.send(lang, code, ctx);
        }
        self.agent.big = false;
        self.exec.big = false;
    }

    /// Listen, the same everywhere (operator, 2026-11-01: "listen+act mode
    /// like in go mode in all of the agent sections, Agents, Files, and Go,
    /// unify function between those where possible - should remain up to
    /// date together"): one mic, one "act as I talk", one list of what was
    /// heard. Run every frame from `poll`, whichever view is up, so nothing
    /// heard waits on a view being drawn; every composer that carries the
    /// button (Agents' and Go's agent, the Files agent, the console) is
    /// given the same state, and a click on any of them points "act as I
    /// talk" at its agent (`listen_to`).
    ///
    /// Each minute's piece is kept, and with "act as I talk" sent to that
    /// agent as it comes - which acts only on what is a clear ask. On Stop the
    /// whole recording is transcribed and kept in the domain's
    /// supporting_docs/ beside its transcript, so nothing said is lost; with
    /// "act as I talk" the agent gets the whole too, to pick up what the
    /// pieces missed, else it lands in that agent's box to send.
    fn listen_frame(&mut self, ctx: &egui::Context) {
        if !crate::voice::Listen::shown() {
            return;
        }
        let now = ctx.input(|i| i.time);
        let agent = std::mem::take(&mut self.agent.listen_clicked) | std::mem::take(&mut self.exec.listen_clicked);
        let file = std::mem::take(&mut self.ask.listen_clicked);
        // The length picked beside either agent is the length everywhere, and
        // a running Listen takes it from the piece it is hearing on.
        for s in [self.agent.listen_secs, self.exec.listen_secs] {
            if s != self.listen_secs {
                self.listen_secs = crate::voice::piece(s);
            }
        }
        (self.agent.listen_secs, self.exec.listen_secs) = (self.listen_secs, self.listen_secs);
        let on = self.listen_mode() != crate::voice::ListenMode::Off;
        if on {
            self.listen.every(now, self.listen_secs);
        }
        let left = on.then(|| self.listen.next_piece_in(now));
        (self.agent.listen_left, self.exec.listen_left) = (left, left);
        if agent || file {
            self.listen_to = if file { ListenTo::File } else { ListenTo::Agent };
            self.listen_click(now);
        }
        self.listen_poll(ctx);
        let mode = self.listen_mode();
        let view = Some((mode, mode != crate::voice::ListenMode::Off || !self.listen.busy(), self.listen_tip(now)));
        self.agent.listen = view.clone();
        self.exec.listen = view.clone();
        self.ask.listen = view;
        if mode != crate::voice::ListenMode::Off || self.listen.busy() {
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
    }

    /// What came back from Listen since the last frame: each piece kept, or
    /// sent to the agent when acting as I talk; the whole when it stops.
    fn listen_poll(&mut self, ctx: &egui::Context) {
        for h in self.listen.poll(ctx, &self.backend) {
            match h {
                crate::voice::Heard::Chunk(t) => {
                    let t = t.trim().to_owned();
                    if t.is_empty() {
                        continue;
                    }
                    let words = t.split_whitespace().count();
                    let fate = if !self.listen_act {
                        "kept - lands in the box when you stop".to_owned()
                    } else if words < 6 {
                        format!("kept, not sent - {words} word{} is too little to act on; the agent gets it with the whole when you stop", if words == 1 { "" } else { "s" })
                    } else {
                        let span = crate::voice::piece_span(self.listen_secs);
                        let ask = if self.presenting() {
                            format!(
                                "(Presenting: said aloud during a live talk, the last {span}, transcribed. Keep what is on screen in step with the talk - move to the slide or section being spoken about, or make the one small change asked for. Change nothing when it already matches. Reply in one line; never stop to explain.)\n\n{t}"
                            )
                        } else {
                            format!(
                                "(Said aloud in Cove while I keep talking - the last {span}, transcribed. Do what in it is a clear, complete ask you can do now; if the rest is half a thought or context, reply briefly with what you took from it and wait for more.)\n\n{t}"
                            )
                        };
                        if self.listen_say(ctx, &ask) {
                            format!("sent to {}", self.listen_who())
                        } else {
                            "kept - the Files agent is still on the last piece; it gets this with the whole when you stop".to_owned()
                        }
                    };
                    self.heard.push((t, fate));
                }
                crate::voice::Heard::Whole { audio, text } => self.listen_done(ctx, audio, text),
                crate::voice::Heard::Error(e) => Ide::fail(&self.errors, e),
            }
        }
    }

    /// Listen acts for an agent whose mode pill is on Presenting.
    fn presenting(&self) -> bool {
        let pane = if self.exec_on || self.domain.is_none() { &self.exec } else { &self.agent };
        matches!(self.listen_to, ListenTo::Agent) && pane.presenting()
    }

    /// Which agent "act as I talk" sends to, in words.
    fn listen_who(&self) -> &'static str {
        match self.listen_to {
            ListenTo::File => "the Files agent",
            ListenTo::Agent if self.exec_on || self.domain.is_none() => "Exec",
            ListenTo::Agent => "the site's agent",
        }
    }

    /// Send `text` to the agent Listen acts for. False when it could not take
    /// it now (the Files agent runs one ask at a time).
    fn listen_say(&mut self, ctx: &egui::Context, text: &str) -> bool {
        match self.listen_to {
            ListenTo::Agent => {
                let current = if self.exec_on || self.domain.is_none() { self.exec.model() } else { self.agent.model() };
                let model = self.presenting().then(|| crate::voice::present_model(current));
                self.go_say_on(ctx, text, model.as_deref());
                true
            }
            ListenTo::File => self.file_say(ctx, text),
        }
    }

    /// Put `text` in the box of the agent Listen acts for, to send.
    fn listen_keep(&mut self, text: &str) {
        match self.listen_to {
            ListenTo::File => {
                if !self.ask.input.trim().is_empty() {
                    self.ask.input.push_str("\n\n");
                }
                self.ask.input.push_str(text);
            }
            ListenTo::Agent if self.exec_on || self.domain.is_none() => self.exec.add_to_input(text),
            ListenTo::Agent => self.agent.add_to_input(text),
        }
    }

    /// What the Files agent works on: the open file (a real one), and the
    /// folder the tree is in - the domain's when the tree is nowhere yet.
    fn ask_where(&self) -> (Option<String>, String) {
        let file = self.docs.get(self.cur).filter(|d| !d.untitled() && d.table.is_none()).map(|d| d.path.clone());
        let folder = if self.files_path.is_empty() { self.domain.as_ref().map(|d| d.path.clone()).unwrap_or_default() } else { self.files_path.clone() };
        (file, folder)
    }

    /// An ask to the Files agent that the person did not type: saved first,
    /// as Send does, and what they were typing left in the box.
    fn file_say(&mut self, ctx: &egui::Context, text: &str) -> bool {
        let Some(host) = self.domain.as_ref().map(|d| d.host.clone()) else {
            self.go_say(ctx, text);
            return true;
        };
        if self.ask.busy() {
            return false;
        }
        let (file, folder) = self.ask_where();
        if file.is_some() && self.docs[self.cur].dirty() {
            self.save(ctx);
        }
        let typed = std::mem::replace(&mut self.ask.input, text.to_owned());
        let Ide { ask, backend, .. } = self;
        ask.start(backend, ctx, &host, file.as_deref(), &folder);
        self.ask.input = typed;
        true
    }

    /// Off, Listen, or Act as I talk, as the mic is now.
    fn listen_mode(&self) -> crate::voice::ListenMode {
        use crate::voice::ListenMode;
        match (matches!(self.listen.state().as_str(), "starting" | "on"), self.listen_act) {
            (false, _) => ListenMode::Off,
            (true, false) => ListenMode::Listen,
            (true, true) => ListenMode::Act,
        }
    }

    /// The Listen button's hover: where it is, what a click does.
    fn listen_tip(&self, now: f64) -> String {
        use crate::voice::ListenMode;
        let n = self.heard.len();
        let heard = if n > 0 { format!(" · {n} piece{} heard", if n == 1 { "" } else { "s" }) } else { String::new() };
        match self.listen_mode() {
            ListenMode::Off if self.listen.busy() => "Listen · transcribing what it heard - ready again in a moment".to_owned(),
            ListenMode::Off => "Listen · keeps the mic open while you talk, a speaker in the room included.\n\
                                Click: Listen - every few minutes (30 s, 1 min or 5 min, beside Auto) what you said is transcribed and kept; the whole lands in this agent's box when you stop.\n\
                                Click again: Act as I talk - each piece goes to this agent as it is heard.\n\
                                Click a third time: stop, and the recording is saved in supporting_docs/.\n\
                                The same Listen shows in Agents, Files and Go: whichever you click, its agent is the one it acts for.".to_owned(),
            ListenMode::Listen => format!("Listening {}{heard} - kept, nothing sent.\nClick: act as I talk - send each piece to this agent from now on.", self.listen.clock(now)),
            ListenMode::Act => format!(
                "Acting as you talk {}{heard} · every {}{}, to {}.\nClick: stop - the whole is transcribed and saved in supporting_docs/.",
                self.listen.clock(now),
                crate::voice::piece_words(self.listen_secs),
                if self.presenting() { ", presenting on Sonnet" } else { "" },
                self.listen_who()
            ),
        }
    }

    /// A click on a Listen button: Off → Listen → Act as I talk → Off.
    fn listen_click(&mut self, now: f64) {
        use crate::voice::ListenMode;
        match self.listen_mode() {
            ListenMode::Off if !self.listen.busy() => {
                self.listen_act = false;
                self.heard.clear();
                self.listen.start(now, self.listen_secs);
            }
            ListenMode::Off => {}
            ListenMode::Listen => self.listen_act = true,
            ListenMode::Act => self.listen.stop(),
        }
    }

    /// The console's Listen button at `d` px, right of its mic: acts for the
    /// site's agent (or Exec), as Agents and Go do.
    fn listen_button(&mut self, ui: &mut egui::Ui, d: f32) {
        if !crate::voice::Listen::shown() {
            return;
        }
        let now = ui.input(|i| i.time);
        let mode = self.listen_mode();
        let enabled = mode != crate::voice::ListenMode::Off || !self.listen.busy();
        let tip = self.listen_tip(now);
        if crate::voice::listen_button(ui, mode, enabled, &tip, d).clicked() {
            self.listen_to = ListenTo::Agent;
            self.listen_click(now);
        }
    }

    /// What Listen is doing and has heard, in words, above a composer that
    /// carries its button - the same list in Agents, Files and Go. `salt`
    /// keeps two of them on screen at once apart.
    fn listen_bar(&self, ui: &mut egui::Ui, salt: &str) {
        if let Some(s) = self.listen_status(ui.input(|i| i.time)) {
            draw_listen(ui, &s, salt);
        }
    }

    /// The list `listen_bar` draws, or None while Listen is idle.
    fn listen_status(&self, now: f64) -> Option<ListenStatus> {
        if !crate::voice::Listen::shown() {
            return None;
        }
        let state = self.listen.state();
        let on = matches!(state.as_str(), "starting" | "on");
        let pending = self.listen.pending();
        let doing = match state.as_str() {
            "starting" => Some("Opening the microphone…".to_owned()),
            "packing" => Some("Saving…".to_owned()),
            "on" => Some(format!(
                "Listening · {}the next {} transcribed in {}s{}",
                if pending > 0 { format!("transcribing {pending} piece{} · ", if pending == 1 { "" } else { "s" }) } else { String::new() },
                if self.heard.is_empty() && pending == 0 { "first piece is" } else { "piece is" },
                self.listen.next_piece_in(now),
                if self.listen_act { format!(" and goes to {} if it is a clear ask", self.listen_who()) } else { String::new() },
            )),
            _ if pending > 0 => Some(format!("Transcribing the last {pending} piece{}…", if pending == 1 { "" } else { "s" })),
            "off" if self.listen.busy() => Some("Transcribing…".to_owned()),
            _ => None,
        };
        (on || doing.is_some() || !self.heard.is_empty()).then(|| ListenStatus { heard: self.heard.clone(), doing })
    }

    /// Where Go's agent is pointed: (its host, its folder) - Exec or the site.
    fn go_target(&self) -> (String, String) {
        if self.exec_on || self.domain.is_none() {
            let s = self.exec_scope();
            (s.host, s.path)
        } else {
            let d = self.domain.as_ref().unwrap();
            (d.host.clone(), d.path.clone())
        }
    }

    /// A message to Go's agent of the moment, leaving the box as it is.
    fn go_say(&mut self, ctx: &egui::Context, text: &str) {
        self.go_say_on(ctx, text, None);
    }

    /// `go_say`, on `model` for this one turn when given.
    fn go_say_on(&mut self, ctx: &egui::Context, text: &str, model: Option<&str>) {
        let tail = self.console_tail(Lang::R, 60);
        let (host, _) = self.go_target();
        let Ide { agent, exec, backend, exec_on, domain, .. } = self;
        if *exec_on || domain.is_none() {
            exec.say_on(backend, ctx, &host, text, &tail, model);
        } else {
            agent.say_on(backend, ctx, &host, text, &tail, model);
        }
    }

    /// Listen stopped: keep the recording and its transcript, then hand the
    /// transcript on.
    fn listen_done(&mut self, ctx: &egui::Context, audio: String, text: Result<String, String>) {
        let pieces = self.heard.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>().join("\n\n");
        let (whole, note) = match text {
            Ok(t) if !t.trim().is_empty() => (t.trim().to_owned(), String::new()),
            Ok(_) => (pieces.clone(), "(the whole recording came back empty; this is the pieces heard as it went)\n\n".to_owned()),
            Err(e) => (pieces.clone(), format!("(the whole recording could not be transcribed: {e}; this is the pieces heard as it went)\n\n")),
        };
        let (_, dir) = self.go_target();
        let stamp = crate::shots::name("go-note", "");
        let stamp = stamp.rsplit_once('.').map_or(stamp.as_str(), |x| x.0).to_owned();
        let ext = match audio.split(';').next().unwrap_or("").trim_start_matches("data:") {
            m if m.contains("mp4") => "m4a",
            m if m.contains("ogg") => "ogg",
            _ => "webm",
        };
        let docs = format!("{dir}/supporting_docs");
        if !dir.is_empty() {
            if let Some(bytes) = audio.split_once(";base64,").and_then(|(_, b)| crate::api::b64_decode(b)) {
                Ide::upload_one(&self.backend, &docs, format!("{stamp}.{ext}"), bytes, &self.errors, &self.files_stale, ctx);
            }
            if !whole.is_empty() {
                let md = format!("# Spoken note · {stamp}\n\nSaid aloud in Cove (Listen); the recording is {stamp}.{ext} beside this file.\n\n{note}{whole}\n");
                Ide::upload_one(&self.backend, &docs, format!("{stamp}.md"), md.into_bytes(), &self.errors, &self.files_stale, ctx);
            }
        }
        if whole.is_empty() {
            Ide::fail(&self.errors, "Listen heard nothing to keep".into());
            return;
        }
        let said = self.listen_act
            && self.listen_say(ctx, &format!(
                "(The whole of what I said in Cove just now, transcribed - saved as supporting_docs/{stamp}.md with the recording. You have had it in pieces as I talked; do whatever in it is a clear ask that has not been done yet, and list anything left open.)\n\n{whole}"
            ));
        if !said {
            self.listen_keep(&whole);
        }
        self.heard.clear();
    }

    fn exec_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let tail = self.console_tail(Lang::R, 60);
        let scope = self.exec_scope();
        let run = {
            let Ide { exec, backend, .. } = self;
            exec.show(ui, ctx, backend, Some(&scope), &tail)
        };
        for (lang, code) in run {
            self.send(lang, code, ctx);
        }
        for p in std::mem::take(&mut self.exec.open) {
            self.open(p, ctx);
            self.show_lower(Lower::Files);
        }
    }

    /// The console for `lang`, or the terminal for the shell.
    fn lang_body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, lang: Lang) {
        if lang == Lang::Shell {
            let Ide { term, backend, .. } = self;
            term.show(ui, ctx, backend);
        } else {
            self.console_body(ui, ctx, lang);
        }
    }

    /// The end of the Inputs page, as the agent is shown it: R, Python and
    /// shell chunks in the order they ran, each band under its language. The
    /// agent's own turns are left out - it has its thread.
    fn console_tail(&self, _lang: Lang, n: usize) -> String {
        let (r, sh, jl) = (self.consoles[ix(Lang::R)].view.lock().unwrap(), self.consoles[ix(Lang::Shell)].view.lock().unwrap(), self.consoles[ix(Lang::Julia)].view.lock().unwrap());
        let views = [&*r, &*sh, &*jl];
        let rows = console_rows(&self.chunks, views, &|_| None);
        let mut out = String::new();
        for row in &rows[rows.len().saturating_sub(n)..] {
            match row {
                Row::Head(Channel::Agent) | Row::Note(Channel::Agent, ..) => {}
                Row::Head(c) => out.push_str(&format!("## {}\n", c.word())),
                Row::Line(_, k, p) => {
                    out.push_str(&views[*k].lines[*p].text);
                    out.push('\n');
                }
                Row::Note(_, _, t) => {
                    out.push_str(t);
                    out.push('\n');
                }
            }
        }
        out
    }

    /// The Inputs page: one page of agent, R, Python and shell runs in the
    /// order they went, a band per channel (`console_rows`); the last five
    /// sent, folded, at the top (operator, 2026-10-15: "the last 5 commands
    /// show up at the top of the window but collapsed so we have some ability
    /// to look at prior commands without them disappearing but also allowing
    /// the new to get in"); and one input whose channel a push left or right
    /// switches, round the ring. Tab completes in the session itself (R's own
    /// completion, Python's rlcompleter through reticulate, the shell's
    /// compgen), so what the environment's modules brought is what completes.
    fn console_body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, _lang: Lang) {
        let chan = self.chan;
        let (rv, sv, jv) = (self.consoles[ix(Lang::R)].view.clone(), self.consoles[ix(Lang::Shell)].view.clone(), self.consoles[ix(Lang::Julia)].view.clone());
        let input_id = console_input(Lang::R);
        let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
        let now = ctx.input(|i| i.time);

        // ── the recent strip ──
        let recent: Vec<usize> = self.chunks.iter().enumerate().filter(|(_, r)| !r.quiet).map(|(i, _)| i).rev().take(5).collect::<Vec<_>>().into_iter().rev().collect();
        let mut recall: Option<(Channel, String)> = None;
        if !recent.is_empty() {
            egui::Frame::new().fill(theme::surface()).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                let head = format!("{} Recent · {}", if self.recent_open { "▾" } else { "▸" }, recent.len());
                if ui.add(egui::Label::new(RichText::new(head).size(11.0).color(theme::hush())).sense(egui::Sense::click())).on_hover_text("the last five sent - click one to see all of it, ↩ to send it again").clicked() {
                    self.recent_open = !self.recent_open;
                }
                if !self.recent_open {
                    return;
                }
                for &i in &recent {
                    let run = &self.chunks[i];
                    let wide = self.recent_wide.contains(&i);
                    let lines: Vec<&str> = run.code.trim_end().lines().collect();
                    let shown = if wide { lines.iter().take(12).copied().collect::<Vec<_>>().join("\n") } else {
                        let more = if lines.len() > 1 { format!("   (+{} lines)", lines.len() - 1) } else { String::new() };
                        format!("{}{more}", lines.first().copied().unwrap_or(""))
                    };
                    let mut toggle = false;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let (bar, _) = ui.allocate_exact_size(egui::vec2(2.0, row_h), egui::Sense::hover());
                        ui.painter().rect_filled(bar, 0.0, run.chan.color());
                        ui.label(RichText::new(run.chan.pill()).monospace().size(10.5).color(run.chan.color()));
                        if ui.small_button("↩").on_hover_text(format!("back into the input, on {}", run.chan.word())).clicked() {
                            recall = Some((run.chan, run.code.clone()));
                        }
                        let l = egui::Label::new(RichText::new(shown).monospace().size(12.0).color(theme::muted())).sense(egui::Sense::click());
                        let l = if wide { l.wrap() } else { l.truncate() };
                        toggle = ui.add(l).on_hover_text(if wide { "fold" } else { "show all of it" }).clicked();
                    });
                    if toggle && !self.recent_wide.remove(&i) {
                        self.recent_wide.insert(i);
                    }
                }
            });
        }
        if let Some((c, code)) = recall {
            self.chan = c;
            self.consoles[ix(Lang::R)].input = code;
            ui.memory_mut(|m| m.request_focus(input_id));
        }

        // ── a completion that has come back ──
        let answer = self.completing.slot.lock().unwrap().take();
        // A completion that could not be had says why, in the hint line - it
        // used to vanish, and Tab looked dead.
        if let Some(Err(e)) = &answer {
            self.assist_note = Some(match e.as_str() {
                "busy" => "Tab: the session is busy - try again when it is done".to_owned(),
                "not running" => "Tab: the session is not running - run a line to start it".to_owned(),
                other => format!("Tab: no completion ({other})"),
            });
        }
        if let Some(Ok(c)) = answer {
            let input = &mut self.consoles[ix(Lang::R)].input;
            if *input == self.completing.snapshot && !c.items.is_empty() {
                let typed: String = input.chars().skip(self.completing.line_at + c.start).take(self.completing.cursor.saturating_sub(self.completing.line_at + c.start)).collect();
                let common = common_prefix(&c.items);
                if c.items.len() == 1 || common.chars().count() > typed.chars().count() {
                    let put = if c.items.len() == 1 { c.items[0].clone() } else { common };
                    let at = self.completing.line_at + c.start;
                    let end = complete_into(input, at, self.completing.cursor, &put);
                    set_cursor(ctx, input_id, end);
                    self.completing.cursor = end;
                    self.completing.snapshot = input.clone();
                }
                if c.items.len() > 1 {
                    self.completing.items = c.items;
                    self.completing.start = c.start;
                    self.completing.sel = 0;
                }
            }
        }
        if self.completing.items.len() > 0 && self.consoles[ix(Lang::R)].input != self.completing.snapshot {
            self.completing.items.clear();
        }
        let listing = !self.completing.items.is_empty();

        // ── a debugger or readline() waiting: on top, as RStudio's console
        // toolbar - under the page it pushed the input off the pane ──
        self.debug_bar(ui, ctx);

        // ── the page ──
        let input_h = row_h * (self.consoles[ix(Lang::R)].input.lines().count().clamp(1, 8) as f32) + 12.0;
        let pop_h = if listing { row_h * self.completing.items.len().min(8) as f32 + 10.0 } else { 0.0 };
        let out_h = (ui.available_height() - input_h - pop_h - ui.spacing().item_spacing.y * 2.0).max(row_h * 3.0);
        egui::Frame::new().fill(ace::bg()).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
            ui.set_height(out_h - 8.0);
            let (r, sh, jl) = (rv.lock().unwrap(), sv.lock().unwrap(), jv.lock().unwrap());
            let views = [&*r, &*sh, &*jl];
            let agent = &self.agent;
            let rows = console_rows(&self.chunks, views, &|k| agent.turn_text(k));
            // show_rows takes every row to be exactly this tall. It was the
            // style's monospace height while rows drew 12.5 pt labels in a
            // horizontal: the estimate moved with the visible range, and stuck
            // to the bottom the page bounced between two offsets on every
            // repaint - the newest chunks jiggling while nothing was typed
            // (2026-10-15). One measured height, every row held to it.
            let line_font = egui::FontId::monospace(12.5);
            let line_h = ui.fonts_mut(|f| f.row_height(&line_font)).ceil();
            egui::ScrollArea::both().id_salt("console-out").stick_to_bottom(true).auto_shrink([false, false]).show_rows(ui, line_h, rows.len(), |ui, range| {
                for row in &rows[range] {
                    let (band, kind, text) = match row {
                        Row::Head(c) => {
                            // The band's head: its channel, then a rule.
                            let w = ui.available_width().max(120.0);
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(w, line_h), egui::Sense::hover());
                            let col = c.color();
                            let g = ui.painter().text(rect.left_center() + egui::vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, c.word(), egui::FontId::monospace(11.0), col);
                            ui.painter().hline((g.right() + 6.0)..=rect.right(), rect.center().y, egui::Stroke::new(1.0, col.gamma_multiply(0.35)));
                            continue;
                        }
                        Row::Line(c, k, p) => {
                            let line = &views[*k].lines[*p];
                            (*c, line.kind, line.text.as_str())
                        }
                        Row::Note(c, kind, t) => (*c, *kind, t.as_str()),
                    };
                    let color = match kind {
                        LineKind::Input => theme::primary(),
                        LineKind::Output => ace::text(),
                        LineKind::Warning => ace::warn(),
                        LineKind::Error => ace::error(),
                        LineKind::System => theme::hush(),
                    };
                    ui.horizontal(|ui| {
                        ui.set_height(line_h);
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let (bar, _) = ui.allocate_exact_size(egui::vec2(2.0, line_h), egui::Sense::hover());
                        ui.painter().rect_filled(bar, 0.0, band.color().gamma_multiply(0.6));
                        let text = if text.is_empty() { " " } else { text };
                        ui.add(egui::Label::new(egui::RichText::new(text).font(line_font.clone()).color(color)).extend());
                    });
                }
            });
        });

        // ── keys ──
        let focused = ui.memory(|m| m.has_focus(input_id));
        let (mut submit, mut stop, mut clear, mut step, mut tab, mut accept) = (false, false, false, 0i32, false, false);
        if focused {
            if listing {
                if take_key(ui, Key::ArrowDown, |m| m.is_none()) {
                    self.completing.sel = (self.completing.sel + 1) % self.completing.items.len();
                } else if take_key(ui, Key::ArrowUp, |m| m.is_none()) {
                    self.completing.sel = (self.completing.sel + self.completing.items.len() - 1) % self.completing.items.len();
                }
                accept = take_key(ui, Key::Enter, |m| !m.shift);
                if take_key(ui, Key::Escape, |_| true) {
                    self.completing.items.clear();
                }
            }
            submit = !accept && take_key(ui, Key::Enter, |m| !m.shift);
            stop = !listing && take_key(ui, Key::Escape, |_| true);
            clear = take_key(ui, Key::L, |m| m.ctrl);
            // Tab never types a tab here: it completes, or steps the list.
            tab = take_key(ui, Key::Tab, |m| m.is_none());
            let c = &mut self.consoles[ix(Lang::R)];
            // Nothing typed: ← and → have no cursor to move, so they step
            // round the channels.
            if c.input.is_empty() {
                if take_key(ui, Key::ArrowLeft, |m| m.is_none()) {
                    step = -1;
                } else if take_key(ui, Key::ArrowRight, |m| m.is_none()) {
                    step = 1;
                }
            }
            // ↑ walks what was sent on this channel, from anywhere.
            let hist = &self.lang_hist[chan.slot()];
            if !listing && !c.input.contains('\n') && !hist.is_empty() {
                if take_key(ui, Key::ArrowUp, |m| m.is_none()) {
                    let at = self.lang_hist_at.map_or(hist.len() - 1, |h| h.saturating_sub(1));
                    self.lang_hist_at = Some(at);
                    c.input = hist[at].clone();
                } else if take_key(ui, Key::ArrowDown, |m| m.is_none()) {
                    match self.lang_hist_at {
                        Some(h) if h + 1 < hist.len() => {
                            self.lang_hist_at = Some(h + 1);
                            c.input = hist[h + 1].clone();
                        }
                        _ => {
                            self.lang_hist_at = None;
                            c.input.clear();
                        }
                    }
                }
            }
        }
        if tab && listing {
            self.completing.sel = (self.completing.sel + 1) % self.completing.items.len();
        } else if tab {
            self.ask_completion(ctx, input_id);
        }

        // ── the completion list, over the input ──
        let mut chosen = accept.then_some(self.completing.sel);
        if listing {
            egui::Frame::new().fill(theme::surface_hi()).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                egui::ScrollArea::vertical().id_salt("console-compl").max_height(row_h * 8.0).show(ui, |ui| {
                    for (i, it) in self.completing.items.iter().enumerate() {
                        let on = i == self.completing.sel;
                        let r = ui.add(egui::Label::new(RichText::new(it).monospace().size(12.0).color(if on { theme::text_strong() } else { theme::muted() }).background_color(if on { theme::moss() } else { Color32::TRANSPARENT })).sense(egui::Sense::click()));
                        if on && tab {
                            r.scroll_to_me(None);
                        }
                        if r.clicked() {
                            chosen = Some(i);
                        }
                    }
                });
            });
        }
        if let Some(i) = chosen.filter(|&i| i < self.completing.items.len()) {
            let put = self.completing.items[i].clone();
            let k = &self.completing;
            let (at, cur) = (k.line_at + k.start, k.cursor);
            let input = &mut self.consoles[ix(Lang::R)].input;
            let end = complete_into(input, at, cur, &put);
            set_cursor(ctx, input_id, end);
            self.completing.items.clear();
            ui.memory_mut(|m| m.request_focus(input_id));
        }

        // ── the input ──
        self.poll_assist(ctx, input_id);
        let busy = self.assisting.is_some()
            || match chan {
                Channel::Code(l) => self.consoles[ix(l.session())].view.lock().unwrap().busy,
                Channel::Agent => self.agent.busy(),
            };
        let ring = self.ring();
        let mut picked = None;
        let mut heard = String::new();
        let row = egui::Frame::new().fill(ace::bg()).inner_margin(egui::Margin { left: 8, right: 8, top: 0, bottom: 4 }).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                // The switcher: the four channels, the one in use lit. Click
                // one, drag across them, or push the input sideways.
                ui.spacing_mut().item_spacing.x = 2.0;
                for c in ring.iter().copied() {
                    let on = c == chan;
                    let text = RichText::new(c.pill()).monospace().size(11.0).color(if on { theme::ground() } else { c.color().gamma_multiply(0.7) });
                    let b = egui::Button::new(text).fill(if on { c.color() } else { Color32::TRANSPARENT }).stroke(egui::Stroke::new(1.0, c.color().gamma_multiply(if on { 1.0 } else { 0.35 }))).corner_radius(3.0).min_size(egui::vec2(0.0, row_h));
                    let r = ui.add(b).interact(egui::Sense::click_and_drag()).on_hover_text(format!("send to {} · push left or right to go round", c.word()));
                    if r.clicked() {
                        picked = Some(c);
                    }
                    if r.dragged() {
                        self.swipe -= r.drag_delta().x;
                    }
                }
                // Agent or not, for these inputs (operator, 2026-10-25: "make
                // checkbox for agent mode or not in Console"): ticked, what is
                // typed is an ask and the code for this language lands in the
                // input to read and run; not ticked, the line runs as typed -
                // a line starting with / included. No command syntax.
                if matches!(chan, Channel::Code(_)) {
                    // A pill like the languages' - outlined off, filled on -
                    // set off from them by a gap (operator, 2026-10-26: "agent
                    // and checkbox look bad": egui's checkbox was a tiny tick
                    // box jammed against the pills).
                    ui.add_space(6.0);
                    let on = self.assist_on;
                    let c = theme::primary();
                    let text = RichText::new("Agent").size(11.5).strong().color(if on { theme::ground() } else { c });
                    let b = egui::Button::new(text)
                        .fill(if on { c } else { Color32::TRANSPARENT })
                        .stroke(egui::Stroke::new(1.0, c.gamma_multiply(if on { 1.0 } else { 0.6 })))
                        .corner_radius(10.0)
                        .min_size(egui::vec2(0.0, row_h));
                    if ui
                        .add(b)
                        .on_hover_text(if on { "Agent is on: say what you want, Enter, and the code for this language lands here to read and run - click to run lines as typed" } else { "click for Agent: say what you want, and the code for this language lands here to read and run" })
                        .clicked()
                    {
                        self.assist_on = !on;
                        self.assist_note = None;
                    }
                    ui.add_space(4.0);
                }
                // 🎤: say what you want; the agent writes it for this channel.
                if crate::voice::Mic::shown() && self.assisting.is_none() {
                    let Ide { console_mic, backend, domain, .. } = &mut *self;
                    console_mic.keep = domain.as_ref().map(|d| d.path.clone());
                    console_mic.ui(ui, ctx, backend, &mut heard);
                }
                // Listen / Act as I talk, one icon right of the mic (as in Go).
                self.listen_button(ui, 32.0);
                ui.add_space(6.0);
                if busy {
                    // the prompt's height: the default spinner is taller and
                    // nudged the row (and the pane) on every busy/idle switch
                    ui.add(egui::Spinner::new().size(row_h));
                } else {
                    ui.label(RichText::new(chan.prompt()).monospace().size(12.5).strong().color(chan.color()));
                }
                let hint = match chan {
                    Channel::Agent if self.domain.is_none() => "Agent · open a domain first - its agent works in that domain's thread".to_owned(),
                    Channel::Agent => "Agent · Enter sends to this domain's agent · Shift+Enter new line · ← → or a sideways push goes round".to_owned(),
                    Channel::Code(_) if self.assisting.is_some() => "the agent is writing it…".to_owned(),
                    Channel::Code(l) => match &self.assist_note {
                        Some(n) => n.clone(),
                        None if self.assist_on => format!("{} · Agent: say what you want, Enter, and the code lands here to read and run · ← → goes round", l.word()),
                        None => format!("{} · Enter runs · Tab completes · ← → or a sideways push goes round · Esc stops", l.word()),
                    },
                };
                let c = &mut self.consoles[ix(Lang::R)];
                ui.add(
                    egui::TextEdit::multiline(&mut c.input)
                        .id(input_id)
                        .code_editor()
                        .frame(egui::Frame::NONE)
                        .background_color(ace::bg())
                        .return_key(Some(egui::KeyboardShortcut::new(egui::Modifiers::SHIFT, Key::Enter)))
                        // Tab completes here: without the lock egui moves focus
                        // off the input on Tab and the completion never lands
                        // (operator, 2026-10-25: "autocomple in the Consoles
                        // seems to have stopped").
                        .lock_focus(true)
                        .desired_rows(1)
                        .desired_width(f32::INFINITY)
                        .hint_text(RichText::new(hint).color(theme::hush())),
                );
            });
        });
        // A sideways push over the input (a trackpad's two fingers, a
        // horizontal wheel): content pushed left brings the next channel.
        if ui.rect_contains_pointer(row.response.rect) {
            self.swipe += ctx.input(|i| i.smooth_scroll_delta.x) * -1.0;
        }
        if step == 0 && self.swipe.abs() > 48.0 && now - self.swiped_at > 0.35 {
            step = self.swipe.signum() as i32;
        }
        if step != 0 {
            self.swipe = 0.0;
            self.swiped_at = now;
            picked = Some(chan.step(step, &ring));
        } else if now - self.swiped_at > 0.6 && ctx.input(|i| i.smooth_scroll_delta.x == 0.0 && !i.pointer.any_down()) {
            self.swipe = 0.0;
        }
        if let Some(c) = picked.filter(|&c| c != chan) {
            self.chan = c;
            self.lang_hist_at = None;
            self.completing.items.clear();
            ui.memory_mut(|m| m.request_focus(input_id));
        }
        // Spoken: always an ask for the channel in use, whether or not Agent
        // is ticked for typing; the answer is put in the input, not run.
        let heard = heard.trim().to_owned();
        if !heard.is_empty() {
            if let Channel::Code(l) = self.chan {
                self.ask_assist(l, &heard, &heard, ctx);
            }
        }
        if let Some(e) = self.console_mic.error() {
            self.assist_note = Some(format!("microphone: {e}"));
        }
        if submit {
            let code = std::mem::take(&mut self.consoles[ix(Lang::R)].input);
            if !code.trim().is_empty() {
                match self.chan {
                    Channel::Code(l) if self.assist_on => {
                        let ask = code.trim().to_owned();
                        self.ask_assist(l, &code, &ask, ctx);
                    }
                    Channel::Code(l) => self.send(l, code, ctx),
                    Channel::Agent => self.send_agent(code, ctx),
                }
            }
        }
        let session = match chan {
            Channel::Code(l) => Some(l.session()),
            Channel::Agent => None,
        };
        if let (true, Some(s)) = (stop, session) {
            self.interrupt(s, ctx);
        }
        if clear {
            self.clear_console(Lang::R);
        }
    }

    /// Tab: ask the session the input is on what may finish the word at the
    /// cursor. The answer lands in `completing.slot`; the next frame types
    /// what is certain and lists the rest.
    fn ask_completion(&mut self, ctx: &egui::Context, input_id: egui::Id) {
        let Channel::Code(lang) = self.chan else { return };
        let input = self.consoles[ix(Lang::R)].input.clone();
        let cursor = egui::TextEdit::load_state(ctx, input_id).and_then(|s| s.cursor.char_range()).map_or(input.chars().count(), |r| r.primary.index.into());
        let chars: Vec<char> = input.chars().collect();
        let cursor = cursor.min(chars.len());
        let line_at = chars[..cursor].iter().rposition(|&c| c == '\n').map_or(0, |p| p + 1);
        let line_end = chars[cursor..].iter().position(|&c| c == '\n').map_or(chars.len(), |p| cursor + p);
        let line: String = chars[line_at..line_end].iter().collect();
        self.completing.snapshot = input;
        self.completing.line_at = line_at;
        self.completing.cursor = cursor;
        let slot = self.completing.slot.clone();
        self.backend.query(Query::Complete { lang, line, cursor: cursor - line_at }, ctx, move |r: Result<api::Completion, String>| {
            *slot.lock().unwrap() = Some(r);
        });
    }

    // ── environment ────────────────────────────────────────────────────────

    /// RStudio's Environment pane (its 2026-10-14 form, back): R's objects and
    /// reticulate's Python's (`py$…`) in RStudio's three groups. Data opens in
    /// the data viewer (View()); a .csv, .tsv, .parquet or .rds opened from
    /// Files opens in Data. Stopped in R's debugger, the frame it stopped in.
    pub fn environment(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let lang = self.data_lang();
        let (env, debug) = {
            let v = self.consoles[ix(lang)].view.lock().unwrap();
            (v.env.clone(), v.debug.clone())
        };
        let mut clear = false;
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(4.0);
            let title = if debug == "R" { "Debugging: the frame browser() stopped in" } else { "Global Environment · R and Python" };
            ui.label(RichText::new(title).size(12.0).color(if debug == "R" { theme::amber() } else { theme::muted() }));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                clear = theme::tool_if(ui, debug.is_empty(), "Clear", "remove every object").clicked();
            });
        });
        if clear {
            self.clear_workspace(ctx);
        }
        let mut view = None;
        egui::ScrollArea::vertical().id_salt(("env", ix(lang))).auto_shrink([false, false]).show(ui, |ui| {
            if env.is_empty() {
                ui.add_space(8.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("Environment is empty").italics().color(theme::hush())));
                return;
            }
            // RStudio's three groups, in its order.
            for (g, title) in ["Data", "Values", "Functions"].iter().enumerate() {
                let rows: Vec<&api::EnvRow> = env.iter().filter(|e| env_group(&e.class) == g).collect();
                if rows.is_empty() {
                    continue;
                }
                egui::Frame::new().fill(theme::surface()).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.label(RichText::new(*title).size(12.0).strong().color(theme::text()));
                });
                // Name · Type · Value, fixed columns: a long value truncates
                // to the pane instead of running off its edge.
                let w = ui.available_width();
                let name_w = (w * 0.26).clamp(90.0, 240.0);
                let type_w = 96.0;
                let val_x = 8.0 + name_w + 8.0 + type_w + 8.0;
                let val_w = (w - val_x - 8.0).max(40.0);
                for (i, e) in rows.into_iter().enumerate() {
                    let (r, resp) = table_row(ui, i, ROW_H);
                    let name = if g == 0 { format!("⊞ {}", e.name) } else { e.name.clone() };
                    cell(ui, r, 8.0, name_w, RichText::new(name).monospace().size(12.0).color(theme::primary()), false);
                    cell(ui, r, 8.0 + name_w + 8.0, type_w, RichText::new(&e.class).monospace().size(11.0).color(theme::hush()), false);
                    cell(ui, r, val_x, val_w, RichText::new(&e.value).monospace().size(11.5).color(theme::muted()), false);
                    let resp = resp.on_hover_text(if g == 0 { format!("{}\nclick: View() - open it as a table", e.value) } else { e.value.clone() });
                    // RStudio's grid icon: the data viewer. Not mid-debug: the
                    // viewer reads the global environment.
                    if g == 0 && debug.is_empty() && resp.clicked() {
                        view = Some(e.name.clone());
                    }
                }
            }
        });
        if let Some(name) = view {
            self.view(lang, &name, ctx);
        }
    }

    /// Stopped at a prompt: R's debugger, pdb, or readline()/input(). The
    /// stepping buttons are RStudio's (Next, Step Into, Finish, Continue,
    /// Stop), sent as the debugger's own commands - n s f c Q in R, n s r c q
    /// in pdb - so what happens is what the console would do.
    fn debug_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let (prompt, debug) = {
            let v = self.consoles[ix(Lang::R)].view.lock().unwrap();
            (v.prompt.clone(), v.debug.clone())
        };
        if prompt.is_empty() {
            return;
        }
        let mut cmd: Option<&str> = None;
        egui::Frame::new().fill(theme::amber().gamma_multiply(0.18)).inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
            ui.set_min_width(ui.available_width());
            ui.horizontal(|ui| {
                let (what, steps): (String, &[(&str, &str, &str)]) = match debug.as_str() {
                    "R" => (format!("R debugger · {}", prompt.trim_end()), &[("Next", "n", "run this line"), ("Step Into", "s", "into the function this line calls"), ("Finish", "f", "run to the end of this function or loop"), ("Continue", "c", "run on to the next stop"), ("Stop", "Q", "leave the debugger")]),
                    "Python" => (format!("Python debugger · {}", prompt.trim_end()), &[("Next", "n", "run this line"), ("Step Into", "s", "into the function this line calls"), ("Finish", "r", "run until this function returns"), ("Continue", "c", "run on to the next stop"), ("Stop", "q", "leave the debugger")]),
                    _ => (format!("waiting for input · {}", prompt.trim_end()), &[]),
                };
                ui.label(RichText::new(what).monospace().size(11.5).color(theme::amber()));
                for (label, c, tip) in steps {
                    if ui.small_button(*label).on_hover_text(format!("{tip} ({c})")).clicked() {
                        cmd = Some(c);
                    }
                }
                if steps.is_empty() {
                    ui.label(RichText::new("type the answer below and press Enter").size(11.0).color(theme::muted()));
                }
            });
        });
        if let Some(c) = cmd {
            self.send(Lang::R, c.to_owned(), ctx);
        }
    }

    // ── history ────────────────────────────────────────────────────────────
    // ── files · plots · packages ───────────────────────────────────────────

    /// Every 3 s while a domain is open: has anything the site is made of
    /// changed on disk - by the agent, the Terminal, git, anything? Then App
    /// reloads (into the hidden frame, swapped in once it has drawn), so the
    /// site follows its files without a refresh (operator, 2026-10-16: "I am
    /// still not convinced that cove.ndexr.io updates in place with files
    /// changing the app viewer" - only saves in Cove's own editor did).
    fn watch_site(&mut self, ctx: &egui::Context, now: f64) {
        if let Some((path, st)) = self.site_stamp_slot.lock().unwrap().take() {
            let changed = self.site_stamp.as_ref().is_some_and(|(p, old)| *p == path && *old != st && !st.is_empty());
            let live = self.domain.as_ref().is_some_and(|d| !self.live_off.contains(&d.host));
            if changed && live && now - self.site_saved_at > 4.0 {
                self.site_seq += 1;
            }
            self.site_stamp = Some((path, st));
        }
        let Some(path) = self.domain.as_ref().map(|d| d.path.clone()) else { return };
        if self.site_stamp_inflight.load(Ordering::Relaxed) || now - self.site_stamp_asked < 3.0 {
            return;
        }
        self.site_stamp_asked = now;
        self.site_stamp_inflight.store(true, Ordering::Relaxed);
        let (slot, inflight) = (self.site_stamp_slot.clone(), self.site_stamp_inflight.clone());
        self.backend.query(Query::SiteStamp { path: path.clone() }, ctx, move |r: Result<String, String>| {
            inflight.store(false, Ordering::Relaxed);
            if let Ok(st) = r {
                *slot.lock().unwrap() = Some((path, st));
            }
        });
    }

    /// Every `refresh_every` seconds: what changed on disk among the open files and the Files
    /// folder. A changed text file is read again and shown - unless it has
    /// edits not yet saved here, which are never overwritten; a changed image
    /// or PDF reloads its frame; a changed folder is listed again. The first
    /// fingerprint of anything only records it.
    fn watch_files(&mut self, ctx: &egui::Context, now: f64) {
        self.watch_site(ctx, now);
        let answer = self.stamp_slot.lock().unwrap().take();
        if let Some(got) = answer {
            for (path, st) in got {
                let before = self.stamps.insert(path.clone(), st.clone());
                if before.is_none_or(|b| b == st) {
                    continue;
                }
                if path == self.files_path {
                    let p = path.clone();
                    self.list(&p, ctx);
                }
                let Some(d) = self.docs.iter().find(|d| d.path == path) else { continue };
                if d.preview {
                    *self.preview_seq.entry(path.clone()).or_default() += 1;
                    #[cfg(not(target_arch = "wasm32"))]
                    {
                        self.textures.remove(&path);
                        self.images.remove(&path);
                    }
                } else if !st.is_empty() {
                    // Read even with edits here: reread decides (it never
                    // overwrites them, it asks).
                    let (slot, errors) = (self.reread.clone(), self.errors.clone());
                    self.backend.query(Query::ReadFile { path }, ctx, move |r: Result<FileText, String>| match r {
                        Ok(f) => slot.lock().unwrap().push(f),
                        Err(e) => Ide::fail(&errors, e),
                    });
                }
            }
        }
        let every = self.refresh_every.clamp(1.0, 3.0);
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(every));
        if self.stamp_inflight.load(Ordering::Relaxed) || now - self.stamp_asked < every {
            return;
        }
        let mut paths: Vec<String> = self.docs.iter().filter(|d| !d.untitled() && d.table.is_none()).map(|d| d.path.clone()).collect();
        if !self.files_path.is_empty() {
            paths.push(self.files_path.clone());
        }
        paths.sort();
        paths.dedup();
        // Forget what is no longer on the page.
        self.stamps.retain(|p, _| paths.contains(p));
        if paths.is_empty() {
            return;
        }
        self.stamp_asked = now;
        self.stamp_inflight.store(true, Ordering::Relaxed);
        let (slot, inflight) = (self.stamp_slot.clone(), self.stamp_inflight.clone());
        let asked = paths.clone();
        self.backend.query(Query::Stamps { paths }, ctx, move |r: Result<Vec<String>, String>| {
            inflight.store(false, Ordering::Relaxed);
            if let Ok(v) = r {
                *slot.lock().unwrap() = Some(asked.into_iter().zip(v).collect());
            }
        });
    }

    fn list(&mut self, path: &str, ctx: &egui::Context) {
        let (listing, errors, seq) = (self.listing.clone(), self.errors.clone(), self.list_seq.clone());
        let mine = seq.fetch_add(1, Ordering::Relaxed) + 1;
        self.backend.query(Query::ListDir { path: path.to_owned() }, ctx, move |r: Result<Listing, String>| {
            // Only the newest ask lands: an older folder's answer arriving
            // late put Files back on the domain just left (operator,
            // 2026-10-19: "i opened a new domain in cove but it didnt look like
            // the location updated in Files").
            if seq.load(Ordering::Relaxed) != mine {
                return;
            }
            if let Err(e) = &r {
                Ide::fail(&errors, e.clone());
            }
            *listing.lock().unwrap() = Some(r);
        });
    }

    fn open(&self, path: String, ctx: &egui::Context) {
        // A SQLite database: never its bytes as text. There is no Data tab
        // any more (operator, 2026-10-18: "we can remove data as a tab").
        if crate::api::is_sqlite(&path) {
            Ide::fail(&self.errors, format!("{path} is a SQLite database - open it from R (DBI::dbConnect(RSQLite::SQLite(), …)) or Python (sqlite3), or download it"));
            ctx.request_repaint();
            return;
        }
        // An image, a PDF …: a tab that shows it, not its bytes as text.
        if crate::api::preview_type(&path).is_some() {
            self.to_preview.lock().unwrap().push(path);
            ctx.request_repaint();
            return;
        }
        if let Some(i) = self.docs.iter().position(|d| d.path == path) {
            self.opened.lock().unwrap().push(FileText { path, text: self.docs[i].text.clone() });
            return;
        }
        let (opened, errors) = (self.opened.clone(), self.errors.clone());
        self.backend.query(Query::ReadFile { path }, ctx, move |r: Result<FileText, String>| match r {
            Ok(f) => opened.lock().unwrap().push(f),
            Err(e) => Ide::fail(&errors, e),
        });
    }

    /// Show `path` in the Files pane (a launch ?folder=).
    /// Open on `path` and step into the domain it belongs to (?folder=).
    pub fn open_folder_domain(&mut self, path: &str, ctx: &egui::Context) {
        self.open_folder(path, ctx);
        self.folder_domain = Some(path.trim_end_matches('/').to_owned());
    }

    pub fn open_folder(&mut self, path: &str, ctx: &egui::Context) {
        self.files_path = path.to_owned();
        // The old folder's listing goes now: Files read its path back every
        // frame and kept showing it until the new one came.
        *self.listing.lock().unwrap() = None;
        self.show_lower(Lower::Files);
        self.list(path, ctx);
    }

    /// A revision just made, for the app to switch the session to (once).
    pub fn take_switch_env(&self) -> Option<String> {
        self.switch_env.lock().unwrap().take()
    }

    /// A source workspace's own strip in Files (source.rs): what changed
    /// against upstream, Checkpoint, Build candidate, and each candidate's
    /// Use, Test and Open; the candidate job's output under them.
    fn workspace_strip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, path: &str) {
        let now = ctx.input(|i| i.time);
        // Status: on entering the workspace, then every 3 s while shown.
        let shown = self.ws.lock().unwrap().clone();
        let root = shown.as_ref().and_then(|r| r.as_ref().ok()).map(|s| s.root.clone()).unwrap_or_default();
        let inside = !root.is_empty() && (path == root || path.starts_with(&format!("{root}/")));
        if (!inside && self.ws_for != path) || now - self.ws_asked > 3.0 {
            self.ws_for = path.to_owned();
            self.ws_asked = now;
            let slot = self.ws.clone();
            self.backend.query(Query::SourceStatus { path: path.to_owned() }, ctx, move |r: Result<crate::source::SourceStatus, String>| {
                *slot.lock().unwrap() = Some(r);
            });
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        let job = self.ws_job.lock().unwrap().clone();
        if job.as_ref().is_some_and(|j| j.running) && now - self.ws_job_asked > 1.0 {
            self.ws_job_asked = now;
            let slot = self.ws_job.clone();
            self.backend.query(Query::CandidateJob, ctx, move |r: Result<JobView, String>| {
                if let Ok(v) = r {
                    *slot.lock().unwrap() = Some(v);
                }
            });
        }
        let Some(Ok(st)) = shown.filter(|r| r.as_ref().is_ok_and(|s| path == s.root || path.starts_with(&format!("{}/", s.root)))) else { return };
        // The snippet a Test runs starts as "which version is this".
        if self.ws_code.is_empty() || !self.ws_code.contains(&st.name) {
            self.ws_code = if st.kind == "R" {
                format!("library({n}); print(packageVersion(\"{n}\"))", n = st.name)
            } else {
                format!("import importlib.metadata as m; print(m.version(\"{}\"))", st.name)
            };
        }
        let (mut checkpoint, mut build, mut test, mut open_dir, mut use_it) = (false, false, None, None, None);
        let mut revise: Option<String> = None;
        egui::Frame::new().fill(theme::surface()).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("</>").monospace().color(theme::primary()));
                ui.label(RichText::new(format!("{} {} {}", st.kind, st.name, st.version)).strong().color(theme::text_strong()));
                let changed = if st.changed.is_empty() { "as upstream".to_owned() } else { format!("{} changed", st.changed.len()) };
                ui.label(RichText::new(format!("on {} · {changed} · {} checkpoint{}", st.branch, st.checkpoints.len(), if st.checkpoints.len() == 1 { "" } else { "s" })).size(11.5).color(theme::muted()))
                    .on_hover_text(format!("upstream: {}\nchanged:\n{}", st.origin, st.changed.join("\n")));
            });
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut self.ws_msg).hint_text("what this change does").desired_width(220.0));
                checkpoint = ui.add_enabled(st.dirty, egui::Button::new("Checkpoint")).on_hover_text("commit what changed on work - it need not build yet").clicked();
                let busy = job.as_ref().is_some_and(|j| j.running);
                if st.buildable {
                    build = ui
                        .add_enabled(!busy && (!st.changed.is_empty()), theme::primary_button("Build candidate"))
                        .on_hover_text("build the work branch in this session's environment into a candidate of its own - the published environment does not change")
                        .clicked();
                } else {
                    ui.label(RichText::new("its build adapter comes next - read and change it here").size(11.0).color(theme::hush()));
                }
            });
            for c in st.candidates.iter().take(3) {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(if c.ok { "✓" } else { "✗" }).color(if c.ok { theme::ok() } else { theme::alt() }));
                    let short = &c.commit[..12.min(c.commit.len())];
                    ui.label(RichText::new(short).monospace().size(11.5)).on_hover_text(format!(
                        "built {} in {}\nfrom {} (upstream {})\n{}\nsha256 {}",
                        c.built_at,
                        if c.env.is_empty() { "PATH" } else { c.env.as_str() },
                        c.commit,
                        &c.upstream[..12.min(c.upstream.len())],
                        c.artifact,
                        c.sha256
                    ));
                    ui.label(RichText::new(if c.env.is_empty() { "PATH" } else { c.env.as_str() }).size(11.0).color(theme::muted()));
                    if c.ok {
                        let into = if c.kind == "R" { "Use in R" } else { "Use in Python" };
                        if ui.small_button(into).on_hover_text("put this candidate first in the live session").clicked() {
                            use_it = Some(c.clone());
                        }
                        if ui.small_button("Test").on_hover_text("run the snippet below as published, then with this candidate").clicked() {
                            test = Some(c.dir.clone());
                        }
                        if ui.small_button("Make environment").on_hover_text("a new environment revision: the published environment with this candidate ahead of it (and any your current revision carries), recorded and never changed - the session switches to it").clicked() {
                            revise = Some(c.dir.clone());
                        }
                    }
                    if ui.small_button("Open").on_hover_text(&c.dir).clicked() {
                        open_dir = Some(c.dir.clone());
                    }
                });
            }
            if st.candidates.iter().any(|c| c.ok) {
                ui.add(egui::TextEdit::multiline(&mut self.ws_code).code_editor().desired_rows(1).desired_width(f32::INFINITY).hint_text("what Test runs"));
            }
            if let Some(j) = &job {
                let head = match (j.running, j.ok) {
                    (true, _) => format!("{} …", j.what),
                    (false, Some(true)) => format!("{} ✓", j.what),
                    (false, Some(false)) => format!("{} ✗", j.what),
                    _ => j.what.clone(),
                };
                ui.horizontal(|ui| {
                    ui.label(RichText::new(head).size(11.5).color(theme::text()));
                    if ui.small_button("copy").on_hover_text("copy this output to the clipboard").clicked() {
                        ui.ctx().copy_text(j.lines.join("\n"));
                    }
                });
                egui::ScrollArea::vertical().id_salt("ws-job").max_height(160.0).stick_to_bottom(true).show(ui, |ui| {
                    for l in j.lines.iter().rev().take(200).rev() {
                        ui.label(RichText::new(l).monospace().size(10.5).color(theme::muted()));
                    }
                });
            }
        });
        let errors = self.errors.clone();
        let refresh = |me: &mut Self| me.ws_asked = f64::NEG_INFINITY;
        if checkpoint {
            let msg = std::mem::take(&mut self.ws_msg);
            let errors = errors.clone();
            self.backend.query(Query::SourceCheckpoint { path: st.root.clone(), message: msg }, ctx, move |r: Result<String, String>| {
                if let Err(e) = r {
                    Ide::fail(&errors, e);
                }
            });
            refresh(self);
        }
        let start_job = |me: &mut Self, q: Query, ctx: &egui::Context| {
            let (slot, errors) = (me.ws_job.clone(), me.errors.clone());
            me.backend.query(q, ctx, move |r: Result<JobView, String>| match r {
                Ok(v) => *slot.lock().unwrap() = Some(v),
                Err(e) => Ide::fail(&errors, e),
            });
            me.ws_job_asked = f64::NEG_INFINITY;
        };
        if build {
            start_job(self, Query::CandidateBuild { path: st.root.clone() }, ctx);
            refresh(self);
        }
        if let Some(dir) = test {
            start_job(self, Query::CandidateTest { dir, code: self.ws_code.clone() }, ctx);
        }
        if let Some(d) = open_dir {
            self.open_folder(&d, ctx);
        }
        if let Some(dir) = revise {
            let (slot, errors) = (self.switch_env.clone(), self.errors.clone());
            self.backend.query(Query::EnvRevise { dir }, ctx, move |r: Result<crate::source::Revision, String>| match r {
                Ok(rev) => *slot.lock().unwrap() = Some(rev.module),
                Err(e) => Ide::fail(&errors, e),
            });
        }
        if let Some(c) = use_it {
            let lib = c.lib.replace('\\', "\\\\").replace('"', "\\\"");
            if c.kind == "R" {
                let n = &c.name;
                self.send(
                    Lang::R,
                    format!("local({{ lib <- \"{lib}\"; if (\"{n}\" %in% loadedNamespaces()) message(\"{n} is already loaded - Session › Restart R, then Use again\") else {{ .libPaths(c(lib, .libPaths())); message(\"{n} \", format(packageVersion(\"{n}\", lib.loc = lib)), \" from candidate {short} comes first now\") }} }})", short = &c.commit[..12.min(c.commit.len())]),
                    ctx,
                );
            } else {
                self.send(Lang::Python, format!("import sys; sys.path.insert(0, \"{lib}\"); print(\"candidate first on sys.path: {lib}\")"), ctx);
            }
        }
        // A job just finished: what it recorded shows on the next status.
        if job.as_ref().is_some_and(|j| !j.running) && self.ws_job_asked.is_finite() && now - self.ws_job_asked < 1.5 {
            refresh(self);
        }
    }

    /// The account - who you are, your cell, Claude, your domains and API
    /// keys (account_tab.rs) - as Account mode draws it (app.rs, request
    /// #167, 2026-11-01: "account should move out of files and into its own
    /// section").
    pub fn account_pane(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Ide { account, backend, .. } = self;
        account.show(ui, ctx, backend);
    }

    pub fn files(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let current = self.listing.lock().unwrap().clone();
        let path = match &current {
            Some(Ok(l)) => l.path.clone(),
            _ => self.files_path.clone(),
        };
        self.files_path = path.clone();
        let mut go: Option<String> = None;
        let mut download: Option<String> = None;
        // Where the agents are writing (request #165): both panes' running
        // turns, matched to this listing by their place in the project.
        let host = self.domain.as_ref().map(|d| d.host.clone()).unwrap_or_default();
        let (mut changing, now_a) = self.agent.changing();
        let (more, now_b) = self.exec.changing();
        for f in more {
            if !changing.contains(&f) {
                changing.push(f);
            }
        }
        let now = now_a.or(now_b);
        let marks = (changing.len(), !changing.is_empty() || now.is_some());
        if marks != self.agent_marks {
            // Listed again when a new file is written and once more when the
            // run ends, since a step is announced before its write lands.
            go = Some(path.clone());
            self.agent_marks = marks;
        }
        let changing: Vec<String> = changing.iter().map(|f| in_project(&host, f).to_owned()).collect();
        // Drawn icons, with words beside them only when the pane has room
        // (operator, 2026-10-15: "shift to icons on smaller screens").
        let words = ui.available_width() > 360.0;
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(4.0);
            use crate::icons::{tool, Glyph};
            if tool(ui, Glyph::Home, None, "the project folder").clicked() {
                go = Some(String::new());
            }
            if tool(ui, Glyph::Up, None, "up a folder").clicked() {
                go = Some(path.rsplit_once('/').map(|(p, _)| if p.is_empty() { "/".to_owned() } else { p.to_owned() }).unwrap_or_default());
            }
            if tool(ui, Glyph::Refresh, None, "refresh").clicked() {
                go = Some(path.clone());
            }
            theme::tool_sep(ui);
            if tool(ui, Glyph::Upload, words.then_some("Upload"), "copy files from this computer into this folder (or drop them on the window)").clicked() {
                self.upload_pick(path.clone(), ctx);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                // The folder shown, as `ndexr glimpse` prints it, onto the
                // clipboard: to paste into another chat (request #164). The
                // account that sat here is a mode of its own (app.rs, #167).
                let asking = matches!(&*self.glimpse.lock().unwrap(), Some((p, Err(e))) if p == &path && e.is_empty());
                if asking {
                    ui.spinner();
                } else if ui.button(if words { "Copy glimpse" } else { "Glimpse" }).on_hover_text("copy this folder's tree and every file's text (ndexr glimpse) - paste it into any chat").clicked() {
                    *self.glimpse.lock().unwrap() = Some((path.clone(), Err(String::new())));
                    let slot = self.glimpse.clone();
                    let p = path.clone();
                    self.backend.query(Query::Glimpse { path: path.clone() }, ctx, move |r: Result<String, String>| {
                        *slot.lock().unwrap() = Some((p, r));
                    });
                }
            });
            // No "Set As Working Directory": the consoles always work from the
            // open domain's folder (operator, 2026-10-16: "there is no set as
            // working directory with code, you will always work from the
            // domain directory") - open_domain sets it.
        });
        let arrived = match &*self.glimpse.lock().unwrap() {
            Some((p, Ok(text))) => Some((p.clone(), Ok(text.clone()))),
            Some((p, Err(e))) if !e.is_empty() => Some((p.clone(), Err(e.clone()))),
            _ => None,
        };
        if let Some((p, r)) = arrived {
            *self.glimpse.lock().unwrap() = None;
            let now = ctx.input(|i| i.time);
            let said = match r {
                Ok(text) => {
                    let lines = text.lines().count();
                    ctx.copy_text(text);
                    format!("Copied the glimpse of {} - {lines} lines", p.rsplit('/').next().unwrap_or(&p))
                }
                Err(e) => e,
            };
            self.glimpse_said = Some((now, said));
        }
        if let Some((at, said)) = self.glimpse_said.clone() {
            if ctx.input(|i| i.time) - at < 6.0 {
                egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
                    ui.label(RichText::new(said).size(12.5).color(theme::text_strong()));
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(500));
            } else {
                self.glimpse_said = None;
            }
        }
        // Inside a star's source workspace: its changes, checkpoints and
        // candidates (operator, 2026-10-17: "customize it, and use the
        // resulting variant in a recorded environment").
        if path.contains("/cove-source/") {
            self.workspace_strip(ui, ctx, &path);
        }
        if self.files_stale.swap(false, Ordering::Relaxed) {
            go = Some(path.clone());
        }
        // Files dropped on the window go into the folder this pane shows.
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() && !self.hold_shots {
            self.upload_dropped(path.clone(), dropped, ctx);
        }
        egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 3)).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(&path).monospace().size(11.0).color(theme::muted())).truncate());
        });
        if let Some(f) = &now {
            // The file the agent is writing, as it writes it: a click goes to
            // its folder and opens it.
            let pulse = 0.55 + 0.45 * (ctx.input(|i| i.time) * 4.0).sin().abs() as f32;
            ctx.request_repaint_after(std::time::Duration::from_millis(80));
            egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 3)).fill(theme::primary().gamma_multiply(0.10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let r = ui.add(egui::Label::new(RichText::new(format!("✎ agent editing  {}", in_project(&host, f).trim_start_matches('/'))).monospace().size(11.5).color(theme::primary().gamma_multiply(pulse))).truncate().sense(egui::Sense::click()));
                if r.on_hover_text(format!("{f}\nclick to open it")).clicked() {
                    if let Some((dir, _)) = f.rsplit_once('/') {
                        go = Some(dir.to_owned());
                    }
                    self.open(f.clone(), ctx);
                }
            });
        }
        // Both ways: a long name widens its column and the listing shifts
        // sideways to it, instead of cutting it off (operator, 2026-10-15).
        let open_paths: std::collections::HashSet<&str> = self.docs.iter().map(|d| d.path.as_str()).collect();
        let dirty_paths: std::collections::HashSet<&str> = self.docs.iter().filter(|d| d.dirty()).map(|d| d.path.as_str()).collect();
        egui::ScrollArea::both().id_salt("files").auto_shrink([false, false]).show(ui, |ui| match &current {
            None => {
                ui.spinner();
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(theme::alt()));
            }
            Some(Ok(l)) => {
                // RStudio's Files grid: full-width rows, Name · Size · Modified.
                let w = ui.available_width();
                let show_mod = w > 380.0;
                let show_size = w > 240.0;
                let (icon_w, size_w, mod_w) = (24.0, if show_size { 78.0 } else { 0.0 }, if show_mod { 132.0 } else { 0.0 });
                let dl_w = 24.0;
                // The longest name sets the floor: wider than the pane, the
                // rows run past it and the listing scrolls sideways.
                let longest = l.entries.iter().map(|e| ui.painter().layout_no_wrap(e.name.clone(), egui::FontId::monospace(12.0), theme::text()).size().x).fold(0.0f32, f32::max);
                let name_w = (w - icon_w - size_w - mod_w - dl_w - 16.0).max(40.0).max(longest + 22.0);
                ui.set_min_width(icon_w + name_w + size_w + mod_w + dl_w + 16.0);
                let size_x = icon_w + name_w + 4.0;
                let mod_x = size_x + size_w + 12.0;
                let head = |t: &str| RichText::new(t).size(11.0).color(theme::hush());
                let hr = table_head(ui, ROW_H);
                cell(ui, hr, icon_w, name_w, head("Name"), false);
                if show_size {
                    cell(ui, hr, size_x, size_w, head("Size"), true);
                }
                if show_mod {
                    cell(ui, hr, mod_x, mod_w, head("Modified (UTC)"), false);
                }
                let mut i = 0;
                if l.path != "/" {
                    let (r, resp) = table_row(ui, i, ROW_H);
                    glyph_cell(ui, r, crate::icons::Glyph::Up, theme::primary());
                    cell(ui, r, icon_w, name_w, RichText::new("..").monospace().size(12.0).color(theme::text_strong()), false);
                    if resp.on_hover_text("up a folder").clicked() {
                        go = Some(l.path.rsplit_once('/').map(|(p, _)| if p.is_empty() { "/".to_owned() } else { p.to_owned() }).unwrap_or_default());
                    }
                    i += 1;
                }
                for e in &l.entries {
                    let full = format!("{}/{}", l.path.trim_end_matches('/'), e.name);
                    let (icon, color) = file_glyph(&e.name, e.dir);
                    // A name that is only spaces (there is a " " in ~) still shows as something.
                    let shown = if e.name.trim().is_empty() { format!("{:?}", e.name) } else { e.name.clone() };
                    let (r, resp) = table_row(ui, i, ROW_H);
                    crate::tour::mark(ui, &format!("file:{full}"), r);
                    glyph_cell(ui, r, icon, color);
                    // Open in the editor: a light after the name, amber with unsaved edits.
                    let lit = if dirty_paths.contains(full.as_str()) { Some(theme::amber()) } else if open_paths.contains(full.as_str()) { Some(theme::primary()) } else { None };
                    if let Some(c) = lit {
                        let tw = ui.painter().layout_no_wrap(shown.clone(), egui::FontId::monospace(12.0), c).size().x.min(name_w - 10.0);
                        ui.painter().circle_filled(egui::pos2(r.left() + icon_w + tw + 7.0, r.center().y), 3.0, c);
                    }
                    // A bar at the row's edge: the agent is changing this file,
                    // or a file somewhere inside this folder (request #165).
                    let rel = in_project(&host, &full);
                    let inside = changing.iter().any(|c| c == rel || (e.dir && c.starts_with(rel) && c[rel.len()..].starts_with('/')));
                    if inside {
                        let hot = now.as_deref().is_some_and(|n| { let n = in_project(&host, n); n == rel || (e.dir && n.starts_with(rel) && n[rel.len()..].starts_with('/')) });
                        let c = if hot { theme::primary() } else { theme::primary().gamma_multiply(0.5) };
                        ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(3.0, r.height())), 0.0, c);
                    }
                    cell(ui, r, icon_w, name_w - 12.0, RichText::new(shown).monospace().size(12.0).color(if e.dir { theme::text_strong() } else { theme::text() }), false);
                    if !e.dir && show_size {
                        cell(ui, r, size_x, size_w, RichText::new(human_bytes(e.bytes)).monospace().size(11.0).color(theme::muted()), true);
                    }
                    if show_mod {
                        cell(ui, r, mod_x, mod_w, RichText::new(utc_minute(e.modified)).monospace().size(11.0).color(theme::hush()), false);
                    }
                    // ⤓ at the row's end: this file to this computer (a folder as .tar.gz).
                    let dl = egui::Rect::from_min_size(egui::pos2(r.max.x - dl_w, r.min.y), egui::vec2(dl_w, r.height()));
                    let what = if e.dir { "download this folder (.tar.gz)" } else { "download to this computer" };
                    let dr = ui.interact(dl, resp.id.with("dl"), egui::Sense::click());
                    crate::icons::paint(ui.painter(), egui::Rect::from_center_size(dl.center(), egui::vec2(13.0, 13.0)), crate::icons::Glyph::Download, if dr.hovered() { theme::text_strong() } else { theme::primary() });
                    if dr.on_hover_text(what).clicked() {
                        download = Some(full.clone());
                    }
                    resp.context_menu(|ui| {
                        if ui.button(if e.dir { "Download folder (.tar.gz)" } else { "Download" }).clicked() {
                            download = Some(full.clone());
                            ui.close();
                        }
                    });
                    let hover = if inside { format!("{full}\nthe agent is changing {}", if e.dir { "files in here" } else { "this file" }) } else { full.clone() };
                    if resp.on_hover_text(hover).clicked() {
                        if e.dir {
                            go = Some(full.clone());
                        } else {
                            self.open(full.clone(), ctx);
                        }
                    }
                    i += 1;
                }
                if l.entries.is_empty() {
                    ui.label(RichText::new("empty folder").italics().color(theme::hush()));
                }
            }
        });
        if let Some(p) = download {
            self.download(p, ctx);
        }
        if let Some(p) = go {
            self.list(&p, ctx);
        }
    }

    // ── transfers (operator, 2026-10-13: download and upload through the ide) ──

    /// `path` (a file, or a folder as .tar.gz) to this computer. The desktop
    /// app asks where to save it, starting in Downloads; a browser downloads
    /// it itself from the server's /download.
    fn download(&self, path: String, ctx: &egui::Context) {
        #[cfg(target_arch = "wasm32")]
        {
            ctx.open_url(egui::OpenUrl::same_tab(self.backend.download_url(&path)));
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (errors, ctx, shown) = (self.errors.clone(), ctx.clone(), path.clone());
            self.backend.download(&path, move |r| {
                let msg = match r {
                    Err(e) => format!("download {shown}: {e}"),
                    Ok((name, bytes)) => {
                        let downloads = std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join("Downloads"));
                        let mut dialog = rfd::AsyncFileDialog::new().set_title("Save download").set_file_name(&name);
                        if let Some(d) = downloads.filter(|d| d.is_dir()) {
                            dialog = dialog.set_directory(d);
                        }
                        match pollster::block_on(dialog.save_file()) {
                            None => return,
                            Some(f) => match std::fs::write(f.path(), &bytes) {
                                Ok(()) => format!("saved {} ({})", f.path().display(), human_bytes(bytes.len() as u64)),
                                Err(e) => format!("could not save {}: {e}", f.path().display()),
                            },
                        }
                    }
                };
                Ide::fail(&errors, msg);
                ctx.request_repaint();
            });
        }
    }

    /// Files › Upload: pick files on this computer, copy them into `dir`.
    fn upload_pick(&self, dir: String, ctx: &egui::Context) {
        let (backend, errors, stale, ctx) = (self.backend.clone(), self.errors.clone(), self.files_stale.clone(), ctx.clone());
        let pick = async move {
            let Some(files) = crate::pick::files("Upload to Cove", true).await else { return };
            for f in files {
                let name = f.file_name();
                let bytes = f.read().await;
                Ide::upload_one(&backend, &dir, name, bytes, &errors, &stale, &ctx);
            }
        };
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || pollster::block_on(pick));
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(pick);
    }

    /// Files dropped on the window (egui hands over handles: a path and the
    /// bytes on the desktop, a browser File on the web).
    fn upload_dropped(&self, dir: String, dropped: Vec<egui::DroppedFileHandle>, ctx: &egui::Context) {
        for f in dropped {
            let name = f.path().file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "upload".into());
            let (backend, errors, stale, ctx, dir) = (self.backend.clone(), self.errors.clone(), self.files_stale.clone(), ctx.clone(), dir.clone());
            #[cfg(not(target_arch = "wasm32"))]
            {
                if f.path().is_dir() {
                    Ide::fail(&errors, format!("{name} is a folder - upload its files, or a .zip / .tar.gz of it"));
                    continue;
                }
                std::thread::spawn(move || match f.bytes() {
                    Ok(bytes) => Ide::upload_one(&backend, &dir, name, bytes, &errors, &stale, &ctx),
                    Err(e) => Ide::fail(&errors, format!("{name}: {e}")),
                });
            }
            #[cfg(target_arch = "wasm32")]
            wasm_bindgen_futures::spawn_local(async move {
                match f.bytes_async().await {
                    Ok(bytes) => Ide::upload_one(&backend, &dir, name, bytes, &errors, &stale, &ctx),
                    Err(e) => Ide::fail(&errors, format!("{name}: {e}")),
                }
            });
        }
    }

    fn upload_one(backend: &Backend, dir: &str, name: String, bytes: Vec<u8>, errors: &Arc<Mutex<Vec<String>>>, stale: &Arc<AtomicBool>, ctx: &egui::Context) {
        let n = bytes.len() as u64;
        let (errors, stale, shown) = (errors.clone(), stale.clone(), name.clone());
        backend.upload(dir, &name, bytes, ctx, move |r| {
            Ide::fail(
                &errors,
                match r {
                    Ok(to) => format!("uploaded {to} ({})", human_bytes(n)),
                    Err(e) => format!("upload {shown}: {e}"),
                },
            );
            stale.store(true, Ordering::Relaxed);
        });
    }

    pub fn plots(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let lang = self.data_lang();
        let n = self.consoles[ix(lang)].view.lock().unwrap().plots;
        let sel = self.plot_sel[ix(lang)].unwrap_or(n.saturating_sub(1)).min(n.saturating_sub(1));
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(4.0);
            if theme::tool_if(ui, sel > 0, "◀", "previous plot").clicked() {
                self.plot_sel[ix(lang)] = Some(sel - 1);
            }
            if theme::tool_if(ui, sel + 1 < n, "▶", "next plot").clicked() {
                self.plot_sel[ix(lang)] = Some(sel + 1);
            }
            theme::tool_sep(ui);
            if n > 0 {
                ui.label(RichText::new(format!("{} / {n}", sel + 1)).monospace().size(11.0).color(theme::muted()));
            }
        });
        let lang = self.data_lang();
        let n = self.consoles[ix(lang)].view.lock().unwrap().plots;
        if n == 0 {
            let hint = match lang {
                Lang::R | Lang::Python => "No plots yet. Base graphics, lattice and ggplot2 draw here, and matplotlib (seaborn, pandas .plot) through reticulate.",
                Lang::Julia => "No plots yet. Plots.jl, CairoMakie and anything else that renders to PNG draw here.",
                Lang::Shell => "",
            };
            ui.add_space(8.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(hint).color(theme::hush())));
            return;
        }
        let sel = self.plot_sel[ix(lang)].unwrap_or(n - 1).min(n - 1);
        let key = (lang, sel);
        if let Some(r) = self.plot_img.lock().unwrap().remove(&key) {
            match r {
                Ok(img) => {
                    let t = ctx.load_texture(format!("plot-{}-{sel}", lang.word()), img, egui::TextureOptions::LINEAR);
                    self.plot_tex.insert(key, t);
                }
                Err(e) => Ide::fail(&self.errors, format!("plot {}: {e}", sel + 1)),
            }
        }
        match self.plot_tex.get(&key) {
            Some(t) => {
                let avail = ui.available_size();
                ui.centered_and_justified(|ui| {
                    ui.add(egui::Image::new(t).max_size(avail).maintain_aspect_ratio(true));
                });
            }
            None => {
                ui.spinner();
                if !self.plot_asked.contains_key(&key) {
                    self.plot_asked.insert(key, true);
                    let img = self.plot_img.clone();
                    self.backend.query(Query::Plot { lang, n: sel }, ctx, move |r: Result<PlotData, String>| {
                        let decoded = r.and_then(|p| {
                            let bytes = api::b64_decode(&p.png_b64).ok_or("bad plot data")?;
                            let rgba = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).map_err(|e| e.to_string())?.to_rgba8();
                            Ok(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
                        });
                        img.lock().unwrap().insert(key, decoded);
                    });
                }
            }
        }
    }

    /// The Software tab: Packages (Find, Tree) and Builds (Curves, Timeline, Queue).
    /// Packages (0), Builds (1) or Install (2): one Software, three tabs.
    /// The Builds mode's strip: where every build machine is now (builds.rs).
    pub fn builds_now(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Ide { software, backend, .. } = self;
        software.where_builds_are(ui, ctx, backend);
    }

    /// Builds mode's pane: the whole stack, what it has and how it got built.
    pub fn stack(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.software(ui, ctx, crate::software::STACK);
    }

    pub fn software(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, group: usize) {
        let act = self.software.ui(ui, ctx, &self.backend, group);
        if let Some(crate::software::Action::Run(lang, code)) = act {
            self.bring(Tab::of_lang(lang));
            self.send(lang, code, ctx);
        }
    }

    fn domain_picker(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.ask_domains(ctx);
        let mut refresh = false;
        let typed = self.domain_filter.trim().to_ascii_lowercase();
        let offers: Vec<String> = match self.domains.lock().unwrap().as_ref() {
            Some(Ok(v)) => new_domain_offers(&typed, v),
            _ => Vec::new(),
        };
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.label(RichText::new("Open a domain").size(12.0).color(theme::text_strong()));
            theme::tool_sep(ui);
            ui.add(egui::TextEdit::singleline(&mut self.domain_filter).hint_text("find one, or type a new name").desired_width(200.0));
            for h in offers.iter().take(3) {
                if theme::tool(ui, &format!("+ Create {h}…"), "choose what kind of app it is, then create it").clicked() {
                    self.choosing = Some(Choosing::new(h));
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                refresh = theme::tool(ui, "⟳", "list again").clicked();
            });
        });
        if refresh {
            self.domains_asked = false;
            *self.domains.lock().unwrap() = None;
        }
        let have = self.domains.lock().unwrap().clone();
        let filter = self.domain_filter.trim().to_lowercase();
        let mut open = None;
        egui::ScrollArea::vertical().id_salt("domains").auto_shrink([false, false]).show(ui, |ui| match &have {
            None => {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.spinner();
                    ui.label(RichText::new("finding your domains…").color(theme::muted()));
                });
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(theme::alt()));
            }
            Some(Ok(v)) if v.is_empty() => {
                ui.add_space(8.0);
                ui.vertical_centered(|ui| ui.label(RichText::new("No domains under the project folder. Point it at your domains/ folder in Tools › Global Options.").color(theme::hush())));
            }
            Some(Ok(v)) => {
                let w = ui.available_width();
                let host_w = (w * 0.42).clamp(140.0, 320.0);
                let state_w = 84.0;
                let title_x = 8.0 + host_w + 10.0;
                let title_w = (w - title_x - state_w - 12.0).max(0.0);
                let hit = |d: &DomainInfo| filter.is_empty() || d.host.contains(&filter) || d.title.to_lowercase().contains(&filter);
                let current_host = self.domain.as_ref().map(|c| c.host.clone()).unwrap_or_default();
                let mut toggle = None;
                let mut i = 0;
                // While finding, every match shows with the parents it sits under.
                for (k, depth, kids) in domain_tree(v, &hit, |h| !filter.is_empty() || self.domains_unfolded.contains(h) || current_host.ends_with(&format!(".{h}"))) {
                    let d = &v[k];
                    let current = current_host == d.host;
                    let (r, resp) = table_row(ui, i, 26.0);
                    let indent = 8.0 + 16.0 * depth as f32;
                    let open_here = !filter.is_empty() || self.domains_unfolded.contains(&d.host) || current_host.ends_with(&format!(".{}", d.host));
                    if kids > 0 {
                        cell(ui, r, indent, 14.0, RichText::new(if open_here { "▾" } else { "▸" }).size(11.0).color(theme::muted()), false);
                    }
                    let name = if kids > 0 { format!("{}  ({kids})", d.host) } else { d.host.clone() };
                    cell(ui, r, indent + 16.0, (host_w - indent - 8.0).max(60.0), RichText::new(name).monospace().size(12.5).color(if current { theme::amber() } else { theme::primary() }), false);
                    if title_w > 40.0 {
                        cell(ui, r, title_x, title_w, RichText::new(&d.title).size(12.0).color(theme::muted()), false);
                    }
                    let state = if d.module { "open ▸" } else { "no module.r" };
                    cell(ui, r, w - state_w - 8.0, state_w, RichText::new(state).size(11.0).color(if d.module { theme::ok() } else { theme::hush() }), true);
                    // The ▸ folds and unfolds its sub-subdomains; the rest of the row opens it.
                    let on_arrow = resp.interact_pointer_pos().is_some_and(|p| p.x < r.left() + indent + 16.0);
                    if resp.on_hover_text(&d.path).clicked() {
                        if kids > 0 && on_arrow {
                            toggle = Some(d.host.clone());
                        } else {
                            open = Some(d.clone());
                        }
                    }
                    i += 1;
                }
                if let Some(h) = toggle {
                    if !self.domains_unfolded.remove(&h) {
                        self.domains_unfolded.insert(h);
                    }
                }
            }
        });
        if let Some(d) = open {
            self.open_domain(d, ctx);
        }
    }

    /// The environment a domain's `.ndexr-env` names now (Pin), here and in
    /// the picker's list, so stepping back in finds the new one.
    pub fn set_domain_env(&mut self, host: &str, env: &str) {
        if let Some(d) = self.domain.as_mut().filter(|d| d.host == host) {
            d.env = env.to_owned();
        }
        if let Some(Ok(all)) = self.domains.lock().unwrap().as_mut() {
            for d in all.iter_mut().filter(|d| d.host == host) {
                d.env = env.to_owned();
            }
        }
    }

    /// The person's domains for the atlas (atlas.rs), asked on first look.
    pub fn domain_list(&mut self, ctx: &egui::Context) -> Option<Result<Vec<DomainInfo>, String>> {
        self.ask_domains(ctx);
        self.domains.lock().unwrap().clone()
    }

    /// The stack's catalog for the atlas, kept current while it is looked at:
    /// it had been fetched once per session, so the front page drew the stack
    /// as it was at sign-in while catalog.json was rewritten every minute
    /// (operator, 2026-10-18: "make sure that we're always keeping up with
    /// live information"). The same 2-minute ETag'd check Builds makes.
    pub fn software_catalog(&mut self, ctx: &egui::Context) -> Option<std::sync::Arc<crate::software::Catalog>> {
        self.software.load(&self.backend, ctx);
        self.software.refresh(&self.backend, ctx);
        self.software.catalog()
    }

    /// The atlas's "Open in Software": that software, in front in Builds
    /// (app.rs opens the mode).
    pub fn reveal_software(&mut self, i: usize) {
        self.software.reveal(i);
    }

    /// Ask ndexr about `names` (and the shares waiting for me), once per set.
    fn ask_names(&mut self, ctx: &egui::Context, names: Vec<String>) {
        let key = names.join(",");
        if self.names_asked.as_deref() == Some(key.as_str()) {
            return;
        }
        self.names_asked = Some(key);
        let slot = self.names.clone();
        self.backend.query(Query::ProjectNames { names }, ctx, move |r: Result<api::ProjectNames, String>| {
            if let Ok(v) = r {
                *slot.lock().unwrap() = Some(v);
            }
        });
    }

    /// Who holds `host`, when someone else does.
    fn taken_by(&self, host: &str) -> Option<String> {
        self.names.lock().unwrap().as_ref()?.taken.iter().find(|t| t.host == host).map(|t| t.owner.clone())
    }

    /// The shares waiting for me.
    fn offered(&self) -> Vec<api::OfferedShare> {
        self.names.lock().unwrap().as_ref().map(|n| n.offered.clone()).unwrap_or_default()
    }

    /// Take a share and open it once it is mounted here.
    fn accept_share(&mut self, s: &api::OfferedShare, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.pending_open = Some((s.domain.clone(), now, 0.0));
        self.names_asked = None;
        self.backend.query(Query::InboxAct { act: api::InboxAct::ShareDecide { id: s.id, accept: true } }, ctx, |_: Result<serde_json::Value, String>| {});
    }

    /// A share accepted: open it when it is among my projects; ask for them
    /// again every 2 s until then, for 40 s.
    fn open_pending(&mut self, ctx: &egui::Context) {
        let Some((host, since, asked)) = self.pending_open.clone() else { return };
        let now = ctx.input(|i| i.time);
        let found = match &*self.domains.lock().unwrap() {
            Some(Ok(v)) => v.iter().find(|d| d.host == host).cloned(),
            _ => None,
        };
        if let Some(d) = found {
            self.pending_open = None;
            self.open_domain(d, ctx);
        } else if now - since > 40.0 {
            self.pending_open = None;
        } else if now - asked > 2.0 {
            self.pending_open = Some((host, since, now));
            self.domains_asked = false;
            self.ask_domains(ctx);
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
    }

    fn ask_domains(&mut self, ctx: &egui::Context) {
        if self.domains_asked {
            return;
        }
        self.domains_asked = true;
        let slot = self.domains.clone();
        self.backend.query(Query::ListDomains, ctx, move |r: Result<Vec<DomainInfo>, String>| {
            *slot.lock().unwrap() = Some(r);
        });
    }

    /// Where new domains go and what Push all sweeps: the owner folder the
    /// open domain sits in (domains/<owner>/), else the folder the domains
    /// were listed from.
    fn domains_root(&self) -> String {
        let parent = |p: &str| p.rsplit_once('/').map(|(a, _)| a.to_owned()).unwrap_or_default();
        if let Some(d) = &self.domain {
            return parent(&d.path);
        }
        match self.domains.lock().unwrap().as_ref() {
            Some(Ok(v)) if !v.is_empty() => parent(&v[0].path),
            _ => String::new(),
        }
    }

    /// The project button: the open project's name, and a tap opens the
    /// Projects dialog (project_dialog), the one place to go to a project or
    /// create one.
    pub fn domain_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, width: f32, sessions: impl FnOnce(&mut egui::Ui)) {
        self.ask_domains(ctx);
        self.open_pending(ctx);
        // The host, plus what serves it when that is not its own module.r
        // (src/r/nginx/target.r): "svbt.ndexr.io · port:8001".
        let served = |d: &DomainInfo| if d.target.is_empty() || d.target == "module" { d.host.clone() } else { format!("{} · {}", d.host, d.target) };
        let mut label = if self.ndexr_open { "ndexr".to_owned() } else { self.domain.as_ref().map_or("Project…".to_owned(), served) };
        // The open project's framework mark in place of the house, so what
        // kind of app this is shows as projects change (operator, 2026-10-28).
        // A project opened from a session or a new create may not carry it:
        // the listing's own record for the host does.
        let fw = match (&self.domain, self.ndexr_open) {
            (Some(d), false) if !d.framework.is_empty() => d.framework.clone(),
            (Some(d), false) => match &*self.domains.lock().unwrap() {
                Some(Ok(all)) => all.iter().find(|x| x.host == d.host).map_or(String::new(), |x| x.framework.clone()),
                _ => String::new(),
            },
            _ => String::new(),
        };
        let marked = crate::frameworks::icon(ui, &fw, 20.0).is_some();
        let house = if marked { "" } else { "⌂ " };
        // Within `width`: on a phone the project shares a row with the
        // panes, and its full name and port pushed the App pane's button off
        // the screen (request #188: "can't see the top bar when on mobile
        // vertical for going to the app, which is more important than
        // showing the port"). What serves it goes first, then the name is
        // cut; the dialog lists it in full.
        let font = egui::FontId::monospace(12.0);
        let fits = |t: &str| ui.ctx().fonts_mut(|f| f.layout_no_wrap(format!("{house}{t} ▾"), font.clone(), egui::Color32::WHITE).size().x) + 16.0 <= width;
        if !fits(&label) {
            if let Some((host, _)) = label.split_once(" · ") {
                label = host.to_owned();
            }
            while !fits(&label) && label.chars().count() > 4 {
                let keep = label.trim_end_matches('…').chars().count().saturating_sub(1);
                label = format!("{}…", label.chars().take(keep).collect::<String>());
            }
        }
        let text = RichText::new(format!("{house}{label} ▾")).monospace().size(12.0).color(if self.domain.is_some() { theme::text_strong() } else { theme::primary() });
        let r = ui.add(egui::Button::new(text).frame(false));
        crate::tour::mark(ui, "project-button", r.rect);
        if r.on_hover_text("your projects: type a name to go to one, or to create it").clicked() && self.choosing.is_none() {
            self.naming = Some(String::new());
        }
        self.bar_frame = ctx.cumulative_frame_nr();
        self.project_dialog(ctx, Some(sessions));
    }

    /// The open projects as a browser's tabs, one row (tabs.rs; operator,
    /// 2026-11-14: "act like a browser with domains as tabs, and it should
    /// have a full tab menu"). A click brings a tab forward, a middle click
    /// closes it, a right click opens its menu; + is a new tab (Projects) and
    /// ⌄ lists every tab and the ones closed lately.
    pub fn tab_strip(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.ask_domains(ctx);
        let listed: Vec<DomainInfo> = match &*self.domains.lock().unwrap() {
            Some(Ok(v)) => v.clone(),
            _ => Vec::new(),
        };
        self.tabs.keep_listed(&listed.iter().map(|d| d.host.clone()).collect::<Vec<_>>());
        if let Some(d) = &self.domain {
            if !self.tabs.open.contains(&d.host) {
                let h = d.host.clone();
                self.tabs.opened(&h, None);
            }
        }
        let active = if self.ndexr_open { None } else { self.domain.as_ref().map(|d| d.host.clone()) };
        let mut act: Option<TabAct> = None;
        let small = ui.ctx().content_rect().width() < 600.0;
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let tail = 56.0;
            let room = (ui.available_width() - tail).max(80.0);
            egui::ScrollArea::horizontal().id_salt("project-tabs").max_width(room).auto_shrink([true, true]).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let n = self.tabs.open.len().max(1) as f32;
                    // As a browser does: tabs share the row, wide while few.
                    let w = (room / n - 2.0).clamp(if small { 84.0 } else { 110.0 }, 200.0);
                    for host in self.tabs.open.clone() {
                        let on = active.as_deref() == Some(host.as_str());
                        let pinned = self.tabs.is_pinned(&host);
                        let info = listed.iter().find(|d| d.host == host);
                        let r = project_tab(ui, &host, info, on, pinned, self.live_off.contains(&host), if pinned { 40.0 } else { w }, small);
                        if r.close {
                            act = Some(TabAct::Close(vec![host.clone()]));
                        } else if r.resp.middle_clicked() {
                            act = Some(TabAct::Close(vec![host.clone()]));
                        } else if r.resp.clicked() && !on {
                            act = Some(TabAct::Switch(host.clone()));
                        }
                        let full = info.map_or(host.clone(), |d| if d.title.is_empty() { d.host.clone() } else { format!("{} · {}", d.host, d.title) });
                        let resp = r.resp.on_hover_text(format!("{full}\nright-click: the tab menu · middle-click: close"));
                        resp.context_menu(|ui| {
                            ui.set_min_width(230.0);
                            if let Some(a) = self.tab_menu(ui, &host, on) {
                                act = Some(a);
                            }
                        });
                    }
                });
            });
            if theme::tool(ui, "+", "new tab - go to a project, or create one (Projects)").clicked() {
                act = Some(TabAct::New);
            }
            ui.menu_button(RichText::new("⌄").size(13.0).color(theme::text_strong()), |ui| {
                ui.set_min_width(260.0);
                ui.label(RichText::new(format!("{} open", self.tabs.open.len())).size(12.0).strong().color(theme::text_strong()));
                for host in self.tabs.open.clone() {
                    let on = active.as_deref() == Some(host.as_str());
                    let mark = if self.tabs.is_pinned(&host) { " · pinned" } else { "" };
                    if ui.selectable_label(on, RichText::new(format!("{host}{mark}")).monospace().size(12.0)).clicked() {
                        if !on {
                            act = Some(TabAct::Switch(host.clone()));
                        }
                        ui.close();
                    }
                }
                ui.separator();
                if ui.button("+ New tab").clicked() {
                    act = Some(TabAct::New);
                    ui.close();
                }
                let last = self.tabs.closed.last().cloned();
                if ui.add_enabled(last.is_some(), egui::Button::new(format!("Reopen closed tab{}", last.as_ref().map_or(String::new(), |h| format!(" · {}", crate::tabs::Tabs::short(h)))))).clicked() {
                    act = Some(TabAct::Reopen);
                    ui.close();
                }
                if !self.tabs.closed.is_empty() {
                    ui.menu_button("Recently closed", |ui| {
                        for h in self.tabs.closed.iter().rev() {
                            if ui.button(RichText::new(h).monospace().size(12.0)).clicked() {
                                act = Some(TabAct::Switch(h.clone()));
                                ui.close();
                            }
                        }
                    });
                }
                if let Some(a) = active.clone() {
                    let others = self.tabs.others(&a);
                    if ui.add_enabled(!others.is_empty(), egui::Button::new(format!("Close {} other tab{}", others.len(), if others.len() == 1 { "" } else { "s" }))).clicked() {
                        act = Some(TabAct::Close(others));
                        ui.close();
                    }
                }
            });
        });
        if let Some(a) = act {
            self.tab_act(a, active.as_deref(), ctx);
        }
    }

    /// One tab's menu, as a browser's: what it opens, its place, what closes.
    fn tab_menu(&self, ui: &mut egui::Ui, host: &str, on: bool) -> Option<TabAct> {
        let mut act = None;
        let mut item = |ui: &mut egui::Ui, enabled: bool, label: String, a: TabAct| {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                act = Some(a);
                ui.close();
            }
        };
        let h = host.to_owned();
        ui.label(RichText::new(host).monospace().size(12.0).strong().color(theme::text_strong()));
        ui.separator();
        item(ui, true, "New tab".into(), TabAct::New);
        item(ui, true, if on { "Reload the site".into() } else { "Go to this tab".into() }, if on { TabAct::Reload } else { TabAct::Switch(h.clone()) });
        item(ui, true, "Open the site in a browser tab ↗".into(), TabAct::Outside(h.clone()));
        item(ui, true, "Copy its address".into(), TabAct::Copy(h.clone()));
        ui.separator();
        let pinned = self.tabs.is_pinned(host);
        item(ui, true, if pinned { "Unpin tab".into() } else { "Pin tab".into() }, TabAct::Pin(h.clone()));
        let live = !self.live_off.contains(host);
        item(ui, true, if live { "Pause live reload".into() } else { "Resume live reload".into() }, TabAct::Live(h.clone()));
        item(ui, true, "Move to start".into(), TabAct::Move(h.clone(), true));
        item(ui, true, "Move to end".into(), TabAct::Move(h.clone(), false));
        ui.separator();
        item(ui, true, "Close tab".into(), TabAct::Close(vec![h.clone()]));
        let others = self.tabs.others(host);
        item(ui, !others.is_empty(), "Close other tabs".into(), TabAct::Close(others));
        let right = self.tabs.right_of(host);
        item(ui, !right.is_empty(), "Close tabs to the right".into(), TabAct::Close(right));
        let left = self.tabs.left_of(host);
        item(ui, !left.is_empty(), "Close tabs to the left".into(), TabAct::Close(left));
        ui.separator();
        let last = self.tabs.closed.last().cloned();
        item(ui, last.is_some(), format!("Reopen closed tab{}", last.as_ref().map_or(String::new(), |h| format!(" · {}", crate::tabs::Tabs::short(h)))), TabAct::Reopen);
        act
    }

    fn tab_act(&mut self, a: TabAct, active: Option<&str>, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        match a {
            TabAct::Switch(h) => self.pending_open = Some((h, now, 0.0)),
            TabAct::New => {
                if self.choosing.is_none() {
                    self.naming = Some(String::new());
                }
            }
            TabAct::Reopen => {
                if let Some(h) = self.tabs.reopen() {
                    self.pending_open = Some((h, now, 0.0));
                }
            }
            TabAct::Reload => self.site_seq += 1,
            TabAct::Outside(h) => ctx.open_url(egui::OpenUrl::new_tab(format!("https://{h}/"))),
            TabAct::Copy(h) => ctx.copy_text(format!("https://{h}/")),
            TabAct::Pin(h) => self.tabs.toggle_pin(&h),
            TabAct::Live(h) => {
                if !self.live_off.remove(&h) {
                    self.live_off.insert(h);
                }
            }
            TabAct::Move(h, start) => self.tabs.move_to(&h, start),
            TabAct::Close(hosts) => match self.tabs.close(&hosts, active) {
                Some(next) => self.pending_open = Some((next, now, 0.0)),
                // The tab in front closed and none is left: no project, as a
                // browser's empty window - Projects opens to pick the next.
                None if active.is_some_and(|a| hosts.iter().any(|h| h == a)) => {
                    self.park_domain();
                    self.domain = None;
                    self.list("", ctx);
                    if self.choosing.is_none() {
                        self.naming = Some(String::new());
                    }
                }
                None => {}
            },
        }
    }

    fn ask_versions(&mut self, ctx: &egui::Context) {
        let Some(d) = &self.domain else { return };
        let slot = self.versions.clone();
        *slot.lock().unwrap() = None;
        self.backend.query(Query::Versions { host: d.host.clone() }, ctx, move |r: Result<Vec<crate::api::Version>, String>| {
            *slot.lock().unwrap() = Some(r);
        });
    }

    /// The open project's Git window (operator, 2026-10-30: "just create a
    /// basic git viewer in cove instead for now, think pycharm"; before it,
    /// 2026-10-25's Versions list: "get older versions ... they dont even
    /// need to have the git repos"): its history, each version's files and
    /// each file's diff (gitview.rs), and Restore - which keeps what is there
    /// now as a version before it puts the old one back. Saved every 30
    /// minutes by the autosave service, mirrored to the private S3 archive.
    fn versions_window(&mut self, ctx: &egui::Context) {
        if !self.versions_open {
            return;
        }
        let Some(d) = self.domain.clone() else { return };
        let list = self.versions.lock().unwrap().clone();
        let said = self.version_said.lock().unwrap().clone();
        let mut open = true;
        let mut act = crate::gitview::GitAct::None;
        // As big as the screen allows: three panes need the room. On a phone
        // it is the screen.
        let screen = ctx.content_rect().size();
        let size = egui::vec2((screen.x - 24.0).clamp(300.0, 1320.0), (screen.y - 90.0).clamp(300.0, 800.0));
        // In front: app.rs hides the App pane's page (an HTML layer over the
        // canvas) for any large foreground area, and a window in the middle
        // layer had the page drawn over its diff.
        egui::Window::new(format!("Git · {}", d.host))
            .id(egui::Id::new("git-window"))
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 12.0))
            .show(ctx, |ui| {
                ui.set_min_size(size);
                act = self.git.ui(ui, &self.backend, &d.host, &list, &said);
            });
        match act {
            crate::gitview::GitAct::Restore(sha) => {
                // The history it showed is about to grow: start it again.
                self.git = Default::default();
                let (said, versions, backend, host) = (self.version_said.clone(), self.versions.clone(), self.backend.clone(), d.host.clone());
                *said.lock().unwrap() = None;
                let ctx2 = ctx.clone();
                self.backend.query(Query::VersionRestore { host: host.clone(), sha }, ctx, move |r: Result<String, String>| {
                    *said.lock().unwrap() = Some(r);
                    *versions.lock().unwrap() = None;
                    backend.query(Query::Versions { host }, &ctx2, move |r: Result<Vec<crate::api::Version>, String>| {
                        *versions.lock().unwrap() = Some(r);
                    });
                });
            }
            crate::gitview::GitAct::Refresh => {
                *self.version_said.lock().unwrap() = None;
                self.ask_versions(ctx);
            }
            crate::gitview::GitAct::None => {}
        }
        if !open {
            self.versions_open = false;
        }
    }

    /// The environment the consoles run in (app.rs, whenever it changes):
    /// the switcher follows it, and a console it does not bring is left.
    pub fn set_env(&mut self, module: &str) {
        if self.env_module.as_deref() == Some(module) {
            return;
        }
        self.env_module = Some(module.to_owned());
        self.env_langs = env_consoles(module);
        if let Channel::Code(l) = self.chan {
            if !self.env_langs.contains(&l) {
                self.chan = Channel::Code(self.env_langs[0]);
            }
        }
    }

    /// Whether the environment brings this console.
    pub fn has_console(&self, l: Lang) -> bool {
        self.env_langs.contains(&l)
    }

    /// The switcher's ring: CHANNELS, in their order, that the environment brings.
    fn ring(&self) -> Vec<Channel> {
        CHANNELS.iter().copied().filter(|c| matches!(c, Channel::Code(l) if self.env_langs.contains(l))).collect()
    }

    /// The person's share of the machine, from the open project's last status
    /// (apps.rs `status`): shown in the runtime inspector (app.rs).
    pub fn app_quota(&self) -> Option<crate::api::Quota> {
        match &*self.app_status.lock().unwrap() {
            Some(Ok(s)) => s.quota.clone(),
            _ => None,
        }
    }

    /// Build, Up or Down the open project (Query::AppAction, apps.rs): its
    /// own build.sh / up.sh / down.sh, run as the person; followed by app_strip.
    pub fn app_action(&mut self, verb: &str, ctx: &egui::Context) {
        let Some(d) = &self.domain else { return };
        let (slot, errors) = (self.app_job.clone(), self.errors.clone());
        *slot.lock().unwrap() = Some(Ok(JobView { what: format!("{verb} {}", d.host), running: true, ..Default::default() }));
        self.app_job_at = ctx.input(|i| i.time);
        self.app_verb = verb.to_owned();
        self.app_routed = false;
        self.app_counted = false;
        *self.app_route.lock().unwrap() = None;
        self.backend.query(Query::AppAction { host: d.host.clone(), verb: verb.to_owned() }, ctx, move |r: Result<JobView, String>| {
            if let Err(e) = &r {
                Ide::fail(&errors, format!("not started: {e}"));
            }
            *slot.lock().unwrap() = Some(r);
        });
    }

    /// The open project's strip, by the app contract (APP-CONTRACT.md): its
    /// language and kind from app.env, a button for each of build.sh /
    /// up.sh / down.sh it has, whether APP_HEALTH answers on its port, and the
    /// job's last line - the whole log a click away. After an Up that worked,
    /// a published domain is routed to the port (Query::AppRoute).
    /// The strip earns its row only when there is something to act on or to
    /// know now (operator, 2026-10-26: "surface what the user can act on now;
    /// keep diagnostic information one interaction away"): a program of its
    /// own (Build/Up/Down and whether it is up), a compile or a job running, a
    /// job that failed, or nothing built yet. A Shiny module at rest has none:
    /// its Versions and Build are in ☰ › Project.
    /// What the walkthroughs see of the IDE this frame (tour.rs `Seen`): read
    /// from the same state the IDE draws from, never assumed.
    pub fn tour_seen(&self) -> crate::tour::Seen {
        let project = self.domain.as_ref().map(|d| (d.host.clone(), d.path.clone()));
        let host = self.domain.as_ref().map(|d| d.host.clone());
        let gate = self.gate.lock().unwrap().clone().filter(|(h, _)| Some(h) == host.as_ref()).map(|(_, g)| g).unwrap_or_default();
        let status = if host.as_deref() == Some(self.app_host.as_str()) { self.app_status.lock().unwrap().clone() } else { None };
        let (kind, language, mut running, built, verbs) = match status {
            Some(Ok(st)) => (st.kind, st.language, st.running, st.built, st.verbs),
            _ => Default::default(),
        };
        // A Shiny module is served by the platform: it runs when its site answers.
        if kind == "module" {
            running = matches!(gate.as_str(), "open" | "login" | "password");
        }
        let job = if host.as_deref() == Some(self.app_host.as_str()) { self.app_job.lock().unwrap().clone() } else { None };
        let (job_running, job_failed) = match job {
            Some(Ok(j)) => (j.running, (j.ok == Some(false)).then(|| j.lines.iter().rev().find(|l| !l.trim().is_empty()).cloned().unwrap_or_else(|| "it stopped without saying why".to_owned()))),
            Some(Err(e)) => (false, Some(e)),
            None => (false, None),
        };
        crate::tour::Seen {
            project,
            kind,
            language,
            naming: self.naming.is_some(),
            choosing: self.choosing.as_ref().map(|c| c.host.clone()),
            open_files: self.docs.iter().map(|d| d.path.clone()).collect(),
            dirty: self.docs.iter().any(|d| d.dirty()),
            saves: self.site_seq,
            ups: self.app_ups,
            running,
            built,
            verbs,
            job_running,
            job_failed,
            gate,
            env_module: self.env_module.clone().unwrap_or_default(),
            app_shown: false,
            narrow: false,
        }
    }

    /// The New project dialog, as its menu item opens it (tour.rs, the welcome).
    pub fn start_new_project(&mut self) {
        if self.choosing.is_none() {
            self.naming = Some(String::new());
        }
    }

    /// Pick `starter` in the open explorer - its category and its way - as a
    /// click on them would; the person still presses Create.
    pub fn choose_starter(&mut self, starter: &str) {
        let Some(c) = self.choosing.as_mut() else { return };
        for (i, cat) in crate::starters::CATEGORIES.iter().enumerate() {
            if let Some(j) = cat.ways.iter().position(|w| w.starter == Some(starter)) {
                c.cat = Some(i);
                c.way = Some(j);
                return;
            }
        }
    }

    /// Open a file in the editor: a path, or a name inside the open project.
    pub fn open_file(&self, path: &str, ctx: &egui::Context) {
        let full = if path.starts_with('/') {
            path.to_owned()
        } else {
            match &self.domain {
                Some(d) => format!("{}/{path}", d.path.trim_end_matches('/')),
                None => return,
            }
        };
        self.open(full, ctx);
    }

    /// The app job's log window.
    pub fn show_app_log(&mut self) {
        self.app_log = true;
    }

    pub fn app_strip_wanted(&self) -> bool {
        if matches!(&*self.app_job.lock().unwrap(), Some(Ok(j)) if j.running || j.ok == Some(false)) {
            return true;
        }
        matches!(&*self.app_status.lock().unwrap(), Some(Ok(s)) if s.kind == "port" || s.compiling.is_some() || !s.built)
    }

    /// The open project's verbs (build.sh / up.sh / down.sh it has), for
    /// ☰ › Project when the strip is not showing.
    pub fn app_verbs(&self) -> Vec<String> {
        match &*self.app_status.lock().unwrap() {
            Some(Ok(s)) => s.verbs.clone(),
            _ => Vec::new(),
        }
    }

    /// ☰ › Project › Git…: the same window as the strip's button.
    pub fn open_versions(&mut self, ctx: &egui::Context) {
        self.versions_open = true;
        self.ask_versions(ctx);
    }

    /// Follows the open project (its status, jobs, routing after Up) and draws
    /// its windows every frame; draws the strip itself only into `ui`, which
    /// the shell passes when app_strip_wanted says so.
    pub fn app_strip(&mut self, ui: Option<&mut egui::Ui>, ctx: &egui::Context) {
        let Some(d) = self.domain.clone() else { return };
        let now = ctx.input(|i| i.time);
        if self.app_host != d.host {
            self.app_host = d.host.clone();
            *self.app_status.lock().unwrap() = None;
            *self.app_job.lock().unwrap() = None;
            self.app_status_at = f64::NEG_INFINITY;
            self.app_log = false;
            self.app_verb.clear();
            *self.app_route.lock().unwrap() = None;
            self.versions_open = false;
            *self.versions.lock().unwrap() = None;
            *self.version_said.lock().unwrap() = None;
        }
        let job = self.app_job.lock().unwrap().clone();
        let running = matches!(&job, Some(Ok(j)) if j.running);
        if running && now - self.app_job_at > 1.0 {
            self.app_job_at = now;
            let slot = self.app_job.clone();
            self.backend.query(Query::AppJob { host: d.host.clone() }, ctx, move |r: Result<JobView, String>| {
                if let Ok(v) = r {
                    *slot.lock().unwrap() = Some(Ok(v));
                }
            });
        }
        // The status every 10 s - every 2 s while something compiles in the
        // folder - and at once when a job has just ended.
        let ended_ok = matches!(&job, Some(Ok(j)) if !j.running && j.ok == Some(true));
        let ended = matches!(&job, Some(Ok(j)) if !j.running && j.ok.is_some()) && self.app_status_at < self.app_job_at;
        let compiling = matches!(&*self.app_status.lock().unwrap(), Some(Ok(s)) if s.compiling.is_some());
        if now - self.app_status_at > if compiling { 2.0 } else { 10.0 } || ended {
            self.app_status_at = now;
            let slot = self.app_status.clone();
            self.backend.query(Query::AppStatus { host: d.host.clone() }, ctx, move |r: Result<crate::api::AppStatus, String>| {
                *slot.lock().unwrap() = Some(r);
            });
        }
        if running || compiling {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        let status = self.app_status.lock().unwrap().clone();
        if ended_ok && !self.app_counted {
            self.app_counted = true;
            if self.app_verb == "up" {
                self.app_ups += 1;
            }
        }
        // Up worked and the app has its own port: route a published domain to it.
        if ended_ok && self.app_verb == "up" && !self.app_routed && matches!(&status, Some(Ok(s)) if s.kind == "port" && s.running) {
            self.app_routed = true;
            let (slot, errors) = (self.app_route.clone(), self.errors.clone());
            self.backend.query(Query::AppRoute { host: d.host.clone() }, ctx, move |r: Result<String, String>| match r {
                Ok(m) => *slot.lock().unwrap() = Some(m),
                Err(e) => Ide::fail(&errors, format!("not routed: {e}")),
            });
        }
        let route = self.app_route.lock().unwrap().clone();
        if let Some(ui) = ui {
        crate::tour::mark(ui, "app-strip", ui.max_rect());
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let s = match &status {
                None => {
                    ui.label(RichText::new("reading the project…").size(12.0).color(theme::text()));
                    return;
                }
                Some(Err(e)) => {
                    ui.add(egui::Label::new(RichText::new(e).size(12.0).color(theme::text())).truncate());
                    return;
                }
                Some(Ok(s)) => s.clone(),
            };
            let lang = match s.language.as_str() {
                "r" => "R · Shiny",
                "julia" => "Julia",
                "node" => "Node",
                "python" => "Python",
                "rust" => "Rust",
                "unreal" => "Unreal Engine",
                "" => "Project",
                other => other,
            };
            ui.label(RichText::new(lang).size(12.5).strong().color(theme::text_strong()));
            for (verb, word, tip) in [
                ("build", "Build", ".cove/build.sh - compile or check it; never starts it"),
                ("up", "Up", ".cove/up.sh - start it and wait until it answers"),
                ("down", "Down", ".cove/down.sh - stop it"),
            ] {
                if !s.verbs.iter().any(|v| v == verb) {
                    continue;
                }
                let b = ui.add_enabled(!running, egui::Button::new(RichText::new(word).size(12.5)));
                crate::tour::mark(ui, &format!("app:{verb}"), b.rect);
                if b.on_hover_text(tip).clicked() {
                    self.app_action(verb, ctx);
                }
            }
            if theme::tool(ui, "?", "show me how: a walkthrough of running this app, on these buttons").clicked() {
                self.want_tour = Some("run");
            }
            let (dot, words) = strip_light(&s, running);
            if !words.is_empty() {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
                ui.painter().circle_filled(r.center(), 4.0, dot);
                ui.label(RichText::new(words).size(12.0).color(theme::text()));
            }
            let last = match &job {
                Some(Ok(j)) => j.lines.iter().rev().find(|l| !l.trim().is_empty()).cloned().map(|l| (l, j.ok)),
                _ => None,
            };
            let last = if self.app_routed && route.is_some() && !running { route.clone().map(|r| (r, Some(true))) } else { last };
            // Every saved version of this project, any of them a click back.
            let versions_btn = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let clicked = ui.add(egui::Button::new(RichText::new("Git").size(12.5))).on_hover_text("this project's history: every version, the files it changed and their diffs - put any of them back").clicked();
                clicked
            }).inner;
            if versions_btn {
                self.versions_open = !self.versions_open;
                if self.versions_open {
                    self.ask_versions(ctx);
                }
            }
            if let Some((line, ok)) = last {
                let col = match ok { Some(false) => theme::alt(), Some(true) => theme::ok(), None => theme::text() };
                if ui.add(egui::Label::new(RichText::new(line).monospace().size(11.5).color(col)).truncate().sense(egui::Sense::click())).on_hover_text("the whole log").clicked() {
                    self.app_log = !self.app_log;
                }
            }
        });
        }
        self.versions_window(ctx);
        if self.app_log {
            if let Some(Ok(j)) = &job {
                let mut open = true;
                egui::Window::new(format!("{} · {}", d.host, j.what)).open(&mut open).default_width(560.0).show(ctx, |ui| {
                    egui::ScrollArea::vertical().max_height(360.0).stick_to_bottom(true).show(ui, |ui| {
                        for l in &j.lines {
                            ui.label(RichText::new(l).monospace().size(11.5).color(theme::text()));
                        }
                        if let Some(r) = &route {
                            ui.label(RichText::new(r).monospace().size(11.5).color(theme::ok()));
                        }
                    });
                });
                self.app_log = open;
            }
        }
    }

    /// The strip folded into a phone's tab row (operator screenshot,
    /// 2026-11-07: "lots of white space ... could be consolidated"): its light
    /// and words, and Git, right to left into what the tabs leave. Build, Up
    /// and Down are ☰ › Project's; the words open the log. app_strip(None)
    /// still follows the project every frame.
    pub fn app_strip_compact(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(Ok(s)) = self.app_status.lock().unwrap().clone() else { return };
        let running = matches!(&*self.app_job.lock().unwrap(), Some(Ok(j)) if j.running);
        if ui.add(egui::Button::new(RichText::new("Git").size(12.5))).on_hover_text("this project's history: every version, the files it changed and their diffs - put any of them back").clicked() {
            self.versions_open = !self.versions_open;
            if self.versions_open {
                self.ask_versions(ctx);
            }
        }
        let (dot, words) = strip_light(&s, running);
        if words.is_empty() {
            return;
        }
        // Right to left: the words take what is left, then the light before them.
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), egui::Sense::hover());
            ui.painter().circle_filled(r.center(), 4.0, dot);
            if ui.add(egui::Label::new(RichText::new(words).size(12.0).color(theme::text())).truncate().sense(egui::Sense::click())).on_hover_text("the whole log").clicked() {
                self.app_log = !self.app_log;
            }
        });
    }

    /// The domains listed again (a new project just registered, say).
    pub fn refresh_domains(&mut self, ctx: &egui::Context) {
        self.domains_asked = false;
        self.ask_domains(ctx);
    }

    /// A project of theirs from its saved repository, then opened.
    fn restore_domain(&mut self, host: String, ctx: &egui::Context) {
        let (opened, errors, domains) = (self.created.clone(), self.errors.clone(), self.domains.clone());
        Ide::fail(&errors, format!("restoring {host} from its saved repository…"));
        self.backend.query(Query::DomainRestore { host }, ctx, move |r: Result<DomainInfo, String>| match r {
            Ok(d) => {
                if let Some(Ok(v)) = domains.lock().unwrap().as_mut() {
                    v.push(d.clone());
                    v.sort_by(|a, b| a.host.cmp(&b.host));
                }
                Ide::fail(&errors, format!("{} restored into {}", d.host, d.path));
                *opened.lock().unwrap() = Some(d);
            }
            Err(e) => Ide::fail(&errors, format!("not restored: {e}")),
        });
    }

    /// Put a domain on the web (Query::PublishDomain): registry, DNS,
    /// certificate and site, the platform's own provisioner. Its page is asked
    /// again in a minute, when the slow half has had time to land.
    pub fn publish_domain(&mut self, host: String, ctx: &egui::Context) {
        let launch_slot = self.launch.as_ref().filter(|l| l.host == host).map(|l| l.publish.clone());
        let errors = self.errors.clone();
        let gate = self.gate.clone();
        self.publishing.insert(host.clone(), ctx.input(|i| i.time));
        let h = host.clone();
        self.backend.query(Query::PublishDomain { host }, ctx, move |r: Result<String, String>| {
            if let Some(slot) = &launch_slot {
                *slot.lock().unwrap() = Some(r.clone());
            }
            match r {
                Ok(_) => {
                    // The launch window says it; a toast would say it twice.
                    if launch_slot.is_none() {
                        Ide::fail(&errors, format!("{h}: publishing - DNS, certificate and site; it answers in a minute or two"));
                    }
                    *gate.lock().unwrap() = None;
                }
                Err(e) => Ide::fail(&errors, format!("{h} not published: {e}")),
            }
        });
    }

    /// The launch window: each step of bringing a new project live, ticking
    /// as it lands - files, the provisioner (registry, DNS, certificate), a
    /// Rust app's build/serve/route as the person, and the address answering
    /// - over the IDE until it is live (or says why not). It cannot be put
    /// away while it works: the person stays with it to the end.
    fn launch_window(&mut self, ctx: &egui::Context) {
        let Some(l) = self.launch.as_mut() else { return };
        let now = ctx.input(|i| i.time);
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        let publish = l.publish.lock().unwrap().clone();
        let published = matches!(publish, Some(Ok(_)));
        // A Rust app: deployed once the provisioner has started (the deploy
        // waits for the vhost itself), then followed every 2 s.
        if l.rust && published {
            if !l.deploy_sent {
                l.deploy_sent = true;
                l.deploy_asked = now;
                let slot = l.deploy.clone();
                self.backend.query(Query::DeployApp { host: l.host.clone() }, ctx, move |r: Result<JobView, String>| {
                    *slot.lock().unwrap() = Some(r);
                });
            } else if now - l.deploy_asked > 2.0 && l.deploy.lock().unwrap().as_ref().is_some_and(|r| r.as_ref().is_ok_and(|v| v.running)) {
                l.deploy_asked = now;
                let slot = l.deploy.clone();
                self.backend.query(Query::DeployJob { host: l.host.clone() }, ctx, move |r: Result<JobView, String>| {
                    if let Ok(v) = r {
                        *slot.lock().unwrap() = Some(Ok(v));
                    }
                });
            }
        }
        if !l.app_asked {
            l.app_asked = true;
            let slot = l.app.clone();
            self.backend.query(Query::AppStatus { host: l.host.clone() }, ctx, move |r: Result<crate::api::AppStatus, String>| {
                *slot.lock().unwrap() = Some(r);
            });
        }
        let app = l.app.lock().unwrap().clone();
        let port = !l.rust && matches!(&app, Some(Ok(s)) if s.kind == "port");
        // Until the project has been read, nothing past publishing is decided.
        let known = l.rust || matches!(&app, Some(_));
        if port && published {
            let up = l.up.lock().unwrap().clone();
            // The project's job slot, every 2 s: first the Build create_domain
            // started, then this Up. Only an Up's own view counts as Up's.
            let ours = |v: &JobView| v.what.starts_with("up ");
            let up_done = matches!(&up, Some(Ok(v)) if ours(v) && !v.running && v.ok.is_some());
            if !up_done && now - l.up_asked > 2.0 {
                l.up_asked = now;
                let slot = l.up.clone();
                self.backend.query(Query::AppJob { host: l.host.clone() }, ctx, move |r: Result<JobView, String>| {
                    if let Ok(v) = r {
                        let mut s = slot.lock().unwrap();
                        // A poll answered before the Up started must not hide it.
                        if !matches!(&*s, Some(Ok(o)) if o.what.starts_with("up ") && !v.what.starts_with("up ")) {
                            *s = Some(Ok(v));
                        }
                    }
                });
            }
            if !l.up_sent && matches!(&up, Some(Ok(v)) if !v.running) {
                l.up_sent = true;
                let slot = l.up.clone();
                *slot.lock().unwrap() = Some(Ok(JobView { what: format!("up {}", l.host), running: true, ..Default::default() }));
                self.backend.query(Query::AppAction { host: l.host.clone(), verb: "up".into() }, ctx, move |r: Result<JobView, String>| {
                    *slot.lock().unwrap() = Some(r);
                });
            }
            // Up worked: route https://<host>/ to the port. "not published
            // yet" means the vhost is still being written - asked again.
            let pending = matches!(&*l.route.lock().unwrap(), Some(Ok(m)) if m.starts_with("not published yet"));
            if matches!(&up, Some(Ok(v)) if ours(v) && v.ok == Some(true)) && (!l.route_sent || (pending && now - l.route_asked > 5.0)) {
                l.route_sent = true;
                l.route_asked = now;
                let slot = l.route.clone();
                self.backend.query(Query::AppRoute { host: l.host.clone() }, ctx, move |r: Result<String, String>| {
                    *slot.lock().unwrap() = Some(r);
                });
            }
        }
        let up = l.up.lock().unwrap().clone();
        let route = l.route.lock().unwrap().clone();
        let routed = matches!(&route, Some(Ok(m)) if !m.starts_with("not published yet"));
        let deploy = l.deploy.lock().unwrap().clone();
        let deployed = known && if l.rust { matches!(&deploy, Some(Ok(v)) if v.ok == Some(true)) } else { !port || routed };
        // The address, asked every 5 s once there is something to answer.
        if published && deployed && now - l.gate_asked > 5.0 {
            l.gate_asked = now;
            let (slot, host) = (l.gate.clone(), l.host.clone());
            self.backend.query(Query::SiteGate { host }, ctx, move |r: Result<String, String>| {
                *slot.lock().unwrap() = Some(r.unwrap_or_default());
            });
        }
        let gate = l.gate.lock().unwrap().clone().unwrap_or_default();
        let live = published && deployed && matches!(gate.as_str(), "open" | "login" | "password");
        let up_failed = port && matches!(&up, Some(Err(_)) | Some(Ok(JobView { ok: Some(false), .. }))) && l.up_sent;
        let deploy_failed = (l.rust && matches!(&deploy, Some(Err(_)) | Some(Ok(JobView { ok: Some(false), .. })))) || up_failed || (port && matches!(&route, Some(Err(_))));
        // A first Rust build is ~7 minutes before routing starts (measured on
        // the host, 2026-10-19), and a first Julia start compiles its
        // packages: they get longer before they are called slow.
        // A first Unreal build compiles and cooks for tens of minutes.
        let unreal = matches!(&app, Some(Ok(s)) if s.language == "unreal");
        let slow = now - l.started > if unreal { 7200.0 } else if l.rust || port { 1200.0 } else { 600.0 } && !live;
        let failed = matches!(publish, Some(Err(_))) || deploy_failed || slow;
        // A name held by someone else (publish.R's own words) is not a failure
        // to retry: no second try can register it (operator's screenshot,
        // 2026-10-23: "500 meet.ndexr.io belongs to someone else" under a
        // spinner and a Try again). Say so, and point at a new name.
        let taken = matches!(&publish, Some(Err(e)) if e.contains("belongs to someone else") || e.contains("is not under a domain of yours"));
        let (host, rust, secs) = (l.host.clone(), l.rust, (now - l.started) as u64);
        let lang = match &app {
            Some(Ok(s)) => match s.language.as_str() {
                "julia" => "Julia",
                "node" => "Node",
                "python" => "Python",
                "rust" => "Rust",
                "unreal" => "Unreal Engine",
                "r" => "R",
                _ => "",
            },
            _ => "",
        };
        let what = if rust {
            "A Rust app - built, served and routed for you".to_owned()
        } else if port {
            let lang = if lang.is_empty() { "web" } else { lang };
            let a = if lang.starts_with(['A', 'E', 'I', 'O', 'U']) { "An" } else { "A" };
            format!("{a} {lang} app on its own port - started and routed for you")
        } else if known {
            "A Shiny module - live as soon as its address answers".to_owned()
        } else {
            "Reading the project…".to_owned()
        };
        let port_no = match &app {
            Some(Ok(s)) => s.port.clone(),
            _ => String::new(),
        };

        if l.quiet {
            if live {
                self.launch = None;
                self.site_seq += 1;
                self.gate_asked.clear();
                return;
            }
            if !failed {
                return;
            }
        }

        let mut close = false;
        let mut retry = false;
        let mut pick_new = false;
        let row = |ui: &mut egui::Ui, state: Option<bool>, text: &str, note: &str| {
            ui.horizontal(|ui| {
                match state {
                    Some(true) => ui.label(RichText::new("✓").size(15.0).color(theme::ok())),
                    Some(false) => ui.label(RichText::new("✕").size(15.0).color(theme::alt())),
                    None => ui.add(egui::Spinner::new().size(14.0)),
                };
                ui.vertical(|ui| {
                    ui.label(RichText::new(text).size(13.5).color(theme::text_strong()));
                    if !note.is_empty() {
                        ui.add(egui::Label::new(RichText::new(note).size(11.5).color(theme::muted())).wrap());
                    }
                });
            });
            ui.add_space(4.0);
        };
        let title = if live { format!("{host} is live") } else { format!("Bringing {host} live") };
        // A corner card, see-through a little, out of the way while the
        // person moves on (operator, 2026-10-28: "can be in the bottom left or
        // right and slightly transparent since it persists across screens").
        // As wide as the screen allows on a phone: a fixed 460 ran off its edge.
        let screen = ctx.content_rect();
        let w = (screen.width() - 24.0).clamp(240.0, 380.0);
        let frame = egui::Frame::window(&ctx.global_style()).fill(theme::surface_hi().gamma_multiply(0.86));
        egui::Window::new(RichText::new(title).size(if screen.width() < 600.0 { 15.0 } else { 16.0 }))
            .id(egui::Id::new("launch-window"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0))
            .frame(frame)
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .default_width(w)
            .max_height((screen.height() * 0.6).max(240.0))
            .show(ctx, |ui| {
                ui.set_width(w);
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(what.as_str()).size(11.5).color(theme::hush())).wrap());
                ui.add_space(6.0);
                row(ui, Some(true), "Project files", "in your domains folder - open in Files");
                match &publish {
                    None => row(ui, None, "Registering the site", "the registry, a DNS name, a certificate and the site's routing"),
                    Some(Ok(_)) => row(ui, Some(true), "Registered: DNS, certificate, routing", ""),
                    Some(Err(e)) => row(ui, Some(false), "Not registered", e),
                }
                if rust {
                    match &deploy {
                        _ if !published => row(ui, None, "Build, serve and route the app", "after it is registered"),
                        None => row(ui, None, "Starting the build", ""),
                        Some(Err(e)) => row(ui, Some(false), "The app was not deployed", e),
                        Some(Ok(v)) => {
                            // The deploy's own steps ("▸ …"), the newest last.
                            let steps: Vec<&String> = v.lines.iter().filter(|x| x.starts_with('▸') || x.starts_with('✓')).collect();
                            let last = steps.last().map(|x| x.trim_start_matches(['▸', '✓', ' ']).to_owned()).unwrap_or_else(|| "building".into());
                            match v.ok {
                                Some(true) => row(ui, Some(true), "Built and served as you, routed", ""),
                                Some(false) => {
                                    let tail: Vec<&str> = v.lines.iter().rev().take(4).map(String::as_str).collect::<Vec<_>>().into_iter().rev().collect();
                                    row(ui, Some(false), "The app was not deployed", &tail.join("\n"))
                                }
                                None => row(ui, None, &format!("{}{}", last[..1].to_uppercase(), &last[1..]), "a first build takes a few minutes"),
                            }
                        }
                    }
                }
                if port {
                    let tail = |v: &JobView| v.lines.iter().rev().take(4).map(String::as_str).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
                    let on = if port_no.is_empty() { String::new() } else { format!(" on port {port_no}") };
                    match &up {
                        _ if !published => row(ui, None, &format!("Start the app{on}"), ".cove/up.sh, as you, after it is registered"),
                        Some(Err(e)) => row(ui, Some(false), "The app did not start", e),
                        Some(Ok(v)) if v.what.starts_with("up ") => match v.ok {
                            Some(true) => row(ui, Some(true), &format!("Started as you{on}"), ""),
                            Some(false) => row(ui, Some(false), "The app did not start", &tail(v)),
                            None => row(ui, None, &format!("Starting it{on}: .cove/up.sh"), v.lines.last().map(String::as_str).unwrap_or("a first start compiles its packages")),
                        },
                        Some(Ok(v)) if v.running => row(ui, None, "Compiling: .cove/build.sh", v.lines.last().map(String::as_str).unwrap_or("a first build takes a few minutes")),
                        _ => row(ui, None, &format!("Start the app{on}"), ".cove/up.sh"),
                    }
                    match &route {
                        Some(Err(e)) => row(ui, Some(false), "Not routed", e),
                        Some(Ok(_)) if routed => row(ui, Some(true), &format!("https://{host}/ goes to the app"), ""),
                        _ => row(ui, None, &format!("Route https://{host}/ to the app"), "once it answers"),
                    }
                }
                match () {
                    _ if taken => row(ui, Some(false), &format!("{host} is taken"), "someone else holds this name, so it cannot be yours - choose another name and create that; the files made here stay in your domains folder"),
                    _ if failed && !slow => row(ui, Some(false), &format!("https://{host}/ is not live"), "the step above stopped it"),
                    _ if live => row(ui, Some(true), &format!("Answering at https://{host}/"), "only you (and admins) can see it until you make it public"),
                    _ if slow => row(ui, Some(false), &format!("https://{host}/ has not answered after {} minutes", if rust { 20 } else { 10 }), "the App tab shows what it says; Try again re-runs every step"),
                    _ if published && deployed => row(ui, None, &format!("Waiting for https://{host}/ to answer"), "DNS and the certificate take a minute or two"),
                    _ => row(ui, None, &format!("https://{host}/"), "answers once the steps above are done"),
                }
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{secs} s")).monospace().size(11.0).color(theme::hush()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if live {
                            if ui.add(theme::primary_button("Start building")).on_hover_text("the agent and your files are ready").clicked() {
                                close = true;
                            }
                            if ui.button("Open site ↗").clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab(format!("https://{host}/")));
                            }
                        } else if taken {
                            if ui.add(theme::primary_button("Choose another name")).on_hover_text("close this; type a new name in the domain menu (⌂ at the top) and create it").clicked() {
                                close = true;
                                pick_new = true;
                            }
                        } else if failed {
                            if ui.add(theme::primary_button("Try again")).clicked() {
                                retry = true;
                            }
                            if ui.button("Close").clicked() {
                                close = true;
                            }
                        }
                    });
                });
                });
            });
        if pick_new {
            Ide::fail(&self.errors, format!("{host} is someone else's - type a new name in the domain menu (⌂ at the top) to create one of yours"));
        }
        if close {
            self.launch = None;
            self.gate_asked.clear();
            // Loaded before it was routed, the frame still shows that 404.
            self.site_seq += 1;
            self.bring(Tab::Agents);
            self.show_lower(Lower::App);
        } else if retry {
            if let Some(l) = self.launch.as_mut() {
                l.started = now;
                *l.publish.lock().unwrap() = None;
                *l.deploy.lock().unwrap() = None;
                *l.gate.lock().unwrap() = None;
                l.deploy_sent = false;
                *l.up.lock().unwrap() = None;
                *l.route.lock().unwrap() = None;
                l.up_sent = false;
                l.route_sent = false;
                l.up_asked = f64::NEG_INFINITY;
                l.gate_asked = f64::NEG_INFINITY;
            }
            self.publish_domain(host, ctx);
        }
    }

    /// For other views (the atlas): the project being brought live, its
    /// current step, and whether it is done.
    #[allow(dead_code)]
    pub fn launching(&self) -> Option<(String, bool)> {
        let l = self.launch.as_ref()?;
        let live = matches!(l.gate.lock().unwrap().as_deref(), Some("open" | "login" | "password"));
        Some((l.host.clone(), live))
    }

    /// Projects (operator, 2026-11-14: "collapse all of that into one thing
    /// that we can type in to either go to a project or if it doesn't exist
    /// then it creates it ... not working very well on mobile because i'll
    /// click on it and then the text box doesn't open up"). It replaced the
    /// project menu, its finder field, "+ New project…" and "Find another
    /// project…": one field, focused as it opens. Empty, it lists the shares
    /// waiting, the recent projects, your ndexr, Inlet and the other sessions.
    /// Typed, it lists the projects that match and what the name can be
    /// created as - Enter opens the first match, or creates the first name.
    /// A dialog, not a menu, because a phone's keyboard comes up for a field
    /// in a dialog and did not for one in a menu.
    ///
    /// Drawn by domain_bar, which has the sessions list; by the IDE's own
    /// frame (`sessions` None) only when the bar is not on screen.
    fn project_dialog(&mut self, ctx: &egui::Context, sessions: Option<impl FnOnce(&mut egui::Ui)>) {
        if sessions.is_none() && ctx.cumulative_frame_nr().saturating_sub(self.bar_frame) <= 1 {
            return;
        }
        let Some(mut name) = self.naming.clone() else {
            self.project_dialog_open = false;
            self.project_pick = 0;
            return;
        };
        let focus = !self.project_dialog_open;
        self.project_dialog_open = true;
        let have = self.domains.lock().unwrap().clone();
        let all = match &have {
            Some(Ok(v)) => v.clone(),
            _ => Vec::new(),
        };
        let text = name.trim().to_ascii_lowercase();
        // Who holds what this would create, and the shares waiting for me
        // (Query::ProjectNames; operator, 2026-11-03: the search "shows
        // projects you are allowed to work on" and "disables ability to
        // create sites already created").
        let mut ask: Vec<String> = if text.is_empty() { Vec::new() } else { std::iter::once(text.clone()).chain(new_domain_offers(&text, &all)).collect() };
        ask.dedup();
        self.ask_names(ctx, ask);
        let held = |h: &str| self.taken_by(h);
        let offered: Vec<api::OfferedShare> = self.offered().into_iter().filter(|o| !all.iter().any(|d| d.host == o.domain)).collect();
        let ring = project_ring(&self.recent_hosts, &all);
        let here = self.domain.as_ref().and_then(|d| ring.iter().position(|x| x.host == d.host));
        let hits = find_projects(&text, &ring);
        let mut offers = if text.is_empty() { Vec::new() } else { new_domain_offers(&text, &all) };
        offers.retain(|h| crate::names::parse(h).is_none() && held(h).is_none() && crate::api::posit_mark(h).is_none() && !offered.iter().any(|o| o.domain == *h));
        let restorable = crate::names::parse(&text).is_some() && !all.iter().any(|d| d.host == text);
        let note = if text.is_empty() {
            None
        } else if let Some(m) = crate::api::posit_mark(&text) {
            Some(crate::api::posit_mark_words(&text, m))
        } else if let Some(who) = held(&text).filter(|_| !hits.iter().any(|d| d.host == text)) {
            Some(format!("{text} is {who}'s - ask them to share it with you, or choose another name"))
        } else if hits.is_empty() && offers.is_empty() && !restorable && !offered.iter().any(|o| o.domain.contains(&text)) {
            Some("Letters, digits and hyphens only, like rix or my-app".to_owned())
        } else {
            None
        };
        let (up, down, enter, left, right) = ctx.input(|i| {
            (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight))
        });
        if !hits.is_empty() {
            if down {
                self.project_pick = (self.project_pick + 1).min(hits.len() - 1);
            }
            if up {
                self.project_pick = self.project_pick.saturating_sub(1);
            }
            self.project_pick = self.project_pick.min(hits.len() - 1);
        } else {
            self.project_pick = 0;
        }
        let screen = ctx.content_rect();
        let w = (screen.width() - 32.0).clamp(260.0, 520.0);
        let (mut open, mut create, mut restore, mut accept, mut close, mut step) = (None, None, None, None, false, None::<isize>);
        let mut want_ndexr = false;
        let mut want_inlet = false;
        egui::Modal::new(egui::Id::new("new-project")).show(ctx, |ui| {
            ui.set_width(w);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Projects").size(20.0).strong().color(theme::text_strong()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("✕").size(15.0)).frame(false).min_size(egui::vec2(32.0, 32.0))).on_hover_text("close · Esc").clicked() {
                        close = true;
                    }
                });
            });
            ui.horizontal(|ui| {
                if ui.add_enabled(ring.len() > 1, egui::Button::new(RichText::new("‹").size(16.0)).min_size(egui::vec2(32.0, 36.0))).on_hover_text("the previous project (← in an empty field)").clicked() {
                    step = Some(-1);
                }
                let fw = (ui.available_width() - 40.0).max(120.0);
                let r = ui.add(
                    egui::TextEdit::singleline(&mut name)
                        .hint_text("type a name: go to it, or create it")
                        .font(egui::TextStyle::Monospace)
                        .desired_width(fw)
                        .min_size(egui::vec2(0.0, 36.0)),
                );
                if focus {
                    r.request_focus();
                }
                if ui.add_enabled(ring.len() > 1, egui::Button::new(RichText::new("›").size(16.0)).min_size(egui::vec2(32.0, 36.0))).on_hover_text("the next project (→ in an empty field)").clicked() {
                    step = Some(1);
                }
            });
            if let Some(n) = &note {
                ui.label(RichText::new(n).size(13.5).color(theme::warn()));
            }
            ui.add_space(4.0);
            let row = |ui: &mut egui::Ui, t: RichText| ui.add(egui::Button::new(t).min_size(egui::vec2(ui.available_width(), 34.0)).truncate());
            egui::ScrollArea::vertical().max_height((screen.height() * 0.6).max(220.0)).auto_shrink([false, true]).show(ui, |ui| {
                let share = |ui: &mut egui::Ui, o: &api::OfferedShare, accept: &mut Option<api::OfferedShare>| {
                    let what = crate::access::role_words(&o.role);
                    let r = row(ui, RichText::new(format!("↘ Accept and open {} · from {} · {what}", o.domain, o.owner)).size(13.0).color(theme::ok()));
                    if r.on_hover_text("it was shared with you: take it, and it opens here").clicked() {
                        *accept = Some(o.clone());
                    }
                };
                if text.is_empty() {
                    if !offered.is_empty() {
                        ui.label(RichText::new("Shared with you, waiting for you").size(12.5).strong().color(theme::text_strong()));
                        for o in &offered {
                            share(ui, o, &mut accept);
                        }
                        ui.separator();
                    }
                    let recent = recent_projects(&self.recent_hosts, &all);
                    if have.is_none() {
                        ui.spinner();
                    } else if let Some(Err(e)) = &have {
                        ui.label(RichText::new(e).size(12.5).color(theme::warn()));
                    } else if recent.is_empty() {
                        ui.label(RichText::new("No projects yet - type a name above to create one").size(13.0).color(theme::text()));
                    } else {
                        ui.label(RichText::new("Recent").size(12.5).strong().color(theme::text()));
                        let current = self.domain.as_ref().map(|d| d.host.clone());
                        for d in recent {
                            if project_row(ui, &d, current.as_deref() == Some(d.host.as_str())).clicked() {
                                open = Some(d);
                            }
                        }
                    }
                    ui.separator();
                    // The person's own ndexr (operator, 2026-10-25: "their
                    // shiny app must be a default project which is whatever
                    // ndexr has to run"): the platform's code, read-only, with
                    // their domains in it (scripts/cells/tenant-ndexr.sh).
                    if !self.home.is_empty() {
                        let b = egui::Button::selectable(self.ndexr_open, RichText::new("⌂ Your ndexr").size(13.0)).min_size(egui::vec2(ui.available_width(), 32.0));
                        if ui.add(b).on_hover_text(format!("{}/ndexr: the platform (read-only) and your domains", self.home)).clicked() {
                            want_ndexr = true;
                        }
                    }
                    // Inlet, the stack's RStudio fork, as a project everyone
                    // has (operator, 2026-11-07): its easyconfig and every
                    // patch, in the person's own workspace (source.rs).
                    if row(ui, RichText::new("◧ Inlet").size(13.0)).on_hover_text("Inlet, ndexr's RStudio fork: its recipe and patches as a project of your own - change it and build your variant").clicked() {
                        want_inlet = true;
                    }
                    if let Some(f) = sessions {
                        f(ui);
                    }
                } else {
                    for (i, d) in hits.iter().enumerate() {
                        if project_row(ui, d, i == self.project_pick).clicked() {
                            open = Some(d.clone());
                        }
                    }
                    for o in offered.iter().filter(|o| o.domain.contains(&text)) {
                        share(ui, o, &mut accept);
                    }
                    // A software project's name (names.rs) not here: its saved
                    // repository, never a fresh empty domain standing in for it.
                    if restorable && row(ui, RichText::new(format!("⤓ Restore {text}")).size(13.5).color(theme::ok())).on_hover_text("clone its saved repository, every version, into your domains").clicked() {
                        restore = Some(text.clone());
                    }
                    if !hits.is_empty() && !offers.is_empty() {
                        ui.add_space(4.0);
                    }
                    for (i, h) in offers.iter().take(3).enumerate() {
                        let first = i == 0 && hits.is_empty() && !restorable;
                        let r = row(ui, RichText::new(format!("+ Create {h}{}", if first { "  ⏎" } else { "" })).size(13.5).color(theme::ok()));
                        if r.on_hover_text("create it, then choose what it starts as").clicked() {
                            create = Some(h.clone());
                        }
                    }
                }
            });
            crate::tour::mark(ui, "new-project-dialog", ui.min_rect());
        });
        // Enter: the picked match, else restore, else the first name it can be.
        if enter && open.is_none() && create.is_none() && restore.is_none() && !text.is_empty() {
            if let Some(d) = hits.get(self.project_pick) {
                open = Some(d.clone());
            } else if restorable {
                restore = Some(text.clone());
            } else if let Some(h) = offers.first() {
                create = Some(h.clone());
            }
        }
        if text.is_empty() && step.is_none() {
            step = if left { Some(-1) } else if right { Some(1) } else { None };
        }
        if let Some(sn) = step.filter(|_| !ring.is_empty()) {
            let n = ring.len() as isize;
            let at = here.map_or(if sn > 0 { 0 } else { n - 1 }, |i| (i as isize + sn).rem_euclid(n));
            if Some(at as usize) != here {
                open = Some(ring[at as usize].clone());
            }
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            close = true;
        }
        let done = open.is_some() || create.is_some() || restore.is_some() || want_ndexr || want_inlet || close;
        if let Some(o) = accept {
            self.accept_share(&o, ctx);
            self.naming = None;
        } else if done {
            self.naming = None;
        } else {
            self.naming = Some(name);
        }
        if done {
            self.domain_filter.clear();
            self.project_pick = 0;
        }
        if let Some(h) = create {
            self.choosing = Some(Choosing::new(&h));
        } else if let Some(d) = open {
            self.open_domain(d, ctx);
        } else if let Some(h) = restore {
            self.restore_domain(h, ctx);
        } else if want_ndexr {
            self.want_folder = Some(format!("{}/ndexr", self.home.trim_end_matches('/')));
        } else if want_inlet {
            self.want_inlet = true;
        }
    }

    /// What a new domain starts as: "What do you want to build?" - a category,
    /// then a language and framework, then Create (operator, 2026-10-24: "a
    /// scientific application launcher ... select something interesting to
    /// build, choose the language and framework, and immediately get a working
    /// application in the IDE"). Every row comes from starters.rs; a row with no
    /// template says "not yet" and cannot be picked.
    fn kind_chooser(&mut self, ctx: &egui::Context) {
        use crate::starters::{ready_count, CATEGORIES};
        let Some(mut c) = self.choosing.clone() else { return };
        // The explorer wants room for three columns of worlds; a category's
        // page keeps the narrower width it reads best at.
        // The width is the screen less the modal's own frame: on a phone the
        // frame's margins and a title that could not wrap made it wider than
        // the screen, and centred, its left edge fell off (operator
        // screenshot, 2026-11-07: "mobile looks bad in the projects menu").
        let screen = ctx.content_rect().width();
        let phone = screen < 600.0;
        let margin: i8 = if phone { 10 } else { 16 };
        let w = (screen - 2.0 * f32::from(margin) - if phone { 12.0 } else { 32.0 }).clamp(240.0, if c.cat.is_none() { 1120.0 } else { 860.0 });
        let mut go: Option<&'static str> = None;
        let mut cancel = false;
        // Placed, not anchored: egui's Modal centres its area from the size it
        // measured, and that area holds the full-screen backdrop too, so on a
        // phone the chooser sat 200 pt off the left edge and stayed there
        // (release screenshots, 2026-11-07). Centred from our own width, and
        // from its own last height.
        let id = egui::Id::new("kind-chooser");
        let screen_rect = ctx.content_rect();
        let total_w = w + 2.0 * f32::from(margin);
        let last_h = ctx.memory(|m| m.area_rect(id)).map_or(screen_rect.height(), |r| r.height().min(screen_rect.height()));
        let at = egui::pos2(
            screen_rect.left() + ((screen_rect.width() - total_w) / 2.0).max(0.0),
            screen_rect.top() + ((screen_rect.height() - last_h) / 2.0).max(if phone { 8.0 } else { 24.0 }),
        );
        let area = egui::Area::new(id).kind(egui::UiKind::Modal).sense(egui::Sense::hover()).order(egui::Order::Foreground).interactable(true).fixed_pos(at)
            // Not constrained either: the area's size counts the backdrop,
            // so egui pushed it back to x = 0 on a desktop (measured).
            .constrain(false);
        let r = egui::Modal::new(id).area(area).frame(egui::Frame::popup(&ctx.global_style()).inner_margin(margin)).show(ctx, |ui| {
            ui.set_width(w);
            ui.set_max_width(w);
            // Most of the window: a ScrollArea's max_height alone let it
            // shrink to egui's default minimum, so the explorer showed one
            // short strip of cards (operator screenshot, 2026-10-26: "this is
            // hard to read its a small modal"). min_scrolled_height makes it
            // take the room.
            let max_h = (ctx.content_rect().height() - 120.0).max(300.0);
            // Both scroll areas below hold their width to `w` and may shrink to
            // it: filling the available width inside a modal that sizes itself
            // grew the modal each frame, past a phone's left edge (screenshot
            // of the release, 2026-11-07: cards at 218 pt, scrollbar at 437).
            match c.cat {
                None => {
                    // The explorer (operator, 2026-10-26: "Choose your next
                    // experiment ... Organize projects into worlds ... sticky
                    // search, filters ... an obvious close action"): title,
                    // search and worlds stay put; the worlds scroll beneath.
                    // The title wraps in a column of its own width, the close
                    // button beside it. (A right-to-left layout here stretched
                    // the auto-sized modal past the screen: measured.)
                    ui.horizontal_top(|ui| {
                        let tw = w - 48.0;
                        ui.allocate_ui_with_layout(egui::vec2(tw, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                            ui.set_max_width(tw);
                            ui.add(egui::Label::new(RichText::new("Choose your next experiment").size(if phone { 21.0 } else { 26.0 }).strong().color(theme::text_strong())).wrap());
                        });
                        if ui.add(egui::Button::new(RichText::new("✕").size(18.0)).min_size(egui::vec2(36.0, 36.0))).on_hover_text("close").clicked() {
                            cancel = true;
                        }
                    });
                    ui.add(egui::Label::new(RichText::new(format!("Discover an idea, see what it does, make it yours - {} starts as a working application, open in the IDE.", c.host)).size(14.5).color(theme::text())).wrap());
                    ui.add_space(10.0);
                    ui.add(egui::TextEdit::singleline(&mut c.find).hint_text("search every experiment: genomics, React, optimizer, Julia…").desired_width(f32::INFINITY).min_size(egui::vec2(0.0, 34.0)));
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(egui::Button::selectable(c.world.is_none(), RichText::new("All worlds").size(13.5)).min_size(egui::vec2(0.0, 30.0))).clicked() {
                            c.world = None;
                        }
                        for (wd, name, line) in crate::starters::WORLDS {
                            if ui.add(egui::Button::selectable(c.world == Some(*wd), RichText::new(*name).size(13.5)).min_size(egui::vec2(0.0, 30.0))).on_hover_text(*line).clicked() {
                                c.world = Some(*wd);
                            }
                        }
                    });
                    ui.add_space(10.0);
                    let find = c.find.trim().to_lowercase();
                    egui::ScrollArea::vertical().max_height(max_h - 210.0).min_scrolled_height(max_h - 210.0).max_width(w).auto_shrink([true, false]).show(ui, |ui| {
                        if !find.is_empty() {
                            // Every starter that matches, whichever world it is in.
                            let mut any = false;
                            for (i, cat) in CATEGORIES.iter().enumerate() {
                                for (j, way) in cat.ways.iter().enumerate() {
                                    let hay = format!("{} {} {} {} {}", cat.title, cat.what, way.lang, way.framework, way.app).to_lowercase();
                                    if way.starter.is_none() || !find.split_whitespace().all(|t| hay.contains(t)) {
                                        continue;
                                    }
                                    any = true;
                                    let resp = egui::Frame::new()
                                        .fill(theme::surface())
                                        .stroke(egui::Stroke::new(1.0, theme::pane_brd()))
                                        .corner_radius(8)
                                        .inner_margin(10)
                                        .show(ui, |ui| {
                                            ui.set_width(w - 20.0);
                                            ui.horizontal_wrapped(|ui| {
                                                crate::frameworks::icon(ui, crate::frameworks::of_way(way.lang, way.framework), 22.0);
                                                ui.label(RichText::new(way.lang).size(15.0).strong().color(lang_color(way.lang)));
                                                ui.label(RichText::new(way.app.split(':').next().unwrap_or(way.app)).size(15.0).strong().color(theme::text_strong()));
                                                ui.label(RichText::new(format!("· {}", cat.title)).size(13.5).color(theme::text()));
                                            });
                                            ui.add(egui::Label::new(RichText::new(way.app.split_once(": ").map_or("", |(_, d)| d)).size(13.5).color(theme::text())).wrap());
                                        })
                                        .response;
                                    if resp.interact(egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                        c.cat = Some(i);
                                        c.way = Some(j);
                                    }
                                    ui.add_space(6.0);
                                }
                            }
                            if !any {
                                ui.label(RichText::new(format!("No experiment matches \"{}\"", c.find.trim())).size(14.0).color(theme::text()));
                            }
                            return;
                        }
                        let cols = if w < 600.0 { 1 } else if w < 900.0 { 2 } else { 3 };
                        let cw = (w - 12.0 * (cols as f32 - 1.0)) / cols as f32;
                        for (wd, name, line) in crate::starters::WORLDS {
                            if c.world.is_some_and(|x| x != *wd) {
                                continue;
                            }
                            let cats: Vec<(usize, &crate::starters::Category)> = CATEGORIES.iter().enumerate().filter(|(_, k)| k.world == *wd).collect();
                            if cats.is_empty() {
                                continue;
                            }
                            ui.label(RichText::new(*name).size(20.0).strong().color(theme::text_strong()));
                            ui.label(RichText::new(*line).size(14.0).color(theme::text()));
                            ui.add_space(8.0);
                            for row in cats.chunks(cols) {
                                ui.horizontal_top(|ui| {
                                    for (i, cat) in row {
                                        if category_card(ui, cat, *i, cw).clicked() {
                                            c.cat = Some(*i);
                                            c.way = CATEGORIES[*i].ways.iter().position(|w| w.starter.is_some());
                                        }
                                    }
                                });
                                ui.add_space(12.0);
                            }
                            ui.add_space(8.0);
                        }
                    });
                }
                Some(i) => {
                    let cat = &CATEGORIES[i];
                    ui.horizontal_wrapped(|ui| {
                        if ui.add(egui::Button::new("‹ All experiments").min_size(egui::vec2(0.0, 32.0))).clicked() {
                            c.cat = None;
                            c.way = None;
                        }
                        ui.add(egui::Label::new(RichText::new(cat.title).size(if phone { 19.0 } else { 22.0 }).strong().color(theme::text_strong())).wrap());
                    });
                    ui.label(RichText::new(cat.what).size(14.5).color(theme::text()));
                    ui.add_space(10.0);
                    egui::ScrollArea::vertical().max_height(max_h - 60.0).min_scrolled_height(max_h - 60.0).max_width(w).auto_shrink([true, false]).show(ui, |ui| {
                        for (j, way) in cat.ways.iter().enumerate() {
                            let can = way.starter.is_some();
                            let on = c.way == Some(j);
                            let accent = lang_color(way.lang);
                            let resp = egui::Frame::new()
                                .fill(if on { accent.gamma_multiply(0.20) } else { theme::surface() })
                                .stroke(egui::Stroke::new(if on { 2.0 } else { 1.0 }, if on { accent } else { theme::pane_brd() }))
                                .corner_radius(8)
                                .inner_margin(12)
                                .show(ui, |ui| {
                                    ui.set_width(w - 24.0);
                                    ui.horizontal_wrapped(|ui| {
                                        crate::frameworks::icon(ui, crate::frameworks::of_way(way.lang, way.framework), 28.0);
                                        ui.label(RichText::new(way.lang).size(17.0).strong().color(if can { accent } else { theme::muted() }));
                                        ui.label(RichText::new(way.framework).size(15.0).color(if can { theme::text_strong() } else { theme::muted() }));
                                        if !can {
                                            ui.label(RichText::new("not yet").size(13.0).color(theme::muted()));
                                        } else if on {
                                            ui.label(RichText::new("✓").size(16.0).color(accent));
                                        }
                                    });
                                    ui.add(egui::Label::new(RichText::new(way.app).size(14.0).color(if can { theme::text() } else { theme::muted() })).wrap());
                                })
                                .response;
                            if can && resp.interact(egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                c.way = Some(j);
                            }
                            ui.add_space(8.0);
                        }
                    });
                    ui.add_space(6.0);
                    let picked = c.way.and_then(|j| cat.ways.get(j)).filter(|w| w.starter.is_some());
                    ui.horizontal_wrapped(|ui| {
                        let label = match picked {
                            Some(way) => format!("Create {} · {} in {}", c.host, way.app.split(':').next().unwrap_or(way.app), way.lang),
                            None if ready_count(cat) == 0 => "Nothing here can be created yet".to_owned(),
                            None => "Pick one".to_owned(),
                        };
                        let label = if c.creating { format!("Creating {}…", c.host) } else { label };
                        // Wraps rather than widening the window past a phone's
                        // edge: the label carries the whole host name.
                        if ui.add_enabled(picked.is_some() && !c.creating, theme::primary_button(&label).wrap().min_size(egui::vec2(0.0, 40.0))).clicked() {
                            go = picked.and_then(|w| w.starter);
                        }
                        if c.creating {
                            ui.spinner();
                        }
                        if ui.add(egui::Button::new("Cancel").min_size(egui::vec2(0.0, 40.0))).clicked() {
                            cancel = true;
                        }
                    });
                    if let Some(Err(e)) = c.outcome.lock().unwrap().as_ref() {
                        ui.add_space(6.0);
                        ui.add(egui::Label::new(RichText::new(format!("Not created: {e}")).size(14.0).color(theme::warn())).wrap());
                    }
                }
            }
        });
        crate::tour::mark_at(ctx, "explorer", r.response.rect);
        // The answer to a Create: made - the dialog goes and the project
        // opens (poll of `created`); refused - it stays, with the reason.
        let done = c.outcome.lock().unwrap().clone();
        match done {
            Some(Ok(())) => {
                self.choosing = None;
                self.domain_filter.clear();
                return;
            }
            Some(Err(_)) => c.creating = false,
            None => {}
        }
        if (cancel || r.should_close()) && !c.creating {
            self.choosing = None;
            return;
        }
        if let Some(starter) = go {
            c.creating = true;
            *c.outcome.lock().unwrap() = None;
            let outcome = c.outcome.clone();
            self.create_domain(c.host.clone(), starter, Some(outcome), ctx);
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        if c.creating {
            ctx.request_repaint_after(std::time::Duration::from_millis(300));
        }
        self.choosing = Some(c);
    }

    fn create_domain(&mut self, host: String, starter: &'static str, outcome: Option<Arc<Mutex<Option<Result<(), String>>>>>, ctx: &egui::Context) {
        let rust = starter == crate::starters::RUST_APP;
        if rust {
            self.creating_rust.insert(host.clone());
        }
        let parent = self.domains_root();
        let (opened, errors, domains) = (self.created.clone(), self.errors.clone(), self.domains.clone());
        self.backend.query(Query::CreateDomain { host, parent, rust, starter: starter.to_owned() }, ctx, move |r: Result<DomainInfo, String>| match r {
            Ok(d) => {
                if let Some(Ok(v)) = domains.lock().unwrap().as_mut() {
                    v.push(d.clone());
                    v.sort_by(|a, b| a.host.cmp(&b.host));
                }
                *opened.lock().unwrap() = Some(d);
                if let Some(o) = &outcome {
                    *o.lock().unwrap() = Some(Ok(()));
                }
            }
            Err(e) => match &outcome {
                Some(o) => *o.lock().unwrap() = Some(Err(e)),
                None => Ide::fail(&errors, format!("not created: {e}")),
            },
        });
    }

    /// git.ndexr.io's sweep for this person: commit and push every repo under
    /// their domains folder.
    pub fn push_all(&mut self, ctx: &egui::Context) {
        let root = self.domains_root();
        let (slot, errors) = (self.push.clone(), self.errors.clone());
        *slot.lock().unwrap() = Some(JobView { what: "push all".into(), running: true, ..Default::default() });
        self.push_asked = f64::NEG_INFINITY;
        self.backend.query(Query::PushAll { root }, ctx, move |r: Result<JobView, String>| match r {
            Ok(v) => *slot.lock().unwrap() = Some(v),
            Err(e) => {
                *slot.lock().unwrap() = Some(JobView { what: "push all".into(), ok: Some(false), lines: vec![e.clone()], ..Default::default() });
                Ide::fail(&errors, format!("push all: {e}"));
            }
        });
    }

    /// The toolbar's Push all: the button, then its state; click the state
    /// for the log.
    pub fn push_button(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, label: &str) {
        let job = self.push.lock().unwrap().clone();
        let now = ctx.input(|i| i.time);
        if job.as_ref().is_some_and(|j| j.running) && now - self.push_asked > 1.0 {
            self.push_asked = now;
            let slot = self.push.clone();
            self.backend.query(Query::PushJob, ctx, move |r: Result<JobView, String>| {
                if let Ok(v) = r {
                    if !v.what.is_empty() {
                        *slot.lock().unwrap() = Some(v);
                    }
                }
            });
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        match &job {
            Some(j) if j.running => {
                ui.add(egui::Spinner::new().size(12.0));
                if theme::tool(ui, "saving…", "commit + push of every repo in your domains folder - click for the log").clicked() {
                    self.push_log = true;
                }
            }
            _ => {
                if theme::tool(ui, label, "keep your work: commit and push every repo in your domains folder (git.ndexr.io's sweep, ndexr bulk-commit)").clicked() {
                    self.push_all(ctx);
                    self.push_log = true;
                }
                if let Some(j) = &job {
                    let (t, c) = if j.ok == Some(true) { ("✓", theme::ok()) } else { ("✗", ace::error()) };
                    if ui.add(egui::Button::new(RichText::new(t).color(c)).frame(false)).on_hover_text("the last push - click for its log").clicked() {
                        self.push_log = true;
                    }
                }
            }
        }
    }

    /// Push all's log, over everything.
    pub fn push_dialog(&mut self, ctx: &egui::Context) -> bool {
        if !self.push_log {
            return false;
        }
        let job = self.push.lock().unwrap().clone().unwrap_or_default();
        let screen = ctx.content_rect();
        let r = egui::Modal::new("push-log".into()).show(ctx, |ui| {
            ui.set_width((screen.width() * 0.7).clamp(320.0, 900.0).min(screen.width() - 24.0));
            let state = match (job.running, job.ok) {
                (true, _) => "running…",
                (false, Some(true)) => "done",
                (false, Some(false)) => "failed",
                _ => "",
            };
            ui.heading(format!("Push all {state}"));
            ui.label(RichText::new(format!("ndexr bulk-commit {}", self.domains_root())).monospace().size(11.0).color(theme::muted()));
            egui::Frame::new().fill(ace::bg()).inner_margin(6).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                egui::ScrollArea::both().max_height((screen.height() * 0.6).max(160.0)).stick_to_bottom(true).show(ui, |ui| {
                    for l in &job.lines {
                        ui.add(egui::Label::new(RichText::new(if l.is_empty() { " " } else { l }).monospace().size(11.0).color(ace::text())).extend());
                    }
                });
            });
            ui.horizontal(|ui| {
                let close = ui.button("Close").clicked();
                if ui.button("Copy log").on_hover_text("the whole log, to the clipboard").clicked() {
                    ui.ctx().copy_text(job.lines.join("\n"));
                }
                close
            })
            .inner
        });
        if r.inner || r.should_close() {
            self.push_log = false;
        }
        true
    }

    /// For the app to keep across restarts: the domain this session is in, and
    /// the files open in each domain (the current one's as they are now).
    pub fn remember(&self) -> (Option<String>, HashMap<String, Vec<String>>) {
        let mut map = self.open_by_domain.clone();
        if let Some(d) = &self.domain {
            let inside = format!("{}/", d.path);
            map.insert(d.host.clone(), self.docs.iter().filter(|x| !x.untitled() && x.table.is_none() && x.path.starts_with(&inside)).map(|x| x.path.clone()).collect());
        }
        (self.domain.as_ref().map(|d| d.host.clone()).or_else(|| self.restore.clone()), map)
    }

    /// The App tab: the one domain this session is in. Pick it or name a new
    /// one, see its site live, and keep it - Push all - or put it away -
    /// Archive. Everything a session does with its domain, in one spot.
    /// Pictures from the page (shots.rs) - a screenshot of the App's site, an
    /// image pasted anywhere - go to the agent's message box, which comes forward.
    fn take_shots(&mut self, ctx: &egui::Context) {
        // Send a request is open: what is pasted or dropped is for it.
        if self.hold_shots {
            return;
        }
        while let Some(shot) = crate::shots::take() {
            let (prefix, url) = match shot {
                crate::shots::Shot::Error(e) => {
                    Ide::fail(&self.errors, format!("screenshot: {e}"));
                    continue;
                }
                crate::shots::Shot::Site(u) => ("screenshot", u),
                crate::shots::Shot::Pasted(u) => ("pasted", u),
                // Picked for a request that has since closed.
                crate::shots::Shot::File(..) => continue,
            };
            // The message box on screen takes it: Exec's when Exec is open
            // (operator, 2026-10-23: a screenshot pasted with Exec showing went
            // into the hidden Site box and looked as if paste did nothing).
            let name = crate::shots::name(prefix, &url);
            let attached = if self.exec_on {
                let scope = self.exec_scope();
                if scope.path.is_empty() {
                    Ide::fail(&self.errors, "Exec has no folder for pictures yet - open a domain once".into());
                    continue;
                }
                self.exec.attach_image(&self.backend, ctx, &scope.path, &name, &url)
            } else {
                let Some(d) = self.domain.clone() else {
                    Ide::fail(&self.errors, "pick a domain first - pictures for the agent go into its supporting_docs/".into());
                    continue;
                };
                self.agent.attach_image(&self.backend, ctx, &d.path, &name, &url)
            };
            match attached {
                Ok(()) => self.bring(Tab::Agents),
                Err(e) => Ide::fail(&self.errors, format!("{prefix}: {e}")),
            }
        }
    }

    /// The Logs tab: the end of the open project's app log, asked every
    /// second while the tab is drawn, kept at the bottom as it grows unless
    /// scrolled up.
    fn logs_tab(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(d) = self.domain.clone() else {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("No project open").color(theme::hush())));
            return;
        };
        let now = ctx.input(|i| i.time);
        if now - self.run_log_asked > 1.0 {
            self.run_log_asked = now;
            let (slot, dir, build) = (self.run_log.clone(), d.path.clone(), self.run_log_build);
            let key = format!("{dir}#{build}");
            self.backend.query(Query::AppLog { path: dir, build }, ctx, move |r: Result<FileText, String>| {
                *slot.lock().unwrap() = Some((key, r));
            });
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
        let key = format!("{}#{}", d.path, self.run_log_build);
        let got = self.run_log.lock().unwrap().clone().filter(|(k, _)| *k == key).map(|(_, r)| r);
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(4.0);
            let where_ = match &got {
                Some(Ok(f)) if !f.path.is_empty() => f.path.strip_prefix(&d.path).unwrap_or(&f.path).trim_start_matches('/').to_owned(),
                _ if self.run_log_build => ".cove/run/build.log".to_owned(),
                _ => ".cove/run/app.log".to_owned(),
            };
            // App: what run.sh prints while it serves; Build: the last Build.
            for (build, word, tip) in [(false, "App", "the app's own output: what run.sh prints, as Up logs it"), (true, "Build", "the last Build's output (.cove/run/build.log)")] {
                if ui.add(egui::Button::selectable(self.run_log_build == build, RichText::new(word).size(11.5))).on_hover_text(tip).clicked() && self.run_log_build != build {
                    self.run_log_build = build;
                    self.run_log_asked = 0.0;
                }
            }
            ui.add(egui::Label::new(RichText::new(where_).monospace().size(11.5).color(theme::hush())).truncate());
        });
        match got {
            None => {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| ui.spinner());
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(theme::warn()));
            }
            Some(Ok(f)) if f.path.is_empty() => {
                ui.add_space(12.0);
                let none = if self.run_log_build { "No build log yet - Build writes its output here" } else { "No app log yet - Up starts the app and its output appears here" };
                ui.vertical_centered(|ui| ui.label(RichText::new(none).color(theme::hush())));
            }
            Some(Ok(f)) => {
                egui::ScrollArea::both().id_salt("app-log").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(f.text).monospace().size(11.5).color(theme::text())).extend());
                });
            }
        }
    }

    pub fn app(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        self.take_shots(ctx);
        // The domain is picked in the toolbar now, beside the environment
        // (app.rs toolbar); the strip keeps what acts on it.
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(4.0);
            let label = self.domain.as_ref().map_or("no domain - pick one in the toolbar".to_owned(), |d| d.host.clone());
            // The site's name reloads it (operator, 2026-10-16: "when clicking
            // the url name ... it should refresh the page").
            let r = ui.add(egui::Label::new(RichText::new(label).monospace().size(11.5).color(theme::hush())).truncate().sense(egui::Sense::click()));
            crate::tour::mark(ui, "app-url", r.rect);
            if self.domain.is_some() && r.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text("reload the site").clicked() {
                self.site_seq += 1;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                let host = self.domain.as_ref().map(|d| d.host.clone());
                // The live site in its own tab, outside Cove (operator, 2026-10-18).
                if let Some(h) = &host {
                    if theme::tool_if(ui, true, "Open ↗", &format!("open https://{h}/ in a new browser tab")).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(format!("https://{h}/")));
                    }
                    // Live refresh, per site: lit, the site reloads when its
                    // files change; hollow, it waits for ⟳ or a click on its name.
                    let live = !self.live_off.contains(h);
                    let (mark, color, tip) = if live {
                        ("●", theme::ok(), "live refresh on - reloads when its files change; click to stop")
                    } else {
                        ("○", theme::hush(), "live refresh off - reloads only when you ask; click to turn on")
                    };
                    if ui.add(egui::Button::new(RichText::new(mark).size(12.0).color(color)).frame(false)).on_hover_text(tip).clicked() {
                        if live {
                            self.live_off.insert(h.clone());
                        } else {
                            self.live_off.remove(h);
                            self.site_seq += 1;
                        }
                    }
                }
            });
        });
        if self.domain.is_some() {
            self.site_view(ui, ctx);
        } else {
            self.domain_picker(ui, ctx);
        }
    }

    /// Ask to Archive or Delete `host` (☰ › Project, app.rs): remove_dialog
    /// confirms it and carries it through. Ignored while one is running.
    pub fn ask_remove(&mut self, host: &str, delete: bool) {
        if self.removing.as_ref().is_some_and(|r| !matches!(r.stage, RemoveStage::Confirm | RemoveStage::Done(_))) {
            return;
        }
        self.removing = Some(Removing { host: host.to_owned(), delete, typed: String::new(), stage: RemoveStage::Confirm, stopped: Default::default(), release: Default::default() });
    }

    /// Archive or Delete, from confirmation to done, in one dialog: stop the
    /// app (its Down), Archive (git.ndexr.io's job, followed by archive_tick),
    /// and for Delete the release of the name.
    fn remove_dialog(&mut self, ctx: &egui::Context) {
        self.archive_tick(ctx);
        let Some(mut r) = self.removing.take() else { return };
        // Advance.
        match r.stage.clone() {
            RemoveStage::Stopping if r.stopped.lock().unwrap().is_some() => {
                self.start_archive(r.host.clone(), ctx);
                r.stage = RemoveStage::Archiving;
            }
            RemoveStage::Archiving => {
                let job = self.archive.lock().unwrap().clone();
                if let Some(j) = job.filter(|j| !j.running && j.ok.is_some()) {
                    let tail = j.lines.iter().rev().filter(|l| !l.trim().is_empty()).take(3).cloned().collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
                    // The job can finish with a refused gate: archived only if the folder is gone.
                    let listed = self.domains.lock().unwrap().clone();
                    match (j.ok, listed) {
                        (Some(false), _) => r.stage = RemoveStage::Done(Err(tail)),
                        (_, Some(Ok(v))) if v.iter().any(|d| d.host == r.host) => r.stage = RemoveStage::Done(Err(format!("its folder is still here - the archive stopped:\n{tail}"))),
                        (_, Some(Ok(_))) if r.delete => {
                            r.stage = RemoveStage::Releasing;
                            let slot = r.release.clone();
                            self.backend.query(Query::ProjectRelease { host: r.host.clone() }, ctx, move |x: Result<String, String>| {
                                *slot.lock().unwrap() = Some(x);
                            });
                        }
                        (_, Some(Ok(_))) => r.stage = RemoveStage::Done(Ok(format!("{} is archived: its history is in S3 and its folder is gone. Restore brings it back.", r.host))),
                        _ => {}
                    }
                }
            }
            RemoveStage::Releasing => {
                if let Some(x) = r.release.lock().unwrap().take() {
                    r.stage = RemoveStage::Done(x.map(|m| format!("{} is deleted. {m}", r.host)));
                }
            }
            _ => {}
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(400));
        let w = (ctx.content_rect().width() - 32.0).clamp(280.0, 520.0);
        let (mut go, mut close) = (false, false);
        let verb = if r.delete { "Delete" } else { "Archive" };
        egui::Modal::new(egui::Id::new("remove-project")).show(ctx, |ui| {
            ui.set_width(w);
            ui.label(RichText::new(format!("{verb} {}", r.host)).size(20.0).strong().color(theme::text_strong()));
            ui.add_space(6.0);
            match &r.stage {
                RemoveStage::Confirm => {
                    let what = if r.delete {
                        "Its app stops, its whole history is proven in S3, its folder is removed, and then its name is given up: the site, its routing and its registration go. An admin can still recover the history."
                    } else {
                        "Its app stops, its whole history is proven in S3, and its folder is removed. The name stays yours; Restore brings it back."
                    };
                    ui.add(egui::Label::new(RichText::new(what).size(14.0).color(theme::text())).wrap());
                    ui.add_space(8.0);
                    let ready = if r.delete {
                        ui.label(RichText::new(format!("Type {} to delete it", r.host)).size(13.0).color(theme::text()));
                        ui.add(egui::TextEdit::singleline(&mut r.typed).font(egui::TextStyle::Monospace).desired_width(f32::INFINITY));
                        r.typed.trim() == r.host
                    } else {
                        true
                    };
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        close = ui.button("Cancel").clicked();
                        go = ui.add_enabled(ready, egui::Button::new(RichText::new(format!("{verb} {}", r.host)).color(theme::bg())).fill(ace::error())).clicked();
                    });
                }
                RemoveStage::Done(res) => {
                    let (text, color) = match res {
                        Ok(m) => (m.clone(), theme::ok()),
                        Err(e) => (format!("Not done: {e}"), ace::error()),
                    };
                    ui.add(egui::Label::new(RichText::new(text).size(14.0).color(color)).wrap());
                    ui.add_space(8.0);
                    close = ui.button("Close").clicked();
                }
                stage => {
                    let (step, note) = match stage {
                        RemoveStage::Stopping => ("Stopping its app", String::new()),
                        RemoveStage::Archiving => ("Archiving: proving its history is in S3", self.archive.lock().unwrap().as_ref().and_then(|j| j.lines.iter().rev().find(|l| !l.trim().is_empty()).cloned()).unwrap_or_default()),
                        _ => ("Giving up the name: registry, ownership, site", String::new()),
                    };
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().size(14.0));
                        ui.label(RichText::new(step).size(14.0).color(theme::text_strong()));
                    });
                    if !note.is_empty() {
                        ui.add(egui::Label::new(RichText::new(note).monospace().size(12.0).color(theme::text())).truncate());
                    }
                }
            }
        });
        if go {
            // Stop its app first when it has its own port: Archive removes the
            // folder it runs from.
            let port = matches!(&*self.app_status.lock().unwrap(), Some(Ok(s)) if s.kind == "port" && s.verbs.iter().any(|v| v == "down")) && self.app_host == r.host;
            if port {
                r.stage = RemoveStage::Stopping;
                let slot = r.stopped.clone();
                self.backend.query(Query::AppAction { host: r.host.clone(), verb: "down".into() }, ctx, move |_: Result<JobView, String>| {
                    *slot.lock().unwrap() = Some(true);
                });
            } else {
                self.start_archive(r.host.clone(), ctx);
                r.stage = RemoveStage::Archiving;
            }
        }
        if !close {
            self.removing = Some(r);
        }
    }

    fn start_archive(&mut self, host: String, ctx: &egui::Context) {
        let (slot, errors) = (self.archive.clone(), self.errors.clone());
        *slot.lock().unwrap() = Some(JobView { what: format!("archive {host}"), running: true, ..Default::default() });
        self.archive_asked = f64::NEG_INFINITY;
        self.backend.query(Query::Archive { host }, ctx, move |r: Result<JobView, String>| match r {
            Ok(v) => *slot.lock().unwrap() = Some(v),
            Err(e) => {
                *slot.lock().unwrap() = Some(JobView { what: "archive".into(), ok: Some(false), lines: vec![e.clone()], ..Default::default() });
                Ide::fail(&errors, format!("archive: {e}"));
            }
        });
    }

    /// Follow a running Archive; when it ends, list the domains again - the
    /// domain is archived only if its folder is really gone (the script
    /// reports per domain, a refused gate does not fail the job).
    fn archive_tick(&mut self, ctx: &egui::Context) -> bool {
        let job = self.archive.lock().unwrap().clone();
        let Some(job) = job else { return false };
        let now = ctx.input(|i| i.time);
        if job.running && now - self.archive_asked > 1.0 {
            self.archive_asked = now;
            let (slot, domains) = (self.archive.clone(), self.domains.clone());
            let was_running = true;
            self.backend.query(Query::ArchiveJob, ctx, move |r: Result<JobView, String>| {
                if let Ok(v) = r {
                    if !v.what.is_empty() {
                        if was_running && !v.running {
                            *domains.lock().unwrap() = None;
                        }
                        *slot.lock().unwrap() = Some(v);
                    }
                }
            });
            ctx.request_repaint_after(std::time::Duration::from_secs(1));
        }
        if !job.running && self.domains.lock().unwrap().is_none() {
            self.domains_asked = false;
            self.ask_domains(ctx);
        }
        // Gone from the list: archived. Leave it - its tabs close, Files goes home.
        if !job.running {
            let listed = self.domains.lock().unwrap().clone();
            if let (Some(d), Some(Ok(v))) = (self.domain.clone(), listed) {
                if !v.iter().any(|x| x.host == d.host) && job.what.ends_with(&d.host) {
                    let inside = format!("{}/", d.path);
                    self.docs.retain(|x| !x.path.starts_with(&inside) || x.dirty());
                    self.cur = self.cur.min(self.docs.len().saturating_sub(1));
                    self.open_by_domain.remove(&d.host);
                    self.domain = None;
                    self.list("", ctx);
                }
            }
        }
        job.running
    }

    /// Step into a domain: its folder in Files, its module.r in Source, the
    /// consoles working there, and the site itself, live, in the Viewer.
    pub fn open_domain(&mut self, d: DomainInfo, ctx: &egui::Context) {
        self.ndexr_open = false;
        self.recent_hosts.retain(|h| *h != d.host);
        self.recent_hosts.insert(0, d.host.clone());
        self.recent_hosts.truncate(RECENT_MAX);
        let from = self.domain.as_ref().map(|c| c.host.clone());
        self.tabs.opened(&d.host, from.as_deref());
        // The domain already open: only its agent and site forward again.
        if self.domain.as_ref().is_some_and(|c| c.host == d.host) {
            self.bring(Tab::Agents);
            self.show_lower(Lower::App);
            return;
        }
        self.park_domain();
        self.open_folder(&d.path, ctx);
        self.unpark(d, ctx);
    }

    /// Leave the domain we were in: remember its open files, and park the
    /// ones with nothing unsaved (a dirty tab stays - nothing is lost).
    fn park_domain(&mut self) {
        if let Some(prev) = self.domain.clone() {
            let inside = |p: &str| p.starts_with(&format!("{}/", prev.path));
            let paths: Vec<String> = self.docs.iter().filter(|x| !x.untitled() && x.table.is_none() && inside(&x.path)).map(|x| x.path.clone()).collect();
            self.open_by_domain.insert(prev.host.clone(), paths);
            let mut keep = Vec::new();
            let mut i = self.docs.len();
            while i > 0 {
                i -= 1;
                if inside(&self.docs[i].path) && !self.docs[i].dirty() && self.docs[i].table.is_none() {
                    keep.push(self.docs.remove(i));
                }
            }
            keep.reverse();
            self.cur = self.cur.min(self.docs.len().saturating_sub(1));
            self.save_as = None;
            self.parked.insert(prev.host.clone(), keep);
        }
    }

    /// The rest of stepping into `d`, once its folder is open: its parked
    /// files back, or the ones it had open, and the rest.
    fn unpark(&mut self, d: DomainInfo, ctx: &egui::Context) {
        let parked = self.parked.remove(&d.host).unwrap_or_default();
        if !parked.is_empty() {
            // Shown now, read again behind: a change made while away arrives
            // through `reread`, which never overwrites unsaved edits.
            for doc in parked {
                let path = doc.path.clone();
                let preview = doc.preview;
                if !self.docs.iter().any(|x| x.path == path) {
                    self.docs.push(doc);
                }
                if !preview {
                    let (slot, errors) = (self.reread.clone(), self.errors.clone());
                    self.backend.query(Query::ReadFile { path }, ctx, move |r: Result<FileText, String>| match r {
                        Ok(f) => slot.lock().unwrap().push(f),
                        Err(e) => Ide::fail(&errors, e),
                    });
                }
            }
            self.cur = self.docs.len().saturating_sub(1);
        } else {
            let again: Vec<String> = self.open_by_domain.get(&d.host).cloned().unwrap_or_default();
            if again.is_empty() {
                let first = if d.module { "module.r" } else { "CONTEXT.md" };
                self.open(format!("{}/{first}", d.path), ctx);
            } else {
                for p in again {
                    self.open(p, ctx);
                }
            }
        }
        // The consoles start again IN the domain, fresh (operator,
        // 2026-10-18: "changing projects should create new environments for
        // the console and clear the other ones, dont save history within
        // cove"): app.rs follow_domain_env makes the domain the project, which
        // restarts R (Python inside it), the shell and the Terminal there. What
        // the last project typed and printed goes with it.
        self.forget_consoles();
        // The agent first, the site beside it.
        self.bring(Tab::Agents);
        self.show_lower(Lower::App);
        self.removing = None;
        self.site_seq += 1;
        self.domain = Some(d);
    }

    /// The App panel's body: the domain's live site, through the IDE. Every
    /// domain is live on the platform, which reloads its .r files per request,
    /// so the site IS the running app - nothing is started here (operator,
    /// 2026-10-14: "it is live always anyways, we just need to see the site
    /// THROUGH the ide"). In a browser it is a frame; in the desktop app an
    /// embedded webview (webview.rs), where no framing rule applies at all.
    fn site_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some(d) = self.domain.clone() else { return };
        // No strip of its own: the domain is already named in the line above,
        // and the site reloads itself on every save in the domain (operator,
        // 2026-10-15: "hide that line which has open and the domain").
        let url = format!("https://{}/", d.host);
        if self.launch.as_ref().is_some_and(|l| l.quiet && l.host == d.host) {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.add(egui::Spinner::new().size(22.0));
                ui.add_space(8.0);
                ui.label(RichText::new(format!("Starting {}", d.host)).size(16.0).color(theme::text_strong()));
            });
            return;
        }
        // Whatever answers at the domain is the app - a module.r, a proxied
        // server like cove.ndexr.io itself, a static page (operator,
        // 2026-10-15: "I dont think choosing to show module.r matters with
        // app because its live somehow regardless"). SiteGate below says when
        // nothing answers.
        // What is in front of the site, asked once per domain (and on ⟳).
        if self.gate_asked != d.host {
            self.gate_asked = d.host.clone();
            let (slot, host) = (self.gate.clone(), d.host.clone());
            self.backend.query(Query::SiteGate { host: host.clone() }, ctx, move |r: Result<String, String>| {
                *slot.lock().unwrap() = Some((host, r.unwrap_or_else(|e| format!("unknown: {e}"))));
            });
        }
        // A site just made settles for a while (its vhost, nginx's reload):
        // the frame and the gate are asked again at 8, 20 and 40 s, so what
        // shows is the app, not an error page from the minute it was born.
        if let Some((h, t0, done)) = self.site_fresh.as_mut() {
            const SETTLE: [f64; 3] = [8.0, 20.0, 40.0];
            if *h != d.host || *done >= SETTLE.len() {
                self.site_fresh = None;
            } else if self.launch.is_none() {
                let now = ctx.input(|i| i.time);
                if now - *t0 > SETTLE[*done] {
                    *done += 1;
                    self.site_seq += 1;
                    self.gate_asked.clear();
                }
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            }
        }
        let gate = self.gate.lock().unwrap().clone().filter(|(h, _)| *h == d.host).map(|(_, s)| s).unwrap_or_default();
        if gate == "down" {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new(format!("Nothing answers at {url} yet.")).color(theme::warn()));
                let started = self.publishing.get(&d.host).copied();
                let now = ctx.input(|i| i.time);
                match started {
                    Some(t) if now - t < 300.0 => {
                        ui.label(RichText::new("Publishing - DNS, certificate and site. It answers in a minute or two.").size(11.5).color(theme::muted()));
                        // Ask again every 20 s until it answers.
                        if now - t > 20.0 && self.gate_asked == d.host && (now - t) % 20.0 < 0.2 {
                            self.gate_asked.clear();
                        }
                        ctx.request_repaint_after(std::time::Duration::from_millis(200));
                    }
                    _ => {
                        ui.label(RichText::new("It is served once it is published: a DNS name, a certificate and the site.").size(11.5).color(theme::muted()));
                        if ui.add(theme::primary_button("Publish it now")).on_hover_text("the platform's provisioner, as route.ndexr.io runs it - your module.r is kept as it is").clicked() {
                            self.publish_domain(d.host.clone(), ctx);
                        }
                    }
                }
            });
            return;
        }
        // A page password's form will not render inside a browser frame
        // (X-Frame-Options: SAMEORIGIN, on purpose). Its unlock cookie is
        // host-scoped and SameSite=Lax, which this same-site frame does send:
        // unlock once in a tab, then reload. The desktop webview is not a
        // frame - the form simply shows there.
        if gate == "password" && cfg!(target_arch = "wasm32") {
            let (mut unlock, mut again) = (false, false);
            theme::strip(ui, theme::TOOL_H + 4.0, theme::surface(), |ui| {
                ui.add_space(6.0);
                ui.label(RichText::new("This site has a page password - unlock it once, then ⟳.").size(11.5).color(theme::warn()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    again = theme::tool(ui, "⟳ unlocked", "reload here once you have entered it in the tab").clicked();
                    unlock = theme::tool(ui, "Unlock in a tab ↗", "enter the page password once (kept 12 hours); the panel then shows the site").clicked();
                });
            });
            if unlock {
                ctx.open_url(egui::OpenUrl::new_tab(&url));
            }
            if again {
                self.site_seq += 1;
            }
        }
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, egui::Sense::hover());
        // The site sits over the canvas (a frame, or a native webview): it
        // steps aside while a menu, a popup or the launch window is open,
        // which it would cover - the launch window's right half was drawn
        // under a 404 frame (operator's screenshot, 2026-10-23).
        if ctx.any_popup_open() || self.launch.is_some() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        site_frame::place_site(ctx, rect, &url, self.site_seq);
        #[cfg(not(target_arch = "wasm32"))]
        if !crate::webview::place(ctx, rect, &url, self.site_seq) {
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("This build has no embedded browser - the site opens in yours.").color(theme::muted()));
                    if ui.add(theme::primary_button("Open the site")).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(&url));
                    }
                });
            });
        }
    }

    /// Show `addr` (a Viewer address) in the Viewer, and bring it forward.
    pub fn show_view(&mut self, addr: String) {
        if addr.is_empty() {
            return;
        }
        if !self.view.cur.is_empty() && self.view.cur != addr {
            self.view.back.push(std::mem::take(&mut self.view.cur));
            self.view.fwd.clear();
        }
        self.view.cur = addr;
        self.view.seq += 1;
        self.show_lower(Lower::Viewer);
    }

    /// The Viewer (RStudio's, back 2026-10-15, operator: "we do need the
    /// viewer back if people launch quarto, jupyter books, md etc"): renders
    /// (their log while they run), what R hands `options(viewer=)` -
    /// htmlwidgets, Shiny apps - and the servers a console or the terminal
    /// starts (quarto preview, mkdocs serve …), shown in place: a frame in the
    /// browser, a webview in the desktop app. Back / forward, reload, open in
    /// a browser tab, clear; the servers seen so far, to go back to.
    pub fn viewer(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.view_term {
            let mut close = false;
            theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
                ui.add_space(4.0);
                ui.add(egui::Label::new(crate::term::strip_label(&self.term)).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_space(4.0);
                    close = theme::tool(ui, "Page", "back to the Viewer's page - the program keeps running here").clicked();
                    if theme::tool(ui, "⟳", "end this terminal and start a new one").clicked() {
                        self.restart_terminal(ctx);
                    }
                });
            });
            if close {
                self.view_term = false;
            } else {
                let Ide { term, backend, .. } = self;
                term.show(ui, ctx, backend);
                return;
            }
        }
        let job = self.render.lock().unwrap().clone();
        let (mut back, mut fwd, mut reload, mut open, mut clear, mut again, mut go) = (false, false, false, false, false, false, None);
        let (mut to_term, term_alive) = (false, self.term.alive());
        let show_log = !job.what.is_empty() && (job.running || job.ok == Some(false) || self.view.log || self.view.cur.is_empty());
        let can_render = !job.running && self.docs.get(self.cur).is_some_and(|d| api::renders(&d.path));
        theme::strip(ui, theme::TOOL_H, theme::bg(), |ui| {
            ui.add_space(4.0);
            back = theme::tool_if(ui, !self.view.back.is_empty(), "◀", "back").clicked();
            fwd = theme::tool_if(ui, !self.view.fwd.is_empty(), "▶", "forward").clicked();
            reload = theme::tool_if(ui, !self.view.cur.is_empty(), "⟳", "reload").clicked();
            // A program run from the shell, still going behind this page.
            if term_alive {
                theme::tool_sep(ui);
                to_term = theme::tool(ui, "Terminal", "back to the program you ran from the Console's shell").clicked();
            }
            theme::tool_sep(ui);
            if !job.what.is_empty() {
                let (state, color) = match (job.running, job.ok) {
                    (true, _) => ("rendering…", theme::warn()),
                    (false, Some(true)) => ("rendered", theme::ok()),
                    (false, Some(false)) => ("render failed", ace::error()),
                    _ => ("", theme::hush()),
                };
                if job.running {
                    ui.add(egui::Spinner::new().size(12.0));
                }
                let tip = if show_log { "show the page" } else { "show the render's log" };
                if ui.add(egui::Button::new(RichText::new(format!("{} {state}", job.what)).size(11.5).color(color)).frame(false)).on_hover_text(tip).clicked() {
                    self.view.log = !show_log;
                }
                theme::tool_sep(ui);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                clear = theme::tool_if(ui, !self.view.cur.is_empty(), "✕", "clear the Viewer").clicked();
                open = theme::tool_if(ui, !self.view.cur.is_empty(), "↗", "open in a browser tab").clicked();
                again = theme::tool_if(ui, can_render, "Render", "render the open file again · Ctrl+Shift+K").clicked();
                if !self.view.servers.is_empty() {
                    ui.menu_button(RichText::new("Servers ▾").size(12.0), |ui| {
                        for s in &self.view.servers {
                            if ui.button(server_label(s)).clicked() {
                                go = Some(s.clone());
                                ui.close();
                            }
                        }
                    });
                }
                if !self.view.cur.is_empty() {
                    ui.add(egui::Label::new(RichText::new(server_label(&self.view.cur)).monospace().size(11.0).color(theme::hush())).truncate()).on_hover_text(&self.view.cur);
                }
            });
        });
        if to_term {
            self.view_term = true;
        }
        if back {
            if let Some(a) = self.view.back.pop() {
                self.view.fwd.push(std::mem::replace(&mut self.view.cur, a));
                self.view.seq += 1;
            }
        }
        if fwd {
            if let Some(a) = self.view.fwd.pop() {
                self.view.back.push(std::mem::replace(&mut self.view.cur, a));
                self.view.seq += 1;
            }
        }
        if reload {
            self.view.seq += 1;
        }
        if let Some(a) = go {
            self.view.log = false;
            self.show_view(a);
        }
        if open {
            ctx.open_url(egui::OpenUrl::new_tab(self.backend.viewer_url(&self.view.cur)));
        }
        if clear {
            self.view.cur.clear();
            self.view.back.clear();
            self.view.fwd.clear();
        }
        if again {
            self.view.log = true;
            self.render_current(ctx);
        }
        if show_log {
            egui::Frame::new().fill(ace::bg()).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
                egui::ScrollArea::both().id_salt("viewer-log").stick_to_bottom(true).auto_shrink([false, false]).show_rows(ui, row_h, job.lines.len(), |ui, rows| {
                    for l in &job.lines[rows] {
                        let color = if l.contains("ERROR") || l.starts_with("Error") { ace::error() } else if l.starts_with("WARN") || l.starts_with("Warning") { ace::warn() } else { ace::text() };
                        ui.add(egui::Label::new(RichText::new(if l.is_empty() { " " } else { l }).monospace().size(11.5).color(color)).extend());
                    }
                });
            });
            return;
        }
        if self.view.cur.is_empty() {
            ui.add_space(12.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("Nothing to view yet.").color(theme::muted()));
                ui.add_space(4.0);
                ui.label(RichText::new("Render a .qmd, .Rmd, .ipynb, .md, a Quarto project, a Jupyter Book or MkDocs (Ctrl+Shift+K); print an htmlwidget; run a Shiny app; or start a server - quarto preview, mkdocs serve, python -m http.server - in the Terminal. It shows here.").size(11.5).color(theme::hush()));
            });
            return;
        }
        let url = self.backend.viewer_url(&self.view.cur);
        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, egui::Sense::hover());
        // Over the canvas: it steps aside while a menu or popup is open.
        if ctx.any_popup_open() {
            return;
        }
        #[cfg(target_arch = "wasm32")]
        {
            let sep = if url.contains('?') { '&' } else { '?' };
            let src = if self.view.seq > 1 { format!("{url}{sep}cove_view={}", self.view.seq) } else { url };
            site_frame::place_viewer(ctx, rect, &src);
        }
        #[cfg(not(target_arch = "wasm32"))]
        if !crate::webview::place_in("viewer", ctx, rect, &url, self.view.seq) {
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("This build has no embedded browser - the page opens in yours.").color(theme::muted()));
                    if ui.add(theme::primary_button("Open the page")).clicked() {
                        ctx.open_url(egui::OpenUrl::new_tab(&url));
                    }
                });
            });
        }
    }
}

/// Put `word` in place of characters `at .. cursor` of `input`; returns
/// where the cursor goes, after it.
fn complete_into(input: &mut String, at: usize, cursor: usize, word: &str) -> usize {
    let chars: Vec<char> = input.chars().collect();
    let (at, cursor) = (at.min(chars.len()), cursor.min(chars.len()).max(at.min(chars.len())));
    let mut out: String = chars[..at].iter().collect();
    out.push_str(word);
    let end = out.chars().count();
    out.extend(&chars[cursor..]);
    *input = out;
    end
}

/// Move a text field's cursor to character `at`.
fn set_cursor(ctx: &egui::Context, id: egui::Id, at: usize) {
    let mut st = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(at))));
    st.store(ctx, id);
}

fn console_input(lang: Lang) -> egui::Id {
    egui::Id::new(("console-input", ix(lang)))
}

impl KernelView {
    /// The state without the transcript (cheap to hold across a frame).
    /// Only what follow_debugger reads.
    fn clone_debug(&self) -> KernelView {
        KernelView { debug: self.debug.clone(), debug_file: self.debug_file.clone(), debug_line: self.debug_line, debug_from: self.debug_from, debug_src: self.debug_src.clone(), ..Default::default() }
    }

    fn clone_head(&self) -> KernelView {
        KernelView { lines: Vec::new(), env: Vec::new(), banner: self.banner.clone(), busy: self.busy, alive: self.alive, next: self.next, plots: self.plots, cwd: self.cwd.clone(), table: None, tables: self.tables, view: self.view.clone(), views: self.views, prompt: self.prompt.clone(), debug: self.debug.clone(), ..Default::default() }
    }
}

/// The Viewer's live site, in a browser: an <iframe> laid over the Viewer
/// pane's rectangle (egui draws to one canvas and cannot hold a page). The
/// site is *.ndexr.io, which its own CSP lets cove.ndexr.io frame, and the same
/// site as the sign-in cookie, so it opens as the person.
#[cfg(target_arch = "wasm32")]
mod site_frame {
    use std::cell::Cell;

    use eframe::egui;

    /// The App's live site.
    const SITE: (&str, u8, &str) = ("ndexr-viewer-site", 1, "#fff");
    /// The Viewer's page: a render, a served folder, a local server (2026-10-15).
    const VIEWER: (&str, u8, &str) = ("cove-viewer", 4, "#fff");
    /// The Files tab's in-place image, PDF, video or sound (2026-10-14) -
    /// frames of their own (`PV`), so they and the App's site can both be on
    /// screen.
    const PREVIEW: (&str, u8, &str) = ("cove-file-preview", 2, "#0f1115");

    thread_local! {
        static SHOWN: Cell<u8> = const { Cell::new(0) };
    }

    fn element(id: &str) -> Option<web_sys::Element> {
        let doc = web_sys::window()?.document()?;
        if let Some(e) = doc.get_element_by_id(id) {
            return Some(e);
        }
        let e = doc.create_element("iframe").ok()?;
        e.set_id(id);
        let _ = e.set_attribute("allow", "clipboard-read; clipboard-write; fullscreen");
        doc.body()?.append_child(&e).ok()?;
        Some(e)
    }

    fn place_in(ctx: &egui::Context, (id, bit, bg): (&str, u8, &str), rect: egui::Rect, src: &str) {
        let Some(e) = element(id) else { return };
        // egui points -> CSS pixels; the canvas fills the page from 0,0.
        let z = ctx.zoom_factor();
        let style = format!(
            "position:absolute;left:{:.1}px;top:{:.1}px;width:{:.1}px;height:{:.1}px;border:0;background:{bg};z-index:2;display:block",
            rect.left() * z, rect.top() * z, rect.width() * z, rect.height() * z
        );
        if e.get_attribute("style").as_deref() != Some(style.as_str()) {
            let _ = e.set_attribute("style", &style);
        }
        if e.get_attribute("src").as_deref() != Some(src) {
            let _ = e.set_attribute("src", src);
        }
        SHOWN.with(|s| s.set(s.get() | bit));
    }

    /// The App's site, in two frames (operator, 2026-10-15: "we need a
    /// smooth refreshing of the app display under the app tab"): a save
    /// reloads the site in the hidden frame, and the frames swap once it has
    /// loaded - so the page on screen never goes blank. The frame is another
    /// origin and cannot be read; its own load event marks it (`data-loaded`).
    /// Another domain loads in front at once: nothing of it is there to keep.
    const SITES: [&str; 2] = ["ndexr-viewer-site", "ndexr-viewer-site-b"];
    /// Cove's launch splash, laid over the App panel while its site loads
    /// (operator, 2026-10-15: "run the cove launching loader inside of it
    /// while it loads, there is a large white screen"). Full while there is
    /// nothing to show - a domain just opened - and a veil over the old page
    /// while a save's reload loads behind it. Styled in web/index.html.
    const LOADER: &str = "cove-site-loader";
    /// A frame's load event fires when its document has arrived, before a
    /// Shiny page has drawn anything into it: the loader stays this long
    /// longer (s), so the white page under it is not what fades in.
    const SETTLE: f64 = 0.5;
    /// Never hold the site behind the loader longer than this (s): a page
    /// that never fires load is still shown.
    const GIVE_UP: f64 = 12.0;

    #[derive(Default)]
    struct Site {
        front: usize,
        src: [String; 2],
        loading: Option<f64>,
        /// When the hidden frame's load was first seen (the swap waits SETTLE).
        back_loaded: Option<f64>,
        /// When the front frame was given a new site, and when it loaded.
        front_since: f64,
        front_loaded: Option<f64>,
    }

    fn loader() -> Option<web_sys::Element> {
        let doc = web_sys::window()?.document()?;
        if let Some(e) = doc.get_element_by_id(LOADER) {
            return Some(e);
        }
        let e = doc.create_element("div").ok()?;
        e.set_id(LOADER);
        let _ = e.set_attribute("aria-hidden", "true");
        e.set_inner_html(concat!(
            r#"<div><svg class="gem" viewBox="-15 -15 30 30"><path d="M-13,-6 L-7,-13 L7,-13 L13,-6 L0,13 Z M-13,-6 L13,-6 M-7,-13 L-3.5,-6 L0,13 M7,-13 L3.5,-6 L0,13" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>"#,
            r#"<p class="title">Cove</p><div class="bar sweep"><span></span></div><p class="status">loading the site…</p></div>"#
        ));
        doc.body()?.append_child(&e).ok()?;
        Some(e)
    }

    thread_local! {
        static SITE_STATE: std::cell::RefCell<Site> = std::cell::RefCell::new(Site::default());
    }

    /// The site frame in front - what "Screenshot → agent" asks.
    #[allow(dead_code)]
    pub fn site_front() -> &'static str {
        SITE_STATE.with(|s| SITES[s.borrow().front])
    }

    pub fn place_site(ctx: &egui::Context, rect: egui::Rect, base: &str, seq: u64) {
        let src = format!("{base}?ide_reload={seq}");
        let now = ctx.input(|i| i.time);
        let z = ctx.zoom_factor();
        let style = |shown: bool| {
            format!(
                "position:absolute;left:{:.1}px;top:{:.1}px;width:{:.1}px;height:{:.1}px;border:0;background:{};z-index:{};display:block;visibility:{};pointer-events:{}",
                rect.left() * z, rect.top() * z, rect.width() * z, rect.height() * z, SITE.2,
                if shown { 2 } else { 1 }, if shown { "visible" } else { "hidden" }, if shown { "auto" } else { "none" }
            )
        };
        let set = |e: &web_sys::Element, attr: &str, v: &str| {
            if e.get_attribute(attr).as_deref() != Some(v) {
                let _ = e.set_attribute(attr, v);
            }
        };
        SITE_STATE.with(|st| {
            let mut st = st.borrow_mut();
            let (f, b) = (st.front, 1 - st.front);
            let (Some(fe), Some(be)) = (element(SITES[f]), element(SITES[b])) else { return };
            for e in [&fe, &be] {
                set(e, "onload", "this.setAttribute('data-loaded', this.getAttribute('src'))");
            }
            let same_site = |a: &str| a.split("?ide_reload=").next() == Some(base);
            if st.src[f] == src {
                st.loading = None;
            } else if !same_site(&st.src[f]) {
                set(&fe, "src", &src);
                st.src[f] = src.clone();
                st.loading = None;
                st.front_since = now;
                st.front_loaded = None;
            } else if st.src[b] != src {
                set(&be, "src", &src);
                st.src[b] = src.clone();
                st.loading = Some(now);
                st.back_loaded = None;
            }
            let mut front = f;
            if let Some(t) = st.loading {
                if st.back_loaded.is_none() && be.get_attribute("data-loaded").as_deref() == Some(src.as_str()) {
                    st.back_loaded = Some(now);
                }
                let ready = st.back_loaded.is_some_and(|l| now - l > SETTLE);
                if ready || now - t > 8.0 {
                    st.front = b;
                    st.loading = None;
                    front = b;
                }
            }
            let (fe, be) = if front == f { (fe, be) } else { (be, fe) };
            set(&fe, "style", &style(true));
            set(&be, "style", &style(false));
            // The loader: full until the front page has loaded and settled,
            // a veil while a reload loads behind it, then it fades out.
            if st.front_loaded.is_none() && fe.get_attribute("data-loaded").as_deref() == Some(st.src[front].as_str()) {
                st.front_loaded = Some(now);
            }
            let settled = st.front_loaded.is_some_and(|l| now - l > SETTLE) || now - st.front_since > GIVE_UP;
            let class = if !settled {
                "cove-site-loader"
            } else if st.loading.is_some() {
                "cove-site-loader veil"
            } else {
                "cove-site-loader off"
            };
            if let Some(l) = loader() {
                set(&l, "class", class);
                set(&l, "style", &format!(
                    "position:absolute;left:{:.1}px;top:{:.1}px;width:{:.1}px;height:{:.1}px;z-index:3;display:grid",
                    rect.left() * z, rect.top() * z, rect.width() * z, rect.height() * z
                ));
            }
            if !settled || st.loading.is_some() {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
        });
        SHOWN.with(|s| s.set(s.get() | SITE.1));
    }

    pub fn place_viewer(ctx: &egui::Context, rect: egui::Rect, src: &str) {
        place_in(ctx, VIEWER, rect, src);
    }

    /// Two frames for previews, so a file that changed is never shown blank
    /// while it loads (operator, 2026-10-14: "show what was there until we
    /// have reloaded and are ready to show the user the new display"): the
    /// new version loads in the hidden one, and they swap once it is in.
    const PV: [&str; 2] = ["cove-file-preview-a", "cove-file-preview-b"];

    #[derive(Default)]
    struct Pv {
        front: usize,
        src: [String; 2],
        /// When the hidden frame was given the new version (ui time).
        loading: Option<f64>,
    }

    thread_local! {
        static PVS: std::cell::RefCell<Pv> = std::cell::RefCell::new(Pv::default());
    }

    /// The file a preview address is about, without its version (`&v=`).
    fn same_file(a: &str, b: &str) -> bool {
        let strip = |s: &str| s.split("&v=").next().unwrap_or(s).to_owned();
        !a.is_empty() && strip(a) == strip(b)
    }

    /// The hidden frame has its document: same origin, past about:blank, done.
    /// A sandboxed one (SVG) cannot be read, so it is given a moment instead.
    fn loaded(e: &web_sys::Element) -> Option<bool> {
        use wasm_bindgen::JsCast;
        let d = e.dyn_ref::<web_sys::HtmlIFrameElement>()?.content_document()?;
        Some(d.ready_state() == "complete" && d.url().is_ok_and(|u| u.contains("/preview")))
    }

    pub fn place_preview(ctx: &egui::Context, rect: egui::Rect, src: &str) {
        let now = ctx.input(|i| i.time);
        let z = ctx.zoom_factor();
        let style = |shown: bool| {
            format!(
                "position:absolute;left:{:.1}px;top:{:.1}px;width:{:.1}px;height:{:.1}px;border:0;background:{};z-index:{};display:block;visibility:{};pointer-events:{}",
                rect.left() * z, rect.top() * z, rect.width() * z, rect.height() * z, PREVIEW.2,
                if shown { 2 } else { 1 }, if shown { "visible" } else { "hidden" }, if shown { "auto" } else { "none" }
            )
        };
        let set = |e: &web_sys::Element, attr: &str, v: &str| {
            if e.get_attribute(attr).as_deref() != Some(v) {
                let _ = e.set_attribute(attr, v);
            }
        };
        PVS.with(|pv| {
            let mut pv = pv.borrow_mut();
            let (f, b) = (pv.front, 1 - pv.front);
            let (Some(fe), Some(be)) = (element(PV[f]), element(PV[b])) else { return };
            if pv.src[f] == src {
                // Showing what is wanted: nothing loads behind it.
                pv.loading = None;
            } else if !same_file(&pv.src[f], src) {
                // Another file: nothing of it is on screen to keep.
                set(&fe, "src", src);
                pv.src[f] = src.to_owned();
                pv.loading = None;
            } else if pv.src[b] != src {
                // A new version of the file on screen: load it behind.
                set(&be, "src", src);
                pv.src[b] = src.to_owned();
                pv.loading = Some(now);
            }
            let mut front = f;
            if let Some(t) = pv.loading {
                let ready = match loaded(&be) {
                    Some(r) => r && now - t > 0.15,
                    None => now - t > 1.2,
                };
                if ready || now - t > 5.0 {
                    pv.front = b;
                    pv.loading = None;
                    front = b;
                } else {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
            }
            let (shown, other) = if front == f { (&fe, &be) } else { (&be, &fe) };
            set(shown, "style", &style(true));
            set(other, "style", &style(false));
        });
        SHOWN.with(|s| s.set(s.get() | PREVIEW.1));
    }

    /// Each frame not placed this frame - or every one, under a dialog - hides.
    pub fn end_frame(hide: bool) {
        let shown = SHOWN.with(|s| s.replace(0));
        for (ids, bit) in [(&[SITES[0], SITES[1], LOADER][..], SITE.1), (&[VIEWER.0][..], VIEWER.1), (&PV[..], PREVIEW.1)] {
            if !hide && shown & bit != 0 {
                continue;
            }
            for id in ids {
                if let Some(e) = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id(id)) {
                    if e.get_attribute("style").is_none_or(|s| !s.contains("display:none")) {
                        let _ = e.set_attribute("style", "display:none");
                    }
                }
            }
        }
    }
}

/// After every frame: the live site stays on screen only while the Viewer
/// showed it this frame, and never over a dialog (`hide`).
pub fn viewer_frame_end(hide: bool) {
    #[cfg(target_arch = "wasm32")]
    site_frame::end_frame(hide);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = hide;
}

/// A name a new domain may take: `mysite.ndexr.io`, `koodle.io`.
/// The domains as a tree, in the order to list them: (index, depth, children).
/// Sub-subdomains sit under their parent (operator, 2026-10-15: "sub sub
/// domains are nested under their parent ... like we do in software"):
/// x.consult.ndexr.io under consult.ndexr.io when that is listed too. Only a
/// parent of three labels or more takes children for now - consult.ndexr.io
/// does not yet roll up into ndexr.io. A domain shows when it matches `hit`
/// or something under it does; a parent's children show when `open(parent)`.
/// What to offer for a new domain typed into a domain box (operator,
/// 2026-10-16: "all users should be able to create any subdomain of theirs ...
/// they need their top level domains filled in for them if they are adding
/// subdomains"): "suno" offers suno.<each of the person's top domains>,
/// "suno.nan" completes to suno.nancydrennan1.ndexr.io, a full new name is
/// offered as typed. The person's own <user>.ndexr.io is always a parent, so a
/// subdomain can go under it before they have it as a domain of its own.
/// The person's own domain: <username>.ndexr.io, in their own domains folder
/// (/home/<username>/domains/…, or the platform's owner folder of that name).
/// The domain `folder` lies in: its own folder or one under it, the deepest
/// when one domain sits inside another.
fn domain_of_folder(list: &[DomainInfo], folder: &str) -> Option<DomainInfo> {
    let f = folder.trim_end_matches('/');
    let by_path = list
        .iter()
        .filter(|d| {
            let p = d.path.trim_end_matches('/');
            !p.is_empty() && (f == p || f.starts_with(&format!("{p}/")))
        })
        .max_by_key(|d| d.path.len())
        .cloned();
    // The same folder by another of its paths: in an account,
    // ~/ndexr/src/domains/<owner> is a link to ~/domains, so a site's "open in
    // Cove" link named fdrennan.ndexr.io by the one path while the list had the
    // other - nothing matched and the session stayed on the Julia project it
    // was in (operator's screenshot, 2026-10-28: "project doesnt seem to match
    // the messaging"). A browser cannot resolve a link, but a domain's folder
    // is named for its host: the deepest segment that is a listed host wins.
    by_path.or_else(|| {
        let segs: Vec<&str> = f.split('/').collect();
        segs.iter().rev().find_map(|s| list.iter().find(|d| !d.path.is_empty() && d.host == *s).cloned())
    })
}

fn own_domain(v: &[DomainInfo]) -> Option<DomainInfo> {
    v.iter()
        .find(|d| {
            let owner = std::path::Path::new(&d.path).parent().and_then(|p| p.file_name()).and_then(|f| f.to_str()).unwrap_or("");
            let user = d.path.strip_prefix("/home/").and_then(|r| r.split('/').next()).unwrap_or("");
            [owner, user].iter().any(|u| !u.is_empty() && *u != "ubuntu" && d.host == format!("{u}.ndexr.io"))
        })
        .cloned()
}

fn new_domain_offers(typed: &str, v: &[DomainInfo]) -> Vec<String> {
    let t = typed.trim().trim_end_matches('.').to_ascii_lowercase();
    let label_ok = |l: &str| !l.is_empty() && l.len() <= 63 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') && !l.starts_with('-') && !l.ends_with('-');
    let (first, rest) = t.split_once('.').unwrap_or((t.as_str(), ""));
    if !label_ok(first) {
        return Vec::new();
    }
    let have = |h: &str| v.iter().any(|d| d.host == h);
    // The top domains: none of the person's own sits above them (a bare apex
    // like ndexr.io does not count - everyone's sites are under it).
    let above = |d: &DomainInfo, o: &DomainInfo| o.host != d.host && o.host.matches('.').count() >= 2 && d.host.ends_with(&format!(".{}", o.host));
    let mut parents: Vec<String> = v.iter().filter(|d| !v.iter().any(|o| above(d, o))).map(|d| d.host.clone()).collect();
    let own = v
        .iter()
        .find_map(|d| d.path.strip_prefix("/home/")?.split('/').next().map(str::to_owned))
        .filter(|u| u != "ubuntu" && label_ok(u))
        .map(|u| format!("{u}.ndexr.io"));
    if let Some(o) = &own {
        parents.retain(|p| p != o);
        parents.insert(0, o.clone());
    } else {
        // The platform's own listing (the operator: every domain, folders
        // under /home/ubuntu): a word is first an ndexr.io site, then one under
        // the owner folder it sits in (src/domains/<owner>/), then the rest A-Z.
        // Before, "rix" offered rix.100days, rix.adanceplace ... and never
        // rix.ndexr.io (operator, 2026-10-25).
        let owner = v
            .iter()
            .find_map(|d| d.path.split("/src/domains/").nth(1)?.split('/').next().filter(|o| !o.contains('.')).map(str::to_owned))
            .filter(|u| label_ok(u))
            .map(|u| format!("{u}.ndexr.io"));
        if let Some(o) = owner {
            parents.retain(|p| *p != o);
            parents.insert(0, o);
        }
        parents.retain(|p| p != "ndexr.io");
        parents.insert(0, "ndexr.io".to_owned());
    }
    let mut out: Vec<String> = Vec::new();
    // As typed, unless it is the start of one of their own ("studio.nan").
    if is_host(&t) && t.contains('.') && !have(&t) && !parents.iter().any(|p| p.starts_with(rest) && p.as_str() != rest) {
        out.push(t.clone());
    }
    for p in parents {
        if rest.is_empty() || p.starts_with(rest) {
            let h = format!("{first}.{p}");
            if !have(&h) && !out.contains(&h) {
                out.push(h);
            }
        }
    }
    out.truncate(6);
    out
}

fn domain_tree(v: &[DomainInfo], hit: &dyn Fn(&DomainInfo) -> bool, open: impl Fn(&str) -> bool) -> Vec<(usize, usize, usize)> {
    let at: HashMap<&str, usize> = v.iter().enumerate().map(|(i, d)| (d.host.as_str(), i)).collect();
    let parent: Vec<Option<usize>> = v
        .iter()
        .map(|d| {
            let mut rest = d.host.as_str();
            while let Some((_, up)) = rest.split_once('.') {
                if up.matches('.').count() < 2 {
                    return None;
                }
                if let Some(&p) = at.get(up) {
                    return Some(p);
                }
                rest = up;
            }
            None
        })
        .collect();
    let mut kids: Vec<Vec<usize>> = vec![Vec::new(); v.len()];
    let mut roots = Vec::new();
    for i in 0..v.len() {
        match parent[i] {
            Some(p) if p != i => kids[p].push(i),
            _ => roots.push(i),
        }
    }
    let by_host = |a: &usize, b: &usize| v[*a].host.cmp(&v[*b].host);
    roots.sort_by(by_host);
    kids.iter_mut().for_each(|k| k.sort_by(by_host));
    // Shown: a match, or the parent of one.
    let mut shown = vec![false; v.len()];
    for i in 0..v.len() {
        if hit(&v[i]) {
            let mut j = Some(i);
            while let Some(k) = j.filter(|k| !shown[*k]) {
                shown[k] = true;
                j = parent[k];
            }
        }
    }
    fn walk(i: usize, depth: usize, kids: &[Vec<usize>], shown: &[bool], v: &[DomainInfo], open: &dyn Fn(&str) -> bool, out: &mut Vec<(usize, usize, usize)>) {
        if !shown[i] {
            return;
        }
        out.push((i, depth, kids[i].len()));
        if open(&v[i].host) {
            for &k in &kids[i] {
                walk(k, depth + 1, kids, shown, v, open, out);
            }
        }
    }
    let mut out = Vec::new();
    for r in roots {
        walk(r, 0, &kids, &shown, v, &open, &mut out);
    }
    out
}

fn is_host(t: &str) -> bool {
    let labels: Vec<&str> = t.split('.').collect();
    labels.len() >= 2 && labels.iter().all(|l| !l.is_empty() && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
}

/// The height of one row in the Files and Environment tables.
const ROW_H: f32 = 22.0;

/// One full-width table row, as RStudio's grids: striped, lit on hover, the
/// whole row clickable. Columns go in with `cell`.
/// The read-only tab showing a stopped function typed in the console (no file
/// of its own), as RStudio's source viewer does.
const DEBUG_DOC: &str = "Debugging · function source";

/// RStudio's Environment groups: 0 Data, 1 Values, 2 Functions.
fn env_group(class: &str) -> usize {
    let c = class.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| c.contains(w));
    if has(&["function", "closure", "builtin", "method"]) {
        2
    } else if has(&["data.frame", "tbl", "matrix", "array", "dataframe", "ndarray", "series"]) {
        0
    } else {
        1
    }
}

fn table_row(ui: &mut egui::Ui, i: usize, h: f32) -> (egui::Rect, egui::Response) {
    let (rect, r) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
    let fill = if r.hovered() {
        theme::surface_hi()
    } else if i % 2 == 1 {
        theme::surface()
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, 0, fill);
    if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    (rect, r)
}

/// A table's header row, with a rule under it.
fn table_head(ui: &mut egui::Ui, h: f32) -> egui::Rect {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0, theme::surface());
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, egui::Stroke::new(1.0, theme::pane_brd()));
    rect
}

/// A file's glyph and colour in Files and on its tab: folders, code in its
/// language's colour, data, images and PDFs, anything else.
fn file_glyph(name: &str, dir: bool) -> (crate::icons::Glyph, Color32) {
    use crate::icons::Glyph;
    if dir {
        (Glyph::Folder, theme::primary())
    } else if let Some(l) = Lang::of_path(name) {
        (Glyph::Code, Channel::Code(l).color())
    } else if crate::api::is_sqlite(name) || [".csv", ".tsv", ".parquet", ".feather", ".rds", ".rda", ".json"].iter().any(|x| name.to_ascii_lowercase().ends_with(x)) {
        (Glyph::Data, theme::amber())
    } else if crate::api::preview_type(name).is_some() {
        (Glyph::Image, theme::ok())
    } else {
        (Glyph::File, theme::hush())
    }
}

/// The glyph column at a row's start.
fn glyph_cell(ui: &egui::Ui, row: egui::Rect, g: crate::icons::Glyph, c: Color32) {
    let r = egui::Rect::from_center_size(egui::pos2(row.left() + 13.0, row.center().y), egui::vec2(14.0, 14.0));
    crate::icons::paint(ui.painter(), r, g, c);
}

/// A tab's name cut to `max` characters in the middle, so both a long
/// name's start and its extension stay: `analysis_of_lake…temps.qmd`.
fn middle_cut(name: &str, max: usize) -> String {
    let n = name.chars().count();
    if n <= max || max < 5 {
        return name.to_owned();
    }
    let tail = (max - 1) / 2;
    let head = max - 1 - tail;
    let start: String = name.chars().take(head).collect();
    let end: String = name.chars().skip(n - tail).collect();
    format!("{start}…{end}")
}

/// Columns `x .. x + w` of a row: one line, cut to fit.
fn cell(ui: &mut egui::Ui, row: egui::Rect, x: f32, w: f32, text: RichText, right: bool) {
    let rect = egui::Rect::from_min_size(egui::pos2(row.left() + x, row.top()), egui::vec2(w.max(1.0), row.height()));
    let layout = if right { egui::Layout::right_to_left(egui::Align::Center) } else { egui::Layout::left_to_right(egui::Align::Center) };
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect).layout(layout), |ui| {
        ui.add(egui::Label::new(text).truncate().selectable(false));
    });
}

/// "2026-10-13 16:21" from seconds since the epoch (UTC); "" when unknown.
/// A path from the project folder on ("/<host>/src/app.r"), so the agent's
/// paths and Files' match wherever each sees the project mounted (a cell's
/// ~/domains/<host> and the host's src/domains/<owner>/<host>). A path
/// outside the project stays whole.
fn in_project<'a>(host: &str, path: &'a str) -> &'a str {
    if host.is_empty() {
        return path;
    }
    let seg = format!("/{host}");
    path.match_indices(&seg).map(|(i, _)| i).find(|&i| matches!(path[i + seg.len()..].chars().next(), None | Some('/'))).map_or(path, |i| &path[i..])
}

fn utc_minute(secs: i64) -> String {
    if secs <= 0 {
        return String::new();
    }
    // Howard Hinnant's civil_from_days: no date crate, and the same in wasm.
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

fn human_bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{:.1} KB", n as f64 / 1024.0),
        n => format!("{n} B"),
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_folder_opens_the_deepest_domain_it_is_in() {
        let d = |host: &str, path: &str| DomainInfo { host: host.into(), path: path.into(), title: String::new(), module: true, ..Default::default() };
        let list = vec![
            d("domains.ndexr.io", "/s/drennanfreddy/domains.ndexr.io"),
            d("recipes.ndexr.io", "/s/drennanfreddy/recipes.ndexr.io"),
            d("ndexr.io", "/s/ndexr.io"),
            d("app.ndexr.io", "/s/ndexr.io/app.ndexr.io"),
        ];
        let host = |f: &str| super::domain_of_folder(&list, f).map(|x| x.host);
        assert_eq!(host("/s/drennanfreddy/recipes.ndexr.io"), Some("recipes.ndexr.io".into()));
        assert_eq!(host("/s/drennanfreddy/recipes.ndexr.io/"), Some("recipes.ndexr.io".into()));
        assert_eq!(host("/s/drennanfreddy/recipes.ndexr.io/src"), Some("recipes.ndexr.io".into()));
        assert_eq!(host("/s/ndexr.io/app.ndexr.io/x"), Some("app.ndexr.io".into()));
        assert_eq!(host("/s/drennanfreddy/recipes.ndexr.iox"), None);
        assert_eq!(host("/home/fdrennan"), None);
        // The same folder by a linked path: its host-named segment finds it.
        let list = vec![
            d("fdrennan.ndexr.io", "/home/fdrennan/domains/fdrennan.ndexr.io"),
            d("afaefsedfaea.fdrennan.ndexr.io", "/home/fdrennan/domains/afaefsedfaea.fdrennan.ndexr.io"),
        ];
        let host = |f: &str| super::domain_of_folder(&list, f).map(|x| x.host);
        assert_eq!(host("/home/fdrennan/ndexr/src/domains/fdrennan/fdrennan.ndexr.io"), Some("fdrennan.ndexr.io".into()));
        assert_eq!(host("/home/fdrennan/ndexr/src/domains/fdrennan/fdrennan.ndexr.io/www"), Some("fdrennan.ndexr.io".into()));
        assert_eq!(host("/home/fdrennan/ndexr/src/domains/fdrennan"), None);
    }

    #[test]
    fn long_tab_names_keep_their_start_and_extension() {
        assert_eq!(middle_cut("analysis_of_lake_surface_temps.qmd", 14), "analysi…ps.qmd");
        assert_eq!(middle_cut("module.r", 14), "module.r");
    }

    #[test]
    fn full_screen_shell_programs_go_to_the_terminal() {
        for c in ["vim app.R", "top", "FOO=1 htop", "ls | less", "cd x && ranger", "claude", "claude --model opus", "python3", "R --vanilla", "ssh router", "git commit", "git rebase -i HEAD~3", "sudo apt update", "/usr/bin/nano f", "crontab -e"] {
            assert!(wants_terminal(c), "{c}");
        }
        for c in ["ls -la", "ml av", "claude -p 'hi'", "python3 x.py", "python3 -c 'print(1)'", "R -e 'x'", "ssh router uptime", "git commit -m msg", "git status", "echo vim", "# vim", "Rscript a.R", "emacs --batch -l x.el", "cat f | head"] {
            assert!(!wants_terminal(c), "{c}");
        }
    }

    #[test]
    fn console_bands_follow_the_order_chunks_ran() {
        use crate::api::Line;
        let l = |k: LineKind, t: &str| Line { kind: k, text: t.into() };
        // R: banner, an R chunk, a Python chunk; the shell: banner, one chunk.
        let r = KernelView { lines: vec![l(LineKind::System, "R 4.6"), l(LineKind::Input, "> x <- 1"), l(LineKind::Input, ">>> 2+2"), l(LineKind::Output, "4"), l(LineKind::Output, "late R")], next: 5, ..Default::default() };
        let sh = KernelView { lines: vec![l(LineKind::System, "bash"), l(LineKind::Input, "$ ls"), l(LineKind::Output, "a.R")], next: 3, ..Default::default() };
        let jl = KernelView { lines: vec![l(LineKind::System, "julia 1.11.7"), l(LineKind::Input, "julia> 1 + 1"), l(LineKind::Output, "2")], next: 3, ..Default::default() };
        let (rr, py, shc, jlc) = (Channel::Code(Lang::R), Channel::Code(Lang::Python), Channel::Code(Lang::Shell), Channel::Code(Lang::Julia));
        let runs = vec![
            Run::code(rr, "x <- 1", 1),
            Run::code(shc, "ls", 1),
            Run { agent: Some("+1".into()), ..Run::code(Channel::Agent, "plot it", 0) },
            Run::code(py, "2+2", 2),
            Run { moved: true, ..Run::code(shc, "vim a.R", 0) },
            Run::code(jlc, "1 + 1", 1),
        ];
        let agent = |k: &str| (k == "+1").then(|| ("plot it".to_owned(), "Done.\nSee Plots.".to_owned(), true));
        let got: Vec<String> = console_rows(&runs, [&r, &sh, &jl], &agent).iter().map(|row| match row {
            Row::Head(c) => format!("# {}", c.word()),
            Row::Line(_, k, p) => [&r, &sh, &jl][*k].lines[*p].text.clone(),
            Row::Note(_, _, t) => t.clone(),
        }).collect();
        assert_eq!(got, ["# R", "R 4.6", "> x <- 1", "# Shell", "$ ls", "a.R", "# Agent", "» plot it", "Done.", "See Plots.", "# Python", ">>> 2+2", "4", "late R", "# Shell", "$ vim a.R", "↗ a full-screen program: it runs in the Terminal tab", "# Julia", "julia> 1 + 1", "2"]);
    }

    #[test]
    fn the_ring_goes_round_and_completion_fills_in_place() {
        assert_eq!(Channel::Code(Lang::Shell).step(1, &CHANNELS), Channel::Code(Lang::Julia));
        assert_eq!(Channel::Code(Lang::Julia).step(1, &CHANNELS), Channel::Code(Lang::R));
        assert_eq!(Channel::Code(Lang::R).step(-1, &CHANNELS), Channel::Code(Lang::Julia));
        assert_eq!(common_prefix(&["summary".into(), "sum".into(), "suppressWarnings".into()]), "su");
        let mut s = "x <- summ(y)".to_owned();
        assert_eq!(complete_into(&mut s, 5, 9, "summary"), 12);
        assert_eq!(s, "x <- summary(y)");
    }

    #[test]
    fn each_environment_brings_its_own_consoles() {
        use super::env_consoles as c;
        use Lang::*;
        assert_eq!(c("env/R/4.6.1-bioc3.23"), [R, Shell]);
        assert_eq!(c("env/Python/3.13.15"), [Python, Shell]);
        assert_eq!(c("env/Julia/1.11.7"), [Julia, Shell]);
        assert_eq!(c("env/Rust/1.96.0"), [Shell]);
        assert_eq!(c("nodejs/22.22.2-GCCcore-13.3.0"), [Shell]);
        // R and Python together only where the environment combines them.
        assert_eq!(c("env/r4.6_py3.13-release"), [R, Python, Shell]);
        // A layered list is its first module's; a person's revision its base's.
        assert_eq!(c("env/R/4.6.1-bioc3.23 GDAL/3.10.0-foss-2024a"), [R, Shell]);
        assert_eq!(c("cove-env/Python-3.13.15/2"), [Python, Shell]);
        assert_eq!(c("cove-env/r4.6_py3.13-release/1"), [R, Python, Shell]);
        // No environment at all (a laptop's own programs): every console.
        assert_eq!(c(""), [R, Python, Shell, Julia]);
        // The ring goes round only what the environment brings.
        let ring = [Channel::Code(Python), Channel::Code(Shell)];
        assert_eq!(Channel::Code(Shell).step(1, &ring), Channel::Code(Python));
        assert_eq!(Channel::Code(Python).step(-1, &ring), Channel::Code(Shell));
    }

    #[test]
    fn recent_projects_are_the_opened_ones_then_the_newest_changed() {
        let d = |h: &str, m: u64| DomainInfo { host: h.into(), modified: m, ..Default::default() };
        let all = vec![d("old.ndexr.io", 10), d("new.ndexr.io", 30), d("mid.ndexr.io", 20), d("seen.ndexr.io", 1)];
        let hosts = |v: Vec<DomainInfo>| v.into_iter().map(|d| d.host).collect::<Vec<_>>();
        // Nothing opened yet: never empty - the domains by when they changed.
        assert_eq!(hosts(super::recent_projects(&[], &all)), ["new.ndexr.io", "mid.ndexr.io", "old.ndexr.io", "seen.ndexr.io"]);
        // Opened first, in the order opened; one gone since is skipped.
        let opened = vec!["seen.ndexr.io".to_owned(), "gone.ndexr.io".to_owned(), "old.ndexr.io".to_owned()];
        assert_eq!(hosts(super::recent_projects(&opened, &all)), ["seen.ndexr.io", "old.ndexr.io", "new.ndexr.io", "mid.ndexr.io"]);
        // At most RECENT_MAX.
        let many: Vec<DomainInfo> = (0..20).map(|k| d(&format!("p{k}.ndexr.io"), k)).collect();
        assert_eq!(super::recent_projects(&[], &many).len(), super::RECENT_MAX);
    }

    #[test]
    fn the_project_field_finds_by_start_then_anywhere() {
        let d = |h: &str, t: &str, m: u64| DomainInfo { host: h.into(), title: t.into(), modified: m, ..Default::default() };
        let all = vec![d("blender.fdrennan.ndexr.io", "", 3), d("system.fdrennan.ndexr.io", "Host monitor", 2), d("myblog.ndexr.io", "", 1)];
        let ring = super::project_ring(&["system.fdrennan.ndexr.io".to_owned()], &all);
        let hosts = |v: Vec<DomainInfo>| v.into_iter().map(|d| d.host).collect::<Vec<_>>();
        assert_eq!(hosts(ring.clone()), ["system.fdrennan.ndexr.io", "blender.fdrennan.ndexr.io", "myblog.ndexr.io"], "opened first, then newest");
        assert_eq!(hosts(super::find_projects("bl", &ring)), ["blender.fdrennan.ndexr.io", "myblog.ndexr.io"], "starts-with before contains");
        assert_eq!(hosts(super::find_projects("monitor", &ring)), ["system.fdrennan.ndexr.io"], "titles count");
        assert!(super::find_projects("  ", &ring).is_empty());
        let named = |hs: &[&str]| hs.iter().map(|h| DomainInfo { host: (*h).to_owned(), ..Default::default() }).collect::<Vec<_>>();
        let mine = named(&["ndexr.io.archive.ndexr.io", "100days.ndexr.io", "ndexr.io", "ndexr.ndexr.io"]);
        assert_eq!(hosts(super::find_projects("ndexr.io", &mine))[0], "ndexr.io", "the name typed whole is first, before ring order");
    }

    #[test]
    fn sub_subdomains_sit_under_their_parent() {
        let d = |h: &str| DomainInfo { host: h.into(), ..Default::default() };
        let v = vec![d("x.consult.ndexr.io"), d("ndexr.io"), d("consult.ndexr.io"), d("a.b.consult.ndexr.io"), d("cove.ndexr.io")];
        let all = |_: &DomainInfo| true;
        let tree: Vec<(&str, usize, usize)> = domain_tree(&v, &all, |_| true).into_iter().map(|(i, dep, k)| (v[i].host.as_str(), dep, k)).collect();
        // consult.ndexr.io does not roll up into ndexr.io (two labels); a.b.… skips the missing b.….
        assert_eq!(tree, vec![("consult.ndexr.io", 0, 2), ("a.b.consult.ndexr.io", 1, 0), ("x.consult.ndexr.io", 1, 0), ("cove.ndexr.io", 0, 0), ("ndexr.io", 0, 0)]);
        // Folded: the parent alone. Finding a child brings its parent with it.
        assert_eq!(domain_tree(&v, &all, |_| false).len(), 3);
        let x = |d: &DomainInfo| d.host.starts_with("x.");
        let found: Vec<usize> = domain_tree(&v, &x, |_| true).into_iter().map(|t| t.0).collect();
        assert_eq!(found, vec![2, 0]);
    }

    use super::*;

    #[test]
    fn a_new_name_gets_the_persons_top_domains() {
        let d = |h: &str| -> DomainInfo { serde_json::from_value(serde_json::json!({ "host": h, "path": format!("/home/nancydrennan1/domains/{h}") })).unwrap() };
        let v = vec![d("nancydrennan1.ndexr.io"), d("suno.nancydrennan1.ndexr.io"), d("chocolate.ndexr.io")];
        assert_eq!(new_domain_offers("studio", &v), vec!["studio.nancydrennan1.ndexr.io", "studio.chocolate.ndexr.io"]);
        assert_eq!(new_domain_offers("studio.nan", &v), vec!["studio.nancydrennan1.ndexr.io"]);
        // what exists is not offered again; a full new name is offered as typed
        assert_eq!(new_domain_offers("suno", &v), vec!["suno.chocolate.ndexr.io"]);
        assert_eq!(new_domain_offers("x.koodle.io", &v)[0], "x.koodle.io");
        assert!(new_domain_offers("bad name", &v).is_empty());
        // their own <user>.ndexr.io is a parent before they have it
        let w = vec![d("chocolate.ndexr.io")];
        assert_eq!(new_domain_offers("lab", &w), vec!["lab.nancydrennan1.ndexr.io", "lab.chocolate.ndexr.io"]);
        // The platform's listing (the operator): ndexr.io first, then the owner folder's domain.
        let op = |h: &str, owner: &str| -> DomainInfo { serde_json::from_value(serde_json::json!({ "host": h, "path": format!("/home/ubuntu/ndexr/src/domains/{owner}/{h}") })).unwrap() };
        let all = vec![op("100days.ndexr.io", "drennanfreddy"), op("admin.ndexr.io", "fdrennan"), op("drennanfreddy.ndexr.io", "drennanfreddy")];
        assert_eq!(&new_domain_offers("rix", &all)[..2], ["rix.ndexr.io", "rix.drennanfreddy.ndexr.io"]);
        let taken = vec![op("rix.ndexr.io", "drennanfreddy"), op("drennanfreddy.ndexr.io", "drennanfreddy")];
        assert_eq!(new_domain_offers("rix", &taken)[0], "rix.drennanfreddy.ndexr.io");
    }

    #[test]
    fn file_times_read_as_utc_minutes() {
        assert_eq!(utc_minute(0), "");
        assert_eq!(utc_minute(1), "1970-01-01 00:00");
        assert_eq!(utc_minute(951_782_400), "2000-02-29 00:00");
        assert_eq!(utc_minute(1_791_908_460), "2026-10-13 16:21");
    }

    #[test]
    fn agent_and_files_paths_meet_at_the_project() {
        let h = "blender.fdrennan.ndexr.io";
        // The cell's mount and the host's owner folder name the same file.
        assert_eq!(in_project(h, "/home/fdrennan/domains/blender.fdrennan.ndexr.io/src/app.r"), "/blender.fdrennan.ndexr.io/src/app.r");
        assert_eq!(in_project(h, "/home/ubuntu/ndexr/src/domains/fdrennan/blender.fdrennan.ndexr.io/src/app.r"), "/blender.fdrennan.ndexr.io/src/app.r");
        assert_eq!(in_project(h, "/x/domains/blender.fdrennan.ndexr.io"), "/blender.fdrennan.ndexr.io");
        // A longer name that starts with the host is not the project.
        assert_eq!(in_project(h, "/x/blender.fdrennan.ndexr.io.bak/a"), "/x/blender.fdrennan.ndexr.io.bak/a");
        assert_eq!(in_project("", "/a/b"), "/a/b");
        assert_eq!(crate::agent::edit_target("Edit · /d/x/module.r"), Some("/d/x/module.r"));
        assert_eq!(crate::agent::edit_target("Bash · ls"), None);
        assert_eq!(crate::agent::edit_target("Write"), None);
    }

    #[test]
    fn ctrl_enter_takes_the_whole_call() {
        let src = "x <- 1\nfit <- lm(y ~ x,\n  data = d)\nsummary(fit)\n";
        let at = src.find("fit").unwrap();
        let (code, next) = statement_at(src, at, Lang::R);
        assert_eq!(code, "fit <- lm(y ~ x,\n  data = d)");
        assert_eq!(src.chars().skip(next).take(7).collect::<String>(), "summary");
    }

    #[test]
    fn ctrl_enter_takes_a_python_block() {
        let src = "def f(x):\n    return x * 2\n\nf(3)\n";
        let (code, _) = statement_at(src, 0, Lang::Python);
        assert_eq!(code, "def f(x):\n    return x * 2\n");
        let (code, _) = statement_at(src, src.find("f(3)").unwrap(), Lang::Python);
        assert_eq!(code, "f(3)");
    }

    #[test]
    fn brackets_in_strings_and_comments_do_not_count() {
        assert_eq!(bracket_depth("paste(\"(\", x) # )"), 0);
        assert_eq!(bracket_depth("c(1, 2,"), 1);
    }

    #[test]
    fn chunks_carry_their_language_and_skip_plain_code() {
        let doc = "---\ntitle: x\n---\n\n```{r setup, include=FALSE}\nlibrary(x)\n```\n\n```python\nnot a chunk\n```\n\n```{python}\n#| eval: false\nimport os\n```\n```{julia}\n1\n```\n```{bash}\nls";
        let c = chunks(doc);
        assert_eq!(c.iter().map(|c| c.engine.as_str()).collect::<Vec<_>>(), ["r", "python", "julia", "bash"]);
        assert_eq!(c[0].lang, Some(Lang::R));
        assert_eq!(chunk_code(doc, &c[0]), "library(x)");
        assert!(c[0].eval && !c[1].eval, "#| eval: false is honoured");
        assert_eq!(c[2].lang, Some(Lang::Julia));
        assert!(!c[3].closed);
        assert_eq!(chunk_code(doc, &c[3]), "ls");
    }

    #[test]
    fn a_document_highlights_every_byte() {
        let doc = "# Title\n\n```{r}\nx <- 1 # c\n```\ntext\n```{python}\ndef f(): pass\n";
        assert_eq!(highlight_doc(doc).text, doc);
        assert_eq!(highlight_doc("").text, "");
    }

    #[test]
    fn lines_and_their_starts() {
        let t = "ab\ncd\n\nef";
        assert_eq!(line_of(t, 0), 0);
        assert_eq!(line_of(t, 4), 1);
        assert_eq!(line_start(t, 1), 3);
        assert_eq!(line_start(t, 3), 7);
    }

    #[test]
    fn highlighting_keeps_every_byte() {
        for (src, lang) in [("x <- c(1, 2) # é note\nif (TRUE) 'a\\'b'", Lang::R), ("def f(s='''x\ny'''):\n    return 1.5e3  # ok", Lang::Python), ("for f in ${HOME}/*.R; do echo \"$f\" ${#f}; done # é", Lang::Shell)] {
            let job = highlight(src, Some(lang));
            assert_eq!(job.text, src);
        }
    }

    #[test]
    fn a_first_visit_opens_the_persons_own_domain() {
        let d = |h: &str, p: &str| -> DomainInfo { serde_json::from_value(serde_json::json!({ "host": h, "path": p })).unwrap() };
        let mine = [d("suno.calais.ndexr.io", "/home/calais/domains/suno.calais.ndexr.io"), d("calais.ndexr.io", "/home/calais/domains/calais.ndexr.io")];
        assert_eq!(super::own_domain(&mine).map(|x| x.host), Some("calais.ndexr.io".into()));
        let platform = [d("cove.ndexr.io", "/home/ubuntu/ndexr/src/domains/drennanfreddy/cove.ndexr.io"), d("drennanfreddy.ndexr.io", "/home/ubuntu/ndexr/src/domains/drennanfreddy/drennanfreddy.ndexr.io")];
        assert_eq!(super::own_domain(&platform).map(|x| x.host), Some("drennanfreddy.ndexr.io".into()));
        assert!(super::own_domain(&[d("x.ndexr.io", "/srv/x.ndexr.io")]).is_none());
    }
}

/// A new domain's pick in the selector (kind_chooser): its name, the category
/// open (None: the categories), and the row chosen in it.
#[derive(Clone)]
struct Choosing {
    host: String,
    cat: Option<usize>,
    way: Option<usize>,
    /// The explorer's search, across every starter of every world.
    find: String,
    /// One world shown, or all of them.
    world: Option<crate::starters::World>,
    /// Create pressed and not answered yet: the dialog stays, saying so.
    creating: bool,
    /// How the create went: Ok closes the dialog (the project opens), Err
    /// stays in it, said in words, with Create and Cancel still there
    /// (operator, 2026-10-25: testy.ndexr.io closed the chooser on an error
    /// nobody saw).
    outcome: Arc<Mutex<Option<Result<(), String>>>>,
}

impl Choosing {
    fn new(host: &str) -> Choosing {
        Choosing { host: host.to_owned(), cat: None, way: None, find: String::new(), world: None, creating: false, outcome: Default::default() }
    }
}

/// Each language's colour in the selector.
fn lang_color(lang: &str) -> Color32 {
    match lang {
        "R" => theme::primary(),
        "Rust" => theme::amber(),
        "Julia" => theme::alt(),
        "React" => theme::net(),
        _ => theme::ok(),
    }
}

/// One category: a small picture of what it makes, its name, a line, and what
/// can be created today.
/// The open project's light and words in the strip. A compile says so, with
/// its age, whoever started it: Cove's Build, the agent or a terminal (apps.rs
/// `compiling`).
fn strip_light(s: &crate::api::AppStatus, running: bool) -> (Color32, String) {
    if let Some(t) = s.compiling {
        (theme::amber(), format!("compiling · {}:{:02}", t / 60, t % 60))
    } else if running {
        (theme::amber(), "working".to_owned())
    } else if !s.built {
        (theme::hush(), "not built yet - Build compiles it".to_owned())
    } else if s.kind == "module" {
        (theme::ok(), "served from module.r".to_owned())
    } else if s.port.is_empty() && s.kind == "port" {
        // Only when it has a program to run that lacks the setting.
        (theme::warn(), if s.inferred { "pick its type in Settings › Info to run it here".to_owned() } else { ".cove/app.env names no APP_PORT".to_owned() })
    } else if s.port.is_empty() {
        (Color32::TRANSPARENT, String::new())
    } else if s.running {
        (theme::ok(), format!("up · port {}", s.port))
    } else {
        (theme::hush(), format!("down · port {}", s.port))
    }
}

fn category_card(ui: &mut egui::Ui, cat: &crate::starters::Category, i: usize, w: f32) -> egui::Response {
    let ready = crate::starters::ready_count(cat);
    // Top-down inside its own column: inside the chooser's horizontal row the
    // card's picture, title and words were laid side by side, and the words
    // got one letter's width (operator's screenshot, 2026-10-24).
    ui.allocate_ui_with_layout(egui::vec2(w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| category_card_body(ui, cat, i, w, ready)).inner
}

fn category_card_body(ui: &mut egui::Ui, cat: &crate::starters::Category, i: usize, w: f32, ready: usize) -> egui::Response {
    egui::Frame::new()
        .fill(theme::surface())
        .stroke(egui::Stroke::new(1.0, theme::pane_brd()))
        .corner_radius(10)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(w - 24.0);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(w - 24.0, 140.0), egui::Sense::hover());
            // Clipped to its frame: Optimization's outer contours were drawn
            // past the card into the next one (operator screenshot, 2026-10-27).
            category_picture(&ui.painter().with_clip_rect(rect), rect, i);
            ui.add_space(8.0);
            ui.label(RichText::new(cat.title).size(18.0).strong().color(theme::text_strong()));
            ui.add(egui::Label::new(RichText::new(cat.what).size(14.0).color(theme::text())).wrap());
            ui.add_space(4.0);
            // Each language once ("ready in R, R" said R twice), and how many
            // experiments the category holds.
            let mut langs: Vec<(&str, &str)> = Vec::new();
            for w in cat.ways.iter().filter(|w| w.starter.is_some()) {
                let key = crate::frameworks::of_way(w.lang, w.framework);
                if !langs.iter().any(|(l, k)| *l == w.lang && *k == key) {
                    langs.push((w.lang, key));
                }
            }
            ui.horizontal_wrapped(|ui| {
                if ready == 0 {
                    ui.label(RichText::new("planned").size(13.0).color(theme::text()));
                }
                for (l, key) in &langs {
                    // Named for its framework where it has one (FastAPI,
                    // Flask, Shiny, WebAssembly): Blank said "Python" four
                    // times, each with a different mark.
                    let name = crate::frameworks::mark(key).map_or(*l, |m| m.name);
                    // A chip never wraps inside itself: at the row's end the
                    // whole chip moves to the next row. A wrapping label here
                    // squeezed "Python" to a letter a line and widened the
                    // card (operator screenshot, 2026-11-01). A Frame does not
                    // make the row wrap by itself either - the row ran past
                    // the card and pushed the next card off the explorer
                    // (operator screenshot, 2026-11-07) - so it is measured
                    // and the row ended by hand.
                    let text_w = ui.fonts_mut(|f| f.layout_no_wrap(name.to_owned(), egui::FontId::proportional(13.0), egui::Color32::WHITE).size().x);
                    let chip_w = 16.0 + 16.0 + 4.0 + text_w + 2.0;
                    // Before wrap: in a wrapping row available_width() is the whole row.
                    if ui.available_size_before_wrap().x < chip_w && ui.cursor().min.x > ui.max_rect().left() + 1.0 {
                        ui.end_row();
                    }
                    egui::Frame::new().fill(lang_color(l).gamma_multiply(0.18)).corner_radius(10).inner_margin(egui::Margin::symmetric(8, 2)).show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.horizontal(|ui| {
                            crate::frameworks::icon(ui, key, 16.0);
                            ui.add(egui::Label::new(RichText::new(name).size(13.0).strong().color(lang_color(l))).extend());
                        });
                    });
                }
                if ready > 1 {
                    // Whole, like a chip: "15 / experiments" split across rows.
                    ui.add(egui::Label::new(RichText::new(format!("{ready} experiments")).size(13.0).color(theme::text())).extend());
                }
            });
        })
        .response
        .interact(egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The card's picture, drawn: the Lorenz attractor, a dependency graph, a
/// dashboard's bars and line, a request and its answer, an empty page.
fn category_picture(p: &egui::Painter, r: egui::Rect, i: usize) {
    p.rect_filled(r, 6.0, theme::ground());
    let m = r.shrink(10.0);
    let at = |fx: f32, fy: f32| egui::pos2(m.left() + fx * m.width(), m.top() + fy * m.height());
    match crate::starters::CATEGORIES[i].title {
        "Interactive Science" => {
            let pts = crate::starters::lorenz_sketch(1400);
            let line: Vec<egui::Pos2> = pts.iter().map(|(x, z)| at(0.5 + x / 50.0, 1.0 - (z - 2.0) / 50.0)).collect();
            let half = line.len() / 2;
            for (k, seg) in line.windows(2).enumerate() {
                let c = if k < half { theme::primary() } else { theme::amber() };
                p.line_segment([seg[0], seg[1]], egui::Stroke::new(1.0, c.gamma_multiply(0.85)));
            }
        }
        "Dependency Explorer" => {
            let nodes = [(0.5, 0.15), (0.25, 0.45), (0.75, 0.45), (0.12, 0.85), (0.38, 0.85), (0.62, 0.85), (0.88, 0.85)];
            let edges = [(0, 1), (0, 2), (1, 3), (1, 4), (2, 4), (2, 5), (2, 6)];
            for (a, b) in edges {
                p.line_segment([at(nodes[a].0, nodes[a].1), at(nodes[b].0, nodes[b].1)], egui::Stroke::new(1.2, theme::muted()));
            }
            for (k, (x, y)) in nodes.iter().enumerate() {
                let c = match k {
                    0 => theme::amber(),
                    1 | 2 => theme::primary(),
                    _ => theme::ok(),
                };
                p.circle_filled(at(*x, *y), if k == 0 { 7.0 } else { 5.0 }, c);
            }
        }
        "Dashboard" => {
            let bars = [0.35f32, 0.6, 0.45, 0.8, 0.55, 0.7, 0.9];
            let bw = 0.6 / bars.len() as f32;
            for (k, h) in bars.iter().enumerate() {
                let x0 = k as f32 * bw * 1.4;
                p.rect_filled(egui::Rect::from_min_max(at(x0, 1.0 - h), at(x0 + bw, 1.0)), 2.0, theme::primary().gamma_multiply(0.8));
            }
            let line: Vec<egui::Pos2> = (0..20)
                .map(|k| {
                    let f = k as f32 / 19.0;
                    at(0.68 + f * 0.32, 0.6 - 0.4 * (f * 5.0).sin().abs() * f)
                })
                .collect();
            p.add(egui::Shape::line(line, egui::Stroke::new(2.0, theme::amber())));
        }
        "Web Service & API" => {
            let f = egui::FontId::monospace(13.0);
            p.text(at(0.0, 0.2), egui::Align2::LEFT_CENTER, "GET /lorenz?rho=28", f.clone(), theme::primary());
            p.text(at(0.0, 0.55), egui::Align2::LEFT_CENTER, "200 { \"x\": 13.79,", f.clone(), theme::ok());
            p.text(at(0.0, 0.8), egui::Align2::LEFT_CENTER, "      \"z\": 34.90 }", f, theme::ok());
        }
        "Epidemics & Populations" => {
            // S falling, I rising and falling, R rising: an SIR run.
            let (mut sv, mut iv, mut rv) = (0.99f32, 0.01f32, 0.0f32);
            let (mut ps, mut pi, mut pr) = (Vec::new(), Vec::new(), Vec::new());
            for k in 0..120 {
                let f = k as f32 / 119.0;
                ps.push(at(f, 1.0 - sv));
                pi.push(at(f, 1.0 - iv * 2.2));
                pr.push(at(f, 1.0 - rv));
                let (inf, rec) = (0.35 * sv * iv, 0.1 * iv);
                sv -= inf;
                iv += inf - rec;
                rv += rec;
            }
            p.add(egui::Shape::line(ps, egui::Stroke::new(2.0, theme::primary())));
            p.add(egui::Shape::line(pi, egui::Stroke::new(2.0, theme::amber())));
            p.add(egui::Shape::line(pr, egui::Stroke::new(2.0, theme::ok())));
        }
        "Clinical Trials & Statistics" => {
            // A power curve against sample size, and the 80% line.
            let line: Vec<egui::Pos2> = (0..60).map(|k| {
                let f = k as f32 / 59.0;
                at(f, 1.0 - 0.95 * (1.0 - (-5.0 * f).exp()))
            }).collect();
            p.line_segment([at(0.0, 0.24), at(1.0, 0.24)], egui::Stroke::new(1.0, theme::muted()));
            p.add(egui::Shape::line(line, egui::Stroke::new(2.0, theme::primary())));
        }
        "Genomics" => {
            // A volcano: most genes at the base, a few up and down.
            let mut seed = 7u32;
            let mut rnd = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / 16_777_216.0
            };
            for _ in 0..220 {
                let fc = (rnd() - 0.5) * 2.0;
                let y = (fc.abs() * 0.9 + rnd() * 0.25).min(1.0);
                let c = if y > 0.6 { if fc > 0.0 { theme::amber() } else { theme::primary() } } else { theme::muted() };
                p.circle_filled(at(0.5 + fc * 0.48, 1.0 - y), 1.8, c);
            }
        }
        "Machine Learning" => {
            // Two classes either side of a curved boundary.
            let mut seed = 11u32;
            let mut rnd = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / 16_777_216.0
            };
            let boundary = |x: f32| 0.5 + 0.25 * (x * 6.0).sin();
            let line: Vec<egui::Pos2> = (0..40).map(|k| { let x = k as f32 / 39.0; at(x, boundary(x)) }).collect();
            for _ in 0..120 {
                let (x, y) = (rnd(), rnd());
                let c = if y < boundary(x) { theme::primary() } else { theme::amber() };
                p.circle_filled(at(x, y), 2.0, c.gamma_multiply(0.85));
            }
            p.add(egui::Shape::line(line, egui::Stroke::new(1.6, theme::text_strong())));
        }
        "Fractals & Complex Systems" => {
            // The Mandelbrot set, coarsely: inside dark, the edge bright.
            let (nx, ny) = (64, 26);
            let (cw, ch) = (m.width() / nx as f32, m.height() / ny as f32);
            for j in 0..ny {
                for i in 0..nx {
                    let (cx, cy) = (-2.2 + 3.0 * i as f32 / nx as f32, -1.15 + 2.3 * j as f32 / ny as f32);
                    let (mut x, mut y, mut n) = (0.0f32, 0.0f32, 0);
                    while n < 40 && x * x + y * y < 4.0 {
                        let t = x * x - y * y + cx;
                        y = 2.0 * x * y + cy;
                        x = t;
                        n += 1;
                    }
                    if n < 40 && n > 2 {
                        let c = if n > 8 { theme::amber() } else { theme::primary() };
                        let r = egui::Rect::from_min_size(egui::pos2(m.left() + i as f32 * cw, m.top() + j as f32 * ch), egui::vec2(cw, ch));
                        p.rect_filled(r, 0.0, c.gamma_multiply((n as f32 / 20.0).min(1.0)));
                    }
                }
            }
        }
        "Finance & Monte Carlo" => {
            // A handful of random price paths from one start.
            let mut seed = 3u32;
            let mut rnd = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / 16_777_216.0 - 0.5
            };
            for k in 0..7 {
                let mut v = 0.5f32;
                let line: Vec<egui::Pos2> = (0..50).map(|t| { v = (v + rnd() * 0.08).clamp(0.02, 0.98); at(t as f32 / 49.0, v) }).collect();
                p.add(egui::Shape::line(line, egui::Stroke::new(1.3, if k == 0 { theme::amber() } else { theme::primary().gamma_multiply(0.7) })));
            }
        }
        "Signals & Fourier" => {
            // A wave on top, its spectrum's bars underneath.
            let line: Vec<egui::Pos2> = (0..120).map(|k| {
                let t = k as f32 / 119.0;
                at(t, 0.28 + 0.14 * (t * 18.0).sin() + 0.07 * (t * 47.0).sin())
            }).collect();
            p.add(egui::Shape::line(line, egui::Stroke::new(1.6, theme::primary())));
            for (k, h) in [0.05f32, 0.1, 0.42, 0.08, 0.06, 0.04, 0.22, 0.05, 0.03, 0.02].iter().enumerate() {
                let x = 0.04 + k as f32 * 0.095;
                p.rect_filled(egui::Rect::from_min_max(at(x, 1.0 - h), at(x + 0.06, 1.0)), 1.0, theme::amber());
            }
        }
        "Text Analysis" => {
            let f = egui::FontId::proportional(12.0);
            for (k, (w, n)) in [("species", 0.9f32), ("natural", 0.72), ("selection", 0.64), ("forms", 0.45), ("variation", 0.38)].iter().enumerate() {
                let y = 0.1 + k as f32 * 0.19;
                p.text(at(0.0, y), egui::Align2::LEFT_CENTER, *w, f.clone(), theme::text());
                p.rect_filled(egui::Rect::from_min_max(at(0.3, y - 0.06), at(0.3 + 0.7 * n, y + 0.06)), 2.0, theme::primary().gamma_multiply(0.8));
            }
        }
        "Data Exploration" => {
            // A correlated cloud, its trend line and the one outlier it flags.
            let mut seed = 23u32;
            let mut rnd = || {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (seed >> 8) as f32 / 16_777_216.0
            };
            for _ in 0..90 {
                let x = rnd();
                let y = 0.85 - 0.65 * x + (rnd() - 0.5) * 0.25;
                p.circle_filled(at(x, y), 2.0, theme::primary().gamma_multiply(0.8));
            }
            p.line_segment([at(0.0, 0.85), at(1.0, 0.2)], egui::Stroke::new(1.4, theme::text_strong()));
            p.circle_stroke(at(0.82, 0.78), 5.0, egui::Stroke::new(1.6, theme::amber()));
        }
        "Games & Play" => {
            // A hopping arc over waves, and the stars it collects.
            let arc: Vec<egui::Pos2> = (0..=40).map(|k| { let t = k as f32 / 40.0; at(t, 0.62 - 0.5 * (t * std::f32::consts::PI).sin()) }).collect();
            p.add(egui::Shape::line(arc, egui::Stroke::new(1.6, theme::primary())));
            for (x, y) in [(0.25, 0.30), (0.5, 0.14), (0.75, 0.30)] {
                p.circle_filled(at(x, y), 4.0, theme::amber());
            }
            for row in 0..3 {
                let wave: Vec<egui::Pos2> = (0..=48).map(|k| { let t = k as f32 / 48.0; at(t, 0.74 + row as f32 * 0.09 + 0.03 * (t * 14.0 + row as f32).sin()) }).collect();
                p.add(egui::Shape::line(wave, egui::Stroke::new(1.2, theme::net().gamma_multiply(0.8 - row as f32 * 0.2))));
            }
        }
        "Design & Color" => {
            // A palette's swatches over the gradient between its two ends.
            let (a, b) = (theme::primary(), theme::amber());
            for k in 0..24 {
                let t = k as f32 / 23.0;
                let c = egui::Color32::from_rgb(
                    (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
                    (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
                    (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
                );
                p.rect_filled(egui::Rect::from_min_max(at(t * 0.96, 0.62), at(t * 0.96 + 0.05, 0.92)), 0.0, c);
            }
            for (k, c) in [theme::primary(), theme::alt(), theme::ok(), theme::amber(), theme::net()].iter().enumerate() {
                p.rect_filled(egui::Rect::from_min_max(at(0.02 + k as f32 * 0.2, 0.08), at(0.17 + k as f32 * 0.2, 0.48)), 4.0, *c);
            }
        }
        "Productivity & Collaboration" => {
            // Three columns of cards, one on its way across.
            for col in 0..3 {
                let x = 0.04 + col as f32 * 0.33;
                p.rect_stroke(egui::Rect::from_min_max(at(x, 0.05), at(x + 0.28, 0.95)), 4.0, egui::Stroke::new(1.0, theme::muted()), egui::StrokeKind::Inside);
                for row in 0..(3 - col) {
                    let y = 0.14 + row as f32 * 0.22;
                    p.rect_filled(egui::Rect::from_min_max(at(x + 0.03, y), at(x + 0.25, y + 0.16)), 3.0, theme::primary().gamma_multiply(0.75));
                }
            }
            p.rect_filled(egui::Rect::from_min_max(at(0.5, 0.58), at(0.72, 0.74)), 3.0, theme::amber());
        }
        "Statistical Exploration" => {
            // Two humps of a bimodal histogram under their fitted density.
            let dens = |x: f32| 0.35 * (-((x - 0.28) / 0.09).powi(2) / 2.0).exp() + 0.65 * (-((x - 0.7) / 0.11).powi(2) / 2.0).exp();
            for k in 0..20 {
                let x = (k as f32 + 0.5) / 20.0;
                let h = dens(x) * 0.8;
                p.rect_filled(egui::Rect::from_min_max(at(x - 0.022, 0.95 - h), at(x + 0.022, 0.95)), 1.0, theme::primary().gamma_multiply(0.7));
            }
            let line: Vec<egui::Pos2> = (0..=60).map(|k| { let x = k as f32 / 60.0; at(x, 0.95 - dens(x) * 0.8) }).collect();
            p.add(egui::Shape::line(line, egui::Stroke::new(1.6, theme::amber())));
        }
        "Optimization" => {
            // Contours of a valley and a path stepping down into it.
            for k in 1..=5 {
                let s = k as f32 * 0.08;
                let ring: Vec<egui::Pos2> = (0..=48).map(|j| { let t = j as f32 / 48.0 * std::f32::consts::TAU; at(0.6 + 0.95 * s * t.cos(), 0.55 + 0.95 * s * t.sin()) }).collect();
                p.add(egui::Shape::line(ring, egui::Stroke::new(1.0, theme::muted().gamma_multiply(1.0 - k as f32 * 0.12))));
            }
            let path = [(0.05, 0.1), (0.2, 0.3), (0.32, 0.42), (0.44, 0.5), (0.54, 0.54), (0.6, 0.55)];
            let line: Vec<egui::Pos2> = path.iter().map(|(x, y)| at(*x, *y)).collect();
            p.add(egui::Shape::line(line.clone(), egui::Stroke::new(1.6, theme::amber())));
            for q in &line {
                p.circle_filled(*q, 2.4, theme::amber());
            }
            p.circle_filled(at(0.6, 0.55), 4.0, theme::ok());
        }
        _ => {
            let page = egui::Rect::from_min_max(at(0.3, 0.05), at(0.7, 0.95));
            p.rect_stroke(page, 4.0, egui::Stroke::new(1.2, theme::muted()), egui::StrokeKind::Inside);
            p.text(page.center(), egui::Align2::CENTER_CENTER, "+", egui::FontId::proportional(28.0), theme::muted());
        }
    }
}
