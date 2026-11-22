//! Signing the desktop app in to ndexr (2026-10-13, operator: "mac users will
//! need to log in to access cvmfs tools, so the desktop version needs a
//! redirect for auth ... mac users will connect to their cell for cvmfs
//! module access").
//!
//! Sign in is the browser's job, the way `gh auth login` and gcloud do it:
//!
//!   1. listen on 127.0.0.1:<random port> for one request
//!   2. open https://api.ndexr.io/?desktop=<port>&state=<random> in the browser
//!      (api.ndexr.io/branch_desktop.r): ndexr's own sign-in if needed, then
//!      an explicit Allow, which mints an API key for that person
//!   3. the browser comes back to http://127.0.0.1:<port>/callback?state&key
//!      — accepted only with our state — and the key is kept where the ndexr
//!      CLI looks for it, ~/.config/ndexr/api_key (0600), so signing the app
//!      in signs the CLI in too
//!
//! The key is the account: /v1/whoami names it and its cell, /v1/token
//! exchanges it for the short-lived ndx_token every ndexr site accepts. Sign
//! out forgets the key here; it stays listed under api.ndexr.io › Keys until
//! revoked there.
//!
//! THE CELL. A Mac has no stack (the CVMFS repo cannot be mounted there), so
//! once signed in the app runs R, Python and the shell in the person's cell:
//! `cove serve` on the cell's loopback :8002 (scripts/cells/cell-entry.sh),
//! reached at https://code.<pd>/proxy/8002/ with `Cookie: ndx_token=…` -
//! ndexr-auth's /__code_session lets only the cell's own account through.
//! Which one a launch uses (`choose`): the person's pick in the account menu
//! (~/.config/ndexr/cove-run-on), else THE CELL whenever signed in - on a Mac,
//! anywhere but Linux with a stack of its own (choose; operator, 2026-10-13: "giving
//! people a way through an ide to build in their cell on the ndexr server and
//! use the environments in the ide but with a remote connection so it can
//! work the same everywhere"). This computer only by explicit pick, or when
//! the cell cannot be reached.
//! A sleeping cell is woken at launch (`wake`) and waited for, up to
//! CELL_WAKE; only then does the app fall back to this computer.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long the browser has to come back.
const WAIT: Duration = Duration::from_secs(300);

/// What the toolbar shows.
#[derive(Clone, Debug, Default)]
pub struct Account {
    /// Signed in as.
    pub email: Option<String>,
    /// The person's cell, https://code.<pd> (from /v1/whoami).
    pub cell: Option<String>,
    /// What this window runs on: Some(cell base) when connected to the cell.
    pub on_cell: Option<String>,
    /// A key is here but api.ndexr.io could not be asked (offline).
    pub unverified: bool,
    /// Waiting for the browser; the page it was sent to, to open by hand.
    pub waiting: Option<String>,
    /// Why this launch is not on the cell (main.rs), kept for the toolbar
    /// rather than only a toast that fades.
    pub cell_error: Option<String>,
    pub note: Option<String>,
}

pub type Shared = Arc<Mutex<Account>>;

pub fn api_base() -> String {
    std::env::var("NDEXR_API_URL").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "https://api.ndexr.io".into()).trim_end_matches('/').to_owned()
}

/// ~/.config/ndexr/api_key — the ndexr CLI's own place (crate telemetry.rs
/// `api_key()`: $XDG_CONFIG_HOME, else ~/.config).
fn key_file() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
    Some(base.join("ndexr/api_key"))
}

fn valid(k: &str) -> bool {
    k.starts_with("ndxr_") && k.len() < 200 && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// The key: $NDEXR_API_KEY, else the file.
pub fn find_key() -> Option<String> {
    if let Ok(k) = std::env::var("NDEXR_API_KEY") {
        let k = k.trim().to_owned();
        if valid(&k) {
            return Some(k);
        }
    }
    let k = std::fs::read_to_string(key_file()?).ok()?.trim().to_owned();
    valid(&k).then_some(k)
}

fn save_key(k: &str) -> Result<(), String> {
    let f = key_file().ok_or("no HOME")?;
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&f, format!("{k}\n")).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
    }
    Ok(())
}

enum Who {
    Email(String, Option<String>),
    Refused,
    Offline(String),
}

