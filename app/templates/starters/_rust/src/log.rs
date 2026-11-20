//! Logging for __HOST__, the standard library only. Every line goes to
//! stdout in the one format every Cove starter writes:
//!
//!   2026-11-18 12:04:31 INFO    [server] serving on 127.0.0.1:8834
//!
//! so .cove/run/app.log (Cove's Logs tab) reads the same in Rust, R, Python,
//! Node and Julia. Times are UTC; the name in brackets is the module the line
//! comes from. Levels, shown when at or above LOG_LEVEL (default INFO):
//! TRACE DEBUG INFO SUCCESS WARN ERROR.
//!
//! ```ignore
//! mod log;                                   // in main.rs
//! log::info!("serving on 127.0.0.1:{port}");
//! log::request(req.method(), &path, 200, t0); // GET /path 200 3 ms
//! ```
#![allow(unused_macros, unused_imports, dead_code)]

use std::io::Write;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

fn rank(level: &str) -> u8 {
    match level {
        "TRACE" => 0,
        "DEBUG" => 1,
        "WARN" => 3,
        "ERROR" => 4,
        _ => 2,
    }
}

fn threshold() -> u8 {
    static T: OnceLock<u8> = OnceLock::new();
    *T.get_or_init(|| rank(&std::env::var("LOG_LEVEL").unwrap_or_default().to_uppercase()))
}

/// `YYYY-MM-DD HH:MM:SS` in UTC, from the civil-from-days algorithm (no
/// chrono needed).
fn stamp() -> String {
    let s = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (s.div_euclid(86_400), s.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// Write one line; the macros call this with their module's path.
pub fn line(level: &str, module: &str, args: std::fmt::Arguments) {
    if rank(level) < threshold() {
        return;
    }
    let name = module.rsplit("::").next().unwrap_or(module);
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{} {level:<7} [{name}] {args}", stamp());
    let _ = out.flush();
}

/// One line per request: method, path, status and the time since `t0`.
/// 5xx are ERROR, 4xx WARN, Cove's health probes TRACE.
pub fn request(method: impl std::fmt::Display, path: &str, status: u16, t0: Instant) {
    let level = match status {
        500.. => "ERROR",
        400.. => "WARN",
        _ if path == "/health" || path == "/__live" => "TRACE",
        _ => "INFO",
    };
    line(level, "request", format_args!("{method} {path} {status} {} ms", t0.elapsed().as_millis()));
}

macro_rules! log_trace { ($($t:tt)*) => { $crate::log::line("TRACE", module_path!(), format_args!($($t)*)) } }
macro_rules! log_debug { ($($t:tt)*) => { $crate::log::line("DEBUG", module_path!(), format_args!($($t)*)) } }
macro_rules! log_info { ($($t:tt)*) => { $crate::log::line("INFO", module_path!(), format_args!($($t)*)) } }
macro_rules! log_success { ($($t:tt)*) => { $crate::log::line("SUCCESS", module_path!(), format_args!($($t)*)) } }
macro_rules! log_warn { ($($t:tt)*) => { $crate::log::line("WARN", module_path!(), format_args!($($t)*)) } }
macro_rules! log_error { ($($t:tt)*) => { $crate::log::line("ERROR", module_path!(), format_args!($($t)*)) } }
// Named log_* inside: `warn` alone collides with the #[warn] attribute.
pub(crate) use {log_debug as debug, log_error as error, log_info as info, log_success as success, log_trace as trace, log_warn as warn};
