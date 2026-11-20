//! `app serve`: the simulation on 127.0.0.1 only - nginx in front decides who
//! may reach it.
//!
//!   GET  /                 the page (page.html)
//!   GET  /health           liveness, uptime, threads
//!   POST /reset?preset=galaxies|disc|collapse|kepler&n=&seed=&soft=
//!   GET  /frame?steps=&g=&soft=&dt=     advance, then the frame as Float32 LE
//!   GET  /state?steps=&g=&soft=&dt=     advance, then the same header as JSON
//!   POST /drop?x=&y=&mass=              a heavy body at rest at (x, y)
//!
//! The universe advances only when a frame is asked for, so an idle app costs
//! nothing. Every page shares the one universe.

use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::sim::{Params, Preset, Sim, N_MAX};

const PAGE: &str = include_str!("page.html");
/// Floats ahead of the positions in a /frame body; page.html reads the same.
const HEADER: usize = 16;
/// Pair interactions one request may compute: the cap that keeps a frame
/// under a second at 20 000 bodies.
const WORK_MAX: f64 = 4e9;

type Reply = Response<Cursor<Vec<u8>>>;

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

/// `key=value` pairs of a query string; values here are numbers and preset
/// names, so no decoding.
fn params(query: &str) -> Vec<(&str, &str)> {
    query.split('&').filter_map(|kv| kv.split_once('=')).collect()
}

fn param<T: std::str::FromStr>(q: &[(&str, &str)], key: &str, default: T) -> Result<T, String> {
    match q.iter().find(|(k, _)| *k == key) {
        Some((_, v)) => v.parse().map_err(|_| format!("{key}: not a number: {v}")),
        None => Ok(default),
    }
}

fn in_range(key: &str, v: f64, lo: f64, hi: f64) -> Result<f64, String> {
    if v.is_finite() && v >= lo && v <= hi {
        Ok(v)
    } else {
        Err(format!("{key} must be in [{lo}, {hi}], got {v}"))
    }
}

fn sim_params(q: &[(&str, &str)]) -> Result<Params, String> {
    Ok(Params {
        g: in_range("g", param(q, "g", 1.0)?, 0.01, 20.0)?,
        soft: in_range("soft", param(q, "soft", 0.02)?, 1e-4, 0.5)?,
        dt: in_range("dt", param(q, "dt", 0.002)?, 1e-5, 0.05)?,
    })
}

/// What one frame reports: the clock, the energy against its reference, and
/// the speed of the force loop.
struct Frame {
    head: [f64; HEADER],
}

/// Advances `steps` and measures it. Steps are cut so one request stays
/// within WORK_MAX pair interactions.
fn advance(sim: &mut Sim, q: &[(&str, &str)], threads: usize) -> Result<Frame, String> {
    let p = sim_params(q)?;
    let asked: usize = param(q, "steps", 4)?;
    let n = sim.n() as f64;
    let steps = asked.min(200).min((WORK_MAX / (n * n)).max(1.0) as usize);
    let t = Instant::now();
    sim.step(p, steps);
    let secs = t.elapsed().as_secs_f64().max(1e-9);
    let (ke, pe) = sim.energy(p.g, p.soft);
    let e0 = sim.e0.unwrap_or(ke + pe);
    let per_s = |work: f64| if steps > 0 { work * steps as f64 / secs } else { 0.0 };
    Ok(Frame {
        head: [
            n,
            sim.n_heavy() as f64,
            sim.t,
            sim.steps as f64,
            ke + pe,
            e0,
            ke,
            pe,
            if steps > 0 { secs * 1e3 / steps as f64 } else { 0.0 },
            per_s(n),     // bodies x steps per second
            per_s(n * n), // pair interactions per second
            threads as f64,
            sim.epoch as f64,
            steps as f64,
            p.dt,
            0.0,
        ],
    })
}

fn frame_json(sim: &Sim, f: &Frame) -> Value {
    let h = &f.head;
    json!({
        "preset": sim.preset.name(),
        "n": h[0], "n_heavy": h[1], "t": h[2], "steps_total": h[3],
        "energy": h[4], "energy0": h[5], "kinetic": h[6], "potential": h[7],
        "drift": (h[4] - h[5]) / h[5].abs().max(1e-300),
        "virial_2k_over_u": 2.0 * h[6] / h[7].abs().max(1e-300),
        "ms_per_step": h[8], "body_steps_per_s": h[9], "interactions_per_s": h[10],
        "threads": h[11], "epoch": h[12], "steps": h[13], "dt": h[14],
        "heavy": (0..sim.n_heavy()).map(|i| json!({ "x": sim.px[i], "y": sim.py[i], "m": sim.m[i] })).collect::<Vec<_>>(),
    })
}

/// The frame body: HEADER floats, then x, y for every body, all Float32
/// little-endian. Heavy bodies come first. 4 000 bodies are 32 KB.
fn frame_bytes(sim: &Sim, f: &Frame) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 * (HEADER + 2 * sim.n()));
    for v in f.head {
        out.extend_from_slice(&(v as f32).to_le_bytes());
    }
    for (x, y) in sim.px.iter().zip(&sim.py) {
        out.extend_from_slice(&(*x as f32).to_le_bytes());
        out.extend_from_slice(&(*y as f32).to_le_bytes());
    }
    out
}

