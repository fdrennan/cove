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
    let install = format!("{} install cvmfs --yes", shell_quote(&cli));
    // With sudo the CLI asks for root itself; pkexec and root run it as root.
    let mut cmd = match how.as_str() {
        "sudo" => install,
        "root" | "pkexec" => as_root(&how, &install),
        _ => return Err(format!("this needs root once: run `{}` in a terminal, then reopen Global Options", v.setup_cmd)),
    };
    if !c.is_empty() {
        cmd = format!("{cmd} && {}", apply_cmd(&how, c));
    }
    jobs::start("stack", "Setting up the ndexr stack".into(), format!("echo '$ ndexr install cvmfs --yes'; {cmd}"), None)
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
mod tests {

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
