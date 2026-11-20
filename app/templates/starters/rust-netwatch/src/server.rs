//! `app serve`: Network Observatory on 127.0.0.1 only - nginx in front
//! decides who may reach it.
//!
//!   GET  /                       the page (page.html)
//!   GET  /health                 liveness, uptime
//!   GET  /api/env                what runs: rustc, crates, stack module, start time, CPUs
//!   GET  /api/snapshot?n=180     the last n samples (all kept by default) and the probes
//!   GET  /api/stream             Server-Sent Events: `sample` each second, `probes` beside it
//!   GET  /api/probes             the probes and their statistics
//!   POST /api/probes?target=host:port&every=2     add one
//!   POST /api/probes/delete?id=  remove one
//!   GET  /api/blob?mb=64         that many MiB of zeros: load to watch the counters move

use std::io::{Cursor, Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server, StatusCode};

use crate::probe::{self, Probes};
use crate::sampler::{self, Feed, Shared};

const PAGE: &str = include_str!("page.html");
/// Open event streams at most: each holds one thread.
const MAX_STREAMS: usize = 64;
const MAX_BLOB_MB: u64 = 2048;

type Reply = Response<Cursor<Vec<u8>>>;

struct App {
    shared: Arc<Shared>,
    probes: Arc<Mutex<Probes>>,
    started: Instant,
    started_unix: u64,
    streams: AtomicUsize,
}

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

/// `%3A` -> `:`, `+` -> space: a probe target arrives encodeURIComponent'd.
fn decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' if i + 2 < b.len() => match u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap_or("zz"), 16) {
                Ok(v) => {
                    out.push(v);
                    i += 3;
                    continue;
                }
                Err(_) => out.push(b'%'),
            },
            b'+' => out.push(b' '),
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn params(query: &str) -> Vec<(String, String)> {
    query.split('&').filter_map(|kv| kv.split_once('=')).map(|(k, v)| (decode(k), decode(v))).collect()
}

fn param<T: std::str::FromStr>(q: &[(String, String)], key: &str, default: T) -> Result<T, String> {
    match q.iter().find(|(k, _)| k == key) {
        Some((_, v)) => v.parse().map_err(|_| format!("{key}: not a number: {v}")),
        None => Ok(default),
    }
}