/// Who the key belongs to, asked of api.ndexr.io.
fn whoami(key: &str) -> Who {
    let mut req = ehttp::Request::get(format!("{}/v1/whoami", api_base()));
    req.headers.insert("Authorization", format!("Bearer {key}"));
    match ehttp::fetch_blocking(&req) {
        Ok(r) if r.status == 200 => {
            let v = r.text().and_then(|t| serde_json::from_str::<serde_json::Value>(t).ok()).unwrap_or_default();
            match v["email"].as_str() {
                Some(e) => Who::Email(e.to_owned(), v["cell"].as_str().filter(|c| c.starts_with("https://")).map(str::to_owned)),
                None => Who::Offline("api.ndexr.io answered without an email".into()),
            }
        }
        Ok(r) if r.status == 401 || r.status == 403 => Who::Refused,
        Ok(r) => Who::Offline(format!("api.ndexr.io answered {}", r.status)),
        Err(e) => Who::Offline(e),
    }
}

/// Help › Send a request from the desktop app: api.ndexr.io files it as the
/// key's owner (api.r `.action_request`, the site's own file_complaint). The
/// machine's half (host.rs send_request) runs the platform's R, which only a
/// Cove server has, so from a laptop, a Mac or a cell it always failed
/// (2026-10-15, operator: "send fails with error").
pub fn send_request(domain: &str, what: &str, trying: &str, blocked: bool) -> Result<i64, String> {
    use crate::backend::url_encode;
    // Not signed in: filed anonymously on cove.ndexr.io (server.rs
    // anon_request_response), what they were trying folded into it.
    let Some(key) = find_key() else {
        let what = if trying.trim().is_empty() { what.to_owned() } else { format!("{what}\n\nTrying to: {trying}") };
        return crate::backend::anon_request(&what, "", &format!("Cove desktop, not signed in, about {domain}"));
    };
    let url = format!(
        "{}/v1/requests?domain={}&what={}&trying={}&urgency={}",
        api_base(),
        url_encode(domain),
        url_encode(&what.chars().take(4000).collect::<String>()),
        url_encode(&trying.chars().take(4000).collect::<String>()),
        if blocked { "blocked" } else { "normal" }
    );
    let mut req = ehttp::Request::get(url);
    req.headers.insert("Authorization", format!("Bearer {key}"));
    let r = ehttp::fetch_blocking(&req)?;
    let v: serde_json::Value = r.text().and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
    match (r.status, v["id"].as_i64()) {
        (200, Some(id)) => Ok(id),
        (401 | 403, _) => Err("your ndexr sign-in was revoked or expired - sign in again".into()),
        (429, _) => Err("too many requests just now - wait a minute and send again".into()),
        (s, _) => Err(v["message"].as_str().map(str::to_owned).unwrap_or_else(|| format!("api.ndexr.io answered {s}"))),
    }
}

/// The person's Claude, from the desktop app: api.ndexr.io's /v1/claude
/// with their key (api.r `.action_claude`), the same store, check and words
/// as console.ndexr.io's Claude tab and cove.ndexr.io's setup. `op` is
/// "status", "connect" (with `token`, sent in a header - never the URL) or
/// "forget". A cell or this computer has no platform to ask, so the app asks
/// ndexr itself wherever its kernels run.
pub fn claude(op: &str, token: &str) -> Result<crate::api::ClaudeView, String> {
    let key = find_key().ok_or("sign in to ndexr first - your Claude is kept with your ndexr account")?;
    let url = format!("{}/v1/claude{}", api_base(), if op == "forget" { "?op=forget" } else { "" });
    let mut req = ehttp::Request::get(url);
    req.headers.insert("Authorization", format!("Bearer {key}"));
    if op == "connect" {
        req.headers.insert("X-Ndexr-Claude", token.trim());
    }
    let r = ehttp::fetch_blocking(&req)?;
    let v: serde_json::Value = r.text().and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
    match r.status {
        // A refusal comes as 200 with its words (api.r: nginx would replace a 4xx body).
        200 if v["error"].is_string() => Err(v["message"].as_str().unwrap_or("not connected").to_owned()),
        200 => serde_json::from_value(v).map_err(|e| format!("api.ndexr.io answered oddly: {e}")),
        401 | 403 => Err("your ndexr sign-in was revoked or expired - sign in again".into()),
        404 => Err("ndexr cannot take it from the app yet - connect it at console.ndexr.io/#claude".into()),
        429 => Err("too many tries just now - wait a minute and try again".into()),
        s => Err(v["message"].as_str().map(str::to_owned).unwrap_or_else(|| format!("api.ndexr.io answered {s}"))),
    }
}

