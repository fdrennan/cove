//! The ndexr stack on this machine, kept out of the person's way (operator,
//! 2026-10-13: "end users ... get the environment without having to manage
//! their environment at all so like cvmfs setup will be in the background for
//! them"). The environment comes from us: stack.ndexr.io on CernVM-FS, at
//! /opt/ndexr/stack, the path every binary was built at. What the person may
//! set is their CVMFS client's proxy and cache, nothing about the repository.
//!
//!   view    is it mounted, the client's effective configuration
//!           (`cvmfs_config showconfig`, no root), and how a change would get
//!           root here: root itself, `sudo` without a password, or one
//!           `pkexec` prompt on a desktop. None of them: the command is shown.
//!   setup   `ndexr install cvmfs --yes` - the ONE canonical client setup
//!           (client, trust anchor, config, fstab mount; cvmfs.ndexr.io) -
//!           as a background job, then the person's settings on top.
//!   apply   the person's settings to config.d/stack.ndexr.io.local, CVMFS's
//!           own override file, and `cvmfs_config reload`.
//!   test    a proxy tried against each of our servers, no root, nothing kept.
//!
//! The jobs (and the renders, host.rs) share `jobs`: one running per kind, its
//! output kept for the UI to poll.

use std::collections::HashMap;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::api::{shell_quote, ConfigLine, CvmfsSettings, JobView, ProxyCheck, StackView};
use crate::runner::capture_within;

/// The repository the IDE is coupled to (operator, 2026-10-13).
pub const REPO: &str = "stack.ndexr.io";
/// Where the person's settings go: read after config.d/<repo>.conf, so they
/// win over the proxy our setup writes there.
const LOCAL: &str = "/etc/cvmfs/config.d/stack.ndexr.io.local";
/// Our servers when no client is configured yet to say them (cvmfs.ndexr.io
/// CONTEXT.md: S3 first, the publisher behind it).
const SERVERS: &str = "http://ndexr.s3.us-east-2.amazonaws.com/cvmfs/@fqrn@;http://cvmfs.ndexr.io/cvmfs/@fqrn@";
/// The parameters the person sees; the rest of the client is ours.
const SHOWN: [&str; 6] = ["CVMFS_SERVER_URL", "CVMFS_KEYS_DIR", "CVMFS_HTTP_PROXY", "CVMFS_CACHE_BASE", "CVMFS_QUOTA_LIMIT", "CVMFS_MOUNT_DIR"];

// ── jobs ────────────────────────────────────────────────────────────────────

pub mod jobs {
    use super::*;

    /// Lines kept per job; a long build log keeps its end.
    const KEEP: usize = 2000;

    fn slots() -> &'static Mutex<HashMap<&'static str, Arc<Mutex<JobView>>>> {
        static S: OnceLock<Mutex<HashMap<&'static str, Arc<Mutex<JobView>>>>> = OnceLock::new();
        S.get_or_init(Default::default)
    }

    pub type Done = Box<dyn FnOnce(bool, &mut JobView) + Send>;

    /// Run `cmd` in a login shell (output and errors together) as job `kind`;
    /// refused while one of that kind runs. `done` sees the outcome and may
    /// add to the view (a render names its output there).
    ///
    /// The output goes to a log file, read back here, never a pipe into this
    /// process: Update replaces the session's process, and a job writing into
    /// a pipe whose reader is gone dies on its next line (SIGPIPE). A podman
    /// build in Containers died that way mid-install when its session took an
    /// update (2026-10-20, 00:12). In its own process group, writing to a file,
    /// it outlives the session and finishes; what it made is still there.
    pub fn start(kind: &'static str, what: String, cmd: String, done: Option<Done>) -> Result<JobView, String> {
        let slot = slots().lock().unwrap().entry(kind).or_default().clone();
        {
            let mut v = slot.lock().unwrap();
            if v.running {
                return Err(format!("{} is still running", v.what));
            }
            *v = JobView { what: what.clone(), running: true, ..Default::default() };
        }
        let dir = std::path::PathBuf::from(crate::host::home()).join(".cache/cove/jobs");
        let log = dir.join(format!("{kind}-{}.log", std::process::id()));
        if let Err(e) = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&log, b"")) {
            slot.lock().unwrap().running = false;
            return Err(format!("could not start {what}: {}: {e}", log.display()));
        }
        let mut c = Command::new(crate::runner::login_shell());
        c.args(["-lc", &crate::runner::with_path(&format!("{{ {cmd}\n}} >>{} 2>&1", shell_quote(&log.display().to_string())))])
            .env("NO_COLOR", "1")
            // A login shell runs ~/.bash_logout on `exit`, and Gentoo's
            // stock one runs `clear`: with no TERM, "TERM environment
            // variable not set." ended the log and stood as the reason a
            // setup failed (operator, 2026-11-19, a fresh Gentoo).
            .env("TERM", "dumb")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            c.process_group(0);
        }
        let mut child = c.spawn().map_err(|e| {
            slot.lock().unwrap().running = false;
            format!("could not start {what}: {e}")
        })?;
        pids().lock().unwrap().insert(kind, child.id());
        let s = slot.clone();
        std::thread::spawn(move || {
            let mut r = match std::fs::File::open(&log) {
                Ok(f) => BufReader::new(f),
                Err(e) => {
                    s.lock().unwrap().lines.push(format!("{}: {e}", log.display()));
                    let _ = child.wait();
                    return;
                }
            };
            // Follow the file as the job writes it: whole lines as they land,
            // then what is left once it has exited.
            let mut partial = String::new();
            let take = |r: &mut BufReader<std::fs::File>, partial: &mut String, last: bool| {
                let mut buf = String::new();
                while r.read_line(&mut buf).unwrap_or(0) > 0 {
                    partial.push_str(&buf);
                    buf.clear();
                    if !partial.ends_with('\n') && !last {
                        break;
                    }
                    let l = std::mem::take(partial);
                    let mut v = s.lock().unwrap();
                    v.lines.push(l.trim_end_matches(['\n', '\r']).to_owned());
                    if v.lines.len() > KEEP {
                        let cut = v.lines.len() - KEEP;
                        v.lines.drain(..cut);
                    }
                }
            };
            let status = loop {
                take(&mut r, &mut partial, false);
                match child.try_wait() {
                    Ok(Some(st)) => break Some(st),
                    Ok(None) => std::thread::sleep(Duration::from_millis(250)),
                    Err(_) => break None,
                }
            };
            take(&mut r, &mut partial, true);
            if !partial.is_empty() {
                s.lock().unwrap().lines.push(std::mem::take(&mut partial));
            }
            let _ = std::fs::remove_file(&log);
            let ok = status.is_some_and(|st| st.success());
            pids().lock().unwrap().remove(kind);
            let mut v = s.lock().unwrap();
            if let Some(f) = done {
                f(ok, &mut v);
            }
            v.ok = Some(ok);
            v.running = false;
        });
        Ok(view(kind))
    }

    /// Each running job's process group (it leads its own, process_group(0)).
    fn pids() -> &'static Mutex<HashMap<&'static str, u32>> {
        static P: OnceLock<Mutex<HashMap<&'static str, u32>>> = OnceLock::new();
        P.get_or_init(Default::default)
    }

    /// Stop job `kind` and everything it started (its whole process group):
    /// Software › Install's Stop. False when none of that kind runs.
    pub fn stop(kind: &str) -> bool {
        let Some(pid) = pids().lock().unwrap().get(kind).copied() else { return false };
        let _ = Command::new("kill").args(["-TERM", &format!("-{pid}")]).status();
        if let Some(v) = slots().lock().unwrap().get(kind) {
            v.lock().unwrap().lines.push("[stopped]".into());
        }
        true
    }

    pub fn view(kind: &str) -> JobView {
        slots().lock().unwrap().get(kind).map(|v| v.lock().unwrap().clone()).unwrap_or_default()
    }
}

