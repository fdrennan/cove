//! `.cove/`: everything Cove runs a project with, out of the person's way
//! (request #169 and the operator, 2026-11-01: "all projects have the run up
//! down files etc inside of a .cove folder ... they should have all project
//! types in that .cove in case the user wants to swap out project types ...
//! all projects existing and future should follow similar interface between
//! cove management and project centralization").
//!
//! ```text
//! <project>/.cove/
//!   app.env      APP_LANGUAGE, APP_KIND, APP_PORT, APP_HEALTH - the contract
//!   app.vars     Settings › Variables, exported for Up
//!   build.sh run.sh    run the type APP_LANGUAGE names: .cove/<language>/…
//!   up.sh down.sh      start and stop a port app; same in every project
//!   r/ python/ julia/ node/ rust/ unreal/   every type's build.sh, env.sh, run.sh
//!   run/         the running app's pid and log
//! ```
//!
//! Switching a project's type is one line of data (APP_LANGUAGE, and the
//! APP_KIND that goes with it), never a copy: every type is already there.
//! Projects made before this had the same files at their root; `adopt`
//! moves them in, rewriting the paths, the first time Cove looks
//! (apps::status) - the same `lay` that lays out a new project, so there is
//! one way to get from the old layout to this one.

use std::path::{Path, PathBuf};

pub const DIR: &str = ".cove";

/// The types a project can be, in the Type menu's order: (APP_LANGUAGE,
/// APP_KIND, what the menu says).
pub const TYPES: &[(&str, &str, &str)] = &[
    ("r", "module", "R · Shiny module, served by the platform"),
    ("python", "port", "Python · its own server"),
    ("julia", "port", "Julia · its own server"),
    ("node", "port", "Node.js · its own server"),
    ("rust", "port", "Rust · its own server"),
    ("unreal", "port", "Unreal Engine · C++, streamed to the browser"),
];

/// The files at a project's root in the old layout that `.cove/` holds now.
/// The language's own scripts go to `.cove/<language>/`, the rest to `.cove/`.
const OWN: [&str; 4] = ["build.sh", "env.sh", "run.sh", "build-web.sh"];
const TOP: [&str; 4] = ["app.env", "app.vars", "up.sh", "down.sh"];

/// Where a project's contract is: `.cove/` once it has one, else the root
/// (a project not adopted yet, or a folder Cove cannot write).
pub fn contract_dir(dir: &Path) -> PathBuf {
    let cove = dir.join(DIR);
    if cove.join("app.env").is_file() {
        cove
    } else {
        dir.to_path_buf()
    }
}

/// A script written for the old layout (it `cd`s to its own folder, the
/// project's root, and names its neighbours `./x`), made to work from
/// `.cove/<language>/`. Every script the starters ever laid, and every one
/// an agent edited in the 83 projects there were on 2026-11-01, uses only
/// these forms.
pub fn rewrite(body: &str, lang: &str) -> String {
    body.replace("\"$(dirname \"$0\")\"", "\"$(dirname \"$0\")/../..\"")
        .replace(". ./app.env", ". .cove/app.env")
        .replace(". ./env.sh", &format!(". .cove/{lang}/env.sh"))
        .replace("./build-web.sh", &format!(".cove/{lang}/build-web.sh"))
        .replace("./build.sh", ".cove/build.sh")
        .replace("./run.sh", ".cove/run.sh")
        .replace("./up.sh", ".cove/up.sh")
        .replace("./down.sh", ".cove/down.sh")
        .replace(".run/", ".cove/run/")
}

/// What a project's CONTEXT.md says about the scripts, as they are now.
fn rewrite_doc(body: &str, lang: &str) -> String {
    let b = body
        .replace("./build-web.sh", &format!(".cove/{lang}/build-web.sh"))
        .replace("./build.sh", ".cove/build.sh")
        .replace("./run.sh", ".cove/run.sh")
        .replace("./up.sh", ".cove/up.sh")
        .replace("./down.sh", ".cove/down.sh")
        .replace(".cove/app.env", "app.env");
    b.replace("app.env", ".cove/app.env")
}

