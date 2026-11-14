//! `cove serve` — the same UI, in a browser, for a machine you reach
//! over the network (a lab server, a cell, a cluster login node). Like
//! rstudio-server: installed from a .deb, run by systemd as the user whose
//! machine it shows (packaging/linux/cove@.service, User=%i).
//!
//!   GET  /login?token=<t>   the one way in: sets an HttpOnly session cookie
//!   GET  /, /<file>         the UI, compiled to WebAssembly (--web-dir)
//!   GET  /view?path=<p>     a page a render wrote (the Viewer), sandboxed
//!   GET  /site/<id>/<path>  a folder the Viewer shows (a render's output, an
//!                           htmlwidget's page), with what sits beside the page
//!   *    /port/<n>/<path>   a server on this machine's 127.0.0.1:<n> (quarto
//!                           preview, shiny, mkdocs serve …), websockets too
//!   POST /api/q             a Query → host.rs's answer
//!   GET  /software.json     the Software tab's catalog (software_response)
//!
//! Like rstudio-server, signing in means running code as this user:
//! `Query::Eval` hands it to the kernels (kernel.rs) - R, Python, or the
//! Prefix shell, the pimp's Terminal (2026-10-13) - and the Files pane reads
//! and writes their files. The language is an enum checked at the boundary.
//!
//! Everything but /login needs the cookie. It runs as this user, so
//! the token is that user's password: 0600 in ~/.config/ndexr/cove-token,
//! made on first start. Bind is 127.0.0.1 by default — put nginx (TLS) in
//! front or reach it over `ssh -L`, as with rstudio-server.
//!
//! Behind ndexr's own gate (laptop.cove.ndexr.io, neo.cove.ndexr.io): nginx has
//! already verified the person and passes their email as X-Auth-Email
//! (auth_request_set; common_proxy.conf blanks any client-sent copy). With
//! COVE_TRUST_EMAILS=a@x,b@y that email is let in with no token —
//! one sign-in, not two (operator, 2026-10-11, at the token page: "you do it").
//! Only over loopback, where the gate's tunnel arrives.
//!
//! In an account's cell (COVE_TRUST_LOOPBACK=1) every loopback request
//! is let in: the host's /__code_session gate has already decided the caller
//! is that account, and arrives through the cell's code-server (/proxy/<port>/).
//! A cell is one account, so loopback is that account's own. Never set it on a
//! machine people share - `cove multiuser` is for those.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tiny_http::{Header, Method, Request, Response, Server};

use crate::api::Query;

const COOKIE: &str = "cove";

struct Ctx {
    token: String,
    /// Emails nginx's gate may vouch for (COVE_TRUST_EMAILS).
    trusted: Vec<String>,
    /// A cell: loopback is the account's own (COVE_TRUST_LOOPBACK=1).
    trust_loopback: bool,
    web: PathBuf,
}

pub fn main(args: &[String]) -> i32 {
    let mut listen = std::env::var("COVE_LISTEN").unwrap_or_else(|_| "127.0.0.1:8788".into());
    let mut web: Option<PathBuf> = std::env::var_os("COVE_WEB").map(PathBuf::from);
    let mut token_file: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--listen" => listen = it.next().cloned().unwrap_or(listen),
            "--web-dir" => web = it.next().map(PathBuf::from),
            "--token-file" => token_file = it.next().map(PathBuf::from),
            "-h" | "--help" => {
                println!("cove serve [--listen 127.0.0.1:8788] [--web-dir DIR] [--token-file FILE]");
                return 0;
            }
            other => {
                eprintln!("serve: unknown argument {other}");
                return 2;
            }
        }
    }
    let web = web.unwrap_or_else(default_web_dir);
    if !web.join("index.html").is_file() {
        eprintln!("serve: no UI at {} (index.html) — pass --web-dir", web.display());
        return 1;
    }
    let token = match load_token(token_file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("serve: token: {e}");
            return 1;
        }
    };
    if let Ok(seed) = std::env::var("COVE_SEED") {
        if let Err(e) = crate::host::seed_settings(&seed) {
            eprintln!("serve: {e}");
        }
    }
    let server = match Server::http(&listen) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("serve: cannot listen on {listen}: {e}");
            return 1;
        }
    };
    println!("cove serving {} on http://{listen}/", web.display());
    println!("sign in: http://{listen}/login?token=<the token in ~/.config/ndexr/cove-token>");
    let trusted: Vec<String> = std::env::var("COVE_TRUST_EMAILS")
        .unwrap_or_default()
        .split(',')
        .map(|e| e.trim().to_lowercase())
        .filter(|e| e.contains('@'))
        .collect();
    if !trusted.is_empty() {
        println!("trusting the gate's X-Auth-Email for: {}", trusted.join(", "));
    }
    crate::runner::set_serving();
    let trust_loopback = std::env::var("COVE_TRUST_LOOPBACK").as_deref() == Ok("1");
    if trust_loopback {
        println!("trusting every loopback request (a cell: COVE_TRUST_LOOPBACK=1)");
    }
    // Stopped (a session ended from multiuser's /admin, systemd): the kernels
    // go with it rather than running on as orphans.
    on_term(crate::kernel::shutdown_all);
    let _ = LISTEN_PORT.set(server.server_addr().to_ip().map_or(0, |a| a.port()));
    let ctx = Arc::new(Ctx { token, trusted, trust_loopback, web });
    keep_current();
    for req in server.incoming_requests() {
        let ctx = ctx.clone();
        std::thread::spawn(move || handle(&ctx, req));
    }
    0
}

