//! The one boundary between the UI and a machine.
//!
//!   Local  the desktop app: queries answered by host.rs on a thread. Native only.
//!   Http   the UI talking to `cove serve` — in a browser (the WASM
//!          build), or a desktop app pointed at a server. Queries POST to /api/q.
//!   Cell   a desktop app signed in, on the person's cell (account.rs): Http
//!          plus the ndx_token session the cell's gate wants. Native only.
//!
//! Code runs only as R or Python handed to a kernel (`Query::Eval`, kernel.rs);
//! there is no shell-command query.
//!
//! The UI never touches a process, a file or the network itself, which is what
//! lets the same UI code compile to WebAssembly.

use eframe::egui;
use serde::de::DeserializeOwned;

use crate::api::Query;

/// The token of the `cove serve` this desktop window talks to over
/// `Backend::Http` (`--connect` with `--token`, or the person's own server),
/// sent on every request as `Authorization: Bearer` (server.rs `authed`). A
/// window has one backend for its whole life, so this is set once at start.
/// Without it `--connect` could reach only a server that trusts loopback - the
/// native client sent no cookie and no header (2026-11-16, the first step to
/// running Cove on a person's own server).
#[cfg(not(target_arch = "wasm32"))]
static SERVER_TOKEN: std::sync::OnceLock<String> = std::sync::OnceLock::new();

#[cfg(not(target_arch = "wasm32"))]
pub fn set_server_token(token: &str) {
    let t = token.trim();
    if !t.is_empty() {
        let _ = SERVER_TOKEN.set(t.to_owned());
    }
}

/// `req` with this window's server token, when there is one. A browser's
/// page needs none: its cookie goes with it.
pub fn with_token(req: ehttp::Request) -> ehttp::Request {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(t) = SERVER_TOKEN.get() {
        let mut req = req;
        req.headers.insert("Authorization", format!("Bearer {t}"));
        return req;
    }
    req
}

#[derive(Clone)]
pub enum Backend {
    #[cfg(not(target_arch = "wasm32"))]
    Local,
    Http { base: String },
    #[cfg(not(target_arch = "wasm32"))]
    Cell { base: String, session: crate::host::account::Session },
}

impl Backend {
    /// Same machine as the UI: terminals, folder pickers and self-update make sense.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))] // native only (host.rs / app.rs)
    pub fn is_local(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        if matches!(self, Backend::Local) {
            return true;
        }
        false
    }