/// The Account tab from the desktop app: api.ndexr.io/account.r for the key's
/// owner, by the key (/v1/account). `revoke`, when not empty, revokes that key
/// of theirs first (/v1/revoke) - never the one this app is signed in with.
pub fn overview(revoke: &str) -> Result<crate::api::AccountView, String> {
    let key = find_key().ok_or("sign in to ndexr first (account menu) - this shows your ndexr account")?;
    if !revoke.trim().bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("not a key id".into());
    }
    let url = if revoke.trim().is_empty() {
        format!("{}/v1/account", api_base())
    } else {
        format!("{}/v1/revoke?key_id={}", api_base(), revoke.trim())
    };
    let mut req = ehttp::Request::get(url);
    req.headers.insert("Authorization", format!("Bearer {key}"));
    let r = ehttp::fetch_blocking(&req)?;
    let v: serde_json::Value = r.text().and_then(|t| serde_json::from_str(t).ok()).unwrap_or_default();
    match r.status {
        200 if v["error"].is_string() => Err(v["message"].as_str().unwrap_or("not done").to_owned()),
        200 => serde_json::from_value(v).map_err(|e| format!("api.ndexr.io answered oddly: {e}")),
        401 | 403 => Err("your ndexr sign-in was revoked or expired - sign in again".into()),
        404 => Err("api.ndexr.io does not serve accounts to the app yet".into()),
        429 => Err("too many tries just now - wait a minute and try again".into()),
        s => Err(v["message"].as_str().map(str::to_owned).unwrap_or_else(|| format!("api.ndexr.io answered {s}"))),
    }
}

/// A Claude credential already on this computer, which setup offers to send
/// to the person's ndexr account - only when they tick that they want it sent.
#[derive(Clone, Debug)]
pub struct FoundClaude {
    /// "an Anthropic API key" / "a Claude Code token".
    pub what: &'static str,
    /// Where it was found, e.g. "$ANTHROPIC_API_KEY in your login shell".
    pub from: String,
    pub value: String,
}

impl FoundClaude {
    pub fn masked(&self) -> String {
        let v = &self.value;
        format!("{}…{}", &v[..v.len().min(12)], &v[v.len().saturating_sub(4)..])
    }
}

/// Look for a Claude key this computer already has: in Cove's own
/// environment, then the login shell's (a Mac app is started without the
/// shell's variables). Read only; nothing leaves the machine from here.
pub fn find_local_claude() -> Vec<FoundClaude> {
    let mut out: Vec<FoundClaude> = Vec::new();
    let mut add = |name: &str, value: String, from: String| {
        let value = value.trim().to_owned();
        let what = match (name, value.as_str()) {
            ("ANTHROPIC_API_KEY", v) if v.starts_with("sk-ant-api") => "an Anthropic API key",
            ("CLAUDE_CODE_OAUTH_TOKEN", v) if v.starts_with("sk-ant-oat") => "a Claude subscription token",
            _ => return,
        };
        if value.len() < 200 && !out.iter().any(|f| f.value == value) {
            out.push(FoundClaude { what, from, value });
        }
    };
    for name in ["CLAUDE_CODE_OAUTH_TOKEN", "ANTHROPIC_API_KEY"] {
        if let Ok(v) = std::env::var(name) {
            add(name, v, format!("${name} in Cove's environment"));
        }
    }
    #[cfg(unix)]
    if let Some(text) = crate::runner::capture_within("printf '%s\\n%s\\n' \"${CLAUDE_CODE_OAUTH_TOKEN:-}\" \"${ANTHROPIC_API_KEY:-}\"", Duration::from_secs(5)) {
        let mut lines = text.lines();
        add("CLAUDE_CODE_OAUTH_TOKEN", lines.next().unwrap_or("").to_owned(), "$CLAUDE_CODE_OAUTH_TOKEN in your login shell".into());
        add("ANTHROPIC_API_KEY", lines.next().unwrap_or("").to_owned(), "$ANTHROPIC_API_KEY in your login shell".into());
    }
    out
}

/// At start: is a key here, and whose is it.
pub fn check(acct: Shared, ctx: eframe::egui::Context) {
    // Airgap mode: api.ndexr.io is not asked who anyone is.
    if crate::host::saved_settings().airgap {
        return;
    }
    std::thread::spawn(move || {
        let Some(key) = find_key() else { return };
        let mut a = acct.lock().unwrap();
        match whoami(&key) {
            Who::Email(e, cell) => {
                a.email = Some(e);
                a.cell = cell;
                a.unverified = false;
            }
            Who::Refused => a.note = Some("your ndexr sign-in was revoked or expired - sign in again".into()),
            Who::Offline(_) => {
                a.email = Some("(offline)".into());
                a.unverified = true;
            }
        }
        drop(a);
        ctx.request_repaint();
    });
}