// ── what the machine has ────────────────────────────────────────────────────

fn have(cmd: &str) -> bool {
    capture_within(&format!("command -v {cmd}"), Duration::from_secs(5)).is_some()
}

#[cfg(unix)]
fn is_root() -> bool {
    capture_within("id -u", Duration::from_secs(5)).as_deref() == Some("0")
}
#[cfg(not(unix))]
fn is_root() -> bool {
    false
}

/// How a root step gets root on this machine, without ever asking in a
/// terminal: root itself, a password-less sudo, or (the desktop app on a
/// desktop session only - a server has no screen to prompt on) pkexec.
pub(crate) fn root_by() -> String {
    if is_root() {
        return "root".into();
    }
    if Command::new("sudo").args(["-n", "true"]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success()) {
        return "sudo".into();
    }
    let desktop = std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if desktop && !crate::runner::serving() && have("pkexec") {
        return "pkexec".into();
    }
    String::new()
}

/// `script` run as root the way `how` says.
pub(crate) fn as_root(how: &str, script: &str) -> String {
    let q = shell_quote(script);
    match how {
        "root" => format!("sh -c {q}"),
        "sudo" => format!("sudo -n sh -c {q}"),
        _ => format!("pkexec sh -c {q}"),
    }
}

/// The ndexr CLI as an absolute path: pkexec runs with a bare PATH.
fn ndexr_cli() -> Option<String> {
    capture_within("command -v ndexr", Duration::from_secs(5))
}

/// `cvmfs_config showconfig`, the client's own answer of what is in force
/// and from which file. Only what the person needs to read.
fn showconfig() -> Vec<ConfigLine> {
    let Some(out) = capture_within(&format!("cvmfs_config showconfig {REPO} 2>/dev/null"), Duration::from_secs(15)) else {
        return Vec::new();
    };
    parse_showconfig(&out)
}

fn parse_showconfig(out: &str) -> Vec<ConfigLine> {
    let mut v: Vec<ConfigLine> = out
        .lines()
        .filter_map(|l| {
            let (key, rest) = l.split_once('=')?;
            if !SHOWN.contains(&key) {
                return None;
            }
            let (value, from) = rest.split_once("# from ").map(|(v, f)| (v.trim(), f.trim())).unwrap_or((rest.trim(), ""));
            Some(ConfigLine { key: key.to_owned(), value: value.trim_matches('"').to_owned(), from: from.to_owned() })
        })
        .collect();
    v.sort_by_key(|c| SHOWN.iter().position(|k| *k == c.key));
    v
}

/// What the person's settings last wrote (the file is world-readable).
fn local_settings() -> CvmfsSettings {
    parse_local(&std::fs::read_to_string(LOCAL).unwrap_or_default())
}

fn parse_local(text: &str) -> CvmfsSettings {
    let mut c = CvmfsSettings::default();
    for l in text.lines() {
        let Some((k, v)) = l.split_once('=') else { continue };
        let v = v.trim().trim_matches('"').to_owned();
        match k.trim() {
            "CVMFS_HTTP_PROXY" => c.proxy = v,
            "CVMFS_CACHE_BASE" => c.cache_base = v,
            "CVMFS_QUOTA_LIMIT" => c.quota_mb = v,
            _ => {}
        }
    }
    c
}

/// The file `apply` writes; empty settings write none.
fn local_file(c: &CvmfsSettings) -> String {
    let mut s = String::from("# Cove - this machine's own CVMFS settings for stack.ndexr.io\n# (Tools > Global Options > Environment). The repository, servers and keys\n# come from config.d/stack.ndexr.io.conf; this file only overrides them.\n");
    for (k, v) in [("CVMFS_HTTP_PROXY", &c.proxy), ("CVMFS_CACHE_BASE", &c.cache_base), ("CVMFS_QUOTA_LIMIT", &c.quota_mb)] {
        let v = v.trim();
        if !v.is_empty() {
            s.push_str(&format!("{k}=\"{v}\"\n"));
        }
    }
    s
}

/// The stack as host.rs sees it, plus the client around it.
pub fn view() -> StackView {
    let mut v = crate::host::stack_view();
    if !cfg!(target_os = "linux") {
        v.unsupported = "the ndexr stack is built for Linux - on this machine, connect to a server running it (cove --connect URL)".into();
        return v;
    }
    if !crate::host::arch_supported() {
        v.unsupported = format!("the ndexr stack is built for Linux on {}; this machine is {} - sign in and run on your cell (account menu › my cell)", crate::host::STACK_ARCHES.join(" and "), std::env::consts::ARCH);
        return v;
    }
    v.prefix = prefix();
    v.client = have("cvmfs2") || Path::new("/usr/bin/cvmfs2").exists();
    if v.client {
        v.config = showconfig();
    }
    v.local = local_settings();
    v.root_by = root_by();
    v.setup_cmd = "ndexr install cvmfs --yes".into();
    let dir = build_dir();
    v.building = dir.join(".build/up.json").is_file();
    v.build_dir = dir.display().to_string();
    v
}

/// The stack installed here and its build, from their own files - no network,
/// no ndexr CLI needed to read them (api.rs LocalInstall).
pub fn local_install() -> crate::api::LocalInstall {
    local_install_at(&build_dir(), &crate::host::stack_root(), &crate::host::own_triple())
}