    /// Where the Viewer loads a Viewer address from (host.rs `view_address`,
    /// api.rs `port_address`): through the server (/site/, /port/), or, in the
    /// desktop app on its own machine, the file or the local server itself.
    /// A web address elsewhere is itself.
    pub fn viewer_url(&self, addr: &str) -> String {
        if !addr.starts_with('/') {
            return addr.to_owned();
        }
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Local => {
                if let Some(rest) = addr.strip_prefix("/port/") {
                    return format!("http://127.0.0.1:{rest}");
                }
                let file = addr.strip_prefix("/site/").and_then(|r| r.split_once('/')).and_then(|(id, rel)| crate::host::site_path(id, &url_decode(rel)));
                file.map_or_else(|| addr.to_owned(), |p| format!("file://{}", p.display()))
            }
            Backend::Http { base } => format!("{base}{addr}"),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { base, .. } => format!("{base}{addr}"),
        }
    }

    /// Where the browser downloads `path` from, packed as `format` ("zip",
    /// "tar.gz", or "" for a file as it is and a folder as .tar.gz): the
    /// server's /download, which the person's sign-in rides along to.
    pub fn download_url(&self, path: &str, format: &str) -> String {
        let q = if format.is_empty() { String::new() } else { format!("&format={}", url_encode(format)) };
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Local => format!("file://{path}"),
            Backend::Http { base } => format!("{base}/download?path={}{q}", url_encode(path)),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { base, .. } => format!("{base}/download?path={}{q}", url_encode(path)),
        }
    }

    /// Where an image, PDF, video or sound is shown in place (the server's
    /// /preview, inline as its own type).
    pub fn preview_url(&self, path: &str) -> String {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Local => format!("file://{path}"),
            Backend::Http { base } => format!("{base}/preview?path={}", url_encode(path)),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { base, .. } => format!("{base}/preview?path={}", url_encode(path)),
        }
    }

    /// `path` from the machine as (file name, bytes), packed as `format`
    /// (download_url) - a folder comes as <name>.tar.gz by default. The desktop
    /// app saves them where the person picks.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn download(&self, path: &str, format: &str, cb: impl FnOnce(Result<(String, Vec<u8>), String>) + Send + 'static) {
        let (path, format) = (path.to_owned(), format.to_owned());
        match self {
            Backend::Local => {
                std::thread::spawn(move || cb(crate::host::download(&path, &format)));
            }
            Backend::Http { .. } | Backend::Cell { .. } => {
                let mut req = ehttp::Request::get(self.download_url(&path, &format));
                if let Backend::Cell { session, .. } = self {
                    req.headers.insert("Cookie", format!("ndx_token={}", session.lock().unwrap()));
                }
                ehttp::fetch(with_token(req), move |res| {
                    cb(match res {
                        Ok(r) if r.ok && r.content_type().is_some_and(|t| t.starts_with("text/html")) => Err("signed out - sign in again".into()),
                        Ok(r) if r.ok => {
                            let name = r
                                .headers
                                .get("content-disposition")
                                .and_then(|d| d.split("filename=\"").nth(1))
                                .and_then(|n| n.split('"').next())
                                .map(str::to_owned)
                                .unwrap_or_else(|| path.rsplit('/').next().unwrap_or("download").to_owned());
                            Ok((name, r.bytes))
                        }
                        Ok(r) => Err(http_err(r.status, &r.bytes)),
                        Err(e) => Err(e),
                    })
                });
            }
        }
    }

    /// Put `bytes` in folder `dir` on the machine as `name` (never over an
    /// existing file - host.rs `upload`). `cb` gets the path it landed at.
    pub fn upload(&self, dir: &str, name: &str, bytes: Vec<u8>, ctx: &egui::Context, cb: impl FnOnce(Result<String, String>) + Send + 'static) {
        let ctx = ctx.clone();
        let done = move |r| {
            cb(r);
            ctx.request_repaint();
        };
        let url = |base: &str| format!("{base}/upload?dir={}&name={}", url_encode(dir), url_encode(name));
        let req = match self {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Local => {
                let (dir, name) = (dir.to_owned(), name.to_owned());
                std::thread::spawn(move || done(crate::host::upload(&dir, &name, &bytes[..])));
                return;
            }
            Backend::Http { base } => ehttp::Request::post(url(base), bytes),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { base, session } => {
                let mut r = ehttp::Request::post(url(base), bytes);
                r.headers.insert("Cookie", format!("ndx_token={}", session.lock().unwrap()));
                r
            }
        };
        let mut req = req;
        req.headers.insert("Content-Type", "application/octet-stream");
        // server.rs refuses an upload without it (no other site can send it).
        req.headers.insert("X-Cove", "1");
        #[cfg(target_arch = "wasm32")]
        {
            req.credentials = ehttp::Credentials::SameOrigin;
        }
        ehttp::fetch(with_token(req), move |res| {
            done(match res {
                Ok(r) if r.ok && r.bytes.trim_ascii_start().starts_with(b"<") => Err("signed out - sign in again".into()),
                Ok(r) if r.ok => serde_json::from_slice::<serde_json::Value>(&r.bytes)
                    .ok()
                    .and_then(|v| v["path"].as_str().map(str::to_owned))
                    .ok_or_else(|| "bad reply from the upload".to_owned()),
                Ok(r) => Err(http_err(r.status, &r.bytes)),
                Err(e) => Err(e),
            })
        });
    }

    /// The Software tab's catalog, raw (`/software.json`, software.rs). Asked
    /// once per session; a browser keeps it by its ETag, so a reload costs a 304.
    pub fn software(&self, ctx: &egui::Context, cb: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static) {
        let ctx = ctx.clone();
        let done = move |r| {
            cb(r);
            ctx.request_repaint();
        };
        let mut req = match self {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Local => {
                std::thread::spawn(move || done(crate::host::software_catalog()));
                return;
            }
            Backend::Http { base } => ehttp::Request::get(format!("{base}/software.json")),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { base, session } => {
                let mut r = ehttp::Request::get(format!("{base}/software.json"));
                r.headers.insert("Cookie", format!("ndx_token={}", session.lock().unwrap()));
                r
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            req.credentials = ehttp::Credentials::SameOrigin;
        }
        req.headers.insert("Accept", "application/json");
        ehttp::fetch(with_token(req), move |res| {
            done(match res {
                Ok(r) if r.ok && r.bytes.trim_ascii_start().starts_with(b"<") => Err("signed out - reload the page to sign in".into()),
                Ok(r) if r.ok => Ok(r.bytes),
                Ok(r) => Err(http_err(r.status, &r.bytes)),
                Err(e) => Err(e),
            })
        });
    }

    /// A recording as text (`POST transcribe`, host.rs transcribe_as): the
    /// browser's data URL in, `{"text": …}` back.
    pub fn transcribe(&self, ctx: &egui::Context, data_url: String, cb: impl FnOnce(Result<String, String>) + Send + 'static) {
        let ctx = ctx.clone();
        let done = move |r| {
            cb(r);
            ctx.request_repaint();
        };
        let mut req = match self {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Local => {
                std::thread::spawn(move || done(crate::host::transcribe(&data_url)));
                return;
            }
            Backend::Http { base } => ehttp::Request::post(format!("{base}/transcribe"), data_url.into_bytes()),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { base, session } => {
                let mut r = ehttp::Request::post(format!("{base}/transcribe"), data_url.into_bytes());
                r.headers.insert("Cookie", format!("ndx_token={}", session.lock().unwrap()));
                r
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            req.credentials = ehttp::Credentials::SameOrigin;
        }
        req.headers.insert("Content-Type", "text/plain");
        req.headers.insert("X-Cove", "1");
        ehttp::fetch(with_token(req), move |res| {
            done(match res {
                Ok(r) if r.ok => serde_json::from_slice::<serde_json::Value>(&r.bytes)
                    .ok()
                    .and_then(|v| v["text"].as_str().map(str::to_owned))
                    .ok_or_else(|| "no text came back".to_owned()),
                Ok(r) => Err(http_err(r.status, &r.bytes)),
                Err(e) => Err(e),
            })
        });
    }

    /// The Builds › Logs view (`/buildlog.json`): the machines' logs, and
    /// `key`'s bytes from `from` on (None: its tail). Asked every 2 s while
    /// it shows, so the log follows the build.
    pub fn build_log(&self, ctx: &egui::Context, key: Option<String>, from: Option<u64>, cb: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static) {
        let ctx = ctx.clone();
        let done = move |r| {
            cb(r);
            ctx.request_repaint();
        };
        let q = match (&key, from) {
            (Some(k), Some(f)) => format!("?key={k}&from={f}"),
            (Some(k), None) => format!("?key={k}"),
            (None, _) => String::new(),
        };
        let mut req = match self {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Local => {
                let path = format!("/buildlog.json{q}");
                std::thread::spawn(move || done(local_or_platform(crate::host::build_log(key.as_deref(), from), &path)));
                return;
            }
            Backend::Http { base } => ehttp::Request::get(format!("{base}/buildlog.json{q}")),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { session, .. } => {
                let session = session.clone();
                std::thread::spawn(move || done(platform_file(&session, &format!("/buildlog.json{q}"))));
                return;
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            req.credentials = ehttp::Credentials::SameOrigin;
        }
        req.headers.insert("Accept", "application/json");
        ehttp::fetch(with_token(req), move |res| {
            done(match res {
                Ok(r) if r.ok && r.bytes.trim_ascii_start().starts_with(b"<") => Err("signed out - reload the page to sign in".into()),
                Ok(r) if r.ok => Ok(r.bytes),
                Ok(r) => Err(http_err(r.status, &r.bytes)),
                Err(e) => Err(e),
            })
        });
    }

    /// The Builds views' live half (`/builds.json`: the machines' queues and
    /// the instance prices), asked every half minute while they show.
    pub fn builds(&self, ctx: &egui::Context, cb: impl FnOnce(Result<Vec<u8>, String>) + Send + 'static) {
        let ctx = ctx.clone();
        let done = move |r| {
            cb(r);
            ctx.request_repaint();
        };
        let mut req = match self {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Local => {
                std::thread::spawn(move || done(local_or_platform(crate::host::builds_live(), "/builds.json")));
                return;
            }
            Backend::Http { base } => ehttp::Request::get(format!("{base}/builds.json")),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { session, .. } => {
                let session = session.clone();
                std::thread::spawn(move || done(platform_file(&session, "/builds.json")));
                return;
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            req.credentials = ehttp::Credentials::SameOrigin;
        }
        req.headers.insert("Accept", "application/json");
        ehttp::fetch(with_token(req), move |res| {
            done(match res {
                Ok(r) if r.ok && r.bytes.trim_ascii_start().starts_with(b"<") => Err("signed out - reload the page to sign in".into()),
                Ok(r) if r.ok => Ok(r.bytes),
                Ok(r) => Err(http_err(r.status, &r.bytes)),
                Err(e) => Err(e),
            })
        });
    }

    /// Ask the machine something; `cb` gets the answer on some thread, then
    /// the UI repaints.
    pub fn query<T: DeserializeOwned + 'static>(
        &self,
        q: Query,
        ctx: &egui::Context,
        cb: impl FnOnce(Result<T, String>) + Send + 'static,
    ) {
        let ctx = ctx.clone();
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Local => {
                std::thread::spawn(move || {
                    // Signed out, with this computer's own Claude ready, the
                    // agent runs here on it (localagent.rs).
                    if let Some(r) = crate::host::localagent::answer(&q) {
                        cb(r.and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string())));
                        ctx.request_repaint();
                        return;
                    }
                    // Asked here; what only the platform has goes there, as
                    // the signed-in person. The laptop's own files stay its
                    // own: only a refusal goes, never a query that works here.
                    let r = match crate::host::answer(&q) {
                        Err(e) if needs_platform(e.as_bytes()) => match (crate::host::account::key_session(), &q) {
                            (Some(s), _) => on_platform(&s, serde_json::to_vec(&q).unwrap_or_default()),
                            // Not signed in, a request still goes: anonymously,
                            // without its files (server.rs anon_request_response).
                            (None, Query::SendRequest { what, trying, .. }) => {
                                let what = if trying.trim().is_empty() { what.clone() } else { format!("{what}\n\n{trying}") };
                                anon_request(&what, "", "Cove desktop, not signed in").and_then(|id| serde_json::from_value(serde_json::Value::from(id)).map_err(|e| e.to_string()))
                            }
                            (None, _) => Err(SIGNED_OUT.to_owned()),
                        },
                        r => r.and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string())),
                    };
                    cb(r);
                    ctx.request_repaint();
                });
            }
            Backend::Http { base } => {
                let body = serde_json::to_vec(&q).unwrap_or_default();
                fetch(post(format!("{base}/api/q"), body), move |r| {
                    cb(r);
                    ctx.request_repaint();
                });
            }
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { base, session } => {
                let body = serde_json::to_vec(&q).unwrap_or_default();
                if q.platform_only() {
                    let session = session.clone();
                    std::thread::spawn(move || {
                        cb(on_platform(&session, body));
                        ctx.request_repaint();
                    });
                    return;
                }
                let mut req = post(format!("{base}/api/q"), body.clone());
                req.headers.insert("Cookie", format!("ndx_token={}", session.lock().unwrap()));
                let session = session.clone();
                ehttp::fetch(with_token(req), move |res| {
                    // What the cell cannot do, the platform does for the same
                    // person (account.rs platform_session).
                    let r = match res {
                        Ok(r) if !r.ok && needs_platform(&r.bytes) => on_platform(&session, body),
                        other => decode(other),
                    };
                    cb(r);
                    ctx.request_repaint();
                });
            }
        }
    }
}

