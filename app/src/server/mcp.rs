//! Cove as a remote MCP server (operator, 2026-11-26: "Cove should expose a
//! remote MCP server so Claude can connect directly to cove.ndexr.io ...
//! a workspace that Claude can operate within"). Claude.ai, Claude Desktop
//! and Claude Code add https://cove.ndexr.io/mcp as a custom connector.
//!
//! - Transport: MCP Streamable HTTP, JSON responses (no server push), at
//!   POST /mcp. Protocol 2025-06-18; older clients are answered in theirs.
//! - Sign-in: OAuth 2.1 with PKCE (S256) and dynamic client registration,
//!   the discovery documents at /.well-known/. Authorizing is a page behind
//!   the same ndexr sign-in as Cove itself: the person says yes, and what
//!   the connector may do beyond reading - change files, run commands,
//!   deploy - is chosen there, off by default.
//! - Every tool runs as that person, through `Host` - which `cove multiuser`
//!   answers with the person's own session, the same queries Cove's page
//!   asks. MCP is an interface to Cove, not a second application. Tools that
//!   change something are marked destructive, so Claude asks before each.
//! - Tokens: random, kept root-only in $COVE_STATE_DIR/mcp.json; access for
//!   30 days, refresh rotating. The person sees and revokes them in Cove
//!   (Query::McpGrants / McpRevoke, ui/connect.rs).

use std::collections::HashMap;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tiny_http::{Header, Response};

use crate::api::Query;

pub type Resp = Response<Cursor<Vec<u8>>>;

const PROTOCOL: &str = "2025-06-18";
const ACCESS_SECS: u64 = 30 * 86400;
const REFRESH_SECS: u64 = 180 * 86400;
const CODE_SECS: u64 = 600;

/// What a connector may do. Reading is every grant's; the rest the person
/// ticks on the consent page.
pub const SCOPES: [(&str, &str); 4] = [
    ("read", "See your projects: list them, read and search their files, their git changes and environments"),
    ("write", "Change files: apply patches to your projects"),
    ("run", "Run commands in your projects, as you, on this server"),
    ("deploy", "Start and publish your apps at their addresses"),
];

/// What MCP needs of the server it is part of.
pub trait Host {
    /// The person's projects: (domain, folder).
    fn projects(&self, user: &str) -> Vec<(String, String)>;
    /// A query, answered as the person - their session's answer.
    fn ask(&self, user: &str, q: Query) -> Result<Value, String>;
    fn audit(&self, user: &str, what: &str, detail: &str);
}

// ── the store ───────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize, Default)]
struct Client {
    name: String,
    redirect_uris: Vec<String>,
    created: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct Grant {
    /// Shown to the person and used to revoke.
    id: String,
    user: String,
    client: String,
    client_name: String,
    scopes: Vec<String>,
    created: u64,
    /// Last call with it.
    used: u64,
    access: String,
    access_expires: u64,
    refresh: String,
    refresh_expires: u64,
}

struct Code {
    user: String,
    client: String,
    redirect_uri: String,
    challenge: String,
    scopes: Vec<String>,
    expires: u64,
}

#[derive(Default, Serialize, Deserialize)]
struct Store {
    clients: HashMap<String, Client>,
    grants: Vec<Grant>,
    #[serde(skip)]
    codes: HashMap<String, Code>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn store_file() -> PathBuf {
    PathBuf::from(std::env::var("COVE_STATE_DIR").unwrap_or_else(|_| "/var/lib/cove".into())).join("mcp.json")
}

fn store() -> &'static Mutex<Store> {
    static S: OnceLock<Mutex<Store>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(std::fs::read(store_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()))
}

fn save(s: &Store) {
    let f = store_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let tmp = f.with_extension("json.new");
    if std::fs::write(&tmp, serde_json::to_vec_pretty(s).unwrap_or_default()).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600));
        }
        let _ = std::fs::rename(&tmp, &f);
    }
}

fn random(n: usize) -> String {
    use std::io::Read;
    let mut b = vec![0u8; n];
    let _ = std::fs::File::open("/dev/urandom").and_then(|mut r| r.read_exact(&mut b));
    b64url(&b)
}