pub fn local_install_at(dir: &Path, root: &Path, triple: &str) -> crate::api::LocalInstall {
    use crate::api::{LocalInstall, LocalStep};
    let b = dir.join(".build");
    let st: serde_json::Value = std::fs::read(b.join("up.json")).ok().and_then(|x| serde_json::from_slice(&x).ok()).unwrap_or_default();
    let tl: serde_json::Value = std::fs::read(b.join("up.timeline.json")).ok().and_then(|x| serde_json::from_slice(&x).ok()).unwrap_or_default();
    let steps: Vec<LocalStep> = tl["steps"].as_array().map(|a| a.iter().map(|s| LocalStep {
        kind: s["kind"].as_str().unwrap_or("").to_owned(),
        name: s["name"].as_str().unwrap_or("").to_owned(),
        version: s["version"].as_str().unwrap_or("").to_owned(),
        status: s["status"].as_str().unwrap_or("").to_owned(),
        started: s["started"].as_i64().unwrap_or(0),
        finished: s["finished"].as_i64().unwrap_or(0),
    }).collect()).unwrap_or_default();
    let with = |status: &str| steps.iter().filter(|s| s.status == status).cloned().collect::<Vec<_>>();
    let mut recent: Vec<LocalStep> = steps.iter().filter(|s| s.status == "done").cloned().collect();
    recent.sort_by_key(|s| std::cmp::Reverse(s.finished));
    recent.truncate(12);
    let tail = std::fs::read(b.join("up.log")).map(|x| {
        let t = String::from_utf8_lossy(&x[x.len().saturating_sub(16_384)..]).into_owned();
        let mut lines: Vec<String> = t.lines().filter(|l| !l.trim().is_empty()).map(str::to_owned).collect();
        let from = lines.len().saturating_sub(20);
        lines.drain(..from);
        lines
    }).unwrap_or_default();
    let alive = std::fs::read_to_string(b.join("up.pid")).ok().and_then(|p| p.split_whitespace().next().and_then(|n| n.parse::<u32>().ok())).is_some_and(|pid| Path::new(&format!("/proc/{pid}")).exists());
    let mut modules: Vec<String> = Vec::new();
    if let Ok(rd) = std::fs::read_dir(root.join(triple).join("eb/modules/all")) {
        for name in rd.flatten() {
            let n = name.file_name().to_string_lossy().into_owned();
            if let Ok(vs) = std::fs::read_dir(name.path()) {
                for v in vs.flatten() {
                    let v = v.file_name().to_string_lossy().trim_end_matches(".lua").to_owned();
                    if !v.starts_with('.') {
                        modules.push(format!("{n}/{v}"));
                    }
                }
            }
        }
    }
    modules.sort_by_key(|m| m.to_lowercase());
    LocalInstall {
        dir: dir.display().to_string(),
        root: root.display().to_string(),
        root_is_dir: root == dir,
        state: st["state"].as_str().unwrap_or("").to_owned(),
        step: st["step"].as_str().unwrap_or("").to_owned(),
        steps: st["steps"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default(),
        started: st["started"].as_i64().unwrap_or(0),
        finished: st["finished"].as_i64().unwrap_or(0),
        rc: st["rc"].as_i64(),
        alive,
        done: steps.iter().filter(|s| s.status == "done").count(),
        failed: steps.iter().filter(|s| s.status == "failed").count(),
        running: with("running"),
        failures: with("failed"),
        recent,
        tail,
        modules,
    }
}

/// Where Cove builds the stack itself: host.rs `stack_root` falls back to it
/// once its use.sh is there, so nothing needs setting afterwards.
pub fn build_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(crate::host::home()).join(".ndexr/stack")
}

/// Build the whole stack on this machine from upstream sources (`ndexr stack
/// up`): no root, no ndexr servers. It detaches and returns at once, so the
/// job is its start; the build itself runs for hours under its own
/// supervisor (`ndexr stack status|logs --dir`).
///
/// Experimental, and opt-in (operator, 2026-11-13: "users should be prompted
/// that an experimental build process can be allowed which does it for them
/// and that the errors can be sent back to the ndexr.io servers"): with
/// `report`, a failure is filed on complaints.ndexr.io (`--report-errors`)
/// and the run waits for the ndexr release that fixes it, upgrades and goes
/// again (`--follow-releases`).
pub fn build(report: bool) -> Result<JobView, String> {
    let report = report || reporting();
    let v = view();
    if !v.unsupported.is_empty() {
        return Err(v.unsupported);
    }
    let cli = ndexr_cli().ok_or("the ndexr CLI is not on this machine - install it first (cvmfs.ndexr.io has the guide)")?;
    let d = shell_quote(&v.build_dir);
    let flags = if report { " --report-errors --follow-releases" } else { "" };
    let cmd = format!(
        "echo '$ ndexr stack up --dir {d}{flags}'; {} stack up --dir {d}{flags} && echo && echo 'Building in the background. Follow it: ndexr stack logs --dir {d} -f'",
        shell_quote(&cli)
    );
    jobs::start("stack", "Building the ndexr stack here".into(), cmd, None)
}

/// The setup's root script: on Gentoo the client from emerge and the verified
/// configurator (setup's note), elsewhere `ndexr install cvmfs --yes`.
/// `get` tries again over IPv4 alone: Gentoo's curl resolves names itself
/// (c-ares), and a home router answered its IPv6 lookup of repo.ndexr.io with
/// nothing - "Could not resolve host ... answer with no data" - while the
/// system resolver and `curl -4` reached it (operator's laptop, 2026-11-19).
///
/// Every download in the run retries on any error, DNS included: a home
/// router answered one lookup in three with nothing ("Could not resolve host
/// ... answer with no data", Gentoo's curl, which resolves names itself), and
/// the configurator's own key download - which retries timeouts, not DNS -
/// failed the mount. A `curl` first on PATH adds the retries inside the one
/// call, so a caller reading `-w '%{http_code}'` still reads one code
/// (operator, 2026-11-19: "make sure it works through the cove app").
fn install_script(cli: &str) -> String {
    format!(
        "real=$(command -v curl) && shim=$(mktemp -d) && printf '#!/bin/sh\\nexec %s \"$@\" --retry 6 --retry-all-errors --retry-delay 2\\n' \"$real\" > \"$shim/curl\" && chmod 755 \"$shim/curl\" && PATH=\"$shim:$PATH\" && export PATH\n\
         if command -v emerge >/dev/null 2>&1; then\n\
         command -v cvmfs2 >/dev/null 2>&1 || {{ echo '$ emerge --noreplace net-fs/cvmfs   (Gentoo: the CernVM-FS client from its own tree, built from source - some minutes)'; emerge --noreplace --quiet-build=y net-fs/cvmfs || exit 1; }}\n\
         u={u}; f=$(mktemp)\n\
         get() {{ curl -fsSL --retry 4 --retry-all-errors --retry-delay 2 \"$@\" || curl -4 -fsSL --retry 2 --retry-delay 2 \"$@\"; }}\n\
         get \"$u\" -o \"$f\" || {{ echo \"could not download $u - this computer's DNS or network failed; Set up again tries once more\"; exit 1; }}\n\
         want=$(get \"$u.sha256\" | cut -d' ' -f1); got=$(sha256sum \"$f\" | cut -d' ' -f1)\n\
         [ -n \"$want\" ] || {{ echo \"could not download $u.sha256 to check the configurator - this computer's DNS or network failed; Set up again tries once more\"; exit 1; }}\n\
         [ \"$want\" = \"$got\" ] || {{ echo \"the configurator's sha256 did not match ($want, got $got) - not running it\"; exit 1; }}\n\
         echo \"$ bash cvmfs.sh   (the configurator ndexr install runs, sha256 verified)\"\n\
         env NDEXR_CVMFS_REPO={REPO} NDEXR_CVMFS_HOST=cvmfs.ndexr.io bash \"$f\"; r=$?; rm -f \"$f\"; exit $r\n\
         fi\n\
         {} install cvmfs --yes",
        shell_quote(cli),
        u = shell_quote("https://repo.ndexr.io/ndexr/install/cvmfs.sh"),
    )
}

