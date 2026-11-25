//! Prefix environments, resolved once (2026-10-15).
//!
//! An environment is a list of the stack's Lmod modules (`env/r4.6_py3.13-release`,
//! perhaps with `GDAL/3.9` layered on). Putting one in effect - `. use.sh && ml …`
//! - took 4.3 s on this host against 0.3 s for a bare login shell, and Cove paid
//! it again for every R start (twice: once to find reticulate's Python), every
//! terminal, every package list and every render.
//!
//! So an environment is resolved ONCE, as RStudio Workbench does for each entry
//! of its r-versions file (`detectREnvironment`: run the module load in a
//! shell, keep the environment it leaves, cache it) and as Positron does with a
//! runtime's metadata (the interpreter's path and version, found by discovery,
//! re-validated before each start). What is kept here:
//!
//! - `script`: the variables the load set or cleared, as a file of `export` /
//!   `unset` lines. Sourcing it (0.45 s measured, use.sh included) is the load.
//!   Lmod's own state (_ModuleTable…, LOADEDMODULES) is among them, so an
//!   `ml jq` afterwards layers on top as it would after the real load.
//! - the R and the Python the environment brings, with their versions.
//!
//! The cache is keyed by the module list and stamped with the stack's catalog
//! revision and this platform's module tree: a publish or a rebuild resolves
//! again. It lives in memory and under the cache dir, so a restarted server
//! starts warm. Every launcher goes through here (kernel.rs `module_prefix`,
//! the terminal, packages, renders), so they all see one environment.
//!
//! The project side, `.ndexr-env`: a folder may name its environment, as an
//! RStudio project's `RVersion:` does - one line of module names, committed
//! with the code. Cove's domains, the terminal, and the RStudio and Positron
//! sessions multiuser starts on a folder all honour it.

use crate::api::shell_quote;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// An interpreter an environment brings: its path and its version.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Runtime {
    pub exe: String,
    pub version: String,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Resolved {
    /// The module list, one space between names.
    pub modules: String,
    /// What it was resolved against (`stamp`); another means resolve again.
    pub stamp: String,
    /// The `export` / `unset` lines that put it in effect.
    pub script: PathBuf,
    pub r: Option<Runtime>,
    pub python: Option<Runtime>,
}

impl Resolved {
    /// Still what it says: the stack has not moved and its files are there
    /// (Positron's `validateMetadata` before a start).
    fn valid(&self, stamp: &str) -> bool {
        self.stamp == stamp && self.script.is_file() && self.r.as_ref().is_none_or(|r| Path::new(&r.exe).is_file())
    }
}

/// One space between names; a word that is not a module name refuses.
pub fn normalise(modules: &str) -> Result<String, String> {
    let m = modules.split_whitespace().collect::<Vec<_>>().join(" ");
    match m.split(' ').find(|w| !w.is_empty() && !crate::host::kernel::valid_module(w)) {
        Some(bad) => Err(format!("not a module name: {bad}")),
        None => Ok(m),
    }
}

/// The stack as it stands: its catalog revision and the modification time of
/// this platform's module tree (a stack built here has no revision).
fn stamp() -> String {
    let root = crate::host::stack_root();
    let tree = root.join(crate::host::own_triple()).join("eb/modules/all");
    let mtime = std::fs::metadata(&tree).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0);
    format!("{}:{mtime}", crate::host::stack_revision())
}

fn dir() -> PathBuf {
    crate::host::kernel::cache_dir().join("envs")
}

/// A file name for a module list: readable, and one per list.
fn file_key(modules: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325; // FNV-1a: stable across builds, unlike DefaultHasher
    for b in modules.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    let name: String = modules.chars().take(48).map(|c| if c.is_ascii_alphanumeric() || c == '.' { c } else { '_' }).collect();
    format!("{name}-{h:016x}")
}

/// One lock per module list: two consoles starting at once wait for one load.
fn slots() -> &'static Mutex<HashMap<String, Arc<Mutex<Option<Arc<Resolved>>>>>> {
    static S: OnceLock<Mutex<HashMap<String, Arc<Mutex<Option<Arc<Resolved>>>>>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// `modules` resolved: from memory, else from disk, else loaded now (once).
