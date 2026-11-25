//! A project's Build, Up and Down, by the app contract
//! (testy.ndexr.io/APP-CONTRACT.md, referral #698; operator, 2026-10-25: "all
//! apps need a build, up, down button the agents can kick those off when it
//! makes sense - but the ui needs it ... non shiny modules will need their
//! own port").
//!
//! The contract is the data: `app.env` at the project's root (APP_LANGUAGE,
//! APP_KIND port|module, APP_PORT, APP_HEALTH) and the executables
//! `build.sh`, `up.sh`, `down.sh` beside it. Cove's buttons run those files
//! in the person's own session - as them, exactly as an agent or a person at a
//! shell runs them - so there is one way to build, start and stop a project,
//! whoever asks. Up means APP_HEALTH answers on 127.0.0.1:APP_PORT: measured,
//! never stored.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::time::Duration;

/// The verbs, in the order the strip shows them.
pub const VERBS: [&str; 3] = ["build", "up", "down"];

/// `app.env`'s KEY=VALUE lines (comments and blanks skipped, quotes dropped).
pub fn read_env(text: &str) -> Vec<(String, String)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let (k, v) = l.split_once('=')?;
            Some((k.trim().to_owned(), v.trim().trim_matches(|c| c == '"' || c == '\'').to_owned()))
        })
        .collect()
}

/// A project's language and kind (r, python, julia, rust, node; port or
/// module): its app.env's, else inferred from its files as the contract's
/// section 5 says (module.r, then Cargo.toml, *.jl, server.js, *.py, run.sh).
/// The third value says it was inferred. Cheap - no process scan - so a domain
/// listing can carry it (host.rs domain_at) and a project's environment and
/// consoles follow from it (operator, 2026-10-26).
pub fn language(dir: &Path) -> (String, String, bool) {
    if let Ok(t) = std::fs::read_to_string(crate::project::dotcove::contract_dir(dir).join("app.env")) {
        let env = read_env(&t);
        let get = |k: &str| env.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap_or_default();
        return (get("APP_LANGUAGE"), get("APP_KIND"), false);
    }
    let has_ext = |ext: &str| std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| e.path().extension().is_some_and(|x| x == ext));
    let (lang, kind) = if dir.join("module.r").is_file() {
        ("r", "module")
    } else if has_ext("uproject") {
        ("unreal", "port")
    } else if dir.join("Cargo.toml").is_file() {
        ("rust", "port")
    } else if has_ext("jl") {
        ("julia", "port")
    } else if dir.join("server.js").is_file() {
        ("node", "port")
    } else if has_ext("py") {
        ("python", "port")
    } else {
        ("", if dir.join("run.sh").is_file() { "port" } else { "" })
    };
    (lang.to_owned(), kind.to_owned(), true)
}

/// What a project is built with, as the key of its mark (frameworks.rs): the
/// framework where its files name one, else its language. Read off a few
/// top-level files only, like language(), so a domain listing can carry it:
/// module.r is Shiny; Python's own imports say FastAPI, Flask or Streamlit;
/// Node with React in its page is React; a Rust crate built as a cdylib for
/// the browser is WebAssembly (operator, 2026-10-28: "icons of the supported
/// frameworks ... to see what kind of app they are working on").
pub fn framework(dir: &Path, language: &str) -> &'static str {
    // The start of each file is enough to find an import or a script tag.
    let head = |p: &Path| -> String {
        use std::io::Read;
        let mut s = String::new();
        if let Ok(f) = std::fs::File::open(p) {
            let _ = f.take(64 * 1024).read_to_string(&mut s);
        }
        s
    };
    let files = |d: &Path, ext: &str| -> Vec<std::path::PathBuf> {
        let mut v: Vec<_> = std::fs::read_dir(d).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == ext)).collect();
        v.sort();
        v.truncate(8);
        v
    };
    let any = |ps: Vec<std::path::PathBuf>, words: &[&str]| ps.iter().any(|p| {
        let t = head(p);
        words.iter().any(|w| t.contains(w))
    });
    match language {
        "r" if dir.join("module.r").is_file() || any(files(dir, "R"), &["shiny"]) || any(files(dir, "r"), &["shiny"]) => "shiny",
        "r" => "r",
        "python" => {
            let py = files(dir, "py");
            if any(py.clone(), &["from fastapi", "import fastapi"]) {
                "fastapi"
            } else if any(py.clone(), &["from flask", "import flask"]) {
                "flask"
            } else if any(py, &["import streamlit"]) {
                "streamlit"
            } else {
                "python"
            }
        }
        "node" => {
            let mut page = files(&dir.join("static"), "js");
            page.extend(files(&dir.join("static"), "html"));
            page.push(dir.join("package.json"));
            if any(page, &["react", "React"]) { "react" } else { "nodejs" }
        }
        "rust" if head(&dir.join("Cargo.toml")).contains("cdylib") => "wasm",
        "rust" => "rust",
        "julia" => "julia",
        // Unreal's own logo is Epic's trademark: the language's mark.
        "unreal" => "cplusplus",
        _ => "",
    }
}