/// Seconds since 1970 as `YYYY-MM-DDTHH:MM:SSZ` (H. Hinnant's days-to-civil).
fn iso(secs: u64) -> String {
    let (days, rem) = ((secs / 86400) as i64, secs % 86400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

fn read_trim(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty())
}

/// What this process is and where it came from, read when asked: the
/// compiler and crate versions build.rs recorded from Cargo.lock, the stack
/// module loaded at build and at run, and the host's CPUs.
fn env_json(app: &App) -> Value {
    let packages: Vec<Value> = env!("NETWATCH_DEPS")
        .split(',')
        .filter_map(|p| p.split_once('='))
        .map(|(n, v)| json!({ "name": n, "version": v }))
        .collect();
    let cpus_host = std::fs::read_to_string("/proc/cpuinfo")
        .map(|s| s.lines().filter(|l| l.starts_with("processor")).count())
        .unwrap_or(0);
    let modules = |s: &str| -> Vec<String> { s.split(':').filter(|m| !m.is_empty()).map(str::to_owned).collect() };
    let rss = std::fs::read("/proc/self/status").map(|b| crate::proc::status_kb(&b, "VmRSS")).unwrap_or(0);
    json!({
        "runtime": "Rust",
        "version": env!("NETWATCH_RUSTC"),
        "target": env!("NETWATCH_TARGET"),
        "profile": env!("NETWATCH_PROFILE"),
        "crate": { "name": env!("CARGO_PKG_NAME"), "version": env!("CARGO_PKG_VERSION") },
        "modules_at_build": modules(env!("NETWATCH_MODULES")),
        "modules_at_run": modules(&std::env::var("LOADEDMODULES").unwrap_or_default()),
        "packages": packages,
        "started": iso(app.started_unix),
        "started_unix": app.started_unix,
        "uptime_s": app.started.elapsed().as_secs(),
        "pid": std::process::id(),
        "cpus_host": cpus_host,
        "cpus_available": std::thread::available_parallelism().map_or(0, |n| n.get()),
        "kernel": read_trim("/proc/sys/kernel/osrelease"),
        "binary_bytes": std::env::current_exe().and_then(std::fs::metadata).map_or(0, |m| m.len()),
        "rss_bytes": rss,
    })
}

/// The last `n` samples (all HISTORY by default) and the probes. A page
/// that cannot hold an event stream open polls this with n=1. The samples
/// are stored serialised, so they are joined as text, not parsed back: a
/// round trip through serde_json Values for 180 samples grew RSS from 6 to
/// 150 MB over six requests (measured 2026-10-26).
fn snapshot(app: &App, n: usize) -> String {
    let probes = app.probes.lock().unwrap_or_else(|e| e.into_inner()).json().to_string();
    let f = app.shared.feed.lock().unwrap_or_else(|e| e.into_inner());
    let skip = f.history.len().saturating_sub(n);
    let size: usize = f.history.iter().skip(skip).map(|s| s.len() + 1).sum();
    let mut out = String::with_capacity(size + probes.len() + 32);
    out.push_str("{\"history\":[");
    for (k, s) in f.history.iter().skip(skip).enumerate() {
        if k > 0 {
            out.push(',');
        }
        out.push_str(s);
    }
    out.push_str("],\"probes\":");
    out.push_str(&probes);
    out.push('}');
    out
}

/// Server-Sent Events, written straight to the socket. tiny_http's chunked
/// body buffers 8 KiB before it sends, which would hold a 2 KB sample for
/// seconds, so the stream takes the raw writer and flushes each event.
fn stream(app: Arc<App>, req: Request) {
    if app.streams.fetch_add(1, Ordering::SeqCst) >= MAX_STREAMS {
        app.streams.fetch_sub(1, Ordering::SeqCst);
        crate::log::warn!("event stream refused: {MAX_STREAMS} already open");
        let _ = req.respond(json_reply(503, json!({ "error": "too many open streams; poll /api/snapshot" })));
        return;
    }
    std::thread::spawn(move || {
        let mut w = req.into_writer();
        let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nCache-Control: no-store\r\n\
                    X-Accel-Buffering: no\r\nConnection: close\r\n\r\nretry: 2000\n\n";
        // From the next sample on: the page seeds itself from /api/snapshot.
        let mut seen = app.shared.feed.lock().unwrap_or_else(|e| e.into_inner()).seq;
        let mut ok = w.write_all(head.as_bytes()).and_then(|_| w.flush()).is_ok();
        while ok {
            let (text, seq) = {
                let f = app.shared.feed.lock().unwrap_or_else(|e| e.into_inner());
                let (f, _) = app
                    .shared
                    .tick
                    .wait_timeout_while(f, Duration::from_secs(15), |f: &mut Feed| f.seq <= seen)
                    .unwrap_or_else(|e| e.into_inner());
                (f.history.back().cloned(), f.seq)
            };
            let msg = match text {
                Some(t) if seq > seen => {
                    seen = seq;
                    let probes = app.probes.lock().unwrap_or_else(|e| e.into_inner()).json();
                    format!("event: sample\ndata: {t}\n\nevent: probes\ndata: {probes}\n\n")
                }
                // Nothing new in 15 s: a comment keeps proxies from closing an idle stream.
                _ => ": keepalive\n\n".to_owned(),
            };
            ok = w.write_all(msg.as_bytes()).and_then(|_| w.flush()).is_ok();
        }
        app.streams.fetch_sub(1, Ordering::SeqCst);
        crate::log::debug!("event stream closed");
    });
}

/// Zeros, `mb` MiB of them, with a Content-Length so the client can time it.
fn blob(q: &[(String, String)]) -> Result<Response<std::io::Take<std::io::Repeat>>, String> {
    let mb: u64 = param(q, "mb", 64)?;
    if !(1..=MAX_BLOB_MB).contains(&mb) {
        return Err(format!("mb is 1..{MAX_BLOB_MB}"));
    }
    let n = mb << 20;
    Ok(Response::new(
        StatusCode(200),
        vec![header("Content-Type", "application/octet-stream"), header("Cache-Control", "no-store")],
        std::io::repeat(0).take(n),
        Some(n as usize),
        None,
    ))
}

fn handle(app: &Arc<App>, req: Request) {
    let url = req.url().to_owned();
    let t0 = Instant::now();
    let method = req.method().clone();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let q = params(query);
    // Behind a prefix (a cell's /proxy/<port>/) the page asks beside itself,
    // so routes match on the path's tail. Longer routes first.
    let route = ["/health", "/api/env", "/api/snapshot", "/api/stream", "/api/probes/delete", "/api/probes", "/api/blob"]
        .into_iter()
        .find(|r| path.ends_with(r))
        .unwrap_or(if path.ends_with('/') || path.ends_with("/index.html") { "/" } else { "" });
    let reply = match (req.method(), route) {
        (Method::Get, "/api/stream") => {
            crate::log::info!("{method} {path}: event stream opened");
            return stream(Arc::clone(app), req);
        }
        (Method::Get, "/api/blob") => {
            let (status, sent) = match blob(&q) {
                Ok(r) => (200, req.respond(r)),
                Err(e) => (400, req.respond(bad(e))),
            };
            if let Err(e) = sent {
                crate::log::warn!("{method} {path}: download cut short: {e}");
            }
            crate::log::request(&method, path, status, t0);
            return;
        }
        (Method::Get, "/") => Response::from_data(PAGE.as_bytes().to_vec())
            .with_header(header("Content-Type", "text/html; charset=utf-8")),
        (Method::Get, "/health") => json_reply(200, json!({
            "ok": true, "service": "__HOST__", "version": env!("CARGO_PKG_VERSION"),
            "uptime_s": app.started.elapsed().as_secs(),
        })),
        (Method::Get, "/api/env") => json_reply(200, env_json(app)),
        (Method::Get, "/api/snapshot") => match param(&q, "n", sampler::HISTORY) {
            Ok(n) => Response::from_data(snapshot(app, n).into_bytes())
                .with_header(header("Content-Type", "application/json"))
                .with_header(header("Cache-Control", "no-store")),
            Err(e) => bad(e),
        },
        (Method::Get, "/api/probes") => json_reply(200, app.probes.lock().unwrap_or_else(|e| e.into_inner()).json()),
        (Method::Post, "/api/probes") => {
            let target = q.iter().find(|(k, _)| k == "target").map(|(_, v)| v.clone()).unwrap_or_default();
            // A name resolves before the probe list is locked: a slow DNS
            // answer must not stall the probes already running.
            match param(&q, "every", 2.0f64).and_then(|every| probe::resolve(&target).map(|addr| (addr, every))) {
                Ok((addr, every)) => app.probes.lock().unwrap_or_else(|e| e.into_inner()).add(&target, addr, every)
                    .map_or_else(bad, |v| json_reply(200, v)),
                Err(e) => bad(e),
            }
        }
        (Method::Post | Method::Delete, "/api/probes/delete") => match param(&q, "id", 0u32) {
            Ok(id) if app.probes.lock().unwrap_or_else(|e| e.into_inner()).remove(id) => json_reply(200, json!({ "removed": id })),
            Ok(id) => json_reply(404, json!({ "error": format!("no probe {id}") })),
            Err(e) => bad(e),
        },
        (_, "") => json_reply(404, json!({ "error": format!("no route: {path}") })),
        (m, r) => json_reply(405, json!({ "error": format!("{r} does not take {m}") })),
    };
    let status = reply.status_code().0;
    if let Err(e) = req.respond(reply) {
        crate::log::error!("{method} {path}: response not sent: {e}");
    }
    if route == "/api/snapshot" && status < 400 {
        crate::log::debug!("{method} {path} {status} {} ms", t0.elapsed().as_millis());
    } else {
        crate::log::request(&method, path, status, t0);
    }
}

pub fn serve(port: u16) {
    let server = match Server::http(("127.0.0.1", port)) {
        Ok(s) => Arc::new(s),
        Err(e) => {
            crate::log::error!("cannot listen on 127.0.0.1:{port}: {e}");
            std::process::exit(1);
        }
    };
    let started = Instant::now();
    let app = Arc::new(App {
        shared: Arc::new(Shared { feed: Mutex::new(Feed::default()), tick: Default::default() }),
        probes: Arc::new(Mutex::new(Probes::default())),
        started,
        started_unix: SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        streams: AtomicUsize::new(0),
    });
    // The page opens with one probe that needs no network: this server's own
    // port over loopback, the floor every other probe is measured against.
    let own = format!("127.0.0.1:{port}");
    if let Ok(addr) = probe::resolve(&own) {
        let _ = app.probes.lock().unwrap_or_else(|e| e.into_inner()).add(&own, addr, 2.0);
    }
    {
        let shared = Arc::clone(&app.shared);
        std::thread::spawn(move || sampler::run(shared, started));
        let probes = Arc::clone(&app.probes);
        std::thread::spawn(move || probe::run(probes));
    }
    // Workers answer the short requests; a stream moves to its own thread, a
    // blob holds its worker for as long as it downloads.
    let workers = 6;
    crate::log::info!("__HOST__: Network Observatory serving on 127.0.0.1:{port}");
    let handles: Vec<_> = (0..workers)
        .map(|_| {
            let (s, app) = (Arc::clone(&server), Arc::clone(&app));
            std::thread::spawn(move || {
                while let Ok(req) = s.recv() {
                    handle(&app, req);
                }
            })
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_targets() {
        assert_eq!(decode("example.org%3A443"), "example.org:443");
        assert_eq!(decode("%5B%3A%3A1%5D%3A22"), "[::1]:22");
        assert_eq!(decode("a%2"), "a%2");
        assert_eq!(decode("100%"), "100%");
    }

    #[test]
    fn iso_dates() {
        assert_eq!(iso(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(iso(1_792_972_800), "2026-10-26T00:00:00Z");
    }
}