/// The errors are the ones a launch would give - no stack, a publish in
/// flight, a module Lmod refused.
pub fn resolve(modules: &str) -> Result<Arc<Resolved>, String> {
    let modules = normalise(modules)?;
    if modules.is_empty() {
        return Err("no modules to resolve".into());
    }
    let use_sh = crate::host::use_sh().ok_or_else(|| format!("{} — no module {modules} to load", crate::host::stack_absent()))?;
    if let Some(gap) = crate::host::stack_gap() {
        return Err(gap);
    }
    let slot = slots().lock().unwrap().entry(modules.clone()).or_default().clone();
    let mut held = slot.lock().unwrap();
    let now = stamp();
    if let Some(r) = held.as_ref().filter(|r| r.valid(&now)) {
        return Ok(r.clone());
    }
    let key = file_key(&modules);
    let meta = dir().join(format!("{key}.json"));
    if let Some(r) = std::fs::read(&meta).ok().and_then(|b| serde_json::from_slice::<Resolved>(&b).ok()).filter(|r| r.modules == modules && r.valid(&now)) {
        let r = Arc::new(r);
        *held = Some(r.clone());
        return Ok(r);
    }
    let r = Arc::new(load(&use_sh, &modules, &key, now)?);
    if let Ok(body) = serde_json::to_vec_pretty(&*r) {
        let _ = std::fs::write(&meta, body);
    }
    *held = Some(r.clone());
    Ok(r)
}

/// Resolve in the background, so the first console in it starts fast
/// (Positron's discovery at startup). Quiet: a launch reports what fails.
pub fn warm(modules: &str) {
    let m = modules.to_owned();
    if m.trim().is_empty() || cfg!(windows) {
        return;
    }
    std::thread::spawn(move || {
        let _ = resolve(&m);
    });
}

/// The shell prefix that puts `modules` in effect: the resolved script when
/// there is one, else the module load itself (a resolution that failed for
/// a reason of its own - a full disk - must not stop a console starting).
pub fn prefix(modules: &str) -> Result<String, String> {
    let modules = normalise(modules)?;
    // No ndexr stack on this computer at all: R, Python and the shell from
    // PATH, as a standalone IDE (2026-10-19) - not a refusal to start.
    if modules.is_empty() || !crate::host::stack_present() {
        return Ok(String::new());
    }
    match resolve(&modules) {
        Ok(r) => {
            let q = shell_quote(&r.script.display().to_string());
            Ok(format!("{{ . {q}; }} >/dev/null 2>&1 || {{ echo \"could not load module {modules} from {}\"; exit 1; }}; ", r.script.display()))
        }
        Err(e) if e.starts_with("could not load module") || e.starts_with("not a module") => Err(e),
        Err(e) if crate::host::use_sh().is_none() || crate::host::stack_gap().is_some() => Err(e),
        Err(_) => load_line(&modules),
    }
}

/// The module load as a launch line: what every launch did before resolving.
fn load_line(modules: &str) -> Result<String, String> {
    let use_sh = crate::host::use_sh().ok_or_else(|| format!("{} — no module {modules} to load", crate::host::stack_absent()))?;
    let q = shell_quote(&use_sh.display().to_string());
    Ok(format!(
        "{{ . {q} && {mu}ml {modules}; }} >/dev/null 2>&1 || {{ echo \"could not load module {modules} through {}\"; exit 1; }}; ",
        use_sh.display(),
        mu = crate::catalog::source::module_use_line(),
    ))
}