/// Where a port app is seen (AppStatus `seen_at`). On ndexr's own servers,
/// not in a cell, Up routes the domain's vhost to the port (route-app.sh), so
/// its public address is the place. Everywhere else - this computer, a
/// person's cell (whose domain goes to the cell's Shiny, nginx cell.conf), a
/// customer's own server - nothing routes a domain to it, and Cove's own
/// /port/<n>/ shows it, behind Cove's sign-in.
fn seen_at(kind: &str, port: &str) -> String {
    let port = port.trim();
    if kind != "port" || port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
        return String::new();
    }
    let cell = std::env::var("COVE_TRUST_LOOPBACK").as_deref() == Ok("1");
    if !cell && Path::new(crate::host::PLATFORM_DOMAINS).is_dir() {
        return String::new();
    }
    format!("/port/{port}/")
}

/// What a project is (language()), the verbs it has, and whether it is up.
pub fn status(dir: &Path) -> crate::api::AppStatus {
    // A project from before .cove/ moves in the first time it is looked
    // at, as the person (dotcove.rs adopt); one Cove cannot write stays as
    // it is, and its root files still run it.
    if let Err(e) = crate::project::dotcove::adopt(dir, &crate::host::starter_files) {
        eprintln!("cove: {} not moved into .cove/: {e}", dir.display());
    }
    let cdir = crate::project::dotcove::contract_dir(dir);
    let mut s = crate::api::AppStatus::default();
    let (language, kind, inferred) = language(dir);
    (s.language, s.kind, s.inferred) = (language, kind, inferred);
    if let Ok(t) = std::fs::read_to_string(cdir.join("app.env")) {
        for (k, v) in read_env(&t) {
            match k.as_str() {
                "APP_PORT" => s.port = v,
                "APP_HEALTH" => s.health = v,
                _ => {}
            }
        }
    }
    s.seen_at = seen_at(&s.kind, &s.port);
    // In .cove/ every project has all four verbs; a module app's Up and
    // Down have nothing to do, so it shows Build alone, as before.
    s.verbs = VERBS.iter().filter(|v| is_executable(&cdir.join(format!("{v}.sh"))) && (s.kind == "port" || **v == "build")).map(|v| (*v).to_owned()).collect();
    s.compiling = compiling(dir);
    // Rust and Unreal Up start what Build made; Julia compiles on its first
    // start, Python and R not at all.
    s.built = match s.language.as_str() {
        "rust" => dir.join("target/release/app").is_file(),
        "unreal" => dir.join("Saved/Packaged/Linux").is_dir(),
        _ => true,
    };
    #[cfg(unix)]
    {
        s.quota = crate::host::limits::mine();
    }
    if s.kind == "port" {
        if let Ok(p) = s.port.parse::<u16>() {
            s.running = answers(p, if s.health.is_empty() { "/" } else { &s.health });
        }
    }
    s
}