pub fn b64url(b: &[u8]) -> String {
    crate::api::b64_encode(b).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

/// PKCE S256: base64url(sha256(verifier)) is the challenge.
pub fn pkce_ok(verifier: &str, challenge: &str) -> bool {
    use sha2::{Digest, Sha256};
    !verifier.is_empty() && same(&b64url(&Sha256::digest(verifier.as_bytes())), challenge)
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The person's connectors, for Cove's Connect Claude window.
pub fn grants_of(user: &str) -> Value {
    let s = store().lock().unwrap();
    let t = now();
    let list: Vec<Value> = s
        .grants
        .iter()
        .filter(|g| g.user == user && g.refresh_expires > t)
        .map(|g| json!({ "id": g.id, "client": g.client_name, "scopes": g.scopes, "created": g.created, "used": g.used }))
        .collect();
    Value::from(list)
}

pub fn revoke(user: &str, id: &str) -> Result<Value, String> {
    let mut s = store().lock().unwrap();
    let before = s.grants.len();
    s.grants.retain(|g| !(g.user == user && g.id == id));
    if s.grants.len() == before {
        return Err("no such connector of yours".into());
    }
    save(&s);
    Ok(json!("revoked"))
}

// ── HTTP ────────────────────────────────────────────────────────────────────

fn json_resp(status: u16, v: &Value) -> Resp {
    Response::from_data(v.to_string().into_bytes())
        .with_status_code(status)
        .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
        .with_header(Header::from_bytes("Cache-Control", "no-store").unwrap())
        .with_header(Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap())
}

fn oauth_error(status: u16, code: &str, what: &str) -> Resp {
    json_resp(status, &json!({ "error": code, "error_description": what }))
}

/// The paths MCP answers without Cove's sign-in: Claude's servers call them.
/// `/mcp/authorize` is not one - the person signs in to say yes.
pub fn is_open_path(path: &str) -> bool {
    matches!(path, "/mcp" | "/mcp/" | "/mcp/register" | "/mcp/token") || path.starts_with("/.well-known/oauth-")
}

/// The public address MCP is reached at: the Host the front was asked for.
pub fn base(host: Option<&str>) -> String {
    let h = host.unwrap_or("cove.ndexr.io").split(',').next().unwrap_or("cove.ndexr.io").trim();
    let local = h.starts_with("127.0.0.1") || h.starts_with("localhost");
    format!("{}://{h}", if local { "http" } else { "https" })
}

pub fn protected_resource(base: &str) -> Resp {
    json_resp(200, &json!({
        "resource": format!("{base}/mcp"),
        "authorization_servers": [base],
        "scopes_supported": SCOPES.iter().map(|s| s.0).collect::<Vec<_>>(),
        "bearer_methods_supported": ["header"],
        "resource_name": "Cove",
    }))
}

pub fn authorization_server(base: &str) -> Resp {
    json_resp(200, &json!({
        "issuer": base,
        "authorization_endpoint": format!("{base}/mcp/authorize"),
        "token_endpoint": format!("{base}/mcp/token"),
        "registration_endpoint": format!("{base}/mcp/register"),
        "scopes_supported": SCOPES.iter().map(|s| s.0).collect::<Vec<_>>(),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
    }))
}

/// A redirect a connector may use: https anywhere, or http back to this
/// computer (Claude Code and Desktop listen on loopback).
fn redirect_ok(u: &str) -> bool {
    u.starts_with("https://") || u.starts_with("http://localhost") || u.starts_with("http://127.0.0.1") || u.starts_with("http://[::1]")
}

/// Dynamic client registration (RFC 7591): Claude registers itself.
pub fn register(body: &[u8]) -> Resp {
    let Ok(v) = serde_json::from_slice::<Value>(body) else { return oauth_error(400, "invalid_client_metadata", "not JSON") };
    let uris: Vec<String> = v["redirect_uris"].as_array().map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_owned)).collect()).unwrap_or_default();
    if uris.is_empty() || !uris.iter().all(|u| redirect_ok(u)) {
        return oauth_error(400, "invalid_redirect_uri", "redirect_uris must be https, or http to this computer");
    }
    let name = v["client_name"].as_str().unwrap_or("a connector").chars().take(80).collect::<String>();
    let id = format!("cove-{}", random(18));
    let mut s = store().lock().unwrap();
    // A registry that only grows is a way to fill the disk: the oldest
    // registrations with no grant go first.
    if s.clients.len() > 5000 {
        let used: std::collections::HashSet<String> = s.grants.iter().map(|g| g.client.clone()).collect();
        let mut old: Vec<(String, u64)> = s.clients.iter().filter(|(k, _)| !used.contains(*k)).map(|(k, c)| (k.clone(), c.created)).collect();
        old.sort_by_key(|x| x.1);
        for (k, _) in old.into_iter().take(1000) {
            s.clients.remove(&k);
        }
    }
    s.clients.insert(id.clone(), Client { name: name.clone(), redirect_uris: uris.clone(), created: now() });
    save(&s);
    json_resp(201, &json!({
        "client_id": id,
        "client_id_issued_at": now(),
        "client_name": name,
        "redirect_uris": uris,
        "grant_types": ["authorization_code", "refresh_token"],
        "response_types": ["code"],
        "token_endpoint_auth_method": "none",
    }))
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn page(status: u16, body: &str) -> Resp {
    let html = format!(
        "<!doctype html><meta charset=utf-8><meta name=viewport content=\"width=device-width,initial-scale=1\"><title>Cove · connect</title><style>body{{margin:0;min-height:100vh;background:#15171c;color:#d5dae3;font:15px/1.5 system-ui,sans-serif;display:grid;place-items:center}}main{{background:#1d2027;border:1px solid #2f343e;border-radius:14px;padding:28px;width:min(92vw,460px)}}h1{{font-size:21px;margin:0 0 6px;color:#eef1f5}}p{{margin:6px 0 14px}}label{{display:flex;gap:10px;align-items:flex-start;padding:8px 0;border-top:1px solid #2f343e}}input{{margin-top:4px}}.row{{display:flex;gap:10px;margin-top:18px}}button{{flex:1;padding:10px;border:0;border-radius:8px;font:inherit;font-weight:600;cursor:pointer}}.yes{{background:#5ec4c0;color:#15171c}}.no{{background:#2f343e;color:#d5dae3}}code{{color:#5ec4c0}}</style><main>{body}</main>"
    );
    Response::from_data(html.into_bytes()).with_status_code(status).with_header(Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap())
}