fn field(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| l.trim().strip_prefix(&format!("{key}=")).map(|v| v.trim().trim_matches(|c| c == '"' || c == '\'').to_owned()))
}

/// A type's own scripts, in the old layout: its shared starter folder's
/// (templates/starters/_<language>) and its blank starter's run.sh.
fn kit(lang: &str, host: &str, files: &dyn Fn(&str) -> Vec<(&'static str, &'static str)>) -> Vec<(String, String)> {
    let blank = match lang {
        "python" => "python-blank-fastapi",
        "julia" => "julia-blank",
        "node" => "node-blank",
        "rust" => "rust-blank",
        "unreal" => "unreal-pixelstream",
        _ => "",
    };
    let mut out: Vec<(String, String)> = files(&format!("_{lang}")).into_iter().filter(|(n, _)| OWN.contains(n)).map(|(n, b)| (n.to_owned(), b.to_owned())).collect();
    for (n, b) in files(blank) {
        if n == "run.sh" {
            out.push((n.to_owned(), b.replace("__HOST__", host)));
        }
    }
    out
}

/// A project's files in the old layout (paths from its root) laid out the
/// `.cove/` way: its contract and its own scripts moved in and rewritten,
/// every other type's scripts beside them, the four verbs on top. Files
/// that are not the contract stay where they are. `files` is the starters'
/// table (host.rs starter_files).
pub fn lay(host: &str, laid: Vec<(String, String)>, files: &dyn Fn(&str) -> Vec<(&'static str, &'static str)>) -> Vec<(String, String)> {
    let env = laid.iter().find(|(n, _)| n == "app.env").map(|(_, b)| b.clone()).unwrap_or_default();
    let lang = field(&env, "APP_LANGUAGE").filter(|l| !l.is_empty() && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')).unwrap_or_else(|| "custom".to_owned());
    let mut out: Vec<(String, String)> = Vec::new();
    let mut own: Vec<(String, String)> = Vec::new();
    let mut gitignore = None;
    for (name, body) in laid {
        if TOP.contains(&name.as_str()) {
            if name == "app.env" || name == "app.vars" {
                out.push((format!("{DIR}/{name}"), body));
            }
        } else if OWN.contains(&name.as_str()) {
            own.push((name, body));
        } else if name == ".gitignore" {
            gitignore = Some(body);
        } else if name == "CONTEXT.md" {
            out.push((name, rewrite_doc(&body, &lang)));
        } else {
            out.push((name, body));
        }
    }
    for verb in ["build.sh", "run.sh", "up.sh", "down.sh"] {
        if let Some((_, b)) = files("_cove").into_iter().find(|(n, _)| *n == verb) {
            out.push((format!("{DIR}/{verb}"), b.to_owned()));
        }
    }
    let mut langs: Vec<&str> = TYPES.iter().map(|t| t.0).collect();
    if !langs.contains(&lang.as_str()) {
        langs.push(&lang);
    }
    for l in langs {
        let mut set = kit(l, host, files);
        if l == lang {
            for (n, b) in &own {
                set.retain(|(x, _)| x != n);
                set.push((n.clone(), b.clone()));
            }
        }
        set.sort();
        for (n, b) in set {
            out.push((format!("{DIR}/{l}/{n}"), rewrite(&b, l)));
        }
    }
    // The running app's pid and log are no one's source.
    // Its variables stay out of the saved versions in their new place too.
    let mut g: String = gitignore.unwrap_or_default().lines().map(|l| if l.trim() == "app.vars" { ".cove/app.vars\n".to_owned() } else { format!("{l}\n") }).collect();
    if !g.lines().any(|l| l.trim() == ".cove/run/") {
        if !g.is_empty() && !g.ends_with('\n') {
            g.push('\n');
        }
        g.push_str(".cove/run/\n");
    }
    // supporting_docs/ is kept in S3, not git (operator, 2026-11-14), like
    // every existing project (ndexr scripts/supporting-docs-out-of-git.sh).
    if !g.lines().any(is_docs_line) {
        g.push_str("supporting_docs/\n");
    }
    out.push((".gitignore".to_owned(), g));
    out
}

/// Write `laid` under `dir`, its scripts executable.
pub fn write(dir: &Path, laid: &[(String, String)]) -> Result<(), String> {
    for (name, body) in laid {
        let p = dir.join(name);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(&p, body).map_err(|e| format!("{name}: {e}"))?;
        #[cfg(unix)]
        if name.ends_with(".sh") {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o775));
        }
    }
    Ok(())
}