/// How long a build has run in `dir`, measured from the processes there:
/// build.sh, build-web.sh, cargo, rustc - whoever started it (Cove's Build,
/// the agent, a terminal), so the strip never says "down" over a compile.
/// The oldest one's age; None when nothing builds.
#[cfg(target_os = "linux")]
pub fn compiling(dir: &Path) -> Option<u64> {
    let dir = dir.canonicalize().ok()?;
    let uptime: f64 = std::fs::read_to_string("/proc/uptime").ok()?.split_whitespace().next()?.parse().ok()?;
    let mut oldest: Option<u64> = None;
    for e in std::fs::read_dir("/proc").ok()?.flatten() {
        let p = e.path();
        if !e.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(cwd) = std::fs::read_link(p.join("cwd")) else { continue };
        if !cwd.starts_with(&dir) {
            continue;
        }
        let cmd = std::fs::read(p.join("cmdline")).unwrap_or_default();
        let argv: Vec<String> = cmd.split(|b| *b == 0).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        let building = argv.iter().take(2).any(|a| {
            let base = a.rsplit('/').next().unwrap_or(a);
            matches!(base, "build.sh" | "build-web.sh" | "cargo" | "rustc" | "wasm-bindgen" | "RunUAT.sh")
        });
        if !building {
            continue;
        }
        // Field 22 of stat, after the parenthesised name: start, in ticks since boot.
        let Ok(stat) = std::fs::read_to_string(p.join("stat")) else { continue };
        let Some(start) = stat.rsplit_once(')').and_then(|(_, r)| r.split_whitespace().nth(19)).and_then(|t| t.parse::<f64>().ok()) else { continue };
        let age = (uptime - start / 100.0).max(0.0) as u64;
        oldest = Some(oldest.map_or(age, |o| o.max(age)));
    }
    oldest
}

#[cfg(not(target_os = "linux"))]
pub fn compiling(_dir: &Path) -> Option<u64> {
    None
}

fn is_executable(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// GET 127.0.0.1:port/path answers 200, within two seconds.
pub fn answers(port: u16, path: &str) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut c) = TcpStream::connect_timeout(&addr, Duration::from_secs(2)) else { return false };
    let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
    let path = if path.starts_with('/') { path.to_owned() } else { format!("/{path}") };
    if c.write_all(format!("GET {path} HTTP/1.0\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n").as_bytes()).is_err() {
        return false;
    }
    let mut head = [0u8; 16];
    let n = c.read(&mut head).unwrap_or(0);
    let line = String::from_utf8_lossy(&head[..n]);
    line.starts_with("HTTP/1.") && line.get(9..12) == Some("200")
}

/// One job slot per project, so its verbs never overlap.
fn slot(host: &str) -> &'static str {
    static KINDS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, &'static str>>> = std::sync::OnceLock::new();
    let mut m = KINDS.get_or_init(Default::default).lock().unwrap();
    m.entry(host.to_owned()).or_insert_with(|| Box::leak(format!("app {host}").into_boxed_str()))
}

/// Run `<verb>.sh` in the project's folder as a job (Cove shows its lines).
pub fn action(host: &str, dir: &Path, verb: &str) -> Result<crate::api::JobView, String> {
    if !VERBS.contains(&verb) {
        return Err(format!("{verb}: not build, up or down"));
    }
    let cdir = crate::project::dotcove::contract_dir(dir);
    let script = cdir.join(format!("{verb}.sh"));
    if !is_executable(&script) {
        return Err(format!("{host} has no {verb}.sh - its app.env and verbs come with the starter (APP-CONTRACT.md)"));
    }
    // From the project's root, by its path from there: .cove/up.sh, or
    // ./up.sh in a project not moved into .cove/.
    let rel = script.strip_prefix(dir).map(|p| p.display().to_string()).unwrap_or_else(|_| format!("{verb}.sh"));
    let vars_rel = cdir.join(crate::ui::ide::settings_tab::VARS).strip_prefix(dir).map(|p| p.display().to_string()).unwrap_or_else(|_| crate::ui::ide::settings_tab::VARS.to_owned());
    // Up starts the app with the project's variables (Settings › Variables,
    // settings_tab.rs) exported, so up.sh's run.sh inherits them; a project's
    // own up.sh needs no change for it.
    let vars = if verb == "up" && dir.join(&vars_rel).is_file() { format!("set -a && . ./{vars_rel} && set +a && ") } else { String::new() };
    // Colour asked for though the app writes to a file: the Logs tab draws
    // it (ansi.rs).
    // Once Up answers, the record of what it runs on (record.rs, `.cove/
    // record.json`): read off the running app, so it is what loaded, not
    // what a script meant to. A record that cannot be taken does not fail Up.
    let record = match (verb, std::env::current_exe()) {
        ("up", Ok(exe)) if cfg!(unix) => format!(" && {{ {} record . || true; }}", crate::api::shell_quote(&exe.display().to_string())),
        _ => String::new(),
    };
    let cmd = format!("cd {} && {vars}{} ./{rel}{record}", crate::api::shell_quote(&dir.display().to_string()), crate::ui::kit::ansi::COLOR_ENV);
    // A failed Build or Up goes home when the person allowed reports (operator,
    // 2026-11-19: a Julia starter's "Package HTTP not found" - "errors like
    // these should be sent back if the user allows error reporting"). Named by
    // what it was and the app's language, never the project's name.
    let lang = std::fs::read_to_string(cdir.join("app.env")).ok().and_then(|t| t.lines().find_map(|l| l.trim().strip_prefix("APP_LANGUAGE=").map(|v| v.trim().to_owned()))).unwrap_or_default();
    let stage = format!("cove:app-{verb}");
    let verb_owned = verb.to_owned();
    let done: crate::host::stack::jobs::Done = Box::new(move |ok, v| {
        if !ok && crate::host::stack::reporting() {
            let mut r = v.clone();
            r.what = format!("{verb_owned} of a {} app", if lang.is_empty() { "Cove" } else { &lang });
            crate::host::stack::report_failure(&stage, &mut r);
            if let Some(last) = r.lines.last() {
                v.lines.push(last.clone());
            }
        }
    });
    crate::host::stack::jobs::start(slot(host), format!("{verb} {host}"), cmd, Some(done))
}