pub fn sign_out(acct: &Shared) {
    if let Some(f) = key_file() {
        let _ = std::fs::remove_file(f);
    }
    let on_cell = acct.lock().unwrap().on_cell.clone();
    *acct.lock().unwrap() = Account { note: Some("signed out on this computer - the key stays listed at api.ndexr.io › Keys".into()), on_cell, ..Default::default() };
}

// ── the cell ────────────────────────────────────────────────────────────────

/// The session cookie a cell request carries, kept fresh by `connect_cell`.
pub type Session = Arc<Mutex<String>>;

/// Where a launch runs: the person's pick, else the cell - remote first, the
/// same on every machine.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RunOn {
    Cell,
    Local,
    /// A Cove server the person was given the address and token of
    /// (`saved_server`): a managed server ndexr runs for them, or one their
    /// company runs (operator, 2026-11-22). No ndexr account is needed.
    Server,
}

fn run_on_file() -> Option<PathBuf> {
    Some(key_file()?.with_file_name("cove-run-on"))
}

pub fn preferred() -> Option<RunOn> {
    match std::fs::read_to_string(run_on_file()?).ok()?.trim() {
        "cell" => Some(RunOn::Cell),
        "local" => Some(RunOn::Local),
        "server" => saved_server().map(|_| RunOn::Server),
        _ => None,
    }
}

pub fn prefer(r: RunOn) {
    if let Some(f) = run_on_file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let word = match r {
            RunOn::Cell => "cell\n",
            RunOn::Local => "local\n",
            RunOn::Server => "server\n",
        };
        let _ = std::fs::write(f, word);
    }
}

// ── a Cove server: managed by ndexr, or the person's company's ─────────────

/// ~/.config/ndexr/cove-server: the server's address on the first line, its
/// token (the server's own ~/.config/ndexr/cove-token) on the second. 0600:
/// the token is the way in.
fn server_file() -> Option<PathBuf> {
    Some(key_file()?.with_file_name("cove-server"))
}

/// The server saved on this computer: (address, token).
pub fn saved_server() -> Option<(String, String)> {
    let raw = std::fs::read_to_string(server_file()?).ok()?;
    let mut lines = raw.lines().map(str::trim);
    let url = lines.next().filter(|u| u.starts_with("https://") || u.starts_with("http://"))?.trim_end_matches('/').to_owned();
    let token = lines.next().filter(|t| !t.is_empty())?.to_owned();
    Some((url, token))
}

/// An address as typed, made a base URL: https:// unless it says otherwise,
/// no trailing slash.
pub fn server_base(typed: &str) -> Result<String, String> {
    let t = typed.trim().trim_end_matches('/');
    if t.is_empty() || t.contains(char::is_whitespace) {
        return Err("type the server's address, e.g. cove.example.com".into());
    }
    Ok(if t.starts_with("https://") || t.starts_with("http://") { t.to_owned() } else { format!("https://{t}") })
}