/// Move a project made before `.cove/` into it: its contract and scripts
/// from the root into `.cove/`, rewritten, every type's beside them, and
/// `.run/` (a running app's pid and log) to `.cove/run/`, so Down still
/// finds what Up started. Nothing to do - Ok(false) - for a project with
/// `.cove/` already or no app.env at all. The new files are written before
/// any old one goes, so a failure leaves the old layout working.
/// `.cove/build.sh` as every project had it before builds kept a log
/// (2026-11-08): replaced by the current one where it is still exactly this,
/// so a project whose dispatcher someone changed is left as it is.
const BUILD_BEFORE_LOG: &str = r#"#!/usr/bin/env bash
# Build: this project as the type .cove/app.env names (APP_LANGUAGE), by that
# type's own .cove/<language>/build.sh. Every type's scripts are in .cove/;
# Cove's Type menu (Settings › Info), or APP_LANGUAGE, picks which runs.
#
#   .cove/build.sh
set -euo pipefail
cove="$(cd "$(dirname "$0")" && pwd)"
. "$cove/app.env"
script="$cove/${APP_LANGUAGE:-}/build.sh"
[ -x "$script" ] || { echo "build: no $script - APP_LANGUAGE in .cove/app.env names no type with a build.sh" >&2; exit 1; }
exec "$script" "$@"
"#;