/// Where the UI ships, beside this binary: <prefix>/share/cove/web
/// (/usr in the .deb, ~/.local from install.sh), or Contents/Resources/web in
/// the macOS app bundle.
pub fn default_web_dir() -> PathBuf {
    let exe = std::env::current_exe().ok();
    let up = exe.as_ref().and_then(|e| e.parent()?.parent().map(Path::to_path_buf));
    [up.as_ref().map(|p| p.join("share/cove/web")), up.as_ref().map(|p| p.join("Resources/web"))]
        .into_iter()
        .flatten()
        .find(|d| d.join("index.html").is_file())
        .unwrap_or_else(|| PathBuf::from("/usr/share/cove/web"))
}

fn load_token(file: Option<PathBuf>) -> Result<String, String> {
    if let Ok(t) = std::env::var("COVE_TOKEN") {
        if t.len() >= 16 {
            return Ok(t);
        }
    }
    let f = file.unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into())).join(".config/ndexr/cove-token")
    });
    if let Ok(t) = std::fs::read_to_string(&f) {
        let t = t.trim().to_owned();
        if t.len() >= 16 {
            return Ok(t);
        }
    }
    let mut b = [0u8; 24];
    std::fs::File::open("/dev/urandom").and_then(|mut r| r.read_exact(&mut b)).map_err(|e| e.to_string())?;
    let t: String = b.iter().map(|x| format!("{x:02x}")).collect();
    std::fs::create_dir_all(f.parent().unwrap_or(Path::new("/"))).map_err(|e| e.to_string())?;
    std::fs::write(&f, format!("{t}\n")).map_err(|e| e.to_string())?;
    use std::os::unix::fs::PermissionsExt;
    let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600));
    println!("made a new token in {}", f.display());
    Ok(t)
}

/// Constant-time, so the comparison leaks nothing about the token.
fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes()).unwrap()
}

/// This machine is the platform (its checkout and .env are here). A cell's
/// server is not.
fn has_platform() -> bool {
    crate::host::platform_root().join(".env").is_file()
}

/// The person's ndx_token, as the cell's gate let it through.
fn ndx_token(req: &Request) -> Option<String> {
    req.headers().iter().filter(|h| h.field.equiv("Cookie")).find_map(|h| {
        h.value.as_str().split(';').find_map(|c| c.trim().strip_prefix("ndx_token=").map(str::to_owned)).filter(|t| !t.is_empty())
    })
}

/// The same request, answered by the person's session on the platform.
fn from_platform(req: &Request, make: impl Fn(&str) -> ehttp::Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let Some(token) = ndx_token(req) else {
        return text(401, "this needs ndexr's platform, and the request carried no ndexr sign-in - sign in again");
    };
    match crate::account::platform_request(&token, make) {
        Ok(r) => {
            let ct = r.content_type().unwrap_or("application/json").to_owned();
            Response::from_data(r.bytes).with_status_code(r.status).with_header(header("Content-Type", &ct)).with_header(header("Cache-Control", "no-store"))
        }
        Err(e) => text(502, &e),
    }
}

fn json<T: serde::Serialize>(status: u16, v: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(serde_json::to_vec(v).unwrap_or_default())
        .with_status_code(status)
        .with_header(header("Content-Type", "application/json"))
        .with_header(header("Cache-Control", "no-store"))
}

fn text(status: u16, s: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(s.as_bytes().to_vec()).with_status_code(status)
}

fn query_param(url: &str, key: &str) -> Option<String> {
    let q = url.split_once('?')?.1;
    q.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == key).then(|| v.to_owned())
    })
}

fn authed(ctx: &Ctx, req: &Request) -> bool {
    let loopback = req.remote_addr().is_some_and(|a| a.ip().is_loopback());
    if loopback && ctx.trust_loopback {
        return true;
    }
    if loopback && !ctx.trusted.is_empty() {
        let vouched = req
            .headers()
            .iter()
            .find(|h| h.field.equiv("X-Auth-Email"))
            .map(|h| h.value.as_str().trim().to_lowercase())
            .is_some_and(|e| ctx.trusted.iter().any(|t| *t == e));
        if vouched {
            return true;
        }
    }
    req.headers().iter().filter(|h| h.field.equiv("Cookie")).any(|h| {
        h.value.as_str().split(';').any(|c| {
            c.trim().split_once('=').is_some_and(|(k, v)| k == COOKIE && same(v, &ctx.token))
        })
    })
}