/// File a failed job as one anonymous report (`ndexr report`: names, paths,
/// emails and IPs removed by the CLI, the machine's shape added), and say so
/// in the job's own log, or why it could not be sent.
pub(crate) fn report_failure(stage: &str, v: &mut JobView) {
    let Some(cli) = ndexr_cli() else {
        v.lines.push("(not reported: the ndexr CLI is not on this machine)".into());
        return;
    };
    let dir = std::path::PathBuf::from(crate::host::home()).join(".cache/cove/jobs");
    let file = dir.join(format!("report-{}.log", std::process::id()));
    if std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&file, report_lines(&v.lines))).is_err() {
        return;
    }
    let msg = format!("Cove: {} failed - {}", v.what, v.why());
    let sent = Command::new(&cli)
        .args(["report", &msg, "--file", &file.display().to_string(), "--stage", stage])
        .stdin(Stdio::null())
        .output();
    let _ = std::fs::remove_file(&file);
    v.lines.push(match sent {
        Ok(o) if o.status.success() => "Sent to ndexr as an anonymous report (you allowed reports at first start; Settings changes it).".into(),
        Ok(o) => format!("(the report was not sent: {})", String::from_utf8_lossy(&o.stderr).lines().last().unwrap_or("").trim()),
        Err(e) => format!("(the report was not sent: {e})"),
    });
}

/// What a report carries of a job's log: its end, without progress chatter.
/// The board keeps a few thousand characters of a report, and emerge's
/// ">>> Jobs: 3 of 14 complete" lines filled them - report #196 (a fresh
/// Gentoo, 2026-11-19) stopped mid-build and never showed why it failed.
fn report_lines(lines: &[String]) -> String {
    let kept: Vec<&str> = lines.iter().map(|l| l.trim_end()).filter(|l| !l.is_empty() && !l.starts_with(">>> Jobs:") && !l.contains("Load avg:")).collect();
    kept[kept.len().saturating_sub(80)..].join("\n")
}

/// The person said yes to anonymous reports, and is not airgapped (api.rs
/// Settings::report): failed builds and setups go home as requests.
pub fn reporting() -> bool {
    let s = crate::host::saved_settings();
    s.report == Some(true) && !s.airgap
}

/// This machine's Gentoo Prefix in the stack, or "" (cheap: no client probes).
pub fn prefix() -> String {
    let v = crate::host::stack_root();
    let mine = crate::host::own_triple();
    let p = v.join(&mine).join("prefix").join(&mine);
    if p.is_dir() {
        p.display().to_string()
    } else {
        String::new()
    }
}

// ── setup, apply, test ──────────────────────────────────────────────────────

/// A language set's packages to this machine, as root: `ndexr pkgs sync
/// <set>` puts them where its module looks (/opt/ndexr/site-library), as on
/// ndexr's own servers. The Julia set is one archive of some 2.3 GB.
pub fn pkgs_sync(set: &str) -> Result<JobView, String> {
    if set.is_empty() || !set.chars().all(|c| c.is_ascii_alphanumeric() || "./_-".contains(c)) {
        return Err(format!("{set:?} is not a package set's name (Julia/1.11.7)"));
    }
    if crate::host::saved_settings().airgap {
        return Err("airgap mode: the packages come over the network".into());
    }
    let cli = ndexr_cli().ok_or("the ndexr CLI is not on this machine - set the stack up first")?;
    let how = root_by();
    if how.is_empty() {
        return Err(format!("this needs root once: run `sudo ndexr pkgs sync {set}` in a terminal"));
    }
    let flag = if reporting() { " --report-errors" } else { "" };
    let cmd = as_root(&how, &format!("{} pkgs sync {set}{flag}", shell_quote(&cli)));
    let lang = set.split('/').next().unwrap_or(set).to_owned();
    let stage = format!("cove:pkgs-sync-{lang}");
    let done: jobs::Done = Box::new(move |ok, v| {
        if !ok && reporting() {
            report_failure(&stage, v);
        }
    });
    jobs::start("stack", format!("Getting the {lang} packages"), format!("echo '$ ndexr pkgs sync {set}'; {cmd}"), Some(done))
}

/// `cove setup-stack [--yes]`: the stack mounted on this machine from a
/// terminal - a server's first start, or the installer's `--stack` (operator,
/// 2026-11-22: "make sure the stack installs on the server version (ask if
/// they would like it installed on server or have a --yes kind of
/// agreement)"). The same job as the app's Set up CernVM-FS, its output
/// printed as it comes; asks first unless --yes.
pub fn setup_cli(args: &[String]) -> i32 {
    let v = view();
    if v.ok {
        println!("The ndexr stack is already mounted at {}.", v.root);
        return 0;
    }
    if !v.unsupported.is_empty() {
        eprintln!("{}", v.unsupported);
        return 1;
    }
    println!("Mount the ndexr stack on this machine: R, Python, Julia, compilers and thousands of");
    println!("scientific libraries, read-only at /opt/ndexr/stack over CernVM-FS, each file fetched");
    println!("the first time it is used. It installs the CernVM-FS client, ndexr's public key, one");
    println!("/etc/fstab line and a profile script; root once ({}), nothing after.", if v.root_by.is_empty() { "run as root, or with sudo" } else { v.root_by.as_str() });
    if !args.iter().any(|a| a == "--yes" || a == "-y") {
        use std::io::{BufRead, IsTerminal, Write};
        if !std::io::stdin().is_terminal() {
            eprintln!("Not asked: no terminal. Run `cove setup-stack --yes` to agree and mount it.");
            return 2;
        }
        print!("Mount it now? [y/N] ");
        let _ = std::io::stdout().flush();
        let mut a = String::new();
        let _ = std::io::stdin().lock().read_line(&mut a);
        if !matches!(a.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("Not mounted. Cove asks again in the browser, or run `cove setup-stack --yes`.");
            return 0;
        }
    }
    if let Err(e) = setup(&crate::host::saved_settings().cvmfs) {
        eprintln!("{e}");
        return 1;
    }
    let mut shown = 0;
    loop {
        let j = jobs::view("stack");
        for l in j.lines.iter().skip(shown) {
            println!("{l}");
        }
        shown = j.lines.len();
        if !j.running {
            let ok = j.ok == Some(true);
            println!("{}", if ok { "The ndexr stack is mounted." } else { "Setup failed - the reason is above." });
            return if ok { 0 } else { 1 };
        }
        std::thread::sleep(Duration::from_millis(300));
    }
}

/// The Caddyfile every Cove server of a person's own uses (packaging/linux/Caddyfile).
const CADDYFILE: &str = include_str!("../packaging/linux/Caddyfile");

/// The unit that runs the stack's Caddy. `ml Caddy` at every start, so a stack
/// update brings a new Caddy without touching this file. DynamicUser with the
/// one capability Caddy needs for :80/:443; its certificates in /var/lib/caddy.
const CADDY_UNIT: &str = "[Unit]
Description=Caddy from the ndexr stack: HTTPS in front of Cove (cove setup-front)
After=network-online.target remote-fs.target
Wants=network-online.target
RequiresMountsFor=/opt/ndexr/stack

