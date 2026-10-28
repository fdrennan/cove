//! `app serve`: the site. On 127.0.0.1 only - nginx in front decides who may
//! reach it (the domain's own sign-in or site password), as with Cove.
//!
//!   POST /api/q              a Query (api.rs) as JSON -> its answer as JSON
//!   POST /api/upload?name=x  the body saved as media/x
//!   GET  /files/<media|out>/x  a file, to play or download
//!   GET  /...                the browser UI (dist/web, from build-web.sh)

use std::io::Read;
use std::path::Path;

use tiny_http::{Header, Method, Request, Response, Server};

use crate::api::{safe_name, Query};
use crate::media;

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("header")
}

fn json(code: u16, v: serde_json::Value) -> Response<std::io::Cursor<Vec<u8>>> {
    Response::from_data(v.to_string().into_bytes()).with_status_code(code).with_header(header("Content-Type", "application/json"))
}

fn kind(path: &str) -> &'static str {
    match crate::api::ext(path).as_str() {
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        "wasm" => "application/wasm",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "json" | "webmanifest" => "application/json",
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "wav" => "audio/wav",
        "mp3" => "audio/mpeg",
        "m4a" | "aac" => "audio/mp4",
        "ogg" | "opus" => "audio/ogg",
        "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}

fn answer(q: Query) -> Result<serde_json::Value, String> {
    match q {
        Query::Atlas => serde_json::to_value(crate::catalog::atlas()?),
        Query::State => serde_json::to_value(media::state()),
        Query::Run { op, file, start, end } => serde_json::to_value(media::run(op, &file, &start, &end)?),
    }
    .map_err(|e| e.to_string())
}

fn file(req: Request, path: &Path) {
    let r = match std::fs::File::open(path) {
        Ok(f) => req.respond(Response::from_file(f).with_header(header("Content-Type", kind(&path.to_string_lossy())))),
        Err(_) => req.respond(Response::from_string("not found").with_status_code(404)),
    };
    let _ = r;
}

fn handle(mut req: Request, web: &Path) {
    let url = req.url().to_owned();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    // Behind a prefix (a cell's /proxy/<port>/) the page asks beside itself.
    let path = path.rsplit_once("/api/").map(|(_, p)| format!("/api/{p}")).unwrap_or_else(|| path.to_owned());
    match (req.method().clone(), path.as_str()) {
        // Answers once the app is serving (app.env APP_HEALTH; up.sh waits on it).
        (Method::Get, "/health") => {
            let _ = req.respond(json(200, serde_json::json!({ "ok": true })));
        }
        (Method::Post, "/api/q") => {
            let mut body = String::new();
            let r = req.as_reader().read_to_string(&mut body).map_err(|e| e.to_string());
            let out = r.and_then(|_| serde_json::from_str::<Query>(&body).map_err(|e| format!("bad query: {e}"))).and_then(answer);
            let _ = match out {
                Ok(v) => req.respond(json(200, v)),
                Err(e) => req.respond(json(400, serde_json::json!({ "error": e }))),
            };
        }
        (Method::Post, "/api/upload") => {
            let name = query.split('&').find_map(|kv| kv.strip_prefix("name=")).map(decode).and_then(|n| safe_name(&n));
            let Some(name) = name else {
                let _ = req.respond(json(400, serde_json::json!({ "error": "upload a video or audio file" })));
                return;
            };
            let to = Path::new("media").join(&name);
            let saved = std::fs::create_dir_all("media")
                .and_then(|_| std::fs::File::create(&to))
                .and_then(|mut f| std::io::copy(req.as_reader(), &mut f));
            let _ = match saved {
                Ok(_) => req.respond(json(200, serde_json::json!(format!("media/{name}")))),
                Err(e) => req.respond(json(500, serde_json::json!({ "error": format!("{name}: {e}") }))),
            };
        }
        (Method::Get, p) if p.contains("/files/") => {
            let rel = decode(p.rsplit_once("/files/").map(|x| x.1).unwrap_or(""));
            match media::checked(&rel) {
                Ok(f) => file(req, &f),
                Err(e) => {
                    let _ = req.respond(Response::from_string(e).with_status_code(404));
                }
            }
        }
        (Method::Get, p) => {
            let name = p.rsplit('/').next().filter(|n| !n.is_empty() && !n.starts_with('.')).unwrap_or("index.html");
            if name == "index.html" && !web.join(name).is_file() {
                let page = "<!doctype html><meta charset=utf-8><title>not built</title><body style='font:16px system-ui;padding:2em'>\
                            The server is running, but the browser UI is not built yet: run <code>./build-web.sh</code> (or <code>./run.sh</code>) in this domain's folder.";
                let _ = req.respond(Response::from_string(page).with_header(header("Content-Type", "text/html; charset=utf-8")));
                return;
            }
            file(req, &web.join(name));
        }
        _ => {
            let _ = req.respond(Response::from_string("method not allowed").with_status_code(405));
        }
    }
}

/// %xx and + in a query or path, back to text.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match ((b[i + 1] as char).to_digit(16), (b[i + 2] as char).to_digit(16)) {
                (Some(h), Some(l)) => {
                    out.push((h * 16 + l) as u8);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn serve(port: u16) {
    let web = std::env::var("APP_WEB").unwrap_or_else(|_| "dist/web".into());
    let server = Server::http(("127.0.0.1", port)).unwrap_or_else(|e| panic!("127.0.0.1:{port}: {e}"));
    eprintln!("serving on http://127.0.0.1:{port}/ (UI from {web}, media in ./media, results in ./out)");
    for req in server.incoming_requests() {
        let web = Path::new(&web).to_path_buf();
        std::thread::spawn(move || handle(req, &web));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn decodes_names() {
        assert_eq!(super::decode("My%20Song+2.wav"), "My Song 2.wav");
        assert_eq!(super::decode("bad%zz"), "bad%zz");
        assert_eq!(super::decode("end%2"), "end%2");
    }
}