/// Does a Cove server answer at `base` and take `token`? The same probe the
/// cell gets, with the token as `Authorization: Bearer` (server.rs `authed`).
pub fn probe_server(base: &str, token: &str) -> Result<(), String> {
    let mut req = ehttp::Request::post(format!("{base}/api/q"), br#"{"q":"probe"}"#.to_vec());
    req.headers.insert("Content-Type", "application/json");
    req.headers.insert("Authorization", format!("Bearer {}", token.trim()));
    let r = ehttp::fetch_blocking(&req).map_err(|e| format!("{base} could not be reached: {e}"))?;
    if r.status == 401 || r.status == 403 {
        return Err("the server refused that token - copy it again from the server (~/.config/ndexr/cove-token there) or from billing.ndexr.io › Servers".into());
    }
    if !r.ok || !r.bytes.trim_ascii_start().starts_with(b"{") {
        return Err(format!("{base} answered {}, not as a Cove server", r.status));
    }
    Ok(())
}

/// Keep the server for this computer, and run on it from the next launch.
pub fn save_server(base: &str, token: &str) -> Result<(), String> {
    let f = server_file().ok_or("no home folder to keep the server in")?;
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&f, format!("{base}\n{}\n", token.trim())).map_err(|e| format!("could not save the server: {e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600));
    }
    prefer(RunOn::Server);
    Ok(())
}

/// Forget the saved server, and run on this computer from the next launch.
pub fn forget_server() {
    if let Some(f) = server_file() {
        let _ = std::fs::remove_file(f);
    }
    prefer(RunOn::Local);
}

fn stack_mode_file() -> Option<PathBuf> {
    Some(key_file()?.with_file_name("cove-stack-mode"))
}

/// The stack mode chosen on this computer (api.rs StackMode), if one was.
pub fn stack_mode() -> Option<crate::api::StackMode> {
    crate::api::StackMode::parse(&std::fs::read_to_string(stack_mode_file()?).ok()?)
}

/// Choose the stack mode, and where Cove runs with it: remote runs on the
/// cell, installed and CernVM-FS here.
pub fn set_stack_mode(m: crate::api::StackMode) {
    if let Some(f) = stack_mode_file() {
        let _ = std::fs::create_dir_all(f.parent().unwrap());
        let _ = std::fs::write(f, format!("{}\n", m.key()));
    }
    prefer(if m == crate::api::StackMode::Remote { RunOn::Cell } else { RunOn::Local });
}

/// The person's pick, else: Linux with the stack on this machine (mounted, or
/// built here through Software › Install) runs here; everything else - a Mac,
/// Windows, Linux without a stack - runs on the cell (operator, 2026-10-15:
/// "linux user should default to their local only if they have chosen to
/// install the stack or have it locally"). Picking "this computer" in the
/// account menu is the choice to run here, and it is saved.
pub fn choose() -> RunOn {
    preferred().unwrap_or_else(|| if cfg!(target_os = "linux") && crate::host::arch_supported() && crate::host::use_sh().is_some() { RunOn::Local } else { RunOn::Cell })
}

/// A session token for the key: (ndx_token, seconds it lives).
fn session_token(key: &str) -> Result<(String, u64), String> {
    let mut req = ehttp::Request::get(format!("{}/v1/token?ttl=3600", api_base()));
    req.headers.insert("Authorization", format!("Bearer {key}"));
    let r = ehttp::fetch_blocking(&req)?;
    if r.status == 401 || r.status == 403 {
        return Err("your ndexr sign-in was revoked or expired - sign in again".into());
    }
    if r.status != 200 {
        return Err(format!("api.ndexr.io answered {}", r.status));
    }
    let v: serde_json::Value = r.text().and_then(|t| serde_json::from_str(t).ok()).ok_or("api.ndexr.io sent no token")?;
    let token = v["token"].as_str().filter(|t| !t.is_empty()).ok_or("api.ndexr.io sent no token")?.to_owned();
    Ok((token, v["expires_in"].as_u64().unwrap_or(900)))
}

/// The platform's Cove (`NDEXR_COVE_URL`; https://cove.ndexr.io).
pub fn cove_base() -> String {
    std::env::var("NDEXR_COVE_URL").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| "https://cove.ndexr.io".into()).trim_end_matches('/').to_owned()
}

/// This person's session on the platform's Cove, as a base URL: a signed-in
/// GET / there redirects to /s/<id>/ (multiuser.rs), the session a browser
/// gets. What a cell cannot answer (Messages, agent runs, the build
/// database - it has no platform checkout) is asked there instead; both work
/// on the same files, since the cell mounts the account's domains folder.
pub fn platform_session(token: &str) -> Result<String, String> {
    let mut req = ehttp::Request::get(format!("{}/", cove_base()));
    req.headers.insert("Cookie", format!("ndx_token={token}"));
    let r = ehttp::fetch_blocking(&req).map_err(|e| format!("ndexr's Cove could not be reached: {e}"))?;
    let id = r.url.split_once("/s/").map(|(_, rest)| rest.split(['/', '?']).next().unwrap_or("").to_owned()).unwrap_or_default();
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(format!("ndexr's Cove opened no session for you (it answered {} at {})", r.status, r.url));
    }
    Ok(format!("{}/s/{id}", cove_base()))
}