fn param(q: &HashMap<String, String>, k: &str) -> String {
    q.get(k).cloned().unwrap_or_default()
}

/// What an authorize request asks, checked: its client and redirect known,
/// a code asked for, with a S256 challenge.
fn checked(q: &HashMap<String, String>) -> Result<(String, Client), String> {
    let client = param(q, "client_id");
    let c = store().lock().unwrap().clients.get(&client).cloned().ok_or("this connector is not registered here - add it again in Claude")?;
    if !c.redirect_uris.contains(&param(q, "redirect_uri")) {
        return Err("the connector's return address does not match its registration".into());
    }
    if param(q, "response_type") != "code" {
        return Err("only the authorization code flow is supported".into());
    }
    if param(q, "code_challenge").is_empty() || param(q, "code_challenge_method") != "S256" {
        return Err("PKCE (S256) is required".into());
    }
    Ok((client, c))
}

/// The consent page, for the person signed in to Cove.
pub fn authorize_page(user: &str, q: &HashMap<String, String>) -> Resp {
    let (_, c) = match checked(q) {
        Ok(x) => x,
        Err(e) => return page(400, &format!("<h1>Cannot connect</h1><p>{}</p>", esc(&e))),
    };
    let hidden: String = ["client_id", "redirect_uri", "state", "code_challenge", "code_challenge_method", "response_type", "resource"]
        .iter()
        .map(|k| format!("<input type=hidden name={k} value=\"{}\">", esc(&param(q, k))))
        .collect();
    let boxes: String = SCOPES
        .iter()
        .map(|(k, what)| {
            let fixed = *k == "read";
            format!("<label><input type=checkbox name=scope_{k} value=1{}><span><b>{}</b><br>{}</span></label>", if fixed { " checked disabled" } else { "" }, k, esc(what))
        })
        .collect();
    page(
        200,
        &format!(
            "<h1>Connect {} to Cove</h1><p>It will work in your Cove as <code>{}</code>, on your projects only. Reading is always allowed; tick what else it may do. Each change it makes is asked of you in Claude first, and every call is logged.</p><form method=post action=/mcp/authorize>{hidden}{boxes}<div class=row><button class=no name=decision value=deny>Cancel</button><button class=yes name=decision value=allow>Allow</button></div></form>",
            esc(&c.name),
            esc(user)
        ),
    )
}

fn to_redirect(uri: &str, pairs: &[(&str, &str)]) -> Resp {
    let sep = if uri.contains('?') { '&' } else { '?' };
    let q: Vec<String> = pairs.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| format!("{k}={}", enc(v))).collect();
    Response::from_data(Vec::new()).with_status_code(302).with_header(Header::from_bytes("Location", format!("{uri}{sep}{}", q.join("&")).as_bytes()).unwrap())
}

