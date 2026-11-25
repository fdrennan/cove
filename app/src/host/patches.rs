//! Patched software, read from what each install recorded (operator,
//! 2026-10-22: "patched software needs to be visible in the atlas").
//!
//! EasyBuild keeps, beside every module it installs, a copy of each patch it
//! applied (`eb/software/<name>/<version>/easybuild/*.patch`). That copy is
//! what was applied, not what a recipe says. A patch whose file is also in the
//! kit's own easyconfigs (ndexr.ndexr.io's stack-kit) is ndexr's; any other
//! came with EasyBuild's upstream recipe. Its reason is the text a patch file
//! carries before its first hunk header, where both write why.

use std::path::{Path, PathBuf};

use crate::api::{Patch, Patched};

/// Every patched module of this machine's stack, newest scan kept ten minutes.
pub fn patched() -> Vec<Patched> {
    use std::time::{Duration, Instant};
    static LAST: std::sync::Mutex<Option<(Instant, Vec<Patched>)>> = std::sync::Mutex::new(None);
    if let Some((at, v)) = LAST.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(600) {
            return v.clone();
        }
    }
    let soft = crate::host::stack_root().join(crate::host::own_triple()).join("eb/software");
    let ours = kit_patches();
    let v = scan(&soft, &ours);
    *LAST.lock().unwrap() = Some((Instant::now(), v.clone()));
    v
}

/// The patch files the kit carries, by file name.
fn kit_patches() -> std::collections::HashSet<String> {
    let root = Path::new(crate::host::PLATFORM_DOMAINS).join("ndexr.ndexr.io/crate/assets/stack-kit/easybuild-suite/easyconfigs");
    let mut out = std::collections::HashSet::new();
    let mut stack = vec![root];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "patch") {
                out.insert(e.file_name().to_string_lossy().into_owned());
            }
        }
    }
    out
}

fn scan(soft: &Path, ours: &std::collections::HashSet<String>) -> Vec<Patched> {
    let mut out = Vec::new();
    let mut names: Vec<PathBuf> = std::fs::read_dir(soft).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    names.sort();
    for n in names {
        let mut versions: Vec<PathBuf> = std::fs::read_dir(&n).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
        versions.sort();
        for v in versions {
            let mut files: Vec<PathBuf> = std::fs::read_dir(v.join("easybuild")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "patch")).collect();
            if files.is_empty() {
                continue;
            }
            files.sort();
            let patches = files
                .iter()
                .map(|f| {
                    let file = f.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    let text = std::fs::read_to_string(f).unwrap_or_default();
                    let (why, added, removed) = read_patch(&text);
                    Patch { ours: ours.contains(&file), file, why, added, removed }
                })
                .collect();
            out.push(Patched {
                name: n.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                version: v.file_name().unwrap_or_default().to_string_lossy().into_owned(),
                patches,
            });
        }
    }
    out
}

// ── what an install was built from (2026-10-28) ──────────────────────────────
//
// Operator: inside Cove a person can "explore an application's upstream
// source, compare modifications, inspect build provenance". Each install's
// `easybuild/` folder says it; the public copy of every file is under the
// stack's records/ (ndexr.ndexr.io crate, `ndexr stack promote --records`),
// so each line here is also a link anyone can open.

/// Where the stack's public sources and build records are (site setting
/// stack.clone_url of the ndexr CLI).
pub const PUBLIC: &str = "https://ndexr-stack-public.s3.us-east-2.amazonaws.com/stack";

/// Vendor archives not copied to the public sources (the CLI's NOT_MIRRORED).
const WITHHELD: &[(&str, &str)] = &[
    ("c/CUDA/", "NVIDIA's licence does not allow handing the installer on"),
    ("c/cuDNN/", "NVIDIA's licence does not allow handing the archive on"),
    ("p/Positron-Server/", "a vendor binary under Posit's licence"),
];