/// A request to `token`'s own session on the platform's Cove: `make` builds it
/// from the session's base URL, and the token goes as the ndx_token cookie.
/// The address is found once and kept; when the platform's server restarted
/// (every release does) and the session is gone, it is found again.
pub fn platform_request(token: &str, make: impl Fn(&str) -> ehttp::Request) -> Result<ehttp::Response, String> {
    static BASE: Mutex<Option<(String, String)>> = Mutex::new(None);
    let ask = |base: &str| {
        let mut req = make(base);
        req.headers.insert("Cookie", format!("ndx_token={token}"));
        ehttp::fetch_blocking(&req)
    };
    // Kept per person: a token's email is its first part (auth_svc.rs).
    let who = token.split('.').next().unwrap_or("").to_owned();
    let known = BASE.lock().unwrap().clone().filter(|(w, _)| *w == who).map(|(_, b)| b);
    if let Some(b) = known {
        match ask(&b) {
            Ok(r) if r.status == 404 => {}
            r => return r,
        }
    }
    let b = platform_session(token)?;
    *BASE.lock().unwrap() = Some((who, b.clone()));
    ask(&b)
}

/// Connect to the person's cell, at launch: (the server's base URL, the
/// session the backend sends). Asks who the key is, takes a session token,
/// checks the cell's ide server answers, and keeps the token fresh on a
/// thread for as long as the app runs.
pub fn connect_cell() -> Result<(String, Session), String> {
    let key = find_key().ok_or("not signed in")?;
    let cell = match whoami(&key) {
        Who::Email(_, Some(cell)) => cell,
        Who::Email(e, None) => return Err(format!("{e} has no cell yet")),
        Who::Refused => return Err("your ndexr sign-in was revoked or expired - sign in again".into()),
        Who::Offline(e) => return Err(format!("ndexr could not be reached: {e}")),
    };
    let base = format!("{}/proxy/8002", cell.trim_end_matches('/'));
    let (token, ttl) = session_token(&key)?;
    let probe = || {
        let mut probe = ehttp::Request::post(format!("{base}/api/q"), br#"{"q":"probe"}"#.to_vec());
        probe.headers.insert("Content-Type", "application/json");
        probe.headers.insert("Cookie", format!("ndx_token={token}"));
        ehttp::fetch_blocking(&probe)
    };
    // A sleeping cell (free cells stop when idle) answers 502. The IDE is
    // remote first (operator, 2026-10-13: "the ide can be a remote ide going
    // forward instead of a local one on mac"), so it wakes the cell rather than
    // giving up: a visit to the person's own site is the platform's wake
    // (nginx cell.conf @cell_wake -> ndexr-cell-agent /wake), then the probe
    // repeats until the cell's ide server answers.
    let mut woke: Option<Instant> = None;
    loop {
        match probe() {
            Ok(r) if r.ok && r.bytes.trim_ascii_start().starts_with(b"{") => break,
            Ok(r) if r.status == 502 || r.status == 503 || r.status == 504 => {
                let started = *woke.get_or_insert_with(|| {
                    wake(&cell);
                    Instant::now()
                });
                if started.elapsed() > CELL_WAKE {
                    return Err(format!("your cell did not start within {}s - open {cell} to check on it, then relaunch", CELL_WAKE.as_secs()));
                }
                std::thread::sleep(Duration::from_secs(2));
            }
            Ok(r) if r.ok => return Err("your cell answered with a sign-in page - sign in again".into()),
            // The gate (ndexr-auth /__code_session) found no editor for this
            // account on this domain - e.g. its personal domain missing from
            // domain_registry (fdrennan.ndexr.io, 2026-10-13).
            Ok(r) if r.status == 403 => return Err(format!("{cell} refused this account (403) - its personal domain or editor is not registered on ndexr yet; ask the operator")),
            // Awake, but nothing on :8002: code-server's proxy says so with a
            // 500. Not a sleeping cell, so no wake and no browser trip to the
            // cell's page (2026-11-02: every cell lacked /opt/ndexr/cove, and
            // "connect" sent the person to code-server instead).
            Ok(r) if r.text().is_some_and(|t| t.contains("ECONNREFUSED")) => return Err("your cell is running, but its Cove server is not (nothing on :8002) - restarting the cell starts it; ask the operator if it persists".into()),
            Ok(r) => return Err(format!("your cell's ide server answered {}", r.status)),
            Err(e) => return Err(format!("your cell could not be reached: {e}")),
        }
    }
    let session: Session = Arc::new(Mutex::new(token));
    keep_fresh(session.clone(), ttl);
    Ok((base, session))
}

/// Renew `session` five minutes before it expires, for as long as the app
/// runs; on a failure, retry in a minute.
fn keep_fresh(session: Session, ttl: u64) {
    std::thread::spawn(move || {
        let mut wait = ttl.saturating_sub(300).max(60);
        loop {
            std::thread::sleep(Duration::from_secs(wait));
            wait = match find_key().map(|k| session_token(&k)) {
                Some(Ok((t, ttl))) => {
                    *session.lock().unwrap() = t;
                    ttl.saturating_sub(300).max(60)
                }
                _ => 60,
            };
        }
    });
}

/// The desktop on its own computer, signed in: a session from its key, for
/// what only the platform answers (backend.rs needs_platform) - Messages,
/// the account, the build views. Made once, kept fresh. None when not signed
/// in, or ndexr cannot be reached.
pub fn key_session() -> Option<Session> {
    static KEPT: Mutex<Option<Session>> = Mutex::new(None);
    let mut kept = KEPT.lock().unwrap();
    if let Some(s) = kept.as_ref() {
        return Some(s.clone());
    }
    let (token, ttl) = session_token(&find_key()?).ok()?;
    let s: Session = Arc::new(Mutex::new(token));
    keep_fresh(s.clone(), ttl);
    *kept = Some(s.clone());
    Some(s)
}

/// How long a launch waits for a sleeping cell to come up.
const CELL_WAKE: Duration = Duration::from_secs(90);

/// Ask the platform to start the person's cell: `https://code.<pd>` → a GET
/// of `https://<pd>/`, which nginx answers from the cell, or - stopped - by
/// waking it. The answer does not matter; the probe that follows does.
fn wake(cell: &str) {
    if let Some(site) = cell.strip_prefix("https://code.") {
        let _ = ehttp::fetch_blocking(&ehttp::Request::get(format!("https://{}/", site.trim_end_matches('/'))));
    }
}

/// Start this app again with the same arguments, for a change of where it
/// runs; the caller closes this window.
pub fn relaunch() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    std::process::Command::new(exe).args(std::env::args().skip(1)).spawn().map(|_| ()).map_err(|e| format!("could not restart: {e}"))
}