/// That project's job as far as it got.
pub fn job(host: &str) -> crate::api::JobView {
    crate::host::stack::jobs::view(slot(host))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_framework_is_read_off_the_projects_files() {
        let d = std::env::temp_dir().join(format!("cove-fw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("static")).unwrap();
        std::fs::write(d.join("app.py"), "import numpy\nfrom fastapi import FastAPI\n").unwrap();
        assert_eq!(framework(&d, "python"), "fastapi");
        std::fs::write(d.join("app.py"), "import streamlit as st\n").unwrap();
        assert_eq!(framework(&d, "python"), "streamlit");
        std::fs::write(d.join("app.py"), "print(1)\n").unwrap();
        assert_eq!(framework(&d, "python"), "python");
        std::fs::write(d.join("static/app.js"), "const { useState } = React;\n").unwrap();
        assert_eq!(framework(&d, "node"), "react");
        std::fs::write(d.join("module.r"), "ui_module <- function(id, request) {}\n").unwrap();
        assert_eq!(framework(&d, "r"), "shiny");
        std::fs::write(d.join("Cargo.toml"), "[lib]\ncrate-type = [\"cdylib\"]\n").unwrap();
        assert_eq!(framework(&d, "rust"), "wasm");
        assert_eq!(framework(&d, ""), "");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_contract_is_read_and_inferred() {
        let env = read_env("# how\nAPP_LANGUAGE=julia\nAPP_KIND=port\nAPP_PORT=\"8830\"\n\nAPP_HEALTH=/health\n");
        assert_eq!(env, [("APP_LANGUAGE", "julia"), ("APP_KIND", "port"), ("APP_PORT", "8830"), ("APP_HEALTH", "/health")].map(|(a, b)| (a.to_owned(), b.to_owned())));
        let d = std::env::temp_dir().join(format!("cove-apps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("server.jl"), "1").unwrap();
        let s = status(&d);
        assert!(s.inferred && s.language == "julia" && s.kind == "port" && s.verbs.is_empty() && !s.running);
        std::fs::write(d.join("module.r"), "1").unwrap();
        assert_eq!(status(&d).kind, "module");
        std::fs::write(d.join("app.env"), "APP_LANGUAGE=python\nAPP_KIND=port\nAPP_PORT=1\nAPP_HEALTH=/health\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::write(d.join("up.sh"), "#!/bin/sh\necho up: ok\n").unwrap();
            std::fs::set_permissions(d.join("up.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Looked at once, the old layout moves into .cove/ (dotcove.rs) with
        // all the verbs; its own up.sh gives way to .cove/up.sh.
        let s = status(&d);
        assert!(!s.inferred && s.language == "python" && s.port == "1" && !s.running);
        assert!(d.join(".cove/app.env").is_file() && !d.join("app.env").exists() && !d.join("up.sh").exists());
        #[cfg(unix)]
        assert_eq!(s.verbs, ["build", "up", "down"]);
        assert!(action("x.ndexr.io", &d, "deploy").is_err());
        // A module app shows Build alone: the platform serves it.
        std::fs::write(d.join(".cove/app.env"), "APP_LANGUAGE=r\nAPP_KIND=module\n").unwrap();
        #[cfg(unix)]
        assert_eq!(status(&d).verbs, ["build"]);
        let _ = std::fs::remove_dir_all(&d);
    }
}
