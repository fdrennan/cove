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
    if let Ok(t) = std::fs::read_to_string(dir.join("app.env")) {
        let env = read_env(&t);
        let get = |k: &str| env.iter().find(|(x, _)| x == k).map(|(_, v)| v.clone()).unwrap_or_default();
        return (get("APP_LANGUAGE"), get("APP_KIND"), false);
    }
    let has_ext = |ext: &str| std::fs::read_dir(dir).into_iter().flatten().flatten().any(|e| e.path().extension().is_some_and(|x| x == ext));
    let (lang, kind) = if dir.join("module.r").is_file() {
        ("r", "module")
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
        _ => "",
    }
}

/// What a project is (language()), the verbs it has, and whether it is up.
pub fn status(dir: &Path) -> crate::api::AppStatus {
    let mut s = crate::api::AppStatus::default();
    let (language, kind, inferred) = language(dir);
    (s.language, s.kind, s.inferred) = (language, kind, inferred);
    if let Ok(t) = std::fs::read_to_string(dir.join("app.env")) {
        for (k, v) in read_env(&t) {
            match k.as_str() {
                "APP_PORT" => s.port = v,
                "APP_HEALTH" => s.health = v,
                _ => {}
            }
        }
    }
    s.verbs = VERBS.iter().filter(|v| is_executable(&dir.join(format!("{v}.sh")))).map(|v| (*v).to_owned()).collect();
    s.compiling = compiling(dir);
    // Rust is the one language whose Up starts what Build made; Julia
    // compiles on its first start, Python and R not at all.
    s.built = s.language != "rust" || dir.join("target/release/app").is_file();
    #[cfg(unix)]
    {
        s.quota = crate::limits::mine();
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
            matches!(base, "build.sh" | "build-web.sh" | "cargo" | "rustc" | "wasm-bindgen")
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
    let script = dir.join(format!("{verb}.sh"));
    if !is_executable(&script) {
        return Err(format!("{host} has no {verb}.sh - its app.env and verbs come with the starter (APP-CONTRACT.md)"));
    }
    let cmd = format!("cd {} && ./{verb}.sh", crate::api::shell_quote(&dir.display().to_string()));
    crate::stack::jobs::start(slot(host), format!("{verb} {host}"), cmd, None)
}

/// That project's job as far as it got.
pub fn job(host: &str) -> crate::api::JobView {
    crate::stack::jobs::view(slot(host))
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
        let s = status(&d);
        assert!(!s.inferred && s.language == "python" && s.port == "1" && !s.running);
        #[cfg(unix)]
        assert_eq!(s.verbs, ["up"]);
        assert!(action("x.ndexr.io", &d, "deploy").is_err());
        assert!(action("x.ndexr.io", &d, "build").is_err(), "no build.sh");
        let _ = std::fs::remove_dir_all(&d);
    }
}