/// A fresh secret for a server this app starts (localserve.rs).
pub fn new_token() -> String {
    random_hex(24)
}

fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    if std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut b)).is_err() {
        // No /dev/urandom (Windows): std's hasher keys, which the OS seeds.
        use std::hash::BuildHasher;
        let rs = std::collections::hash_map::RandomState::new();
        for (i, x) in b.iter_mut().enumerate() {
            *x = rs.hash_one(i) as u8;
        }
    }
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Start signing in: returns the page to open in the browser; the rest runs
/// on a thread until the browser comes back (or WAIT passes).
pub fn sign_in(acct: Shared, ctx: eframe::egui::Context) -> Result<String, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|e| format!("could not listen for the browser: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let state = random_hex(16);
    let url = format!("{}/?desktop={port}&state={state}", api_base());
    {
        let mut a = acct.lock().unwrap();
        a.waiting = Some(url.clone());
        a.note = None;
    }
    std::thread::spawn(move || {
        let result = wait_for_browser(&listener, &state).and_then(|key| {
            save_key(&key)?;
            match whoami(&key) {
                Who::Email(e, cell) => {
                    acct.lock().unwrap().cell = cell;
                    Ok(e)
                }
                Who::Refused => Err("api.ndexr.io did not accept the new key".into()),
                Who::Offline(e) => Err(format!("signed in, but api.ndexr.io could not be asked who you are: {e}")),
            }
        });
        let mut a = acct.lock().unwrap();
        a.waiting = None;
        match result {
            Ok(email) => {
                a.note = Some(format!("signed in as {email}"));
                a.email = Some(email);
                a.unverified = false;
            }
            Err(e) => a.note = Some(e),
        }
        drop(a);
        ctx.request_repaint();
    });
    Ok(url)
}

