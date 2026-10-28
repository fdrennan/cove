//! `app serve`: the page, the game module and the score board, on 127.0.0.1
//! only - nginx in front decides who may reach it.
//!
//!   GET  /                    web/index.html
//!   GET  /static/<file>       web/<file>; game.wasm from dist/web/
//!   GET  /health              liveness
//!   GET  /api/scores          the ten best runs on this site
//!   POST /api/scores          {"name","score","pearls","seconds"} -> rank
//!   GET  /api/env             what runs: crate, the module's size and age
//!
//! Files are read from disk on every request, so an edit to web/ or a fresh
//! ./build-web.sh shows on the next reload without a restart.

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

type Reply = Response<Cursor<Vec<u8>>>;

/// Runs the board keeps; a run below the last of them is not kept.
const KEEP: usize = 20;
/// A pearl is worth 20 and a gate 5, so a score past this is not from a run
/// of the game: the board refuses it rather than let one POST own the top.
const MAX_SCORE: u64 = 100_000;
const MAX_BODY: u64 = 4096;
const SCORES: &str = "data/scores.json";

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("header")
}

fn json_reply(code: u16, v: Value) -> Reply {
    Response::from_data(v.to_string().into_bytes())
        .with_status_code(code)
        .with_header(header("Content-Type", "application/json"))
        .with_header(header("Cache-Control", "no-store"))
}

fn bad(msg: impl Into<String>) -> Reply {
    json_reply(400, json!({ "error": msg.into() }))
}

fn file_reply(path: &Path) -> Reply {
    let kind = match path.extension().and_then(|e| e.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        // Served as application/wasm, or instantiateStreaming refuses it.
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    };
    match std::fs::read(path) {
        Ok(body) => Response::from_data(body).with_header(header("Content-Type", kind)).with_header(header("Cache-Control", "no-cache")),
        Err(_) if kind == "application/wasm" => json_reply(404, json!({ "error": "no dist/web/game.wasm: run ./build-web.sh" })),
        Err(_) => json_reply(404, json!({ "error": "not found" })),
    }
}

/// web/<name>, for a plain file name only: no directories, so nothing outside
/// web/ (or dist/web/ for the module) can be named.
fn static_path(name: &str) -> Option<PathBuf> {
    let ok = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) && !name.starts_with('.');
    match name {
        "game.wasm" => Some(PathBuf::from("dist/web/game.wasm")),
        _ if ok => Some(Path::new("web").join(name)),
        _ => None,
    }
}

// ---- the board -------------------------------------------------------------
fn now_s() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn load() -> Vec<Value> {
    let rows: Vec<Value> = std::fs::read_to_string(SCORES).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
    rows.into_iter().filter(|r| r["score"].as_u64().is_some_and(|s| s <= MAX_SCORE) && r["name"].is_string()).take(KEEP).collect()
}

/// Temp file then rename: a crash leaves the old board or the new one.
fn save(rows: &[Value]) {
    let tmp = format!("{SCORES}.{}.tmp", std::process::id());
    let written = std::fs::create_dir_all("data")
        .and_then(|_| std::fs::write(&tmp, serde_json::to_vec_pretty(rows).unwrap_or_default()))
        .and_then(|_| std::fs::rename(&tmp, SCORES));
    if let Err(e) = written {
        eprintln!("scores not written: {e}");
    }
}

/// What a person typed: 1-16 characters, nothing that could be markup.
fn clean_name(v: &Value) -> String {
    let name: String = v.as_str().unwrap_or("").chars().filter(|c| !c.is_control() && !"<>&\"'`".contains(*c)).take(16).collect();
    let name = name.trim();
    if name.is_empty() { "Anonymous jellyfish".into() } else { name.into() }
}