/// One place for a project's logs (operator, 2026-11-08: "every project
/// should have standardized location in cove to view application logs"):
/// `.cove/run/build.log` and `.cove/run/app.log`. A project whose build.sh
/// predates the log gets the one that keeps it, and `.cove/run/` stays out
/// of its versions.
fn refresh_verbs(dir: &Path, files: &dyn Fn(&str) -> Vec<(&'static str, &'static str)>) -> bool {
    let cove = dir.join(DIR);
    let mut changed = false;
    let b = cove.join("build.sh");
    if std::fs::read_to_string(&b).is_ok_and(|t| t == BUILD_BEFORE_LOG) {
        if let Some((_, now)) = files("_cove").into_iter().find(|(p, _)| *p == "build.sh") {
            changed |= std::fs::write(&b, now).is_ok();
        }
    }
    // Where a Julia project's packages are (operator, 2026-11-20: "the local
    // version of cove not knowing where to find packages"): an env.sh Cove
    // wrote before julia_set() trusted a module's link to a site library a
    // laptop never has, and every `using` failed ("Package HTTP not found",
    // request #197). A copy Cove wrote - its first line is the starter's - and
    // that predates the fix gets the current one; the old is kept beside it. So
    // does one that fetched the whole 2 GB set before it came in pieces
    // (chunks.tsv, 2026-11-20).
    let je = cove.join("julia").join("env.sh");
    if let Ok(t) = std::fs::read_to_string(&je) {
        if t.starts_with("# Sourced by run.sh and build.sh: app.env, and Julia from the ndexr stack") && !t.contains("chunks.tsv") {
            if let Some((_, now)) = files("_julia").into_iter().find(|(p, _)| *p == "env.sh") {
                let _ = std::fs::write(cove.join("julia").join("env.sh.prev"), &t);
                changed |= std::fs::write(&je, rewrite(now, "julia")).is_ok();
            }
        }
    }
    // Cove's lines in the app's log (operator, 2026-11-17: "logs arent
    // helpful if they never say anything"): an up.sh or down.sh Cove wrote
    // before them - its own heading, no [cove] mark - gets the current one,
    // the old kept beside it. One a person changed so it lost that heading is
    // theirs and stays.
    for (verb, heading) in [("up.sh", "# Up: start .cove/run.sh in the background"), ("down.sh", "# Down: stop what .cove/up.sh started")] {
        let f = cove.join(verb);
        if let Ok(t) = std::fs::read_to_string(&f) {
            if t.contains(heading) && !t.contains("[cove]") {
                if let Some((_, now)) = files("_cove").into_iter().find(|(p, _)| *p == verb) {
                    let _ = std::fs::write(cove.join(format!("{verb}.prev")), &t);
                    changed |= std::fs::write(&f, now).is_ok();
                }
            }
        }
    }
    let gi = dir.join(".gitignore");
    if cove.join("app.env").is_file() {
        let g = std::fs::read_to_string(&gi).unwrap_or_default();
        if !g.lines().any(|l| matches!(l.trim(), ".cove/run/" | ".cove/run" | "/.cove/run/" | ".cove/")) {
            let sep = if g.is_empty() || g.ends_with('\n') { "" } else { "\n" };
            changed |= std::fs::write(&gi, format!("{g}{sep}.cove/run/\n")).is_ok();
        }
        let g = std::fs::read_to_string(&gi).unwrap_or_default();
        if !g.lines().any(is_docs_line) {
            let sep = if g.is_empty() || g.ends_with('\n') { "" } else { "\n" };
            changed |= std::fs::write(&gi, format!("{g}{sep}supporting_docs/\n")).is_ok();
        }
    }
    changed
}

/// A .gitignore line that keeps supporting_docs/ out of git: it lives in S3
/// (ndexr bulk-commit copies it to docs/<key>/ on every save).
fn is_docs_line(l: &str) -> bool {
    matches!(l.trim(), "supporting_docs/" | "supporting_docs" | "/supporting_docs/" | "/supporting_docs")
}

pub fn adopt(dir: &Path, files: &dyn Fn(&str) -> Vec<(&'static str, &'static str)>) -> Result<bool, String> {
    // The vhost's last copy (src/r/nginx/target.r writes it to .cove/ now)
    // is no one's source either; one left at the root moves in.
    let tidied = dir.join(DIR).is_dir() && dir.join("nginx.conf.prev").is_file() && std::fs::rename(dir.join("nginx.conf.prev"), dir.join(DIR).join("nginx.conf.prev")).is_ok();
    if dir.join(DIR).join("app.env").is_file() || !dir.join("app.env").is_file() {
        let refreshed = refresh_verbs(dir, files);
        return Ok(tidied || refreshed);
    }
    let host = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut old: Vec<(String, String)> = Vec::new();
    for name in TOP.iter().chain(OWN.iter()).chain([".gitignore", "CONTEXT.md"].iter()) {
        if let Ok(b) = std::fs::read_to_string(dir.join(name)) {
            old.push(((*name).to_owned(), b));
        }
    }
    let laid = lay(&host, old, files);
    write(dir, &laid)?;
    for name in TOP.iter().chain(OWN.iter()) {
        let _ = std::fs::remove_file(dir.join(name));
    }
    let run = dir.join(".run");
    if run.is_dir() {
        let to = dir.join(DIR).join("run");
        if std::fs::rename(&run, &to).is_err() {
            for e in std::fs::read_dir(&run).into_iter().flatten().flatten() {
                let _ = std::fs::rename(e.path(), to.join(e.file_name()));
            }
            let _ = std::fs::remove_dir(&run);
        }
    }
    if dir.join("nginx.conf.prev").is_file() {
        let _ = std::fs::rename(dir.join("nginx.conf.prev"), dir.join(DIR).join("nginx.conf.prev"));
    }
    let gi = dir.join(".gitignore");
    if let Ok(g) = std::fs::read_to_string(&gi) {
        let kept: String = g.lines().filter(|l| !matches!(l.trim(), ".run/" | ".run")).map(|l| format!("{l}\n")).collect();
        let _ = std::fs::write(&gi, kept);
    }
    Ok(true)
}