[Service]
DynamicUser=yes
StateDirectory=caddy
Environment=HOME=/var/lib/caddy XDG_DATA_HOME=/var/lib/caddy XDG_CONFIG_HOME=/var/lib/caddy
AmbientCapabilities=CAP_NET_BIND_SERVICE
ExecStart=/bin/bash -c 'set +u; . /opt/ndexr/stack/use.sh && ml Caddy && exec caddy run --config /etc/caddy/Caddyfile --adapter caddyfile'
ExecReload=/bin/bash -c 'set +u; . /opt/ndexr/stack/use.sh && ml Caddy && exec caddy reload --config /etc/caddy/Caddyfile --adapter caddyfile --force'
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
";

/// `cove setup-front [--yes]`: HTTPS in front of this server's Cove, from the
/// stack's Caddy (operator, 2026-11-24: "build it into the stack so when cvmfs
/// is used its there, that's how we need to do everything, caddy too"). Writes
/// the Caddyfile Cove ships to /etc/caddy (a different one is kept as
/// Caddyfile.bak), a `cove-caddy` unit that runs `ml Caddy`, and starts it. A
/// hand-installed caddy.service is stopped and disabled - two Caddys cannot
/// both have :443. Needs the stack mounted (setup-stack first) and root once.
pub fn setup_front_cli(args: &[String]) -> i32 {
    let v = view();
    if !v.ok {
        eprintln!("The ndexr stack is not mounted here: run `cove setup-stack --yes` first.");
        return 1;
    }
    let Some(caddy) = capture_within("set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1 && ml Caddy >/dev/null 2>&1 && command -v caddy", Duration::from_secs(30)) else {
        eprintln!("The stack mounted here has no Caddy module yet (`ml Caddy` found nothing). It comes with the next stack publish.");
        return 1;
    };
    println!("HTTPS for this Cove: Caddy from the ndexr stack ({caddy}), on ports 80 and 443,");
    println!("a certificate for each name the first time it is opened, and only for names Cove says it");
    println!("serves (/__tls/ask). It writes /etc/caddy/Caddyfile and the cove-caddy service; root once.");
    let how = root_by();
    if !matches!(how.as_str(), "sudo" | "root" | "pkexec") {
        eprintln!("This needs root once: run `sudo cove setup-front --yes`.");
        return 1;
    }
    if !args.iter().any(|a| a == "--yes" || a == "-y") {
        use std::io::{BufRead, IsTerminal, Write};
        if !std::io::stdin().is_terminal() {
            eprintln!("Not asked: no terminal. Run `cove setup-front --yes` to agree and start it.");
            return 2;
        }
        print!("Set it up now? [y/N] ");
        let _ = std::io::stdout().flush();
        let mut a = String::new();
        let _ = std::io::stdin().lock().read_line(&mut a);
        if !matches!(a.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("Not set up.");
            return 0;
        }
    }
    // The organisation's server (cove-multiuser, :8787) when it runs here,
    // else one person's (cove serve, :8788): the Caddyfile names 8788.
    let org = Command::new("systemctl").args(["is-enabled", "cove-multiuser"]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
    let caddyfile = if org { CADDYFILE.replace("127.0.0.1:8788", "127.0.0.1:8787") } else { CADDYFILE.to_owned() };
    let script = format!(
        "set -e\n\
         mkdir -p /etc/caddy\n\
         if [ -f /etc/caddy/Caddyfile ] && ! printf '%s' {file} | cmp -s - /etc/caddy/Caddyfile; then cp /etc/caddy/Caddyfile /etc/caddy/Caddyfile.bak; echo 'kept the old Caddyfile as /etc/caddy/Caddyfile.bak'; fi\n\
         printf '%s' {file} > /etc/caddy/Caddyfile\n\
         printf '%s' {unit} > /etc/systemd/system/cove-caddy.service\n\
         if systemctl is-enabled caddy >/dev/null 2>&1 || systemctl is-active caddy >/dev/null 2>&1; then systemctl disable --now caddy; echo 'stopped the hand-installed caddy.service: cove-caddy has :443 now'; fi\n\
         systemctl daemon-reload\n\
         systemctl enable cove-caddy >/dev/null 2>&1\n\
         systemctl restart cove-caddy\n\
         sleep 3\n\
         systemctl is-active cove-caddy >/dev/null || {{ journalctl -u cove-caddy -n 20 --no-pager; exit 1; }}\n\
         echo 'cove-caddy is running'",
        file = shell_quote(&caddyfile),
        unit = shell_quote(CADDY_UNIT),
    );
    let st = Command::new("sh").args(["-c", &as_root(&how, &script)]).status();
    match st {
        Ok(s) if s.success() => {
            println!("HTTPS is in front of Cove. Point your DNS (the name and *.name) at this server, and set");
            println!("COVE_NAME=<your domain> in Cove's environment so the first certificate is allowed.");
            0
        }
        _ => {
            eprintln!("Setting up the front failed - the reason is above.");
            1
        }
    }
}

/// The organisation's server, run from the stack: `cove multiuser` as root,
/// a `cove serve` per signed-in person, as them.
const MULTIUSER_UNIT: &str = "[Unit]
Description=Cove for this organisation's people, from the ndexr stack (cove setup-server)
After=network-online.target remote-fs.target
Wants=network-online.target
RequiresMountsFor=/opt/ndexr/stack

[Service]
Type=simple
User=root
EnvironmentFile=-/etc/cove/multiuser.env
ExecStart=/bin/bash -c 'set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1 && ml cove && exec cove multiuser'
Restart=on-failure
RestartSec=3
NoNewPrivileges=no

[Install]
WantedBy=multi-user.target
";

/// One person's server, run from the stack as them.
const SINGLE_UNIT: &str = "[Unit]
Description=Cove for %i, from the ndexr stack (cove setup-server --user)
After=network-online.target remote-fs.target
Wants=network-online.target
RequiresMountsFor=/opt/ndexr/stack

[Service]
Type=simple
User=%i
WorkingDirectory=~
EnvironmentFile=-/etc/cove/%i.env
ExecStart=/bin/bash -c 'set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1 && ml cove && exec cove serve'
Restart=on-failure
RestartSec=3

[Install]
WantedBy=multi-user.target
";

const MULTIUSER_CONF: &str = include_str!("../packaging/linux/multiuser.conf");
const PAM_COVE: &str = include_str!("../packaging/linux/pam.d-cove");

/// `cove setup-server [--user NAME] [--name example.org] [--yes]`: Cove on this
/// server, run from the ndexr stack, nothing installed (operator, 2026-11-24:
/// "always many people will be using cove on a server, it is the future of hpc
/// management for scientific software").
///   default        the organisation's: cove-multiuser - people sign in with
///                  their own accounts (PAM, /etc/pam.d/cove), each gets their
///                  own sessions as themselves; /admin for admin-groups.
///                  /etc/cove/multiuser.conf is written once, then the
///                  organisation's own.
///   --user NAME    one person's: cove-stack@NAME (cove serve, a token).
///   --name N       the server's address (COVE_NAME), so its front can ask
///                  for a certificate before anyone has reached it.
/// The unit runs `ml cove` at every start, so a stack update is a Cove update.
pub fn setup_server_cli(args: &[String]) -> i32 {
    let v = view();
    if !v.ok {
        eprintln!("The ndexr stack is not mounted here: run `cove setup-stack --yes` first.");
        return 1;
    }
    let arg = |k: &str| args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned();
    let user = arg("--user");
    let name = arg("--name").unwrap_or_default();
    if user.as_deref().is_some_and(|u| u.is_empty() || !u.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))) {
        eprintln!("--user needs an account name");
        return 2;
    }
    if !name.is_empty() && !name.chars().all(|c| c.is_ascii_alphanumeric() || ".-".contains(c)) {
        eprintln!("--name needs a host name, such as cove.example.org");
        return 2;
    }
    let how = root_by();
    if !matches!(how.as_str(), "sudo" | "root" | "pkexec") {
        eprintln!("This needs root once: run it with sudo.");
        return 1;
    }
    match &user {
        Some(u) => println!("Cove for {u} on this server, from the ndexr stack: cove-stack@{u} runs `ml cove` and `cove serve` as {u}."),
        None => println!("Cove for this organisation, from the ndexr stack: cove-multiuser - people sign in with their own accounts (PAM)\nand each gets their own sessions, run as them. Who may sign in: /etc/cove/multiuser.conf."),
    }
    if !args.iter().any(|a| a == "--yes" || a == "-y") {
        use std::io::{BufRead, IsTerminal, Write};
        if !std::io::stdin().is_terminal() {
            eprintln!("Not asked: no terminal. Add --yes to agree.");
            return 2;
        }
        print!("Set it up now? [y/N] ");
        let _ = std::io::stdout().flush();
        let mut a = String::new();
        let _ = std::io::stdin().lock().read_line(&mut a);
        if !matches!(a.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("Not set up.");
            return 0;
        }
    }
    let name_line = if name.is_empty() { String::new() } else { format!("COVE_NAME={name}\n") };
    let script = match &user {
        Some(u) => format!(
            "set -e\n\
             mkdir -p /etc/cove\n\
             printf '%s' {unit} > /etc/systemd/system/cove-stack@.service\n\
             {env}\
             systemctl daemon-reload\n\
             systemctl enable cove-stack@{u} >/dev/null 2>&1\n\
             systemctl restart cove-stack@{u}\n\
             sleep 2; systemctl is-active cove-stack@{u} >/dev/null || {{ journalctl -u cove-stack@{u} -n 20 --no-pager; exit 1; }}\n\
             echo 'cove-stack@{u} is running'",
            unit = shell_quote(SINGLE_UNIT),
            env = if name.is_empty() { String::new() } else { format!("grep -q '^COVE_NAME=' /etc/cove/{u}.env 2>/dev/null || printf '%s' {} >> /etc/cove/{u}.env\n", shell_quote(&name_line)) },
        ),
        None => format!(
            "set -e\n\
             mkdir -p /etc/cove\n\
             [ -f /etc/cove/multiuser.conf ] || printf '%s' {conf} > /etc/cove/multiuser.conf\n\
             [ -f /etc/pam.d/cove ] || printf '%s' {pam} > /etc/pam.d/cove\n\
             {env}\
             printf '%s' {unit} > /etc/systemd/system/cove-multiuser.service\n\
             systemctl daemon-reload\n\
             systemctl enable cove-multiuser >/dev/null 2>&1\n\
             systemctl restart cove-multiuser\n\
             sleep 2; systemctl is-active cove-multiuser >/dev/null || {{ journalctl -u cove-multiuser -n 20 --no-pager; exit 1; }}\n\
             echo 'cove-multiuser is running on 127.0.0.1:8787'",
            conf = shell_quote(MULTIUSER_CONF),
            pam = shell_quote(PAM_COVE),
            unit = shell_quote(MULTIUSER_UNIT),
            env = if name.is_empty() { String::new() } else { format!("grep -q '^COVE_NAME=' /etc/cove/multiuser.env 2>/dev/null || printf '%s' {} >> /etc/cove/multiuser.env\n", shell_quote(&name_line)) },
        ),
    };
    let ok = Command::new("sh").args(["-c", &as_root(&how, &script)]).status().is_ok_and(|s| s.success());
    if !ok {
        eprintln!("Setting up the server failed - the reason is above.");
        return 1;
    }
    match &user {
        Some(u) => println!("Sign in with the token in ~{u}/.config/ndexr/cove-token (written at its first start); the desktop app connects with it (Connect to a server)."),
        None => println!("People sign in at http://127.0.0.1:8787/login with their accounts on this server; /admin for admin-groups."),
    }
    println!("HTTPS in front, from the stack's Caddy: `cove setup-front --yes`.");
    0
}