const LOGIN_PAGE: &str = r#"<!doctype html><meta charset="utf-8"><title>Cove · sign in</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>body{margin:0;min-height:100vh;display:grid;place-items:center;background:#0f1220;color:#f3eee4;font-family:system-ui,sans-serif}
form{background:#1c1f2b;border:1px solid #2b2f3d;border-radius:12px;padding:28px;width:min(92vw,380px)}
input{width:100%;box-sizing:border-box;padding:10px;margin:12px 0;border-radius:8px;border:1px solid #2b2f3d;background:#0f1220;color:inherit}
button{padding:10px 16px;border:0;border-radius:8px;background:#6a96c4;color:#0f1220;font-weight:600}</style>
<form action="/login"><b>Cove</b><p>Paste the token from <code>~/.config/ndexr/cove-token</code> on this server.</p>
<input name="token" type="password" autofocus><button>Sign in</button></form>"#;

fn handle(ctx: &Ctx, mut req: Request) {
    let url = req.url().to_owned();
    let path = url.split('?').next().unwrap_or("/").to_owned();
    let method = req.method().clone();

    if path == "/login" {
        let resp = match query_param(&url, "token") {
            Some(t) if same(&t, &ctx.token) => text(302, "")
                .with_header(header("Set-Cookie", &format!("{COOKIE}={t}; Path=/; HttpOnly; SameSite=Strict; Max-Age=2592000")))
                .with_header(header("Location", "/")),
            _ => Response::from_data(LOGIN_PAGE.as_bytes().to_vec())
                .with_status_code(if url.contains("token=") { 401 } else { 200 })
                .with_header(header("Content-Type", "text/html; charset=utf-8")),
        };
        let _ = req.respond(resp);
        return;
    }
    if !authed(ctx, &req) {
        let resp = if path.starts_with("/api/") {
            text(401, "sign in at /login")
        } else {
            text(302, "").with_header(header("Location", "/login"))
        };
        let _ = req.respond(resp);
        return;
    }

    // Transfers (the Files pane's Download / Upload) move whole files, so they
    // are answered before the 8 MB /api/q body cap below.
    if method == Method::Get && path == "/download" {
        let resp = match query_param(&url, "path").and_then(|p| url_decode(&p)) {
            None => text(400, "no path"),
            Some(p) => match crate::host::download(&p) {
                Ok((name, bytes)) => Response::from_data(bytes)
                    .with_header(header("Content-Type", "application/octet-stream"))
                    .with_header(header("Content-Disposition", &format!("attachment; filename=\"{}\"", name.replace(['"', '\\', '\r', '\n'], "_"))))
                    .with_header(header("Cache-Control", "no-store")),
                Err(e) => text(404, &e),
            },
        };
        let _ = req.respond(resp);
        return;
    }
    // The Files tab's in-place view of an image, PDF, video or sound: served
    // inline as its own type. SVG can carry script, so it is sandboxed into an
    // origin of its own; the rest may load nothing but themselves.
    if method == Method::Get && path == "/preview" {
        let resp = match query_param(&url, "path").and_then(|p| url_decode(&p)) {
            None => text(400, "no path"),
            Some(p) => match crate::host::preview(&p) {
                Ok((ct, bytes)) => {
                    let csp = if ct == "image/svg+xml" { "sandbox; default-src 'none'; style-src 'unsafe-inline'; img-src data:" } else { "default-src 'none'; img-src 'self'; media-src 'self'; object-src 'self'; style-src 'unsafe-inline'" };
                    let r = Response::from_data(bytes)
                        .with_header(header("Content-Type", ct))
                        .with_header(header("Content-Disposition", "inline"))
                        .with_header(header("X-Content-Type-Options", "nosniff"))
                        .with_header(header("Cache-Control", "no-store"));
                    // A PDF draws in the browser's own viewer, which a CSP sandbox blocks.
                    if ct == "application/pdf" { r } else { r.with_header(header("Content-Security-Policy", csp)) }
                }
                Err(e) => text(404, &e),
            },
        };
        let _ = req.respond(resp);
        return;
    }
    if method == Method::Post && path == "/upload" {
        // A custom header no other site's page can send without a CORS
        // preflight (which this server never grants): a form on another
        // origin cannot write files here with the person's cookie.
        let ours = req.headers().iter().any(|h| h.field.equiv("X-Cove") && h.value.as_str() == "1");
        let dir = query_param(&url, "dir").and_then(|p| url_decode(&p));
        let name = query_param(&url, "name").and_then(|p| url_decode(&p));
        let resp = match (ours, dir, name) {
            (false, ..) => text(403, "uploads come from Cove"),
            (true, Some(d), Some(n)) => match crate::host::upload(&d, &n, req.as_reader()) {
                Ok(to) => json(200, &serde_json::json!({ "path": to })),
                Err(e) => text(400, &e),
            },
            _ => text(400, "upload needs ?dir= and ?name="),
        };
        let _ = req.respond(resp);
        return;
    }

    if method == Method::Get && path == "/software.json" {
        let resp = software_response(&req);
        let _ = req.respond(resp);
        return;
    }
    // Speech for an agent box (host.rs transcribe): the recording's data URL.
    // Only Cove's own page sends X-Cove, as for /upload.
    if method == Method::Post && path == "/transcribe" {
        let ours = req.headers().iter().any(|h| h.field.equiv("X-Cove") && h.value.as_str() == "1");
        let mut body = String::new();
        let _ = req.as_reader().take(40 << 20).read_to_string(&mut body);
        let resp = match ours {
            false => text(403, "recordings come from Cove"),
            true => match crate::host::transcribe(&body) {
                Ok(t) => json(200, &serde_json::json!({ "text": t })),
                Err(e) => text(500, &e),
            },
        };
        let _ = req.respond(resp);
        return;
    }
    // The Viewer: a served folder, or a local server (before the body cap -
    // a proxied request carries its own body, and a websocket none).
    if let Some(rest) = path.strip_prefix("/port/") {
        proxy_port(req, rest, &url);
        return;
    }
    if method == Method::Get && path.starts_with("/site/") {
        let resp = match path["/site/".len()..].split_once('/').and_then(|(id, rel)| Some((id, url_decode(rel)?))) {
            Some((id, rel)) => match crate::host::site_file(id, &rel) {
                Some((ct, bytes)) => Response::from_data(bytes).with_header(header("Content-Type", ct)).with_header(header("Cache-Control", "no-store")),
                None => text(404, "not in a folder the Viewer shows - render it again"),
            },
            None => text(400, "no page"),
        };
        let _ = req.respond(resp);
        return;
    }
    // A cell has no build database: the build views come from the platform,
    // as the person who asked (their ndx_token, through the cell's gate).
    if method == Method::Get && (path == "/builds.json" || path == "/buildlog.json") && !has_platform() {
        let resp = from_platform(&req, |base| {
            let mut r = ehttp::Request::get(format!("{base}{url}"));
            r.headers.insert("Accept", "application/json");
            r
        });
        let _ = req.respond(resp);
        return;
    }
    if method == Method::Get && path == "/builds.json" {
        let _ = req.respond(builds_response());
        return;
    }
    if method == Method::Get && path == "/buildlog.json" {
        let _ = req.respond(build_log_response(&url));
        return;
    }

    let mut body = Vec::new();
    let _ = req.as_reader().take(40 << 20).read_to_end(&mut body);
    let resp = match (method, path.as_str()) {
        (Method::Post, "/api/q") => match serde_json::from_slice::<Query>(&body) {
            // What only the platform answers, from a cell: asked there as the
            // person, so a desktop app from before 0.1.9 works on a cell too
            // (request #187: Messages said "no build database here").
            Ok(q) if q.platform_only() && !has_platform() => from_platform(&req, |base| {
                let mut r = ehttp::Request::post(format!("{base}/api/q"), body.clone());
                r.headers.insert("Content-Type", "application/json");
                r
            }),
            Ok(q) => match crate::host::answer(&q) {
                Ok(v) => json(200, &v),
                Err(e) => text(500, &e),
            },
            Err(e) => text(400, &e.to_string()),
        },
        (Method::Get, "/view") => view_page(&url),
        (Method::Get, p) => static_file(&ctx.web, p),
        _ => text(405, "method not allowed"),
    };
    let _ = req.respond(resp);
}

/// Where the public view's slides live: showcase.json and the pictures it
/// names. The operator's to edit; nothing in the repo writes it.
pub fn showcase_dir() -> PathBuf {
    std::env::var_os("COVE_SHOWCASE_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/srv/ndexr/cove/showcase"))
}

/// The public view (operator, 2026-10-17: "make this possible for people
/// without signing in, they cant like do stuff but we should make sure the
/// data about the builds and such is all available to the public for
/// viewing"). Everything under /view/ is read-only and the same for everyone:
/// the UI's own files, the catalog, the builds, the build logs (masked as for
/// a signed-in person), every domain anyone may visit, and the showcase. Nothing here runs code, opens a
/// session or names a person; anything else under /view/ is 404. The json and
/// the pictures are CORS-open, so a session's front page - on another origin
/// in a cell - can show the same slides.
// Cove's licence and the notices of every crate it is built from. Public at
// /view/third-party-notices: whoever has a copy of Cove may read what it owes,
// signed in or not (onprem/compliance.py checks it is served).
fn notices_response() -> Response<std::io::Cursor<Vec<u8>>> {
    const NOTICES: &str = include_str!("../THIRD-PARTY-NOTICES.txt");
    text(200, NOTICES).with_header(header("Content-Type", "text/plain; charset=utf-8"))
}

pub fn public_response(req: &Request, web: &Path, url: &str) -> Option<Response<std::io::Cursor<Vec<u8>>>> {
    let path = url.split('?').next().unwrap_or("/");
    if path == "/view" {
        return Some(Response::from_data(Vec::new()).with_status_code(302).with_header(header("Location", "/view/")));
    }
    let tail = path.strip_prefix("/view/")?;
    if *req.method() != Method::Get && *req.method() != Method::Head {
        return Some(text(405, "the public view is read-only"));
    }
    let open = |r: Response<std::io::Cursor<Vec<u8>>>| r.with_header(header("Access-Control-Allow-Origin", "*"));
    Some(match tail {
        "third-party-notices" => open(notices_response()),
        "software.json" => open(software_response(req)),
        "builds.json" => open(builds_response()),
        "kernels.json" => open(kernels_response()),
        // An env set's settings for the tour (host.rs env_tour_facts).
        "env.json" => {
            let set = url.split_once("set=").map(|(_, s)| s.split('&').next().unwrap_or("")).unwrap_or("");
            open(match crate::host::env_tour_facts(set) {
                Ok(lines) => Response::from_data(serde_json::to_vec(&serde_json::json!({ "set": set, "lines": lines })).unwrap_or_default())
                    .with_header(header("Content-Type", "application/json"))
                    .with_header(header("Cache-Control", "public, max-age=300")),
                Err(e) => text(404, &e),
            })
        }
        // What each env set's module loads, from its easyconfig (host.rs env_uses).
        "envdeps.json" => {
            let q = url.split_once("m=").map(|(_, s)| s.split('&').next().unwrap_or("")).unwrap_or("");
            let q = q.replace("%2F", "/").replace("%2f", "/").replace("%2C", ",").replace("%2c", ",");
            let mods: Vec<&str> = q.split(',').filter(|m| !m.is_empty()).collect();
            open(Response::from_data(serde_json::to_vec(&crate::host::env_uses(&mods)).unwrap_or_default())
                .with_header(header("Content-Type", "application/json"))
                .with_header(header("Cache-Control", "public, max-age=300")))
        }
        // The kit's sets and combinations (host.rs env_sets); `with=` the
        // sets whose packages to list.
        "envsets.json" => {
            let q = url.split_once("with=").map(|(_, s)| s.split('&').next().unwrap_or("")).unwrap_or("");
            let q = q.replace("%2F", "/").replace("%2f", "/").replace("%2C", ",").replace("%2c", ",");
            let with: Vec<&str> = q.split(',').filter(|m| !m.is_empty()).take(16).collect();
            open(Response::from_data(serde_json::to_vec(&crate::host::env_sets(&with)).unwrap_or_default())
                .with_header(header("Content-Type", "application/json"))
                .with_header(header("Cache-Control", "public, max-age=300")))
        }
        // Every patched module and the patches its install recorded (patches.rs).
        "patches.json" => open(Response::from_data(serde_json::to_vec(&crate::patches::patched()).unwrap_or_default())
            .with_header(header("Content-Type", "application/json"))
            .with_header(header("Cache-Control", "public, max-age=600"))),
        // What a module's installs were built from (patches.rs record): the
        // upstream sources and checksums, recipe, patches, build log - each
        // with its public copy. The same files are public in the stack's
        // records/, so this says nothing a reader could not fetch.
        "record.json" => {
            let name = query_param(url, "name").and_then(|v| url_decode(&v)).unwrap_or_default();
            open(Response::from_data(serde_json::to_vec(&crate::patches::record(&name)).unwrap_or_default())
                .with_header(header("Content-Type", "application/json"))
                .with_header(header("Cache-Control", "public, max-age=600")))
        }
        // One applied patch's text, to read and compare in the atlas.
        "patch.txt" => {
            let q = |k: &str| query_param(url, k).and_then(|v| url_decode(&v)).unwrap_or_default();
            open(match crate::patches::patch_text(&q("name"), &q("version"), &q("file")) {
                Some(t) => Response::from_data(t.into_bytes())
                    .with_header(header("Content-Type", "text/plain; charset=utf-8"))
                    .with_header(header("Cache-Control", "public, max-age=3600")),
                None => text(404, "no such patch in this machine's stack"),
            })
        }
        // How each installed package's machine code came (host.rs provenance).
        "provenance.json" => open(Response::from_data(serde_json::to_vec(&crate::host::provenance()).unwrap_or_default())
            .with_header(header("Content-Type", "application/json"))
            .with_header(header("Cache-Control", "public, max-age=600"))),
        "buildlog.json" => open(build_log_response(url)),
        // Not signed in, whoever asks: public sites only (host.rs visible_domains).
        "domains.json" => open(match crate::host::visible_domains(None) {
            Ok(b) => Response::from_data(b).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-cache")),
            Err(e) => text(503, &e),
        }),
        "showcase.json" => open(static_file(&showcase_dir(), "showcase.json").with_header(header("Cache-Control", "no-cache"))),
        t if t.starts_with("showcase/") => open(static_file(&showcase_dir(), &t["showcase/".len()..]).with_header(header("Cache-Control", "public, max-age=86400"))),
        // Only the UI's own files: never api/q, upload, port/ or a session.
        t if t.is_empty() || !t.contains('/') && [".html", ".js", ".wasm", ".png", ".svg", ".webp", ".ico", ".webmanifest", ".css"].iter().any(|x| t.ends_with(x)) => static_file(web, t),
        _ => text(404, "not part of the public view"),
    })
}

/// The kernel floors each built stack stands on (host.rs kernel_floors).
pub fn kernels_response() -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(crate::host::kernel_floors()).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-cache"))
}

/// The Builds views' live half (host.rs builds_live), the same for everyone.
pub fn builds_response() -> Response<std::io::Cursor<Vec<u8>>> {
    match crate::host::builds_live() {
        Ok(b) => Response::from_data(b).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-store")),
        Err(e) => text(503, &e),
    }
}

/// The Builds › Logs view (host.rs build_log), the same for everyone.
pub fn build_log_response(url: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let key = query_param(url, "key");
    let from = query_param(url, "from").and_then(|f| f.parse().ok());
    match crate::host::build_log(key.as_deref(), from) {
        Ok(b) => Response::from_data(b).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-store")),
        Err(e) => text(503, &e),
    }
}

/// The Software tab's catalog, as `ndexr aggregate`'s `software` spec wrote
/// it (catalog.json, .gz and .etag). The same for everyone, so `cove
/// multiuser` answers it itself rather than through a session. The ETag is
/// the catalog's own hash: a browser keeps its copy and asks again with
/// If-None-Match, so a reload costs a 304 until the catalog changes.
pub fn software_response(req: &Request) -> Response<std::io::Cursor<Vec<u8>>> {
    let dir = crate::host::software_dir();
    let etag = std::fs::read_to_string(dir.join("catalog.etag")).map(|s| format!("\"{}\"", s.trim())).unwrap_or_default();
    let asked = |name: &'static str| req.headers().iter().find(|h| h.field.equiv(name)).map(|h| h.value.as_str().to_owned()).unwrap_or_default();
    if !etag.is_empty() && asked("If-None-Match").split(',').any(|t| t.trim() == etag) {
        return Response::from_data(Vec::new()).with_status_code(304).with_header(header("ETag", &etag)).with_header(header("Cache-Control", "no-cache"));
    }
    let gzip = asked("Accept-Encoding").contains("gzip");
    let file = dir.join(if gzip { "catalog.json.gz" } else { "catalog.json" });
    match std::fs::read(&file) {
        Ok(bytes) => {
            let mut r = Response::from_data(bytes)
                .with_header(header("Content-Type", "application/json"))
                .with_header(header("Cache-Control", "no-cache"));
            if gzip {
                r = r.with_header(header("Content-Encoding", "gzip"));
            }
            if !etag.is_empty() {
                r = r.with_header(header("ETag", &etag));
            }
            r
        }
        // Not the ndexr host (a cell, a desktop's own server): the public
        // catalog, cached, as a desktop on its own computer reads it
        // (host.rs software_catalog). A cell answered 404 here and its
        // Software tab stayed empty (2026-11-02).
        Err(_) => match crate::host::software_catalog() {
            Ok(bytes) => Response::from_data(bytes).with_header(header("Content-Type", "application/json")).with_header(header("Cache-Control", "no-cache")),
            Err(e) => text(503, &e),
        },
    }
}

/// SIGTERM / SIGINT: run `f` once (end the kernels, the sessions), then exit.
/// The handler only sets a flag; a thread does the work, where it may.
pub fn on_term(f: impl FnOnce() + Send + 'static) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static HIT: AtomicBool = AtomicBool::new(false);
    extern "C" fn hit(_: libc::c_int) {
        HIT.store(true, Ordering::SeqCst);
    }
    unsafe {
        libc::signal(libc::SIGTERM, hit as *const () as libc::sighandler_t);
        libc::signal(libc::SIGINT, hit as *const () as libc::sighandler_t);
    }
    std::thread::spawn(move || {
        while !HIT.load(Ordering::SeqCst) {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        f();
        std::process::exit(0);
    });
}

/// A rendered page, only if a render here wrote it. Sandboxed: its scripts
/// run (plotly, htmlwidgets) in an origin of their own, so a document cannot
/// call /api/q with the IDE's cookie.
fn view_page(url: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let Some(path) = query_param(url, "path").and_then(|p| url_decode(&p)) else { return text(400, "no path") };
    match crate::host::rendered_page(&path) {
        Some(page) => Response::from_data(page)
            .with_header(header("Content-Type", "text/html; charset=utf-8"))
            .with_header(header("Content-Security-Policy", "sandbox allow-scripts allow-popups allow-downloads"))
            .with_header(header("Cache-Control", "no-store")),
        None => text(404, "not a page this server rendered - render it again"),
    }
}

// ── the Viewer's way to local servers: /port/<n>/… ─────────────────────────
// RStudio Server's /p/<port>/ (ServerSessionProxy.cpp) and Positron's
// /proxy/<port>/: whatever a person starts on 127.0.0.1 (quarto preview,
// shiny::runApp, mkdocs serve, a plumber API) shows in the Viewer through the
// same sign-in as the IDE. It runs as them, so it reaches nothing they could
// not reach from their own terminal. The IDE's own cookie never goes on.

/// The port this server listens on (a websocket's socket is found by it).
pub static LISTEN_PORT: std::sync::OnceLock<u16> = std::sync::OnceLock::new();

/// Headers that belong to one hop, never passed through.
const HOP: [&str; 8] = ["host", "connection", "keep-alive", "content-length", "transfer-encoding", "upgrade", "te", "proxy-connection"];

fn upgrade_asked(req: &Request) -> bool {
    req.headers().iter().any(|h| h.field.equiv("Upgrade") && h.value.as_str().eq_ignore_ascii_case("websocket"))
}

/// The browser's cookies but ours: `cove=` stays here.
fn cookies_but_ours(req: &Request) -> String {
    req.headers()
        .iter()
        .filter(|h| h.field.equiv("Cookie"))
        .flat_map(|h| h.value.as_str().split(';'))
        .map(str::trim)
        .filter(|c| !c.is_empty() && !c.starts_with(&format!("{COOKIE}=")))
        .collect::<Vec<_>>()
        .join("; ")
}

/// The request's head for `127.0.0.1:port`, its headers passed on but the
/// hop-by-hop ones and the cookie (`cookie` replaces it, or it is the
/// browser's own without ours).
pub fn request_head(req: &Request, port: u16, url: &str, cookie: Option<String>, body_len: Option<usize>, upgrade: bool) -> String {
    let mut head = format!("{} {url} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n", req.method().as_str());
    for h in req.headers() {
        let k = h.field.as_str().as_str().to_ascii_lowercase();
        if HOP.contains(&k.as_str()) || k == "cookie" || k == "x-auth-email" {
            continue;
        }
        head.push_str(&format!("{}: {}\r\n", h.field.as_str(), h.value.as_str()));
    }
    let cookie = cookie.unwrap_or_else(|| cookies_but_ours(req));
    if !cookie.is_empty() {
        head.push_str(&format!("Cookie: {cookie}\r\n"));
    }
    if upgrade {
        head.push_str("Connection: Upgrade\r\nUpgrade: websocket\r\n");
    } else {
        head.push_str("Connection: close\r\n");
    }
    if let Some(n) = body_len {
        head.push_str(&format!("Content-Length: {n}\r\n"));
    }
    head.push_str("\r\n");
    head
}

/// `/port/<n>/<rest>`: the request to 127.0.0.1:<n>, the answer back.
fn proxy_port(mut req: Request, rest: &str, url: &str) {
    let (port, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let Ok(port) = port.parse::<u16>() else {
        let _ = req.respond(text(400, "not a port"));
        return;
    };
    if port < 1024 {
        let _ = req.respond(text(403, "the Viewer shows servers on ports 1024 and up"));
        return;
    }
    if tail.is_empty() && !rest.ends_with('/') {
        // Its folder's slash, so the page's relative addresses stay under /port/<n>/.
        let q = url.split_once('?').map(|(_, q)| format!("?{q}")).unwrap_or_default();
        let _ = req.respond(text(302, "").with_header(header("Location", &format!("{port}/{q}"))));
        return;
    }
    let q = url.split_once('?').map(|(_, q)| format!("?{q}")).unwrap_or_default();
    let upstream = format!("/{tail}{q}");
    if upgrade_asked(&req) {
        tunnel(req, port, &upstream, None);
        return;
    }
    let mut body = Vec::new();
    let _ = req.as_reader().take(64 << 20).read_to_end(&mut body);
    let resp = match exchange(&req, port, &upstream, &body) {
        Ok((status, headers, payload)) => {
            let mut r = Response::from_data(payload).with_status_code(status);
            // Depth of this page under /port/<n>/, to turn the server's own
            // addresses into ones relative to here.
            let up = "../".repeat(tail.matches('/').count());
            for (k, v) in headers {
                let v = if k.eq_ignore_ascii_case("location") { relocate(&v, port, &up) } else { v };
                r = r.with_header(header(&k, &v));
            }
            r
        }
        Err(e) => text(502, &format!("nothing answering on 127.0.0.1:{port}: {e}")),
    };
    let _ = req.respond(resp);
}

/// A redirect to the server's own address (absolute, or from its root) made
/// relative to the page that asked, so it stays inside /port/<n>/.
fn relocate(loc: &str, port: u16, up: &str) -> String {
    let own = ["http://127.0.0.1:", "http://localhost:", "http://0.0.0.0:"].iter().find_map(|h| loc.strip_prefix(h)).and_then(|r| r.strip_prefix(&port.to_string()));
    match own {
        Some(path) => format!("{}{}", if up.is_empty() { "./" } else { up }, path.trim_start_matches('/')),
        None if loc.starts_with('/') && !loc.starts_with("//") => format!("{}{}", if up.is_empty() { "./" } else { up }, loc.trim_start_matches('/')),
        None => loc.to_owned(),
    }
}

/// One request to 127.0.0.1:<port> and its whole answer: (status, the
/// headers to pass on, the body un-chunked).
fn exchange(req: &Request, port: u16, url: &str, body: &[u8]) -> std::io::Result<(u16, Vec<(String, String)>, Vec<u8>)> {
    use std::io::Write;
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port))?;
    s.set_read_timeout(Some(std::time::Duration::from_secs(300)))?;
    s.write_all(request_head(req, port, url, None, Some(body.len()), false).as_bytes())?;
    s.write_all(body)?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw)?;
    let split = raw.windows(4).position(|w| w == b"\r\n\r\n").ok_or_else(|| std::io::Error::other("no answer"))?;
    let head = String::from_utf8_lossy(&raw[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|c| c.parse().ok()).unwrap_or(502);
    let mut chunked = false;
    let mut headers = Vec::new();
    for l in lines {
        let Some((k, v)) = l.split_once(':') else { continue };
        let (k, v) = (k.trim(), v.trim());
        let lk = k.to_ascii_lowercase();
        if lk == "transfer-encoding" {
            chunked = v.to_ascii_lowercase().contains("chunked");
        }
        if !HOP.contains(&lk.as_str()) {
            headers.push((k.to_owned(), v.to_owned()));
        }
    }
    let payload = &raw[split + 4..];
    Ok((status, headers, if chunked { dechunk(payload) } else { payload.to_vec() }))
}

/// A chunked HTTP/1.1 body, decoded: `<hex size>[;ext]\r\n<data>\r\n` … `0\r\n\r\n`.
pub fn dechunk(mut b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    while let Some(eol) = b.windows(2).position(|w| w == b"\r\n") {
        let size_field = String::from_utf8_lossy(&b[..eol]);
        let Ok(n) = usize::from_str_radix(size_field.split(';').next().unwrap_or("").trim(), 16) else { break };
        let start = eol + 2;
        if n == 0 || start + n > b.len() {
            break;
        }
        out.extend_from_slice(&b[start..start + n]);
        b = b.get(start + n + 2..).unwrap_or(&[]);
    }
    out
}

/// A websocket through to 127.0.0.1:<port> (`cookie` replaces the browser's,
/// as multiuser does for a session). The handshake goes up; a 101 comes back
/// through tiny_http's upgrade; then the bytes run both ways until either
/// side closes. tiny_http hands an upgraded connection back as one boxed
/// stream that cannot be read and written from two threads, so the
/// connection's socket is found by its two ends (`socket_of`) and the bytes
/// are copied on that.
pub fn tunnel(req: Request, port: u16, url: &str, cookie: Option<String>) {
    use std::io::Write;
    let peer = req.remote_addr().copied();
    let mut up = match std::net::TcpStream::connect(("127.0.0.1", port)) {
        Ok(s) => s,
        Err(e) => {
            let _ = req.respond(text(502, &format!("nothing answering on 127.0.0.1:{port}: {e}")));
            return;
        }
    };
    if up.write_all(request_head(&req, port, url, cookie, None, true).as_bytes()).is_err() {
        let _ = req.respond(text(502, "the server did not take the handshake"));
        return;
    }
    // The answer's head, a byte at a time so nothing after it is taken.
    let mut head = Vec::new();
    let mut b = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && head.len() < 64 << 10 {
        match up.read(&mut b) {
            Ok(1) => head.push(b[0]),
            _ => break,
        }
    }
    let text_head = String::from_utf8_lossy(&head).into_owned();
    let mut lines = text_head.split("\r\n");
    let status: u16 = lines.next().and_then(|l| l.split_whitespace().nth(1)).and_then(|c| c.parse().ok()).unwrap_or(502);
    let headers: Vec<(String, String)> = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
        .filter(|(k, _)| !HOP.contains(&k.to_ascii_lowercase().as_str()))
        .collect();
    if status != 101 {
        let mut rest = Vec::new();
        let _ = up.read_to_end(&mut rest);
        let mut r = Response::from_data(rest).with_status_code(status);
        for (k, v) in headers {
            r = r.with_header(header(&k, &v));
        }
        let _ = req.respond(r);
        return;
    }
    let mut r = Response::from_data(Vec::new()).with_status_code(101);
    for (k, v) in headers {
        r = r.with_header(header(&k, &v));
    }
    let held = req.upgrade("websocket", r);
    let Some(client) = peer.and_then(|p| socket_of(p, *LISTEN_PORT.get().unwrap_or(&0))) else {
        eprintln!("viewer: websocket to :{port} - its connection was not found");
        return;
    };
    let (Ok(mut c_read), Ok(mut u_write)) = (client.try_clone(), up.try_clone()) else { return };
    let down = std::thread::spawn(move || {
        let _ = std::io::copy(&mut up, &mut &client);
        let _ = client.shutdown(std::net::Shutdown::Both);
        let _ = up.shutdown(std::net::Shutdown::Both);
    });
    let _ = std::io::copy(&mut c_read, &mut u_write);
    let _ = u_write.shutdown(std::net::Shutdown::Both);
    let _ = c_read.shutdown(std::net::Shutdown::Both);
    let _ = down.join();
    // tiny_http reads the connection again once this is dropped: it is shut by now.
    drop(held);
}

/// The open socket of this process whose far end is `peer` and near end the
/// port this server listens on, as a stream of its own (a duplicate).
fn socket_of(peer: std::net::SocketAddr, local_port: u16) -> Option<std::net::TcpStream> {
    use std::os::fd::FromRawFd;
    let addr_of = |fd: i32, f: unsafe extern "C" fn(i32, *mut libc::sockaddr, *mut libc::socklen_t) -> i32| -> Option<std::net::SocketAddr> {
        let mut ss: libc::sockaddr_storage = unsafe { std::mem::zeroed() };
        let mut len = std::mem::size_of::<libc::sockaddr_storage>() as libc::socklen_t;
        if unsafe { f(fd, &mut ss as *mut _ as *mut libc::sockaddr, &mut len) } != 0 {
            return None;
        }
        match ss.ss_family as i32 {
            libc::AF_INET => {
                let a: &libc::sockaddr_in = unsafe { &*(&ss as *const _ as *const libc::sockaddr_in) };
                Some(std::net::SocketAddr::from((u32::from_be(a.sin_addr.s_addr).to_be_bytes(), u16::from_be(a.sin_port))))
            }
            libc::AF_INET6 => {
                let a: &libc::sockaddr_in6 = unsafe { &*(&ss as *const _ as *const libc::sockaddr_in6) };
                Some(std::net::SocketAddr::from((a.sin6_addr.s6_addr, u16::from_be(a.sin6_port))))
            }
            _ => None,
        }
    };
    let max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) }.clamp(256, 65536) as i32;
    (3..max).find_map(|fd| {
        if addr_of(fd, libc::getpeername)? != peer || addr_of(fd, libc::getsockname)?.port() != local_port {
            return None;
        }
        let dup = unsafe { libc::dup(fd) };
        (dup >= 0).then(|| unsafe { std::net::TcpStream::from_raw_fd(dup) })
    })
}