/// A path for a URL: S3 reads a literal `+` as a space.
fn url_path(rel: &str) -> String {
    rel.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// A file's sha256 by the system's `sha256sum` (no hashing crate in Cove).
fn sha256_file(p: &Path) -> String {
    std::process::Command::new("sha256sum")
        .arg(p)
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .and_then(|s| s.split_whitespace().next().map(str::to_string))
        .filter(|h| h.len() == 64)
        .unwrap_or_default()
}

/// The upstream files a build used and the sha256 it checked, from the
/// EasyBuild log's `Checksum verification for .../.state/sources/<rel> using
/// {'<file>': '<sha256>'}` lines (patches come from the recipe's folder).
fn log_sources(log: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for l in log.lines() {
        let Some(rest) = l.split("Checksum verification for ").nth(1) else { continue };
        let path = rest.split(" using ").next().unwrap_or("");
        let Some(rel) = path.split("/.state/sources/").nth(1) else { continue };
        let sha = rest.split('\'').filter(|w| w.len() == 64 && w.bytes().all(|c| c.is_ascii_hexdigit())).next().unwrap_or("").to_lowercase();
        if !out.iter().any(|(r, _)| r == rel) {
            out.push((rel.to_string(), sha));
        }
    }
    out
}

/// Every installed version of module `name` on this machine, as built.
pub fn record(name: &str) -> Vec<crate::api::BuildRecord> {
    if name.is_empty() || name.contains('/') || name.starts_with('.') {
        return vec![];
    }
    let triple = crate::host::own_triple();
    let dir = crate::host::stack_root().join(&triple).join("eb/software").join(name);
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.join("easybuild").is_dir()).collect();
    versions.sort();
    versions.iter().filter_map(|v| record_of(name, v, &triple)).collect()
}

fn record_of(name: &str, vdir: &Path, triple: &str) -> Option<crate::api::BuildRecord> {
    use crate::api::{BuildRecord, RecordFile};
    let eb = vdir.join("easybuild");
    let version = vdir.file_name()?.to_string_lossy().into_owned();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&eb).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    files.sort();
    let fname = |p: &PathBuf| p.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let log = files.iter().filter(|p| { let f = fname(p); f.starts_with("easybuild-") && f.ends_with(".log") }).next_back()?;
    let build = fname(log).trim_end_matches(".log").rsplit('-').next().unwrap_or("").to_string();
    let text = std::fs::read_to_string(log).unwrap_or_default();
    let here = format!("{PUBLIC}/records/{}", url_path(&format!("{name}/{version}/{triple}/{build}")));
    let file = |p: &PathBuf| RecordFile {
        file: fname(p),
        url: format!("{here}/{}", url_path(&fname(p))),
        sha256: sha256_file(p),
        note: String::new(),
    };
    let recipe = files.iter().find(|p| fname(p).ends_with(".eb")).map(&file).unwrap_or_default();
    let homepage = files.iter().find(|p| fname(p).ends_with(".eb")).and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|t| t.lines().find_map(|l| l.strip_prefix("homepage").map(|r| r.trim_start_matches([' ', '=']).trim().trim_matches(['\'', '"']).to_string())))
        .unwrap_or_default();
    let sources = log_sources(&text)
        .into_iter()
        .map(|(rel, sha)| {
            let held = WITHHELD.iter().find(|(p, _)| rel.starts_with(p)).map(|(_, why)| why.to_string());
            RecordFile {
                file: rel.rsplit('/').next().unwrap_or(&rel).to_string(),
                url: if held.is_some() { String::new() } else { format!("{PUBLIC}/sources/{}", url_path(&rel)) },
                sha256: sha,
                note: held.unwrap_or_default(),
            }
        })
        .collect();
    Some(BuildRecord {
        name: name.to_string(),
        records: format!("{PUBLIC}/records/{}/index.html", url_path(&format!("{name}/{version}"))),
        easybuild: text.lines().find_map(|l| l.split("This is EasyBuild ").nth(1)).and_then(|r| r.split_whitespace().next()).unwrap_or("").to_string(),
        version,
        triple: triple.to_string(),
        build,
        homepage,
        recipe,
        log: file(log),
        sources,
        patches: files.iter().filter(|p| fname(p).ends_with(".patch")).map(file).collect(),
    })
}

/// The text of one applied patch, as the install recorded it.
pub fn patch_text(name: &str, version: &str, file: &str) -> Option<String> {
    let bad = |s: &str| s.is_empty() || s.contains('/') || s.starts_with('.');
    if bad(name) || bad(version) || bad(file) || !file.ends_with(".patch") {
        return None;
    }
    let p = crate::host::stack_root().join(crate::host::own_triple()).join("eb/software").join(name).join(version).join("easybuild").join(file);
    std::fs::read_to_string(p).ok()
}