/// Set the stack up on this machine, in the background.
pub fn setup(c: &CvmfsSettings) -> Result<JobView, String> {
    if crate::host::saved_settings().airgap {
        return Err("airgap mode: CernVM-FS needs the network - point Cove at a copy of the stack on this machine instead".into());
    }
    let v = view();
    if !v.unsupported.is_empty() {
        return Err(v.unsupported);
    }
    let cli = ndexr_cli().ok_or("the ndexr CLI is not on this machine - install it first (cvmfs.ndexr.io has the guide)")?;
    c.check()?;
    let how = v.root_by.clone();
    // Gentoo (operator, 2026-11-19, a fresh Gentoo laptop): the client comes
    // from Gentoo's own tree - net-fs/cvmfs, stable on amd64, built from
    // source in some minutes - and the CLI's preflight, which knows apt-get and
    // dnf only and fails even with the client installed, is stepped over: the
    // same published configurator the CLI runs (install/cvmfs.sh, its sha256
    // checked as the CLI checks it) does the configuration, key and mount, and
    // skips its own install when cvmfs2 is there. Referrals #802 (aws.ndexr.io,
    // the configurator) and #803 (ndexr.ndexr.io, the preflight); drop this
    // branch when both land.
    let install = install_script(&cli);
    // As root: pkexec and root as they are; with sudo, the CLI is happy run
    // as root too, and emerge needs it.
    let mut cmd = match how.as_str() {
        "sudo" | "root" | "pkexec" => as_root(&how, &install),
        _ => return Err(format!("this needs root once: run `{}` in a terminal, then reopen Global Options", v.setup_cmd)),
    };
    if !c.is_empty() {
        cmd = format!("{cmd} && {}", apply_cmd(&how, c));
    }
    // A failed setup goes home whole when the person allowed reports
    // (operator, 2026-11-19: "when there are issues setting up cvmfs or
    // anything like that with initial setup those errors should be sent back
    // if allowed"): the installer's output and the diagnostics after it, which
    // the CLI's own --report-errors never saw.
    let done: jobs::Done = Box::new(|ok, v| {
        if !ok && reporting() {
            report_failure("cove:setup-cvmfs", v);
        }
    });
    jobs::start("stack", "Setting up the ndexr stack".into(), format!("echo '$ ndexr install cvmfs --yes'; {cmd}; rc=$?\n{}", why_unmounted(&how)), Some(done))
}

