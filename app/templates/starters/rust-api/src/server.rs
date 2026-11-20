//! `app serve`: the API on 127.0.0.1 only - nginx in front decides who may
//! reach it.
//!
//!   GET  /                 the page (page.html)
//!   GET  /health           liveness, uptime, threads
//!   POST /stats            numbers -> mean, sd, median, quantiles
//!   POST /fit/linear       {x, y} -> least-squares line, r2, standard errors
//!   GET  /primes?n=        sieve of Eratosthenes
//!   GET  /lorenz?steps=&dt=&sigma=&rho=&beta=&x0=&y0=&z0=   RK4 series
//!   GET  /mandelbrot.png?w=&h=&cx=&cy=&scale=&iter=
//!
//! Every JSON answer carries `elapsed_us`, the server's compute time; the PNG
//! carries it as the X-Elapsed-Us header.

use std::io::{Cursor, Read};
use std::sync::Arc;
use std::time::Instant;

use serde_json::{json, Value};
use tiny_http::{Header, Method, Request, Response, Server};

use crate::num;

const PAGE: &str = include_str!("page.html");
const BODY_MAX: u64 = 32 << 20;

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

/// Times `f` and folds `elapsed_us` into its JSON object.
fn timed(f: impl FnOnce() -> Result<Value, String>) -> Reply {
    let t = Instant::now();
    match f() {
        Ok(mut v) => {
            v["elapsed_us"] = json!(t.elapsed().as_micros() as u64);
            json_reply(200, v)
        }
        Err(e) => bad(e),
    }
}

/// `key=value` pairs of a query string; values here are numbers, so no decoding.
fn params(query: &str) -> Vec<(&str, &str)> {
    query.split('&').filter_map(|kv| kv.split_once('=')).collect()
}

fn param<T: std::str::FromStr>(q: &[(&str, &str)], key: &str, default: T) -> Result<T, String> {
    match q.iter().find(|(k, _)| *k == key) {
        Some((_, v)) => v.parse().map_err(|_| format!("{key}: not a number: {v}")),
        None => Ok(default),
    }
}

fn numbers(v: &Value, what: &str) -> Result<Vec<f64>, String> {
    let arr = v.as_array().ok_or_else(|| format!("{what}: expected an array of numbers"))?;
    arr.iter()
        .enumerate()
        .map(|(i, x)| x.as_f64().filter(|f| f.is_finite()).ok_or_else(|| format!("{what}[{i}]: not a finite number: {x}")))
        .collect()
}

fn body(req: &mut Request) -> Result<Value, String> {
    let mut s = String::new();
    req.as_reader().take(BODY_MAX).read_to_string(&mut s).map_err(|e| e.to_string())?;
    serde_json::from_str(&s).map_err(|e| format!("bad JSON: {e}"))
}

fn stats(v: Value) -> Result<Value, String> {
    let default = [0.05, 0.25, 0.5, 0.75, 0.95];
    let (xs, probs) = match &v {
        Value::Array(_) => (numbers(&v, "body")?, default.to_vec()),
        Value::Object(o) => (
            numbers(o.get("x").unwrap_or(&Value::Null), "x")?,
            match o.get("probs") {
                Some(p) => numbers(p, "probs")?,
                None => default.to_vec(),
            },
        ),
        _ => return Err("send [numbers] or {\"x\": [numbers]}".into()),
    };
    num::stats(&xs, &probs)
}

fn fit(v: Value) -> Result<Value, String> {
    let x = numbers(&v["x"], "x")?;
    let y = numbers(&v["y"], "y")?;
    num::fit_linear(&x, &y)
}

fn primes(q: &[(&str, &str)]) -> Result<Value, String> {
    let n: u64 = param(q, "n", 1_000_000)?;
    if n > num::PRIMES_MAX {
        return Err(format!("n is at most {}", num::PRIMES_MAX));
    }
    Ok(num::primes(n))
}