/// A patch's reason (the text before its first file header, at most 600
/// characters) and how many lines it adds and removes.
fn read_patch(text: &str) -> (String, u32, u32) {
    let mut why = String::new();
    let (mut added, mut removed, mut body) = (0u32, 0u32, false);
    for l in text.lines() {
        if l.starts_with("diff ") || l.starts_with("--- ") || l.starts_with("Index: ") || l.starts_with("===") {
            body = true;
        }
        if !body {
            if why.len() < 600 && !l.trim().is_empty() {
                if !why.is_empty() {
                    why.push(' ');
                }
                why.push_str(l.trim());
            }
            continue;
        }
        if l.starts_with('+') && !l.starts_with("+++") {
            added += 1;
        } else if l.starts_with('-') && !l.starts_with("---") {
            removed += 1;
        }
    }
    if why.chars().count() > 600 {
        why = why.chars().take(600).collect::<String>() + "…";
    }
    (why, added, removed)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_patch_says_why_and_how_much() {
        let p = "ndexr: glibc 2.42 removed struct termio.\nRestore it.\n--- a/x.c\n+++ b/x.c\n@@ -1,2 +1,3 @@\n a\n-b\n+c\n+d\n";
        let (why, add, rm) = super::read_patch(p);
        assert_eq!(why, "ndexr: glibc 2.42 removed struct termio. Restore it.");
        assert_eq!((add, rm), (2, 1));
    }

    #[test]
    fn a_build_log_gives_each_source_its_checksum_once() {
        let sha = "912df414d1ac94e5ead85fa3e68f593f204d6851ef94862af35e78c52793eced";
        let log = format!("== INFO Checksum verification for /opt/x/.state/sources/p/pimp-my-rstudio/v2026.04.0+526.tar.gz using {{'v2026.04.0+526.tar.gz': '{sha}'}}\n== INFO Checksum verification for /opt/x/.state/sources/p/pimp-my-rstudio/v2026.04.0+526.tar.gz using {{'v2026.04.0+526.tar.gz': '{sha}'}}\n");
        assert_eq!(super::log_sources(&log), vec![("p/pimp-my-rstudio/v2026.04.0+526.tar.gz".to_string(), sha.to_string())]);
        assert_eq!(super::url_path("p/x/v2026.04.0+526.tar.gz"), "p/x/v2026.04.0%2B526.tar.gz");
    }

    #[test]
    fn a_patch_is_read_only_by_plain_names() {
        assert!(super::patch_text("../etc", "1", "x.patch").is_none());
        assert!(super::patch_text("R", "4.6.1", "../../passwd").is_none());
        assert!(super::patch_text("R", "4.6.1", "notes.txt").is_none());
    }

    /// On this machine: the RStudio fork's record names its upstream release
    /// with the checksum its build verified, and every patch with a public URL.
    #[test]
    fn the_stack_records_what_its_rstudio_fork_was_built_from() {
        let v = super::record("pimp-my-rstudio");
        if v.is_empty() {
            return;
        }
        let r = &v[0];
        assert!(r.sources.iter().any(|s| s.file.starts_with("v2026.04.0") && s.sha256.len() == 64 && s.url.contains("%2B")));
        assert!(r.patches.len() >= 13 && r.patches.iter().all(|p| p.url.starts_with(super::PUBLIC) && p.sha256.len() == 64));
        assert!(r.records.ends_with("/index.html") && !r.build.is_empty() && !r.easybuild.is_empty());
    }

    /// On this machine: GCCcore's own glibc patch is ndexr's, and EasyBuild's
    /// upstream patches are told apart from it.
    #[test]
    fn the_stack_names_its_patched_modules() {
        let v = super::patched();
        if v.is_empty() {
            return;
        }
        let gcc = v.iter().find(|p| p.name == "GCCcore").expect("GCCcore is patched");
        assert!(gcc.patches.iter().any(|p| p.ours && p.file.contains("glibc242") && p.why.contains("glibc")));
        assert!(v.iter().flat_map(|p| &p.patches).any(|p| !p.ours));
        let ours = v.iter().filter(|p| p.patches.iter().any(|x| x.ours)).count();
        println!("{} patched modules, {ours} with ndexr's own patches", v.len());
    }
}