/// Run the load once, in the same kind of shell a launch uses, and keep the
/// environment before and after it, and the interpreters it brings.
fn load(use_sh: &Path, modules: &str, key: &str, stamp: String) -> Result<Resolved, String> {
    let d = dir();
    std::fs::create_dir_all(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    let work = d.join(format!(".load-{}-{key}", std::process::id()));
    let (before, after) = (work.with_extension("before"), work.with_extension("after"));
    let q = |p: &Path| shell_quote(&p.display().to_string());
    let cmd = format!(
        "umask 077; env -0 > {b} && {{ . {u} && {mu}ml {modules}; }} >/dev/null 2>&1 && env -0 > {a} && {{ \
         printf 'R\\t%s\\t%s\\n' \"$(command -v R)\" \"$(R --version 2>/dev/null | head -n 1)\"; \
         printf 'PY\\t%s\\n' \"$(python3 -c 'import sys; print(sys.executable + chr(9) + sys.version.split()[0])' 2>/dev/null)\"; \
         echo OK; }} || echo FAIL",
        b = q(&before),
        a = q(&after),
        u = q(use_sh),
        // The person's own environment revisions (source.rs), when they have any.
        mu = crate::catalog::source::module_use_line(),
    );
    let out = crate::host::runner::capture_within(&cmd, Duration::from_secs(120));
    let read = |p: &Path| std::fs::read(p).ok();
    let (b, a) = (read(&before), read(&after));
    let _ = std::fs::remove_file(&before);
    let _ = std::fs::remove_file(&after);
    let out = out.ok_or_else(|| format!("resolving {modules}: the shell gave no answer within 120 s"))?;
    if !out.lines().any(|l| l == "OK") {
        return Err(format!("could not load module {modules} through {}", use_sh.display()));
    }
    let (Some(b), Some(a)) = (b, a) else {
        return Err(format!("resolving {modules}: the environment was not written to {}", d.display()));
    };
    let script = d.join(format!("{key}.sh"));
    let tmp = script.with_extension("sh.new");
    std::fs::write(&tmp, script_text(modules, &parse_env(&b), &parse_env(&a))).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &script).map_err(|e| format!("{}: {e}", script.display()))?;
    let (r, python) = parse_probe(&out);
    Ok(Resolved { modules: modules.to_owned(), stamp, script, r, python })
}

/// `env -0` output as name → value.
fn parse_env(raw: &[u8]) -> HashMap<String, String> {
    raw.split(|&b| b == 0)
        .filter_map(|kv| {
            let kv = String::from_utf8_lossy(kv);
            let (k, v) = kv.split_once('=')?;
            Some((k.to_owned(), v.to_owned()))
        })
        .collect()
}

/// A variable the replay leaves alone: the shell's own, and exported bash
/// functions (`BASH_FUNC_ml%%`) - `ml` itself comes from use.sh, which every
/// shell that wants it sources.
fn shells_own(k: &str) -> bool {
    matches!(k, "PWD" | "OLDPWD" | "SHLVL" | "_") || k.starts_with("BASH_FUNC_") || !k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || k.is_empty()
}

/// The `export` / `unset` lines that turn `before` into `after`, sorted so
/// the file reads the same each time it is made.
fn script_text(modules: &str, before: &HashMap<String, String>, after: &HashMap<String, String>) -> String {
    let mut set: Vec<_> = after.iter().filter(|(k, v)| !shells_own(k) && before.get(*k) != Some(v)).collect();
    set.sort();
    let mut gone: Vec<_> = before.keys().filter(|k| !shells_own(k) && !after.contains_key(*k)).collect();
    gone.sort();
    let mut s = format!("# Cove (envs.rs): `ml {modules}`, resolved - sourced in place of the load\n");
    for k in gone {
        s.push_str(&format!("unset {k}\n"));
    }
    for (k, v) in set {
        s.push_str(&export_line(k, v, before.get(k)));
    }
    s
}