fn lorenz(q: &[(&str, &str)]) -> Result<Value, String> {
    let steps: usize = param(q, "steps", 5000)?;
    let dt: f64 = param(q, "dt", 0.01)?;
    if steps > 200_000 {
        return Err("steps is at most 200000".into());
    }
    if !(dt > 0.0 && dt <= 0.1) {
        return Err("dt must be in (0, 0.1]".into());
    }
    let sys = num::Lorenz { sigma: param(q, "sigma", 10.0)?, rho: param(q, "rho", 28.0)?, beta: param(q, "beta", 8.0 / 3.0)? };
    let start = [param(q, "x0", 1.0)?, param(q, "y0", 1.0)?, param(q, "z0", 1.0)?];
    let pts = sys.rk4(start, dt, steps);
    if pts.iter().flatten().any(|v| !v.is_finite()) {
        return Err("the solution diverged: try a smaller dt".into());
    }
    let col = |d: usize| pts.iter().map(|p| p[d]).collect::<Vec<_>>();
    Ok(json!({
        "steps": steps, "dt": dt, "t_end": steps as f64 * dt,
        "sigma": sys.sigma, "rho": sys.rho, "beta": sys.beta,
        "x": col(0), "y": col(1), "z": col(2),
    }))
}

fn mandelbrot(q: &[(&str, &str)]) -> Result<Reply, String> {
    let v = num::View {
        w: param(q, "w", 640)?,
        h: param(q, "h", 400)?,
        cx: param(q, "cx", -0.6)?,
        cy: param(q, "cy", 0.0)?,
        scale: param(q, "scale", 3.0)?,
        iter: param(q, "iter", 256)?,
    };
    if v.w == 0 || v.h == 0 || v.w > 2048 || v.h > 2048 {
        return Err("w and h are 1..2048".into());
    }
    if !(v.iter >= 1 && v.iter <= 20_000) {
        return Err("iter is 1..20000".into());
    }
    if !(v.scale > 0.0 && v.scale.is_finite()) {
        return Err("scale must be positive".into());
    }
    let t = Instant::now();
    let px = num::mandelbrot(&v);
    let us = t.elapsed().as_micros();
    let img = image::RgbImage::from_raw(v.w, v.h, px).ok_or("pixel buffer size")?;
    let mut png = Cursor::new(Vec::new());
    img.write_to(&mut png, image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(Response::from_data(png.into_inner())
        .with_header(header("Content-Type", "image/png"))
        .with_header(header("Cache-Control", "no-store"))
        .with_header(header("X-Elapsed-Us", &us.to_string())))
}

fn handle(mut req: Request, started: Instant, threads: usize) {
    let url = req.url().to_owned();
    let t0 = Instant::now();
    let method = req.method().clone();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let q = params(query);
    // Behind a prefix (a cell's /proxy/<port>/) the page asks beside itself,
    // so routes match on the path's tail.
    let route = ["/health", "/stats", "/fit/linear", "/primes", "/lorenz", "/mandelbrot.png"]
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
        (Method::Post, "/stats") => match body(&mut req) {
            Ok(v) => timed(|| stats(v)),
            Err(e) => bad(e),
        },
        (Method::Post, "/fit/linear") => match body(&mut req) {
            Ok(v) => timed(|| fit(v)),
            Err(e) => bad(e),
        },
        (Method::Get, "/primes") => timed(|| primes(&q)),
        (Method::Get, "/lorenz") => timed(|| lorenz(&q)),
        (Method::Get, "/mandelbrot.png") => mandelbrot(&q).unwrap_or_else(bad),
        (_, "") => json_reply(404, json!({ "error": format!("no route: {path}") })),
        (m, r) => json_reply(405, json!({ "error": format!("{r} does not take {m}") })),
    };
    let status = reply.status_code().0;
    if let Err(e) = req.respond(reply) {
        crate::log::error!("{method} {path}: response not sent: {e}");
    }
    crate::log::request(&method, path, status, t0);
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
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
    crate::log::info!("__HOST__: scientific API serving on 127.0.0.1:{port} ({threads} workers)");
    let workers: Vec<_> = (0..threads)
        .map(|_| {
            let s = Arc::clone(&server);
            std::thread::spawn(move || {
                while let Ok(req) = s.recv() {
                    handle(req, started, threads);
                }
            })
        })
        .collect();
    for w in workers {
        let _ = w.join();
    }
}