fn url_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => {
                let h = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
                out.push(u8::from_str_radix(h, 16).ok()?);
                i += 3;
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

pub fn static_file(root: &Path, p: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let rel = p.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    // No climbing out of the web dir.
    if rel.split('/').any(|seg| seg == ".." || seg.starts_with('.')) {
        return text(404, "not found");
    }
    let f = root.join(rel);
    let Ok(data) = std::fs::read(&f) else { return text(404, "not found") };
    let ct = match f.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript",
        Some("wasm") => "application/wasm",
        Some("css") => "text/css",
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("ico") => "image/x-icon",
        Some("webmanifest") => "application/manifest+json",
        _ => "application/octet-stream",
    };
    // The page names its UI with the build's own version, so a release is new
    // URLs and the browser cannot keep running the last one: the same
    // ./cove.js and ./cove_bg.wasm every release, sent with no caching
    // headers, left a person on the old UI after updating (operator,
    // 2026-10-20: the Containers mode "i didnt see it when the app restarted",
    // while the session served it). The page itself is never cached.
    if rel == "index.html" {
        let v = std::fs::metadata(root.join("cove_bg.wasm")).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        let page = String::from_utf8_lossy(&data).replace("\"./cove.js\"", &format!("\"./cove.js?v={v}\"")).replace("\"./cove_bg.wasm\"", &format!("\"./cove_bg.wasm?v={v}\""));
        return Response::from_data(page.into_bytes()).with_header(header("Content-Type", ct)).with_header(header("Cache-Control", "no-cache"));
    }
    Response::from_data(data).with_header(header("Content-Type", ct))
}