/// A cell's Cove server has no platform checkout: Messages, agent runs and the
/// build views answer 500 with one of these (host.rs). Operator screenshot,
/// 2026-11-03: "500 no platform checkout on this machine" for an agent's `ls`,
/// "500 no build database here" in Messages.
/// Measured again 2026-11-02, every read-only query from the laptop, on its
/// own and through its cell: the account and the admin views refuse the
/// same way, worded differently.
#[cfg(not(target_arch = "wasm32"))]
const PLATFORM_ONLY: [&str; 6] = [
    "no platform checkout on this machine",
    "no build database here",
    "no build logs on this machine",
    "needs the platform checked out on this machine",
    "for ndexr admins",
    "managed by admins signed in at cove.ndexr.io",
];

/// A build view's file asked here first; refused (no build database on a
/// laptop), the platform's, as the signed-in person.
#[cfg(not(target_arch = "wasm32"))]
fn local_or_platform(here: Result<Vec<u8>, String>, path: &str) -> Result<Vec<u8>, String> {
    match here {
        Err(e) if needs_platform(e.as_bytes()) => match crate::host::account::key_session() {
            Some(s) => platform_file(&s, path),
            None => Err(SIGNED_OUT.to_owned()),
        },
        r => r,
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn needs_platform(body: &[u8]) -> bool {
    let t = String::from_utf8_lossy(body);
    PLATFORM_ONLY.iter().any(|m| t.contains(m))
}

/// A request to this person's session on the platform's Cove.
#[cfg(not(target_arch = "wasm32"))]
fn on_platform_raw(session: &crate::host::account::Session, make: impl Fn(&str) -> ehttp::Request) -> Result<ehttp::Response, String> {
    let token = session.lock().unwrap().clone();
    crate::host::account::platform_request(&token, make)
}

/// What a query only ndexr answers says on a computer nobody has signed in
/// from: the agent, Messages, the build views. It was the server's own words
/// with "sign in" after them - "this needs the platform checked out on this
/// machine (src/r/host.r)", a red card in Agents on a new laptop (operator's
/// screenshot, 2026-11-14: "installing on laptop without prior ndexr access,
/// looks a little weird"). setup.rs connect_button puts Sign in under it.
pub const SIGNED_OUT: &str = "This runs on ndexr, and this computer is not signed in. Sign in to ndexr to use it - an account is free.";

/// Why POST /view/request said no, from its status alone: nginx's error
/// pages (ndexr_errors.conf) replace the server's own words on a 4xx with
/// the platform's HTML 404 page, measured 2026-11-14.
pub fn anon_refused(status: u16) -> String {
    match status {
        429 => "too many requests without signing in just now - try again in a few minutes, or sign in".into(),
        400 => "say what you need first".into(),
        s => format!("the request could not be filed (http {s}) - try again, or sign in"),
    }
}

/// A request filed with no sign-in at the platform's public view (server.rs
/// anon_request_response): its number, or why not.
#[cfg(not(target_arch = "wasm32"))]
pub fn anon_request(what: &str, contact: &str, page: &str) -> Result<i64, String> {
    let body = serde_json::to_vec(&serde_json::json!({ "what": what, "contact": contact, "page": page })).unwrap_or_default();
    let mut req = ehttp::Request::post(format!("{}/view/request", crate::host::account::cove_base()), body);
    req.headers.insert("Content-Type", "text/plain");
    let r = ehttp::fetch_blocking(&req)?;
    if !r.ok {
        return Err(anon_refused(r.status));
    }
    serde_json::from_slice::<serde_json::Value>(&r.bytes).ok().and_then(|v| v["id"].as_i64()).ok_or_else(|| "no request number came back".into())
}

/// The question again, at this person's session on the platform's Cove.
#[cfg(not(target_arch = "wasm32"))]
fn on_platform<T: DeserializeOwned>(session: &crate::host::account::Session, body: Vec<u8>) -> Result<T, String> {
    decode(on_platform_raw(session, |base| post(format!("{base}/api/q"), body.clone())))
}

/// One of the build views' files, from the platform: a cell has no build
/// database, so they are never asked of it.
#[cfg(not(target_arch = "wasm32"))]
fn platform_file(session: &crate::host::account::Session, path: &str) -> Result<Vec<u8>, String> {
    let r = on_platform_raw(session, |base| {
        let mut r = ehttp::Request::get(format!("{base}{path}"));
        r.headers.insert("Accept", "application/json");
        r
    })?;
    if r.ok && !r.bytes.trim_ascii_start().starts_with(b"<") {
        Ok(r.bytes)
    } else if r.ok {
        Err("signed out - sign in again".into())
    } else {
        Err(http_err(r.status, &r.bytes))
    }
}

fn post(url: String, body: Vec<u8>) -> ehttp::Request {
    let mut r = ehttp::Request::post(url, body);
    r.headers.insert("Content-Type", "application/json");
    // ehttp's browser default is `credentials: "omit"`: no cookie went with
    // any call, so cove.ndexr.io's admin gate 302'd all 447 /api/q POSTs to the
    // console login and every button did nothing (nginx access.log, 2026-10-13).
    #[cfg(target_arch = "wasm32")]
    {
        r.credentials = ehttp::Credentials::SameOrigin;
    }
    r
}

/// One request, its JSON body decoded. The session cookie rides along (same
/// origin, see `post`), so the gate applies to the UI's every call.
fn fetch<T: DeserializeOwned + 'static>(req: ehttp::Request, cb: impl FnOnce(Result<T, String>) + Send + 'static) {
    ehttp::fetch(with_token(req), move |res| cb(decode(res)));
}

/// A reply as the answer it carries, or the words for what went wrong.
fn decode<T: DeserializeOwned>(res: Result<ehttp::Response, String>) -> Result<T, String> {
    {
        match res {
            // A gate redirect lands on the login page: HTML, not an answer.
            Ok(r) if r.ok && r.bytes.trim_ascii_start().starts_with(b"<") => Err("signed out - reload the page to sign in".into()),
            Ok(r) if r.ok => serde_json::from_slice(&r.bytes).map_err(|e| format!("bad reply: {e}")),
            // The session this page belongs to is gone - the IDE server
            // restarted (every deploy does). Say so once and reopen; never
            // pour multiuser's 404 page into a toast (operator screenshot,
            // 2026-10-14: "What is this?").
            Ok(r) if r.status == 404 && String::from_utf8_lossy(&r.bytes).contains("No such session") => {
                SESSION_ENDED.store(true, std::sync::atomic::Ordering::Relaxed);
                Err("this session has ended - reopening".into())
            }
            Ok(r) => Err(http_err(r.status, &r.bytes)),
            Err(e) => Err(e),
        }
    }
}

/// Set once a request finds this page's session gone (app.rs reopens).
static SESSION_ENDED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn session_ended() -> bool {
    SESSION_ENDED.load(std::sync::atomic::Ordering::Relaxed)
}

/// An error body as one readable line: an HTML page reduced to its text (its
/// <main> when it has one), whitespace collapsed, cut to 240 characters.
/// A failed answer as words. 502/503/504 come from nginx while Cove's server
/// restarts after a release, which takes seconds: the agent had shown
/// nginx's page as "502 502 Bad Gateway 502 Bad Gateway nginx" (operator,
/// 2026-10-18, a screenshot captioned "what").
pub fn http_err(status: u16, bytes: &[u8]) -> String {
    match status {
        502..=504 => "Cove is restarting after an update - try again in a few seconds".to_owned(),
        // The page is a newer build than the session answering it: the page
        // asked for something the session's build does not know yet (operator,
        // 2026-10-20, the Logs tab: "400 unknown variant `container_log`").
        400 if readable(bytes).contains("unknown variant") => "this needs the newer Cove - press Update now at the top (your session is on an older build than this page)".to_owned(),
        _ => format!("{status} {}", readable(bytes)),
    }
}

pub fn readable(bytes: &[u8]) -> String {
    let s = String::from_utf8_lossy(bytes);
    let s = s.trim();
    if !s.starts_with('<') {
        return s.chars().take(240).collect();
    }
    let body = s.find("<main").and_then(|i| s[i..].find("</main>").map(|j| &s[i..i + j])).unwrap_or(s);
    let mut text = String::new();
    let mut in_tag = false;
    let mut in_style = false;
    let lower = body.to_ascii_lowercase();
    for (i, c) in body.char_indices() {
        if c == '<' {
            in_tag = true;
            in_style = lower[i..].starts_with("<style") || (in_style && !lower[i..].starts_with("</style"));
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag && !in_style {
            text.push(c);
        }
    }
    text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(240).collect()
}

/// `url_encode` undone (an invalid escape stays as it is).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))] // native only (Backend::Local)
pub fn url_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (b[i], b.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok())) {
            (b'%', Some(v)) => {
                out.push(v);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Percent-encoding for one query value: everything but RFC 3986's unreserved
/// characters.
pub fn url_encode(s: &str) -> String {
    s.bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b"-._~/".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect()
}

#[cfg(test)]
mod readable_tests {
    #[test]
    fn an_html_error_reads_as_its_text() {
        let page = b"<!doctype html><title>not found</title><style>body{margin:0}</style><main><p>No such session of yours - it may have ended. <a href=/>Your sessions</a></p></main>";
        assert_eq!(super::readable(page), "No such session of yours - it may have ended. Your sessions");
        assert_eq!(super::readable(b"  plain words  "), "plain words");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn query_values_are_encoded() {
        assert_eq!(super::url_encode("/home/a b/r&d.html"), "/home/a%20b/r%26d.html");
    }
}