/// One `export`. A path list the load only added to (Lmod prepends, and
/// drops duplicates as it goes) is written against the shell's own value:
/// the entries it added before the first one that was there already go in
/// front, the rest behind. So a shell whose PATH differs from the resolving
/// one's - the Terminal's, after the person's ~/.bashrc - keeps its own
/// entries. A load that took an entry away is written whole. No stray ':'
/// when the shell's value is empty.
fn export_line(k: &str, v: &str, before: Option<&String>) -> String {
    let whole = format!("export {k}={}\n", shell_quote(v));
    let Some(b) = before.filter(|b| !b.is_empty()) else { return whole };
    let had: Vec<&str> = b.split(':').filter(|e| !e.is_empty()).collect();
    let now: Vec<&str> = v.split(':').collect();
    if had.is_empty() || had.iter().any(|e| !now.contains(e)) || now.iter().any(|e| e.is_empty()) {
        return whole;
    }
    let first_old = now.iter().position(|e| had.contains(e)).unwrap_or(now.len());
    let added = |r: &[&str]| r.iter().filter(|e| !had.contains(e)).copied().collect::<Vec<_>>().join(":");
    let (front, back) = (added(&now[..first_old]), added(&now[first_old..]));
    let own = format!("${k}");
    match (front.is_empty(), back.is_empty()) {
        (true, true) => whole,
        (false, true) => format!("export {k}={}\"${{{k}:+:{own}}}\"\n", shell_quote(&front)),
        (true, false) => format!("export {k}=\"${{{k}:+{own}:}}\"{}\n", shell_quote(&back)),
        (false, false) => format!("export {k}={}\"${{{k}:+:{own}}}\"{}\n", shell_quote(&front), shell_quote(&format!(":{back}"))),
    }
}

/// The probe's R and PY lines. "R version 4.6.1 (2026-06-24) -- …" → 4.6.1.
fn parse_probe(out: &str) -> (Option<Runtime>, Option<Runtime>) {
    let (mut r, mut py) = (None, None);
    for l in out.lines() {
        let f: Vec<&str> = l.split('\t').collect();
        match f.as_slice() {
            ["R", exe, banner] if exe.starts_with('/') => {
                let version = banner.strip_prefix("R version ").and_then(|v| v.split_whitespace().next()).unwrap_or_default().to_owned();
                r = Some(Runtime { exe: exe.to_string(), version });
            }
            ["PY", exe, version] if exe.starts_with('/') => py = Some(Runtime { exe: exe.to_string(), version: version.to_string() }),
            _ => {}
        }
    }
    (r, py)
}

// ── the project's own environment: .ndexr-env ───────────────────────────────

pub const PIN_FILE: &str = ".ndexr-env";

/// The environment `dir` names in its `.ndexr-env`: the first line that is
/// not blank or a `#` comment, as module names. None when there is no file,
/// or its line holds something that is not a module name.
pub fn pinned(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join(PIN_FILE)).ok()?;
    let line = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
    normalise(line).ok().filter(|m| !m.is_empty())
}

/// Name `modules` as `dir`'s environment, or ("") stop naming one.
pub fn pin(dir: &Path, modules: &str) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("{} is not a folder", dir.display()));
    }
    let f = dir.join(PIN_FILE);
    let modules = normalise(modules)?;
    if modules.is_empty() {
        return match std::fs::remove_file(&f) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(format!("{}: {e}", f.display())),
            _ => Ok(()),
        };
    }
    let body = format!("# The ndexr stack environment this folder runs in (Cove, Inlet and Positron sessions; `ml` names).\n{modules}\n");
    std::fs::write(&f, body).map_err(|e| format!("{}: {e}", f.display()))
}

// ── what the project asks for: renv.lock, .python-version ───────────────────

/// The R version a project's renv.lock records (`"R": {"Version": "4.5.2"}`).
fn renv_r(dir: &Path) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(dir.join("renv.lock")).ok()?).ok()?;
    v.get("R")?.get("Version")?.as_str().map(str::to_owned)
}

/// The Python a project's .python-version names (pyenv, uv): "3.12" or "3.12.4".
fn python_version_file(dir: &Path) -> Option<String> {
    let t = std::fs::read_to_string(dir.join(".python-version")).ok()?;
    let v = t.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
    v.chars().next().is_some_and(|c| c.is_ascii_digit()).then(|| v.to_owned())
}

/// "4.5.2" → "4.5".
fn minor(v: &str) -> String {
    v.split('.').take(2).collect::<Vec<_>>().join(".")
}