fn enc(s: &str) -> String {
    s.bytes().map(|b| if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect()
}

/// The person's answer: a code back to the connector, or its refusal.
pub fn authorize_decide(user: &str, form: &HashMap<String, String>, host: &dyn Host) -> Resp {
    let (client, _) = match checked(form) {
        Ok(x) => x,
        Err(e) => return page(400, &format!("<h1>Cannot connect</h1><p>{}</p>", esc(&e))),
    };
    let redirect = param(form, "redirect_uri");
    let state = param(form, "state");
    if param(form, "decision") != "allow" {
        return to_redirect(&redirect, &[("error", "access_denied"), ("state", &state)]);
    }
    let scopes: Vec<String> = SCOPES.iter().filter(|(k, _)| *k == "read" || form.contains_key(&format!("scope_{k}"))).map(|(k, _)| (*k).to_owned()).collect();
    let code = random(24);
    store().lock().unwrap().codes.insert(code.clone(), Code { user: user.to_owned(), client, redirect_uri: redirect.clone(), challenge: param(form, "code_challenge"), scopes: scopes.clone(), expires: now() + CODE_SECS });
    host.audit(user, "mcp-authorize", &scopes.join(" "));
    to_redirect(&redirect, &[("code", &code), ("state", &state)])
}

/// The token endpoint: a code (with its PKCE verifier) or a refresh token
/// for a fresh access token.
pub fn token(form: &HashMap<String, String>) -> Resp {
    let mut s = store().lock().unwrap();
    let t = now();
    s.codes.retain(|_, c| c.expires > t);
    match param(form, "grant_type").as_str() {
        "authorization_code" => {
            let Some(c) = s.codes.remove(&param(form, "code")) else { return oauth_error(400, "invalid_grant", "the code is unknown, used or expired") };
            if c.client != param(form, "client_id") || c.redirect_uri != param(form, "redirect_uri") {
                return oauth_error(400, "invalid_grant", "the code was issued to another client or return address");
            }
            if !pkce_ok(&param(form, "code_verifier"), &c.challenge) {
                return oauth_error(400, "invalid_grant", "the PKCE verifier does not match");
            }
            let name = s.clients.get(&c.client).map(|x| x.name.clone()).unwrap_or_default();
            let g = Grant { id: random(6), user: c.user, client: c.client, client_name: name, scopes: c.scopes, created: t, used: t, access: random(32), access_expires: t + ACCESS_SECS, refresh: random(32), refresh_expires: t + REFRESH_SECS };
            let out = token_json(&g);
            s.grants.push(g);
            save(&s);
            json_resp(200, &out)
        }
        "refresh_token" => {
            let r = param(form, "refresh_token");
            let Some(g) = s.grants.iter_mut().find(|g| same(&g.refresh, &r) && g.refresh_expires > t) else { return oauth_error(400, "invalid_grant", "the refresh token is unknown, revoked or expired") };
            if !param(form, "client_id").is_empty() && g.client != param(form, "client_id") {
                return oauth_error(400, "invalid_grant", "the refresh token belongs to another client");
            }
            g.access = random(32);
            g.access_expires = t + ACCESS_SECS;
            g.refresh = random(32);
            g.refresh_expires = t + REFRESH_SECS;
            let out = token_json(g);
            save(&s);
            json_resp(200, &out)
        }
        _ => oauth_error(400, "unsupported_grant_type", "authorization_code or refresh_token"),
    }
}

fn token_json(g: &Grant) -> Value {
    json!({ "access_token": g.access, "token_type": "Bearer", "expires_in": g.access_expires.saturating_sub(now()), "refresh_token": g.refresh, "scope": g.scopes.join(" ") })
}

/// The person and the scopes behind a bearer token.
fn bearer(auth: Option<&str>) -> Option<(String, Vec<String>)> {
    let tok = auth?.trim().strip_prefix("Bearer ")?.trim();
    let mut s = store().lock().unwrap();
    let t = now();
    let g = s.grants.iter_mut().find(|g| same(&g.access, tok) && g.access_expires > t)?;
    // The last use, kept to the minute: not a disk write per call.
    if t - g.used > 60 {
        g.used = t;
        let out = (g.user.clone(), g.scopes.clone());
        save(&s);
        return Some(out);
    }
    Some((g.user.clone(), g.scopes.clone()))
}

/// POST /mcp: one JSON-RPC message (or a batch) from a connector.
pub fn endpoint(base: &str, method: &str, auth: Option<&str>, body: &[u8], host: &dyn Host) -> Resp {
    let unauthorized = || {
        Response::from_data(b"{\"error\":\"invalid_token\"}".to_vec())
            .with_status_code(401)
            .with_header(Header::from_bytes("WWW-Authenticate", format!("Bearer resource_metadata=\"{base}/.well-known/oauth-protected-resource\"").as_bytes()).unwrap())
            .with_header(Header::from_bytes("Content-Type", "application/json").unwrap())
    };
    let Some((user, scopes)) = bearer(auth) else { return unauthorized() };
    match method {
        "POST" => {}
        // No stream to open: every answer comes back on its POST.
        "GET" => return Response::from_data(Vec::new()).with_status_code(405).with_header(Header::from_bytes("Allow", "POST").unwrap()),
        "DELETE" => return Response::from_data(Vec::new()).with_status_code(200),
        _ => return Response::from_data(Vec::new()).with_status_code(405),
    }
    let Ok(msg) = serde_json::from_slice::<Value>(body) else {
        return json_resp(400, &json!({ "jsonrpc": "2.0", "id": null, "error": { "code": -32700, "message": "parse error" } }));
    };
    let answers: Vec<Value> = match &msg {
        Value::Array(all) => all.iter().filter_map(|m| rpc(m, &user, &scopes, host)).collect(),
        m => rpc(m, &user, &scopes, host).into_iter().collect(),
    };
    match (msg.is_array(), answers.len()) {
        // Notifications and answers only: nothing to say back.
        (_, 0) => Response::from_data(Vec::new()).with_status_code(202),
        (false, _) => json_resp(200, &answers[0]),
        (true, _) => json_resp(200, &Value::from(answers)),
    }
}

/// One JSON-RPC message; None for a notification.
fn rpc(m: &Value, user: &str, scopes: &[String], host: &dyn Host) -> Option<Value> {
    let id = m.get("id").cloned()?;
    let method = m["method"].as_str().unwrap_or("");
    let ok = |result: Value| json!({ "jsonrpc": "2.0", "id": id, "result": result });
    let err = |code: i64, msg: &str| json!({ "jsonrpc": "2.0", "id": m["id"], "error": { "code": code, "message": msg } });
    Some(match method {
        "initialize" => {
            let asked = m["params"]["protocolVersion"].as_str().unwrap_or(PROTOCOL);
            let version = if ["2025-06-18", "2025-03-26", "2024-11-05"].contains(&asked) { asked } else { PROTOCOL };
            ok(json!({
                "protocolVersion": version,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "cove", "title": "Cove", "version": env!("CARGO_PKG_VERSION") },
                "instructions": "Cove is the person's workspace on the ndexr stack. Start with list_projects; a project is named by its domain (e.g. mysite.ndexr.io) and paths are relative to its folder. Read before you change; apply_patch takes a unified diff; run_command runs in the project's folder as the person; deploy_project starts the app and publishes it at its address.",
            }))
        }
        "ping" => ok(json!({})),
        "tools/list" => ok(json!({ "tools": tools(scopes) })),
        "tools/call" => {
            let name = m["params"]["name"].as_str().unwrap_or("");
            let args = m["params"]["arguments"].clone();
            let (text, is_error) = match call(name, &args, user, scopes, host) {
                Ok(v) => (if let Value::String(s) = v { s } else { serde_json::to_string_pretty(&v).unwrap_or_default() }, false),
                Err(e) => (e, true),
            };
            host.audit(user, "mcp-tool", &format!("{name} {}", if is_error { "refused" } else { "done" }));
            ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        "resources/list" => ok(json!({ "resources": [] })),
        "prompts/list" => ok(json!({ "prompts": [] })),
        _ => err(-32601, &format!("no method {method}")),
    })
}

// ── the tools ───────────────────────────────────────────────────────────────

struct Tool {
    name: &'static str,
    title: &'static str,
    scope: &'static str,
    description: &'static str,
    schema: fn() -> Value,
}

fn project_only() -> Value {
    json!({ "type": "object", "properties": { "project": { "type": "string", "description": "the project's domain, from list_projects" } }, "required": ["project"] })
}

const TOOLS: &[Tool] = &[
    Tool { name: "list_projects", title: "List projects", scope: "read", description: "Your Cove projects: each one's domain (its name everywhere else) and folder.", schema: || json!({ "type": "object", "properties": {} }) },
    Tool { name: "get_project", title: "Get project", scope: "read", description: "One project's context: its app (language, port, running or not), its top-level files, and its CONTEXT.md / CLAUDE.md / README.md.", schema: project_only },
    Tool {
        name: "list_files",
        title: "List files",
        scope: "read",
        description: "The files and folders in a project folder (or a folder inside it).",
        schema: || json!({ "type": "object", "properties": { "project": { "type": "string" }, "path": { "type": "string", "description": "a folder relative to the project, \"\" for its root" } }, "required": ["project"] }),
    },
    Tool {
        name: "read_file",
        title: "Read file",
        scope: "read",
        description: "A file's text. Long files can be read in parts with offset and limit (lines).",
        schema: || json!({ "type": "object", "properties": { "project": { "type": "string" }, "path": { "type": "string" }, "offset": { "type": "integer", "minimum": 0 }, "limit": { "type": "integer", "minimum": 1 } }, "required": ["project", "path"] }),
    },
    Tool {
        name: "search_code",
        title: "Search code",
        scope: "read",
        description: "Search every file in a project for text (or a regular expression); each match with its file and line.",
        schema: || json!({ "type": "object", "properties": { "project": { "type": "string" }, "query": { "type": "string" }, "regex": { "type": "boolean" }, "case_sensitive": { "type": "boolean" } }, "required": ["project", "query"] }),
    },
    Tool {
        name: "git_diff",
        title: "Git diff",
        scope: "read",
        description: "What changed since the project's last saved version: every changed file (staged, not staged, new), or one file's unified diff.",
        schema: || json!({ "type": "object", "properties": { "project": { "type": "string" }, "path": { "type": "string", "description": "one file's diff; omit for the list" }, "staged": { "type": "boolean" } }, "required": ["project"] }),
    },
    Tool { name: "get_environment", title: "Get environment", scope: "read", description: "The exact software a project runs on, as recorded: environment module, interpreter and package versions, toolchain, stack revision.", schema: project_only },
    Tool {
        name: "apply_patch",
        title: "Apply patch",
        scope: "write",
        description: "Change a project's files with a unified diff (git diff format, paths relative to the project). Checked first; nothing is changed if any part does not apply.",
        schema: || json!({ "type": "object", "properties": { "project": { "type": "string" }, "patch": { "type": "string" } }, "required": ["project", "patch"] }),
    },
    Tool {
        name: "run_command",
        title: "Run command",
        scope: "run",
        description: "Run a shell command in the project's folder, as you, in your own account on the server (a login shell with the ndexr stack). Returns its exit code and output.",
        schema: || json!({ "type": "object", "properties": { "project": { "type": "string" }, "command": { "type": "string" }, "timeout_secs": { "type": "integer", "minimum": 1, "maximum": 600 } }, "required": ["project", "command"] }),
    },
    Tool { name: "deploy_project", title: "Deploy project", scope: "deploy", description: "Start the project's app (.cove/up.sh) and publish it at https://<project>/ - registry, DNS, certificate and routing.", schema: project_only },
];

fn tools(scopes: &[String]) -> Vec<Value> {
    TOOLS
        .iter()
        .filter(|t| scopes.iter().any(|s| s == t.scope))
        .map(|t| {
            let read = t.scope == "read";
            json!({
                "name": t.name,
                "title": t.title,
                "description": t.description,
                "inputSchema": (t.schema)(),
                "annotations": { "title": t.title, "readOnlyHint": read, "destructiveHint": !read, "idempotentHint": read, "openWorldHint": t.scope == "deploy" },
            })
        })
        .collect()
}

fn arg(a: &Value, k: &str) -> String {
    a[k].as_str().unwrap_or("").trim().to_owned()
}

/// A project of the person's, by its domain: its folder.
fn folder(host: &dyn Host, user: &str, a: &Value) -> Result<(String, String), String> {
    let want = arg(a, "project").to_ascii_lowercase();
    host.projects(user).into_iter().find(|(h, _)| *h == want).ok_or_else(|| format!("{want} is not one of your projects - list_projects names them"))
}

/// `rel` inside `root`: relative, no way up or out.
pub fn inside(root: &str, rel: &str) -> Result<String, String> {
    let rel = rel.trim().trim_start_matches("./");
    if rel.starts_with('/') || rel.split('/').any(|c| c == "..") || rel.contains('\0') {
        return Err(format!("{rel}: paths are relative to the project, with no .."));
    }
    let root = root.trim_end_matches('/');
    Ok(if rel.is_empty() || rel == "." { root.to_owned() } else { format!("{root}/{}", rel.trim_end_matches('/')) })
}

fn call(name: &str, a: &Value, user: &str, scopes: &[String], host: &dyn Host) -> Result<Value, String> {
    let tool = TOOLS.iter().find(|t| t.name == name).ok_or_else(|| format!("no tool {name}"))?;
    if !scopes.iter().any(|s| s == tool.scope) {
        return Err(format!("{name} needs the \"{}\" permission, which this connector was not given - remove it in Claude and connect again to ask for it", tool.scope));
    }
    match name {
        "list_projects" => Ok(Value::from(host.projects(user).into_iter().map(|(h, p)| json!({ "project": h, "folder": p })).collect::<Vec<_>>())),
        "get_project" => {
            let (h, dir) = folder(host, user, a)?;
            let app = host.ask(user, Query::AppStatus { host: h.clone() }).unwrap_or(Value::Null);
            let files = host.ask(user, Query::ListDir { path: dir.clone() }).map(|v| names(&v)).unwrap_or_default();
            let mut docs = serde_json::Map::new();
            for f in ["CONTEXT.md", "CLAUDE.md", "README.md"] {
                if files.iter().any(|n| n == f) {
                    if let Ok(v) = host.ask(user, Query::ReadFile { path: format!("{dir}/{f}") }) {
                        let t = v["text"].as_str().unwrap_or("");
                        docs.insert(f.to_owned(), Value::from(t.chars().take(12_000).collect::<String>()));
                    }
                }
            }
            Ok(json!({ "project": h, "folder": dir, "app": app, "files": files, "docs": docs }))
        }
        "list_files" => {
            let (_, dir) = folder(host, user, a)?;
            let v = host.ask(user, Query::ListDir { path: inside(&dir, &arg(a, "path"))? })?;
            let list: Vec<Value> = v["entries"].as_array().map(|e| e.iter().map(|x| json!({ "name": x["name"], "dir": x["dir"], "size": x["size"] })).collect()).unwrap_or_default();
            Ok(Value::from(list))
        }
        "read_file" => {
            let (_, dir) = folder(host, user, a)?;
            let v = host.ask(user, Query::ReadFile { path: inside(&dir, &arg(a, "path"))? })?;
            let text = v["text"].as_str().unwrap_or("");
            let offset = a["offset"].as_u64().unwrap_or(0) as usize;
            let limit = a["limit"].as_u64().map(|l| l as usize).unwrap_or(usize::MAX);
            let lines: Vec<&str> = text.lines().collect();
            let part: String = lines.iter().skip(offset).take(limit).copied().collect::<Vec<_>>().join("\n");
            let cut: String = part.chars().take(200_000).collect();
            let more = if offset + limit.min(lines.len()) < lines.len() || cut.len() < part.len() { format!("\n\n[{} lines in all; read on with offset]", lines.len()) } else { String::new() };
            Ok(Value::String(cut + &more))
        }
        "search_code" => {
            let (_, dir) = folder(host, user, a)?;
            let q = arg(a, "query");
            if q.is_empty() {
                return Err("say what to search for".into());
            }
            host.ask(user, Query::Grep { root: dir, query: q, case: a["case_sensitive"].as_bool().unwrap_or(false), regex: a["regex"].as_bool().unwrap_or(false) })
        }
        "git_diff" => {
            let (h, _) = folder(host, user, a)?;
            let path = arg(a, "path");
            if path.is_empty() {
                return host.ask(user, Query::GitStatus { host: h });
            }
            inside("", &path)?;
            let area = if a["staged"].as_bool().unwrap_or(false) { "staged" } else { "" };
            host.ask(user, Query::GitDiff { host: h, sha: String::new(), path, old: String::new(), area: area.into(), full: false })
        }
        "get_environment" => {
            let (h, _) = folder(host, user, a)?;
            let record = host.ask(user, Query::Record { host: h.clone(), capture: false }).unwrap_or(Value::Null);
            let app = host.ask(user, Query::AppStatus { host: h }).unwrap_or(Value::Null);
            Ok(json!({ "record": record, "app": app }))
        }
        "apply_patch" => {
            let (h, _) = folder(host, user, a)?;
            let patch = a["patch"].as_str().unwrap_or("").to_owned();
            host.ask(user, Query::GitPatch { host: h.clone(), action: "check".into(), patch: patch.clone() }).map_err(|e| format!("the patch does not apply: {e}"))?;
            let r = host.ask(user, Query::GitPatch { host: h.clone(), action: "apply".into(), patch });
            host.audit(user, "mcp-apply-patch", &format!("{h} {}", if r.is_ok() { "applied" } else { "refused" }));
            r
        }
        "run_command" => {
            let (h, dir) = folder(host, user, a)?;
            let command = a["command"].as_str().unwrap_or("").to_owned();
            if command.trim().is_empty() {
                return Err("say what to run".into());
            }
            host.audit(user, "mcp-run", &format!("{h} {}", command.chars().take(200).collect::<String>()));
            host.ask(user, Query::RunCommand { dir, command, timeout_secs: a["timeout_secs"].as_u64().unwrap_or(120).clamp(1, 600) })
        }
        "deploy_project" => {
            let (h, _) = folder(host, user, a)?;
            let up = host.ask(user, Query::AppAction { host: h.clone(), verb: "up".into() });
            let publish = host.ask(user, Query::PublishDomain { host: h.clone() });
            host.audit(user, "mcp-deploy", &format!("{h} up:{} publish:{}", up.is_ok(), publish.is_ok()));
            Ok(json!({ "project": h, "start": up.unwrap_or_else(|e| Value::String(format!("not started: {e}"))), "publish": publish.unwrap_or_else(|e| Value::String(format!("not published: {e}"))), "address": format!("https://{h}/") }))
        }
        _ => Err(format!("no tool {name}")),
    }
}

fn names(listing: &Value) -> Vec<String> {
    listing["entries"].as_array().map(|e| e.iter().filter_map(|x| x["name"].as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc_7636() {
        // RFC 7636 appendix B.
        assert!(pkce_ok("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk", "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"));
        assert!(!pkce_ok("wrong", "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"));
        assert!(!pkce_ok("", ""));
    }

    #[test]
    fn paths_stay_inside_the_project() {
        assert_eq!(inside("/p/x", "").unwrap(), "/p/x");
        assert_eq!(inside("/p/x/", "src/app.R").unwrap(), "/p/x/src/app.R");
        assert!(inside("/p/x", "../y").is_err());
        assert!(inside("/p/x", "/etc/passwd").is_err());
        assert!(inside("/p/x", "a/../../b").is_err());
    }

    struct Fake(Mutex<Vec<String>>);
    impl Host for Fake {
        fn projects(&self, user: &str) -> Vec<(String, String)> {
            if user == "ann" { vec![("ann.ndexr.io".into(), "/home/ann/domains/ann.ndexr.io".into())] } else { Vec::new() }
        }
        fn ask(&self, _: &str, q: Query) -> Result<Value, String> {
            self.0.lock().unwrap().push(serde_json::to_string(&q).unwrap());
            match q {
                Query::ReadFile { path } => Ok(json!({ "path": path, "text": "a\nb\nc\nd" })),
                Query::ListDir { .. } => Ok(json!({ "entries": [{ "name": "app.R", "dir": false, "size": 3 }] })),
                _ => Ok(json!("ok")),
            }
        }
        fn audit(&self, _: &str, _: &str, _: &str) {}
    }

    #[test]
    fn tools_follow_the_grant_and_stay_in_the_persons_projects() {
        let f = Fake(Mutex::new(Vec::new()));
        let read = vec!["read".to_owned()];
        assert_eq!(tools(&read).len(), 7, "read-only tools only");
        assert_eq!(tools(&SCOPES.iter().map(|s| s.0.to_owned()).collect::<Vec<_>>()).len(), TOOLS.len());
        let r = call("read_file", &json!({ "project": "ann.ndexr.io", "path": "app.R", "offset": 1, "limit": 2 }), "ann", &read, &f).unwrap();
        assert!(r.as_str().unwrap().starts_with("b\nc"), "{r}");
        assert!(f.0.lock().unwrap()[0].contains("/home/ann/domains/ann.ndexr.io/app.R"));
        assert!(call("read_file", &json!({ "project": "bob.ndexr.io", "path": "x" }), "ann", &read, &f).is_err(), "not her project");
        assert!(call("read_file", &json!({ "project": "ann.ndexr.io", "path": "../../etc/shadow" }), "ann", &read, &f).is_err());
        let e = call("run_command", &json!({ "project": "ann.ndexr.io", "command": "ls" }), "ann", &read, &f).unwrap_err();
        assert!(e.contains("\"run\" permission"), "{e}");
    }

    fn body_of(r: Resp) -> (u16, Vec<(String, String)>, String) {
        let status = r.status_code().0;
        let heads = r.headers().iter().map(|h| (h.field.to_string(), h.value.to_string())).collect();
        let mut b = String::new();
        use std::io::Read;
        let _ = r.into_reader().read_to_string(&mut b);
        (status, heads, b)
    }

    /// The whole lifecycle a Claude connector goes through: discovery,
    /// registration, the person's yes, the code for a token (PKCE), then
    /// MCP calls with the token - and refresh and revoke.
    #[test]
    fn a_connector_registers_is_allowed_and_calls_tools() {
        let dir = std::env::temp_dir().join(format!("cove-mcp-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::env::set_var("COVE_STATE_DIR", &dir);
        let f = Fake(Mutex::new(Vec::new()));
        let base = "https://cove.example";
        // No token: 401, pointing at the resource metadata.
        let (st, heads, _) = body_of(endpoint(base, "POST", None, b"{}", &f));
        assert_eq!(st, 401);
        assert!(heads.iter().any(|(k, v)| k.eq_ignore_ascii_case("WWW-Authenticate") && v.contains("/.well-known/oauth-protected-resource")));
        let (_, _, meta) = body_of(authorization_server(base));
        assert!(meta.contains("\"registration_endpoint\":\"https://cove.example/mcp/register\""), "{meta}");
        // Registration.
        let (st, _, reg) = body_of(register(br#"{"client_name":"Claude","redirect_uris":["https://claude.ai/api/mcp/auth_callback"]}"#));
        assert_eq!(st, 201);
        let client = serde_json::from_str::<Value>(&reg).unwrap()["client_id"].as_str().unwrap().to_owned();
        assert_eq!(body_of(register(br#"{"redirect_uris":["http://evil.example/cb"]}"#)).0, 400, "plain http elsewhere is refused");
        // The person allows read and run.
        let verifier = "a-long-random-verifier-string-for-pkce-0123456789";
        let challenge = {
            use sha2::{Digest, Sha256};
            b64url(&Sha256::digest(verifier.as_bytes()))
        };
        let mut form: HashMap<String, String> = [
            ("client_id", client.as_str()),
            ("redirect_uri", "https://claude.ai/api/mcp/auth_callback"),
            ("response_type", "code"),
            ("code_challenge", challenge.as_str()),
            ("code_challenge_method", "S256"),
            ("state", "xyz"),
            ("decision", "allow"),
            ("scope_run", "1"),
        ]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
        assert_eq!(body_of(authorize_page("ann", &form)).0, 200);
        let (st, heads, _) = body_of(authorize_decide("ann", &form, &f));
        assert_eq!(st, 302);
        let loc = heads.iter().find(|(k, _)| k.eq_ignore_ascii_case("Location")).unwrap().1.clone();
        assert!(loc.starts_with("https://claude.ai/api/mcp/auth_callback?code=") && loc.ends_with("&state=xyz"), "{loc}");
        let code = loc.split("code=").nth(1).unwrap().split('&').next().unwrap().to_owned();
        // A wrong verifier fails, and spends nothing; the right one gets a token.
        form.clear();
        for (k, v) in [("grant_type", "authorization_code"), ("code", code.as_str()), ("client_id", client.as_str()), ("redirect_uri", "https://claude.ai/api/mcp/auth_callback"), ("code_verifier", "wrong")] {
            form.insert(k.into(), v.into());
        }
        assert_eq!(body_of(token(&form)).0, 400);
        let (st, _, _) = body_of(token(&form));
        assert_eq!(st, 400, "a code is single use, even after a failed try");
        // Fresh consent, right verifier.
        let mut allow: HashMap<String, String> = [("client_id", client.as_str()), ("redirect_uri", "https://claude.ai/api/mcp/auth_callback"), ("response_type", "code"), ("code_challenge", challenge.as_str()), ("code_challenge_method", "S256"), ("decision", "allow"), ("scope_run", "1")].iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect();
        let (_, heads, _) = body_of(authorize_decide("ann", &allow, &f));
        let loc = heads.iter().find(|(k, _)| k.eq_ignore_ascii_case("Location")).unwrap().1.clone();
        let code = loc.split("code=").nth(1).unwrap().split('&').next().unwrap().to_owned();
        allow.clear();
        for (k, v) in [("grant_type", "authorization_code"), ("code", code.as_str()), ("client_id", client.as_str()), ("redirect_uri", "https://claude.ai/api/mcp/auth_callback"), ("code_verifier", verifier)] {
            allow.insert(k.into(), v.into());
        }
        let (st, _, tok) = body_of(token(&allow));
        assert_eq!(st, 200, "{tok}");
        let tok: Value = serde_json::from_str(&tok).unwrap();
        assert_eq!(tok["scope"], "read run");
        let access = format!("Bearer {}", tok["access_token"].as_str().unwrap());
        // MCP with the token.
        let (st, _, init) = body_of(endpoint(base, "POST", Some(&access), br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#, &f));
        assert_eq!(st, 200);
        assert!(init.contains("\"protocolVersion\":\"2025-06-18\""));
        assert_eq!(body_of(endpoint(base, "POST", Some(&access), br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#, &f)).0, 202);
        let (_, _, list) = body_of(endpoint(base, "POST", Some(&access), br#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, &f));
        assert!(list.contains("run_command") && !list.contains("apply_patch") && !list.contains("deploy_project"), "only what was allowed: {list}");
        let (_, _, called) = body_of(endpoint(base, "POST", Some(&access), br#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_projects","arguments":{}}}"#, &f));
        assert!(called.contains("ann.ndexr.io"), "{called}");
        // Refresh rotates; the old refresh token is spent.
        let mut r: HashMap<String, String> = HashMap::new();
        r.insert("grant_type".into(), "refresh_token".into());
        r.insert("refresh_token".into(), tok["refresh_token"].as_str().unwrap().into());
        assert_eq!(body_of(token(&r)).0, 200);
        assert_eq!(body_of(token(&r)).0, 400);
        // Revoked, the token no longer works.
        let id = grants_of("ann")[0]["id"].as_str().unwrap().to_owned();
        assert!(revoke("bob", &id).is_err(), "only the person revokes their own");
        revoke("ann", &id).unwrap();
        assert_eq!(body_of(endpoint(base, "POST", Some(&access), br#"{"jsonrpc":"2.0","id":4,"method":"ping"}"#, &f)).0, 401);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rpc_answers_initialize_and_ignores_notifications() {
        let f = Fake(Mutex::new(Vec::new()));
        let s = vec!["read".to_owned()];
        let init = rpc(&json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": { "protocolVersion": "2025-03-26" } }), "ann", &s, &f).unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(rpc(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }), "ann", &s, &f).is_none());
        let call = rpc(&json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/call", "params": { "name": "list_projects", "arguments": {} } }), "ann", &s, &f).unwrap();
        assert!(call["result"]["content"][0]["text"].as_str().unwrap().contains("ann.ndexr.io"));
        assert_eq!(call["result"]["isError"], false);
    }
}