/// Make the project the type `lang` (TYPES): APP_LANGUAGE and APP_KIND in
/// `.cove/app.env`, with a port - `port()` finds a free one - when it
/// becomes its own server and has none. Every other line stays. Refused
/// while the app runs: Down first, so nothing is left holding its port.
pub fn set_type(dir: &Path, lang: &str, running: bool, port: &dyn Fn() -> Result<u16, String>) -> Result<String, String> {
    let Some((_, kind, name)) = TYPES.iter().find(|t| t.0 == lang) else { return Err(format!("{lang}: not a type Cove knows")) };
    if running {
        return Err("it is running - Down first, then change its type".to_owned());
    }
    let p = dir.join(DIR).join("app.env");
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let mut set: Vec<(&str, String)> = vec![("APP_LANGUAGE", lang.to_owned()), ("APP_KIND", (*kind).to_owned())];
    if *kind == "port" {
        if field(&text, "APP_PORT").filter(|v| !v.is_empty()).is_none() {
            set.push(("APP_PORT", port()?.to_string()));
        }
        if field(&text, "APP_HEALTH").filter(|v| !v.is_empty()).is_none() {
            set.push(("APP_HEALTH", "/health".to_owned()));
        }
    }
    let mut lines: Vec<String> = text.lines().map(str::to_owned).collect();
    for (k, v) in set {
        let line = format!("{k}={v}");
        match lines.iter().position(|l| l.trim_start().starts_with(&format!("{k}="))) {
            Some(i) => lines[i] = line,
            None => lines.push(line),
        }
    }
    std::fs::write(&p, lines.join("\n") + "\n").map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(format!("now {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Request #197: an env.sh Cove wrote before julia_set() is replaced (the
    /// old kept as env.sh.prev); one the person wrote is left alone.
    #[test]
    fn an_old_julia_env_gets_the_package_set_rule() {
        let d = std::env::temp_dir().join(format!("cove-julia-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".cove/julia")).unwrap();
        std::fs::write(d.join(".cove/app.env"), "APP_LANGUAGE=julia\nAPP_KIND=port\n").unwrap();
        let old = "# Sourced by run.sh and build.sh: app.env, and Julia from the ndexr stack in\n. .cove/app.env\nml Julia/1.11.7\n";
        std::fs::write(d.join(".cove/julia/env.sh"), old).unwrap();
        assert!(refresh_verbs(&d, &files));
        let now = std::fs::read_to_string(d.join(".cove/julia/env.sh")).unwrap();
        assert!(now.contains("chunks.tsv") && now.contains(". .cove/app.env"), "the current rule, with the project's own paths");
        assert_eq!(std::fs::read_to_string(d.join(".cove/julia/env.sh.prev")).unwrap(), old);
        assert!(!refresh_verbs(&d, &files), "a second look changes nothing");
        std::fs::write(d.join(".cove/julia/env.sh"), "# mine\nml Julia/1.11.7\n").unwrap();
        refresh_verbs(&d, &files);
        assert_eq!(std::fs::read_to_string(d.join(".cove/julia/env.sh")).unwrap(), "# mine\nml Julia/1.11.7\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    fn files(id: &str) -> Vec<(&'static str, &'static str)> {
        crate::host::starter_files(id)
    }

    #[test]
    fn old_scripts_work_from_their_new_folder() {
        let old = "#!/usr/bin/env bash\nset -euo pipefail\ncd \"$(dirname \"$0\")\"\n. ./env.sh\n[ -x ./build-web.sh ] && ./build-web.sh\nfind . -not -path './.run/*'\n";
        let new = rewrite(old, "rust");
        assert!(new.contains("cd \"$(dirname \"$0\")/../..\""), "{new}");
        assert!(new.contains(". .cove/rust/env.sh") && new.contains("[ -x .cove/rust/build-web.sh ] && .cove/rust/build-web.sh"), "{new}");
        assert!(new.contains("'./.cove/run/*'"), "{new}");
        assert_eq!(rewrite(&new, "rust"), new, "a second pass changes nothing");
        let env = rewrite(". ./app.env\nexport PORT=\"${PORT:-$APP_PORT}\"\n", "node");
        assert!(env.starts_with(". .cove/app.env"), "{env}");
    }

    #[test]
    fn a_project_carries_every_type_and_its_own_scripts() {
        let laid = vec![
            ("app.env".to_owned(), "APP_LANGUAGE=node\nAPP_KIND=port\nAPP_PORT=8840\nAPP_HEALTH=/health\n".to_owned()),
            ("run.sh".to_owned(), "cd \"$(dirname \"$0\")\"\n. ./env.sh\nexec node mine.js\n".to_owned()),
            ("up.sh".to_owned(), "old".to_owned()),
            ("server.js".to_owned(), "1".to_owned()),
            ("CONTEXT.md".to_owned(), "- app.env and ./up.sh\n".to_owned()),
        ];
        let out = lay("x.ndexr.io", laid, &files);
        let get = |p: &str| out.iter().find(|(n, _)| n == p).map(|(_, b)| b.clone());
        for p in [".cove/app.env", ".cove/build.sh", ".cove/run.sh", ".cove/up.sh", ".cove/down.sh", "server.js", ".cove/node/run.sh", ".cove/node/build.sh", ".cove/node/env.sh", ".cove/python/run.sh", ".cove/julia/run.sh", ".cove/rust/run.sh", ".cove/unreal/run.sh", ".cove/unreal/build.sh", ".cove/r/build.sh"] {
            assert!(get(p).is_some(), "{p} missing: {:?}", out.iter().map(|x| &x.0).collect::<Vec<_>>());
        }
        assert!(get(".cove/node/run.sh").unwrap().contains("mine.js"), "the project's own run.sh wins");
        assert!(get("up.sh").is_none() && get("app.env").is_none());
        assert!(get(".cove/up.sh").unwrap().contains(".cove/run/app.pid"));
        assert_eq!(get("CONTEXT.md").unwrap(), "- .cove/app.env and .cove/up.sh\n");
        assert!(get(".gitignore").unwrap().contains(".cove/run/"));
        for (n, b) in &out {
            if n.starts_with(".cove/") && n.ends_with(".sh") && n.matches('/').count() == 2 {
                assert!(!b.contains("cd \"$(dirname \"$0\")\"\n"), "{n} still cds to its own folder");
                assert!(!b.contains(". ./"), "{n} still sources a neighbour");
            }
        }
    }

    #[test]
    fn a_build_from_before_the_log_gets_the_one_that_keeps_it() {
        let d = std::env::temp_dir().join(format!("cove-refresh-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(DIR)).unwrap();
        std::fs::write(d.join(DIR).join("app.env"), "APP_LANGUAGE=node\n").unwrap();
        std::fs::write(d.join(DIR).join("build.sh"), BUILD_BEFORE_LOG).unwrap();
        std::fs::write(d.join(".gitignore"), "node_modules/").unwrap();
        assert!(refresh_verbs(&d, &crate::host::starter_files));
        let b = std::fs::read_to_string(d.join(DIR).join("build.sh")).unwrap();
        assert!(b.contains("run/build.log"), "build.sh does not keep its log");
        assert_eq!(std::fs::read_to_string(d.join(".gitignore")).unwrap(), "node_modules/\n.cove/run/\nsupporting_docs/\n");
        assert!(!refresh_verbs(&d, &crate::host::starter_files), "a second look changed something");
        // Someone's own dispatcher is theirs.
        std::fs::write(d.join(DIR).join("build.sh"), "#!/bin/sh\nmake\n").unwrap();
        refresh_verbs(&d, &crate::host::starter_files);
        assert_eq!(std::fs::read_to_string(d.join(DIR).join("build.sh")).unwrap(), "#!/bin/sh\nmake\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_up_from_before_the_marks_gets_the_one_that_writes_them() {
        let d = std::env::temp_dir().join(format!("cove-marks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(DIR)).unwrap();
        std::fs::write(d.join(DIR).join("app.env"), "APP_LANGUAGE=node\n").unwrap();
        let old_up = "#!/usr/bin/env bash\n# Up: start .cove/run.sh in the background and wait until APP_HEALTH answers.\nsetsid nohup .cove/run.sh >.cove/run/app.log 2>&1 </dev/null &\n";
        std::fs::write(d.join(DIR).join("up.sh"), old_up).unwrap();
        assert!(refresh_verbs(&d, &crate::host::starter_files));
        let up = std::fs::read_to_string(d.join(DIR).join("up.sh")).unwrap();
        assert!(up.contains("[cove]") && up.contains(">>\"$log\""), "up.sh writes no marks or overwrites the log");
        assert_eq!(std::fs::read_to_string(d.join(DIR).join("up.sh.prev")).unwrap(), old_up, "the old one is kept beside it");
        assert!(!refresh_verbs(&d, &crate::host::starter_files), "a second look changed something");
        // Someone's own up.sh is theirs.
        std::fs::write(d.join(DIR).join("up.sh"), "#!/bin/sh\n./serve &\n").unwrap();
        refresh_verbs(&d, &crate::host::starter_files);
        assert_eq!(std::fs::read_to_string(d.join(DIR).join("up.sh")).unwrap(), "#!/bin/sh\n./serve &\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn adopt_moves_a_project_in_and_set_type_switches_it() {
        let d = std::env::temp_dir().join(format!("cove-dot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".run")).unwrap();
        std::fs::write(d.join("app.env"), "# how\nAPP_LANGUAGE=r\nAPP_KIND=module\n").unwrap();
        std::fs::write(d.join("build.sh"), "cd \"$(dirname \"$0\")\"\necho ok\n").unwrap();
        std::fs::write(d.join(".run/app.pid"), "1").unwrap();
        std::fs::write(d.join(".gitignore"), ".run/\n*.log\n").unwrap();
        std::fs::write(d.join("nginx.conf.prev"), "old").unwrap();
        assert!(adopt(&d, &files).unwrap());
        assert!(!d.join("app.env").exists() && !d.join("build.sh").exists() && !d.join(".run").exists());
        assert!(d.join(".cove/run/app.pid").is_file() && d.join(".cove/r/build.sh").is_file() && d.join(".cove/nginx.conf.prev").is_file() && !d.join("nginx.conf.prev").exists());
        assert_eq!(std::fs::read_to_string(d.join(".gitignore")).unwrap(), "*.log\n.cove/run/\nsupporting_docs/\n");
        assert_eq!(contract_dir(&d), d.join(".cove"));
        assert!(!adopt(&d, &files).unwrap(), "once");
        assert!(set_type(&d, "python", true, &|| Ok(8999)).is_err());
        set_type(&d, "python", false, &|| Ok(8999)).unwrap();
        let env = std::fs::read_to_string(d.join(".cove/app.env")).unwrap();
        assert_eq!(env, "# how\nAPP_LANGUAGE=python\nAPP_KIND=port\nAPP_PORT=8999\nAPP_HEALTH=/health\n");
        set_type(&d, "r", false, &|| unreachable!()).unwrap();
        assert!(std::fs::read_to_string(d.join(".cove/app.env")).unwrap().contains("APP_KIND=module\nAPP_PORT=8999"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