fn record(board: &Mutex<Vec<Value>>, body: &Value) -> Result<Value, String> {
    let score = body["score"].as_u64().filter(|s| *s <= MAX_SCORE).ok_or(format!("score must be a whole number in 0..{MAX_SCORE}"))?;
    let at = now_s();
    let row = json!({
        "name": clean_name(&body["name"]),
        "score": score,
        "pearls": body["pearls"].as_u64().unwrap_or(0),
        "seconds": body["seconds"].as_f64().filter(|s| s.is_finite() && *s >= 0.0).map_or(0, |s| s.round() as u64),
        "at": at,
    });
    let mut rows = board.lock().unwrap_or_else(|e| e.into_inner());
    // Ties keep the earlier run ahead: insert after every row at least as high.
    let rank = rows.iter().take_while(|r| r["score"].as_u64().unwrap_or(0) >= score).count() + 1;
    let kept = rank <= KEEP;
    if kept {
        rows.insert(rank - 1, row);
        rows.truncate(KEEP);
        save(&rows);
    }
    Ok(json!({ "rank": if kept { Some(rank) } else { None }, "kept": kept, "board": rows.iter().take(10).collect::<Vec<_>>() }))
}

fn read_body(req: &mut Request) -> Result<Value, String> {
    let mut s = String::new();
    req.as_reader().take(MAX_BODY + 1).read_to_string(&mut s).map_err(|e| e.to_string())?;
    if s.len() as u64 > MAX_BODY {
        return Err("body over 4 KB".into());
    }
    serde_json::from_str(&s).map_err(|_| "body is not JSON".into())
}

fn environment(board: &Mutex<Vec<Value>>, started: Instant) -> Value {
    let wasm = std::fs::metadata("dist/web/game.wasm").ok();
    let age = wasm.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).map(|d| d.as_secs());
    json!({
        "crate": { "name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION") },
        "server": ["tiny_http 0.12.0", "serde_json 1.0.154"],
        "module": { "file": "dist/web/game.wasm", "bytes": wasm.map(|m| m.len()), "built_s_ago": age, "imports": 0 },
        "stack": std::env::var("LOADEDMODULES").unwrap_or_default().split(':').filter(|m| m.starts_with("Rust/")).collect::<Vec<_>>(),
        "board": { "runs": board.lock().map_or(0, |b| b.len()), "file": SCORES },
        "process": { "pid": std::process::id(), "uptime_s": started.elapsed().as_secs() },
    })
}

fn handle(mut req: Request, board: &Mutex<Vec<Value>>, started: Instant) {
    let url = req.url().to_owned();
    let path = url.split_once('?').map_or(url.as_str(), |(p, _)| p);
    let method = req.method().clone();
    let reply = match (&method, path) {
        (Method::Get, "/" | "/index.html") => file_reply(Path::new("web/index.html")),
        (Method::Get, "/health") => json_reply(200, json!({ "ok": true, "service": "__HOST__", "uptime_s": started.elapsed().as_secs() })),
        (Method::Get, "/api/scores") => json_reply(200, json!(board.lock().map_or(vec![], |b| b.iter().take(10).cloned().collect()))),
        (Method::Post, "/api/scores") => match read_body(&mut req).and_then(|b| record(board, &b)) {
            Ok(v) => json_reply(201, v),
            Err(e) => bad(e),
        },
        (Method::Get, "/api/env") => json_reply(200, environment(board, started)),
        // /static/, not /vendor/ or /images/: nginx's shared static_dirs.conf
        // claims those on every site.
        (Method::Get, p) if p.starts_with("/static/") => match static_path(&p["/static/".len()..]) {
            Some(f) => file_reply(&f),
            None => json_reply(404, json!({ "error": "not found" })),
        },
        _ => json_reply(404, json!({ "error": format!("no route: {method} {path}") })),
    };
    let _ = req.respond(reply);
}

pub fn serve(port: u16) {
    let server = match Server::http(("127.0.0.1", port)) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            eprintln!("cannot listen on 127.0.0.1:{port}: {e}");
            std::process::exit(1);
        }
    };
    let started = Instant::now();
    let board = Arc::new(Mutex::new(load()));
    println!("__HOST__: Jellyfish Drift on http://127.0.0.1:{port} ({} runs on the board)", board.lock().map_or(0, |b| b.len()));
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let (s, board) = (Arc::clone(&server), Arc::clone(&board));
            std::thread::spawn(move || {
                while let Ok(req) = s.recv() {
                    handle(req, &board, started);
                }
            })
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
}