/// Serve the one callback: GET /callback?state=…&key=…. Anything else (a
/// favicon, a stranger) is answered and ignored until the deadline.
fn wait_for_browser(listener: &TcpListener, state: &str) -> Result<String, String> {
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    let until = Instant::now() + WAIT;
    while Instant::now() < until {
        match listener.accept() {
            Ok((mut s, _)) => {
                let _ = s.set_nonblocking(false);
                let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
                if let Some(key) = callback(&mut s, state) {
                    return Ok(key);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("waiting for the browser: {e}")),
        }
    }
    Err("sign-in timed out - press Sign in to try again".into())
}

fn callback(s: &mut TcpStream, state: &str) -> Option<String> {
    let mut buf = [0u8; 8192];
    let n = s.read(&mut buf).ok()?;
    let head = String::from_utf8_lossy(&buf[..n]);
    let target = head.lines().next()?.strip_prefix("GET ")?.split(' ').next()?.to_owned();
    let found = parse_callback(&target, state);
    let (code, body) = match &found {
        Some(_) => ("200 OK", "<h2>Signed in</h2><p>You can close this tab and go back to Cove.</p>"),
        None => ("400 Bad Request", "<h2>Not signed in</h2><p>This link is not the one Cove is waiting for. Press Sign in in the app again.</p>"),
    };
    let page = format!("<!doctype html><meta charset=utf-8><title>Cove</title><body style='font:15px system-ui;background:#15171c;color:#d5dae3;padding:40px'>{body}</body>");
    let _ = write!(s, "HTTP/1.1 {code}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{page}", page.len());
    found
}

/// `/callback?state=S&key=K` → K, when S is ours and K looks like a key.
fn parse_callback(target: &str, state: &str) -> Option<String> {
    let (path, q) = target.split_once('?')?;
    if path != "/callback" {
        return None;
    }
    let mut got_state = None;
    let mut key = None;
    for kv in q.split('&') {
        let (k, v) = kv.split_once('=')?;
        let v = decode(v)?;
        match k {
            "state" => got_state = Some(v),
            "key" => key = Some(v),
            _ => {}
        }
    }
    (got_state.as_deref() == Some(state)).then_some(())?;
    key.filter(|k| valid(k))
}

fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_only_our_callback() {
        let k = "ndxr_0123456789ab_00112233445566778899aabbccddeeff0011223344556677";
        assert_eq!(parse_callback(&format!("/callback?state=abc123&key={k}"), "abc123").as_deref(), Some(k));
        assert_eq!(parse_callback(&format!("/callback?key={k}&state=abc123"), "abc123").as_deref(), Some(k));
        assert_eq!(parse_callback(&format!("/callback?state=other&key={k}"), "abc123"), None);
        assert_eq!(parse_callback(&format!("/elsewhere?state=abc123&key={k}"), "abc123"), None);
        assert_eq!(parse_callback("/callback?state=abc123&key=not-a-key", "abc123"), None);
        assert_eq!(parse_callback("/favicon.ico", "abc123"), None);
    }

    #[test]
    fn browser_round_trip_on_loopback() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let k = "ndxr_0123456789ab_00112233445566778899aabbccddeeff0011223344556677";
        let t = std::thread::spawn(move || {
            // A stray request first, then the real one.
            for path in ["/favicon.ico".to_owned(), format!("/callback?state=s3cr3t&key={k}")] {
                let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
                write!(c, "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").unwrap();
                let mut r = String::new();
                let _ = c.read_to_string(&mut r);
            }
        });
        assert_eq!(wait_for_browser(&listener, "s3cr3t").unwrap(), k);
        t.join().unwrap();
    }

    #[test]
    fn a_typed_address_becomes_a_base_url() {
        assert_eq!(server_base("cove.example.com/").unwrap(), "https://cove.example.com");
        assert_eq!(server_base(" http://10.0.0.5:8788 ").unwrap(), "http://10.0.0.5:8788");
        assert!(server_base("").is_err());
        assert!(server_base("two words").is_err());
    }

    /// Saved, read back, chosen at launch, and forgotten - in a config folder
    /// of the test's own (XDG_CONFIG_HOME), never the person's.
    #[test]
    fn a_saved_server_is_chosen_and_forgotten() {
        let dir = std::env::temp_dir().join(format!("cove-server-test-{}", std::process::id()));
        std::env::set_var("XDG_CONFIG_HOME", &dir);
        assert_eq!(saved_server(), None);
        save_server("https://cove.example.com", " tok3n ").unwrap();
        assert_eq!(saved_server(), Some(("https://cove.example.com".into(), "tok3n".into())));
        assert_eq!(preferred(), Some(RunOn::Server));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("ndexr/cove-server")).unwrap().permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        forget_server();
        assert_eq!(saved_server(), None);
        assert_eq!(preferred(), Some(RunOn::Local));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The probe against a live `cove serve`, when one is named
    /// (COVE_PROBE_TEST="<base> <token>"): the token is let in, another is not.
    #[test]
    fn the_probe_tells_a_good_token_from_a_bad_one() {
        let Ok(v) = std::env::var("COVE_PROBE_TEST") else { return };
        let (base, token) = v.split_once(' ').unwrap();
        probe_server(base, token).unwrap();
        assert!(probe_server(base, "not-the-token").unwrap_err().contains("refused"));
    }
}
