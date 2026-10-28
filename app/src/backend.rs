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

#[derive(Clone)]
pub enum Backend {
    #[cfg(not(target_arch = "wasm32"))]
    Local,
    Http { base: String },
    #[cfg(not(target_arch = "wasm32"))]
    Cell { base: String, session: crate::account::Session },
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

    /// Where the browser downloads `path` from (a file, or a folder as .tar.gz):
    /// the server's /download, which the person's sign-in rides along to.
    pub fn download_url(&self, path: &str) -> String {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Local => format!("file://{path}"),
            Backend::Http { base } => format!("{base}/download?path={}", url_encode(path)),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { base, .. } => format!("{base}/download?path={}", url_encode(path)),
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

    /// `path` from the machine as (file name, bytes) - a folder comes as
    /// <name>.tar.gz. The desktop app saves them where the person picks.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn download(&self, path: &str, cb: impl FnOnce(Result<(String, Vec<u8>), String>) + Send + 'static) {
        let path = path.to_owned();
        match self {
            Backend::Local => {
                std::thread::spawn(move || cb(crate::host::download(&path)));
            }
            Backend::Http { .. } | Backend::Cell { .. } => {
                let mut req = ehttp::Request::get(self.download_url(&path));
                if let Backend::Cell { session, .. } = self {
                    req.headers.insert("Cookie", format!("ndx_token={}", session.lock().unwrap()));
                }
                ehttp::fetch(req, move |res| {
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
        ehttp::fetch(req, move |res| {
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
        ehttp::fetch(req, move |res| {
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
        ehttp::fetch(req, move |res| {
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
                std::thread::spawn(move || done(crate::host::build_log(key.as_deref(), from)));
                return;
            }
            Backend::Http { base } => ehttp::Request::get(format!("{base}/buildlog.json{q}")),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { base, session } => {
                let mut r = ehttp::Request::get(format!("{base}/buildlog.json{q}"));
                r.headers.insert("Cookie", format!("ndx_token={}", session.lock().unwrap()));
                r
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            req.credentials = ehttp::Credentials::SameOrigin;
        }
        req.headers.insert("Accept", "application/json");
        ehttp::fetch(req, move |res| {
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
                std::thread::spawn(move || done(crate::host::builds_live()));
                return;
            }
            Backend::Http { base } => ehttp::Request::get(format!("{base}/builds.json")),
            #[cfg(not(target_arch = "wasm32"))]
            Backend::Cell { base, session } => {
                let mut r = ehttp::Request::get(format!("{base}/builds.json"));
                r.headers.insert("Cookie", format!("ndx_token={}", session.lock().unwrap()));
                r
            }
        };
        #[cfg(target_arch = "wasm32")]
        {
            req.credentials = ehttp::Credentials::SameOrigin;
        }
        req.headers.insert("Accept", "application/json");
        ehttp::fetch(req, move |res| {
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
                    cb(crate::host::answer(&q).and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string())));
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
                let mut req = post(format!("{base}/api/q"), serde_json::to_vec(&q).unwrap_or_default());
                req.headers.insert("Cookie", format!("ndx_token={}", session.lock().unwrap()));
                fetch(req, move |r| {
                    cb(r);
                    ctx.request_repaint();
                });
            }
        }
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
    ehttp::fetch(req, move |res| {
        cb(match res {
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
        })
    });
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
