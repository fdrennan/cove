//! __HOST__: `app serve <port>` serves on 127.0.0.1. run.sh starts it again when
//! ./build.sh makes a new binary; files in static/ are read on each request.

mod log;

use std::path::{Path, PathBuf};
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tiny_http::{Header, Response, Server};

fn secs(t: SystemTime) -> f64 {
    t.duration_since(UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64())
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().is_dir() {
            walk(&e.path(), out);
        } else {
            out.push(e.path());
        }
    }
}

/// What live.js watches: a restart, or any file served changing.
fn version(started: f64) -> String {
    let mut files = Vec::new();
    for d in ROOTS {
        walk(Path::new(d), &mut files);
    }
    let newest = files.iter().filter_map(|p| p.metadata().ok()?.modified().ok()).map(secs).fold(0.0, f64::max);
    format!("{started}-{}-{newest}", files.len())
}

/// Where a path may be served from, in order.
const ROOTS: &[&str] = &["static", "dist/web"];

fn serve_file(path: &str) -> Option<(Vec<u8>, &'static str)> {
    let rel = if path == "/" { "index.html" } else { path.trim_start_matches('/').trim_start_matches("static/") };
    if rel.split('/').any(|s| s == "..") {
        return None;
    }
    let bytes = ROOTS.iter().find_map(|d| std::fs::read(Path::new(d).join(rel)).ok())?;
    let ty = match Path::new(rel).extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    };
    Some((bytes, ty))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let port = match args.as_slice() {
        [s, p] if s == "serve" => p.parse::<u16>().unwrap_or_else(|_| panic!("not a port: {p}")),
        _ => {
            eprintln!("usage: app serve <port>");
            std::process::exit(2)
        }
    };
    let started = secs(SystemTime::now());
    let server = Server::http(("127.0.0.1", port)).expect("bind");
    log::info!("__HOST__: blank Rust app serving on 127.0.0.1:{port}");
    for req in server.incoming_requests() {
        let t0 = Instant::now();
        let path = req.url().split('?').next().unwrap_or("/").to_owned();
        let (status, body, ty) = match path.as_str() {
            "/health" => (200, br#"{"ok":true}"#.to_vec(), "application/json"),
            "/__live" => (200, format!(r#"{{"version":"{}"}}"#, version(started)).into_bytes(), "application/json"),
            _ => match serve_file(&path) {
                Some((b, t)) => (200, b, t),
                None => (404, br#"{"error":"not found"}"#.to_vec(), "application/json"),
            },
        };
        let mut resp = Response::from_data(body).with_status_code(status);
        for (k, v) in [("Content-Type", ty), ("Cache-Control", "no-cache")] {
            resp.add_header(Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("header"));
        }
        let method = req.method().clone();
        match req.respond(resp) {
            Ok(()) => log::request(&method, &path, status, t0),
            Err(e) => log::error!("{method} {path}: response not sent: {e}"),
        }
    }
}