/// After setup, when the mount still has no use.sh: why, in the job's own
/// log, and the mount tried once more where its error shows. The configurator
/// (aws.ndexr.io provision/stages/cvmfs.sh) reports a failed mount or trust
/// anchor with an echo and goes on, so all a person saw was the CLI's "setup
/// ran but /opt/ndexr/stack has no use.sh" (operator, 2026-11-14). Expects
/// `rc` set; ends the job with it, or 0 when the retry mounted it.
fn why_unmounted(how: &str) -> String {
    let mnt = "/opt/ndexr/stack";
    let curls: String = servers()
        .iter()
        .map(|s| format!("printf '  %s → ' {u}; curl -sS -o /dev/null -m 10 -w '%{{http_code}}\\n' {u} 2>&1 | tail -1\n", u = shell_quote(&format!("{s}/.cvmfspublished"))))
        .collect();
    let mount = as_root(how, &format!("mount {mnt}"));
    format!(
        "if [ ! -r {mnt}/use.sh ]; then\n\
         echo; echo '--- why {mnt} has no use.sh'\n\
         findmnt {mnt} || echo '  not mounted'\n\
         grep -E '[[:space:]]{mnt}[[:space:]]' /etc/fstab || echo '  no /etc/fstab line for {mnt}'\n\
         ls -l /etc/cvmfs/keys/ndexr.io/{REPO}.pub 2>&1 || echo '  no trust anchor: the key download failed'\n\
         command -v cvmfs2 >/dev/null || echo '  the CernVM-FS client is not installed'\n\
         cvmfs_config showconfig {REPO} 2>&1 | grep -E '^CVMFS_(SERVER_URL|KEYS_DIR|MOUNT_DIR|HTTP_PROXY)='\n\
         echo '  the repository from here:'\n\
         {curls}\
         echo '$ mount {mnt}'; {mount} 2>&1\n\
         if [ -r {mnt}/use.sh ]; then echo 'mounted on the second try'; rc=0\n\
         else\n\
         journalctl --since -3min --no-pager -t cvmfs2 2>/dev/null | tail -n 6\n\
         {expired}\
         fi\n\
         fi\n\
         exit $rc",
        expired = whitelist_check()
    )
}

/// The repository's whitelist - ndexr's signature over its signing key, good
/// for 30 days - expired: every new client is refused ("bad whitelist") while
/// mounted ones carry on. It lapsed on 2026-11-11 and a new laptop's setup
/// failed three days later with nothing on screen saying so. This says it,
/// and whose it is to fix.
fn whitelist_check() -> String {
    let Some(server) = servers().into_iter().next() else { return String::new() };
    format!(
        "wl=$(curl -sS -m 10 {u} 2>/dev/null | grep -a -m1 '^E[0-9]\\{{14\\}}' | cut -c2-15)\n\
         if [ -n \"$wl\" ] && [ \"$wl\" -lt \"$(date -u +%Y%m%d%H%M%S)\" ]; then\n\
         echo; echo \"ndexr's signature on the stack expired on $(echo $wl | cut -c1-8) (UTC), so CernVM-FS refuses it.\"\n\
         echo \"This is on ndexr's side - nothing on this computer needs changing. Once ndexr re-signs it, Check again mounts it.\"\n\
         fi\n",
        u = shell_quote(&format!("{server}/.cvmfswhitelist"))
    )
}

fn apply_cmd(how: &str, c: &CvmfsSettings) -> String {
    let body = local_file(c);
    let mut script = if c.is_empty() {
        format!("rm -f {LOCAL}")
    } else {
        format!("umask 022 && printf %s {} > {LOCAL}.new && mv {LOCAL}.new {LOCAL}", shell_quote(&body))
    };
    let base = c.cache_base.trim();
    if !base.is_empty() {
        script.push_str(&format!(" && install -d -m 700 {b} && {{ chown cvmfs:cvmfs {b} 2>/dev/null || true; }}", b = shell_quote(base)));
    }
    // A mounted client re-reads its configuration; the cache folder is the
    // one thing that waits for the next mount.
    script.push_str(&format!(" && {{ cvmfs_config reload {REPO} 2>&1 || true; }}"));
    as_root(how, &script)
}

/// The person's settings, onto this machine's client.
pub fn apply(c: &CvmfsSettings) -> Result<JobView, String> {
    c.check()?;
    let v = view();
    if !v.client {
        return Err("the CVMFS client is not installed here yet - Set up the stack first".into());
    }
    if v.root_by.is_empty() {
        return Err(format!("writing {LOCAL} needs root: put these lines in it by hand, then `sudo cvmfs_config reload {REPO}`:\n{}", local_file(c)));
    }
    let note = if c.cache_base.trim() != v.local.cache_base.trim() && !c.cache_base.trim().is_empty() { "; the new cache folder is used from the next mount" } else { "" };
    jobs::start("stack", "Applying CVMFS settings".into(), format!("{}; echo 'settings written to {LOCAL}{note}'", apply_cmd(&v.root_by, c)), None)
}

/// "http://p1|http://p2;DIRECT" → each proxy once, in order.
fn proxies(list: &str) -> Vec<String> {
    let mut v: Vec<String> = Vec::new();
    for p in list.split([';', '|']).map(str::trim).filter(|p| !p.is_empty()) {
        if !v.iter().any(|x| x == p) {
            v.push(p.to_owned());
        }
    }
    if v.is_empty() {
        v.push("DIRECT".into());
    }
    v
}

/// Our servers, from the client when it has them.
fn servers() -> Vec<String> {
    let cfg = showconfig();
    let list = cfg.iter().find(|c| c.key == "CVMFS_SERVER_URL").map(|c| c.value.clone()).unwrap_or_else(|| SERVERS.into());
    list.split(';').map(|s| s.trim().replace("@fqrn@", REPO)).filter(|s| s.starts_with("http")).collect()
}

/// Each proxy against each server: the repository's signed manifest, fetched
/// the way the client fetches it.
pub fn test(proxy: &str) -> Result<Vec<ProxyCheck>, String> {
    CvmfsSettings { proxy: proxy.into(), ..Default::default() }.check()?;
    let pairs: Vec<(String, String)> = proxies(proxy).into_iter().flat_map(|p| servers().into_iter().map(move |s| (p.clone(), s))).collect();
    let handles: Vec<_> = pairs
        .into_iter()
        .map(|(p, s)| {
            std::thread::spawn(move || {
                let via = if p == "DIRECT" { "--noproxy '*'".to_owned() } else { format!("--proxy {}", shell_quote(&p)) };
                // curl's own error first (stderr), then the status line; a
                // failed connection still says why, not only that it failed.
                let cmd = format!("curl -sS -o /dev/null -m 10 -w '\\n%{{http_code}} %{{time_total}}' {via} {}/.cvmfspublished 2>&1; true", shell_quote(&s));
                let out = capture_within(&cmd, Duration::from_secs(15)).unwrap_or_default();
                ProxyCheck { proxy: p, server: s, ..read_curl(&out) }
            })
        })
        .collect();
    Ok(handles.into_iter().filter_map(|h| h.join().ok()).collect())
}