fn reset(sim: &Mutex<Sim>, q: &[(&str, &str)]) -> Result<Value, String> {
    let name = q.iter().find(|(k, _)| *k == "preset").map_or("galaxies", |(_, v)| v);
    let preset = Preset::parse(name).ok_or_else(|| format!("preset is galaxies, disc, collapse or kepler: {name}"))?;
    let n: usize = param(q, "n", 4000)?;
    if !(2..=N_MAX).contains(&n) {
        return Err(format!("n is 2..{N_MAX}"));
    }
    let seed: u64 = param(q, "seed", 7)?;
    let soft = in_range("soft", param(q, "soft", 0.02)?, 1e-4, 0.5)?;
    let t = Instant::now();
    let fresh = Sim::new(preset, n, seed, soft);
    let mut s = sim.lock().unwrap_or_else(|e| e.into_inner());
    let epoch = s.epoch + 1;
    *s = fresh;
    s.epoch = epoch;
    crate::log::info!("universe {epoch}: {} with {} bodies, seed {seed}, built in {} ms", preset.name(), s.n(), t.elapsed().as_millis());
    Ok(json!({ "preset": preset.name(), "n": s.n(), "n_heavy": s.n_heavy(), "seed": seed,
               "epoch": epoch, "elapsed_us": t.elapsed().as_micros() as u64 }))
}

fn drop_body(sim: &Mutex<Sim>, q: &[(&str, &str)]) -> Result<Value, String> {
    let x = in_range("x", param(q, "x", 0.0)?, -1e3, 1e3)?;
    let y = in_range("y", param(q, "y", 0.0)?, -1e3, 1e3)?;
    let mass = in_range("mass", param(q, "mass", 0.5)?, crate::sim::HEAVY, 10.0)?;
    let mut s = sim.lock().unwrap_or_else(|e| e.into_inner());
    if s.n() >= N_MAX {
        return Err(format!("already {N_MAX} bodies"));
    }
    s.drop_body(x, y, mass);
    Ok(json!({ "n": s.n(), "n_heavy": s.n_heavy(), "epoch": s.epoch }))
}

fn handle(req: Request, sim: &Mutex<Sim>, started: Instant, threads: usize) {
    let url = req.url().to_owned();
    let t0 = Instant::now();
    let method = req.method().clone();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let q = params(query);
    // Behind a prefix (a cell's /proxy/<port>/) the page asks beside itself,
    // so routes match on the path's tail.
    let route = ["/health", "/reset", "/frame", "/state", "/drop"]
        .into_iter()
        .find(|r| path.ends_with(r))
        .unwrap_or(if path.ends_with('/') || path.ends_with("/index.html") { "/" } else { "" });
    let reply = match (req.method(), route) {
        (Method::Get, "/") => Response::from_data(PAGE.as_bytes().to_vec()).with_header(header("Content-Type", "text/html; charset=utf-8")),
        (Method::Get, "/health") => json_reply(
            200,
            json!({ "ok": true, "service": "__HOST__", "version": env!("CARGO_PKG_VERSION"),
                    "uptime_s": started.elapsed().as_secs(), "threads": threads }),
        ),
        (Method::Get | Method::Post, "/reset") => reset(sim, &q).map_or_else(bad, |v| json_reply(200, v)),
        (Method::Get | Method::Post, "/drop") => drop_body(sim, &q).map_or_else(bad, |v| json_reply(200, v)),
        (Method::Get, "/frame" | "/state") => {
            let mut s = sim.lock().unwrap_or_else(|e| e.into_inner());
            match advance(&mut s, &q, threads) {
                Ok(f) if route == "/state" => json_reply(200, frame_json(&s, &f)),
                Ok(f) => Response::from_data(frame_bytes(&s, &f))
                    .with_header(header("Content-Type", "application/octet-stream"))
                    .with_header(header("Cache-Control", "no-store")),
                Err(e) => bad(e),
            }
        }
        (_, "") => json_reply(404, json!({ "error": format!("no route: {path}") })),
        (m, r) => json_reply(405, json!({ "error": format!("{r} does not take {m}") })),
    };
    let status = reply.status_code().0;
    if let Err(e) = req.respond(reply) {
        crate::log::error!("{method} {path}: response not sent: {e}");
    }
    if route == "/frame" && status < 400 {
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
    // The force loop's pool: every core rayon sees.
    let threads = rayon::current_num_threads();
    let sim = Arc::new(Mutex::new(Sim::new(Preset::Galaxies, 4000, 7, 0.02)));
    // Few HTTP workers: frames serialise on the one universe anyway, and the
    // cores belong to the force loop.
    let workers = 4;
    crate::log::info!("__HOST__: Particle Universe serving on 127.0.0.1:{port} ({threads} compute threads)");
    let handles: Vec<_> = (0..workers)
        .map(|_| {
            let (s, sim) = (Arc::clone(&server), Arc::clone(&sim));
            std::thread::spawn(move || {
                while let Ok(req) = s.recv() {
                    handle(req, &sim, started, threads);
                }
            })
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
}