/// A server stays current on its own, the way laptop.svbt's autoupdate timer
/// keeps svbt current, but from inside: every 2 min (COVE_UPDATE_SECS;
/// 0 = never) ask repo.ndexr.io, swap a newer build in place, then exit 75.
/// The kernels go down with it; the browser's next command starts them again. The supervisor
/// starts the new binary: systemd's Restart=on-failure (laptop, the .deb),
/// launchd's KeepAlive (neo). A managed or dev install is never touched.
fn keep_current() {
    let every: u64 = std::env::var("COVE_UPDATE_SECS").ok().and_then(|v| v.parse().ok()).unwrap_or(120);
    if every == 0 {
        return;
    }
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(every));
        match crate::update::swap_if_newer() {
            Ok(Some(v)) => {
                println!("serve: v{v} installed in place; restarting into it");
                std::process::exit(75);
            }
            Ok(None) => {}
            Err(e) => eprintln!("serve: update check: {e}"),
        }
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn view_paths_decode() {
        assert_eq!(super::url_decode("/home/a%20b/r%26d.html").as_deref(), Some("/home/a b/r&d.html"));
        assert_eq!(super::url_decode("%zz"), None);
        assert_eq!(super::url_decode(&crate::backend::url_encode("/x/é ü.html")).as_deref(), Some("/x/é ü.html"));
    }
}