/// `curl -w '\n<code> <seconds>'` output, errors included, as a check.
fn read_curl(out: &str) -> ProxyCheck {
    let mut lines: Vec<&str> = out.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let last = lines.pop().unwrap_or("");
    let (code, secs) = last.split_once(' ').unwrap_or((last, ""));
    let ok = code == "200";
    let said = match (code, lines.last()) {
        ("" , _) => "no answer in 10 s".to_owned(),
        ("000", Some(err)) => err.trim_start_matches("curl: ").to_owned(),
        ("000", None) => "no answer".to_owned(),
        _ => format!("{code} in {secs} s"),
    };
    ProxyCheck { ok, said, ..Default::default() }
}

#[cfg(test)]
mod report_tests {
    #[test]
    fn a_report_keeps_the_end_without_progress_lines() {
        let mut l: Vec<String> = (0..200).map(|i| format!(">>> Jobs: {i} of 14 complete   Load avg: 0.5")).collect();
        l.push("✘ pkg manager   no apt-get".into());
        l.push("".into());
        l.push("verdict: installation would NOT work yet".into());
        assert_eq!(super::report_lines(&l), "✘ pkg manager   no apt-get\nverdict: installation would NOT work yet");
        let many: Vec<String> = (0..100).map(|i| format!("line {i}")).collect();
        let r = super::report_lines(&many);
        assert!(r.starts_with("line 20\n") && r.ends_with("line 99"));
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_setup_script_parses_on_every_branch() {
        let sh = super::install_script("/x/ndexr");
        assert!(sh.contains("emerge --noreplace") && sh.contains("install cvmfs --yes"));
        let o = std::process::Command::new("bash").args(["-n", "-c", &sh]).output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    }


    #[test]
    fn the_unmounted_diagnosis_is_valid_shell_and_keeps_the_exit_code() {
        for how in ["sudo", "pkexec", "root"] {
            let script = format!("rc=3\n{}", super::why_unmounted(how));
            let out = std::process::Command::new("bash").args(["-n", "-c", &script]).output().unwrap();
            assert!(out.status.success(), "{how}: {}", String::from_utf8_lossy(&out.stderr));
            assert!(script.contains("mount /opt/ndexr/stack") && script.ends_with("exit $rc"));
        }
    }

    #[test]
    fn a_job_reads_its_log_file_to_the_end() {
        let v = jobs::start("test-job-log", "test".into(), "echo first; sleep 1; echo second; printf last".into(), None).unwrap();
        assert!(v.running);
        let until = std::time::Instant::now() + Duration::from_secs(20);
        let mut got = jobs::view("test-job-log");
        while got.running && std::time::Instant::now() < until {
            std::thread::sleep(Duration::from_millis(100));
            got = jobs::view("test-job-log");
        }
        assert_eq!(got.ok, Some(true));
        assert_eq!(got.lines, ["first", "second", "last"]);
    }

    use super::*;

    #[test]
    fn curl_says_why() {
        let r = read_curl("curl: (7) Failed to connect to 127.0.0.1 port 9: Connection refused\n000 0.000310");
        assert!(!r.ok && r.said.starts_with("(7) Failed to connect"), "{}", r.said);
        let r = read_curl("\n200 0.21");
        assert!(r.ok && r.said == "200 in 0.21 s");
        assert_eq!(read_curl("").said, "no answer in 10 s");
    }

    #[test]
    fn showconfig_keeps_what_the_person_reads() {
        let out = "CVMFS_REPOSITORY_NAME=stack.ndexr.io\nCVMFS_CACHE_BASE=/var/lib/cvmfs    # from /etc/cvmfs/default.conf\nCVMFS_HTTP_PROXY=DIRECT    # from /etc/cvmfs/config.d/stack.ndexr.io.conf\nCVMFS_SERVER_URL=\"http://a/cvmfs/stack.ndexr.io;http://b/cvmfs/stack.ndexr.io\"    # from x\n";
        let v = parse_showconfig(out);
        assert_eq!(v.iter().map(|c| c.key.as_str()).collect::<Vec<_>>(), ["CVMFS_SERVER_URL", "CVMFS_HTTP_PROXY", "CVMFS_CACHE_BASE"]);
        assert_eq!(v[1].from, "/etc/cvmfs/config.d/stack.ndexr.io.conf");
        assert_eq!(v[0].value, "http://a/cvmfs/stack.ndexr.io;http://b/cvmfs/stack.ndexr.io");
    }

    #[test]
    fn the_local_file_round_trips_and_says_only_what_was_set() {
        let c = CvmfsSettings { proxy: "http://squid:3128;DIRECT".into(), cache_base: String::new(), quota_mb: "20000".into() };
        let f = local_file(&c);
        assert!(!f.contains("CVMFS_CACHE_BASE"));
        assert_eq!(parse_local(&f), c);
    }

    #[test]
    fn proxy_lists_split_as_cvmfs_reads_them() {
        assert_eq!(proxies("http://a:3128|http://b:3128;DIRECT"), ["http://a:3128", "http://b:3128", "DIRECT"]);
        assert_eq!(proxies(""), ["DIRECT"]);
    }
}

#[cfg(test)]
mod local_install_tests {
    #[test]
    fn a_local_build_is_read_from_its_own_files() {
        let d = std::env::temp_dir().join(format!("cove-local-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let b = d.join(".build");
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(b.join("up.json"), r#"{"state":"running","step":"suite","steps":["kit","prefix","ndexr-cli","suite","cove","packages"],"started":100,"rc":null}"#).unwrap();
        std::fs::write(b.join("up.timeline.json"), r#"{"generated":1,"steps":[
            {"kind":"eb","name":"GCC","version":"13.3.0","started":10,"finished":50,"status":"done"},
            {"kind":"eb","name":"R","version":"4.6.1","started":60,"finished":0,"status":"running"},
            {"kind":"eb","name":"Qt6","version":"6.7.2","started":20,"finished":30,"status":"failed"}]}"#).unwrap();
        std::fs::write(b.join("up.log"), "one

two
").unwrap();
        let m = d.join("x86_64-pc-linux-gnu/eb/modules/all/R");
        std::fs::create_dir_all(&m).unwrap();
        std::fs::write(m.join("4.6.1-foss-2024a.lua"), "").unwrap();
        let v = super::local_install_at(&d, &d, "x86_64-pc-linux-gnu");
        assert_eq!((v.state.as_str(), v.step.as_str(), v.steps.len()), ("running", "suite", 6));
        assert_eq!((v.done, v.failed, v.running.len()), (1, 1, 1));
        assert_eq!(v.running[0].name, "R");
        assert_eq!(v.failures[0].name, "Qt6");
        assert_eq!(v.tail, ["one", "two"]);
        assert_eq!(v.modules, ["R/4.6.1-foss-2024a"]);
        assert!(v.root_is_dir);
        assert!(!v.alive, "no up.pid");
        let none = super::local_install_at(&d.join("nothing"), &d.join("nothing"), "x");
        assert!(none.state.is_empty() && none.modules.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