/// The env/ module, out of `envs` (newest first), that brings R `r` and/or
/// Python `py` at their minor version - a -release before a -devel.
fn matching_env(envs: &[String], r: Option<&str>, py: Option<&str>) -> Option<String> {
    let fits = |m: &&String| {
        let set = m.strip_prefix("env/").unwrap_or(m);
        r.is_none_or(|r| set.starts_with(&format!("r{}_", minor(r)))) && py.is_none_or(|p| set.contains(&format!("_py{}-", minor(p))))
    };
    envs.iter().filter(fits).find(|m| m.ends_with("-release")).or_else(|| envs.iter().find(fits)).cloned()
}

/// What to tell a person whose project asks for another R or Python than
/// the environment it runs in (RStudio's renv "project requested R version
/// X but is using Y"), with the env/ module that would match - or None.
/// `envs` is only asked when there is a mismatch to explain.
pub fn mismatch(dir: &Path, running: &Resolved, envs: impl FnOnce() -> Vec<String>) -> Option<String> {
    let want_r = renv_r(dir);
    let want_py = python_version_file(dir);
    let off = |want: &Option<String>, have: Option<&Runtime>| match (want, have) {
        (Some(w), Some(h)) if !h.version.is_empty() => minor(w) != minor(&h.version),
        _ => false,
    };
    let (r_off, py_off) = (off(&want_r, running.r.as_ref()), off(&want_py, running.python.as_ref()));
    if !r_off && !py_off {
        return None;
    }
    let mut said = Vec::new();
    if r_off {
        said.push(format!("renv.lock records R {} - this session runs R {}", want_r.as_deref().unwrap_or("?"), running.r.as_ref().map_or("?", |r| r.version.as_str())));
    }
    if py_off {
        said.push(format!(".python-version names Python {} - this session runs Python {}", want_py.as_deref().unwrap_or("?"), running.python.as_ref().map_or("?", |p| p.version.as_str())));
    }
    let hint = match matching_env(&envs(), want_r.as_deref(), want_py.as_deref()) {
        Some(m) => format!("{m} matches: pick it from Environment, then Pin keeps it for this folder"),
        None => "no environment on the stack matches".to_owned(),
    };
    Some(format!("{}. {hint}.", said.join("; ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn the_script_holds_only_what_the_load_changed() {
        let before = env(&[("PATH", "/usr/bin"), ("HOME", "/h"), ("GONE", "x"), ("PWD", "/a"), ("SHLVL", "1")]);
        let after = env(&[("PATH", "/stack/bin:/usr/bin"), ("HOME", "/h"), ("EBROOTR", "/stack/R it's"), ("PWD", "/b"), ("SHLVL", "2"), ("BASH_FUNC_ml%%", "() { :; }")]);
        let s = script_text("env/x", &before, &after);
        assert!(s.contains("export PATH='/stack/bin'\"${PATH:+:$PATH}\"\n"), "{s}");
        assert!(s.contains(r"export EBROOTR='/stack/R it'\''s'"));
        assert!(s.contains("unset GONE\n"));
        assert!(!s.contains("HOME") && !s.contains("PWD") && !s.contains("SHLVL") && !s.contains("BASH_FUNC"));
    }

    #[test]
    fn a_replayed_script_is_the_load() {
        // P as Lmod leaves it: its entries in front, the duplicate /usr/bin dropped.
        let before = env(&[("A", "1"), ("B", "gone"), ("P", "/usr/bin:/bin:/usr/bin"), ("Q", "/x"), ("D", "/a:/b")]);
        let after = env(&[("A", "multi\nline 'q' $HOME"), ("C", "new"), ("P", "/stack/bin:/stack/sbin:/usr/bin:/bin"), ("Q", "/x:/y"), ("D", "/a")]);
        let dir = std::env::temp_dir().join(format!("cove-envs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("s.sh");
        std::fs::write(&f, script_text("m", &before, &after)).unwrap();
        let out = std::process::Command::new("bash")
            .args(["--noprofile", "--norc", "-c", &format!(". {}; printf '%s|%s|%s|%s|%s|%s' \"$A\" \"${{B-unset}}\" \"$C\" \"$P\" \"$Q\" \"$D\"", f.display())])
            .env("A", "1")
            .env("B", "gone")
            // This shell's own PATH differs from the resolving shell's; Q is empty here.
            .env("P", "/home/me/bin:/usr/bin")
            .env("Q", "")
            .env("D", "/mine")
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(String::from_utf8_lossy(&out.stdout), "multi\nline 'q' $HOME|unset|new|/stack/bin:/stack/sbin:/home/me/bin:/usr/bin|/y|/a");
    }

    #[test]
    fn env_dumps_split_on_nul_and_keep_equals_in_values() {
        let e = parse_env(b"A=1\0B=x=y\0\0C=\0");
        assert_eq!(e.get("B").map(String::as_str), Some("x=y"));
        assert_eq!(e.get("C").map(String::as_str), Some(""));
        assert_eq!(e.len(), 3);
    }

    #[test]
    fn the_probe_reads_both_interpreters() {
        let (r, py) = parse_probe("R\t/s/R/bin/R\tR version 4.6.1 (2026-06-24) -- \"Happy Hop\"\nPY\t/s/py/bin/python3\t3.13.15\nOK");
        assert_eq!(r, Some(Runtime { exe: "/s/R/bin/R".into(), version: "4.6.1".into() }));
        assert_eq!(py.unwrap().version, "3.13.15");
        let (r, py) = parse_probe("R\t\t\nPY\t\nOK");
        assert!(r.is_none() && py.is_none());
    }

    #[test]
    fn module_lists_are_tidied_or_refused() {
        assert_eq!(normalise("  env/a   jq ").unwrap(), "env/a jq");
        assert_eq!(normalise("").unwrap(), "");
        assert!(normalise("env/a; id").is_err());
        assert!(normalise("$(id)").is_err());
        assert_ne!(file_key("env/a"), file_key("env/a jq"));
        assert!(!file_key("env/a b/c").contains('/'));
    }

    #[test]
    fn a_pin_round_trips_and_ignores_comments() {
        let dir = std::env::temp_dir().join(format!("cove-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(pinned(&dir), None);
        pin(&dir, "env/r4.5_py3.12-release  GDAL/3.9").unwrap();
        assert_eq!(pinned(&dir).as_deref(), Some("env/r4.5_py3.12-release GDAL/3.9"));
        std::fs::write(dir.join(PIN_FILE), "# mine\n\n env/r4.4_py3.11-release \n").unwrap();
        assert_eq!(pinned(&dir).as_deref(), Some("env/r4.4_py3.11-release"));
        std::fs::write(dir.join(PIN_FILE), "env/x; rm -rf /\n").unwrap();
        assert_eq!(pinned(&dir), None);
        assert!(pin(&dir, "a b; c").is_err());
        pin(&dir, "").unwrap();
        assert!(!dir.join(PIN_FILE).exists());
        pin(&dir, "").unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_project_that_asks_for_another_r_is_told_which_env_matches() {
        let dir = std::env::temp_dir().join(format!("cove-renv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("renv.lock"), r#"{"R": {"Version": "4.5.2", "Repositories": []}, "Packages": {}}"#).unwrap();
        let running = Resolved { r: Some(Runtime { exe: "/r".into(), version: "4.6.1".into() }), python: Some(Runtime { exe: "/p".into(), version: "3.13.15".into() }), ..Default::default() };
        let envs = || vec!["env/r4.6_py3.13-release".to_owned(), "env/r4.5_py3.13-devel".to_owned(), "env/r4.5_py3.13-release".to_owned(), "env/r4.5_py3.12-release".to_owned()];
        let said = mismatch(&dir, &running, envs).unwrap();
        assert!(said.contains("R 4.5.2") && said.contains("R 4.6.1"), "{said}");
        assert!(said.contains("env/r4.5_py3.13-release matches"), "{said}");
        std::fs::write(dir.join(".python-version"), "3.12\n").unwrap();
        assert!(mismatch(&dir, &running, envs).unwrap().contains("env/r4.5_py3.12-release matches"));
        // The same minor version is no mismatch, whatever the patch level.
        std::fs::write(dir.join("renv.lock"), r#"{"R": {"Version": "4.6.0"}}"#).unwrap();
        std::fs::remove_file(dir.join(".python-version")).unwrap();
        assert_eq!(mismatch(&dir, &running, || panic!("not asked without a mismatch")), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
