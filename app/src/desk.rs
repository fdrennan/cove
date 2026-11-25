//! Desktop Cove hands over to a build with the App panel's embedded browser
//! when one can start here (operator, 2026-11-24: Cove must not need WebKitGTK
//! installed - "build it into the stack so when cvmfs is used its there").
//!
//! The Linux tarball carries three binaries:
//!   bin/cove                   this one: no WebKit, no GTK, starts anywhere
//!   lib/cove/bin/cove-desk-stack   the webview, linked against the stack's
//!                              WebKitGTK (`ml WebKitGTK`, its loader and RPATH
//!                              all under /opt/ndexr/stack)
//!   lib/cove/bin/cove-desk     the webview, linked against the system's
//!                              libwebkit2gtk-4.1 - what every desktop ran
//!                              before, kept so a machine with it loses nothing
//! On a desktop session, the stack's build is tried first, then the system's:
//! each must start (`--version`, every library resolved) before this process
//! becomes it (exec, same arguments). Neither does, and this one runs on: the
//! App panel opens sites in the browser.
//!
//! COVE_DESK in the environment stops it: set by the hand-over itself (no
//! loop), or COVE_DESK=0 by a person who wants the plain build.

use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Run `cmd`, give it `limit`, and return its stdout when it exited 0.
fn run_within(mut cmd: Command, limit: Duration) -> Option<Vec<u8>> {
    use std::io::Read;
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out.read_to_end(&mut buf);
        buf
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => {
                let buf = reader.join().ok()?;
                return st.success().then_some(buf);
            }
            Ok(None) if start.elapsed() < limit => std::thread::sleep(Duration::from_millis(50)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}

/// The environment `ml WebKitGTK` makes: GIO's TLS module (glib-networking),
/// GTK's schemas and icons, the module's own PATH. None when the stack is not
/// mounted here or has no WebKitGTK.
fn stack_env() -> Option<Vec<(String, String)>> {
    if !Path::new("/opt/ndexr/stack/use.sh").is_file() {
        return None;
    }
    let mut cmd = Command::new("/bin/bash");
    cmd.args([
        "--noprofile",
        "--norc",
        "-c",
        "set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1 && ml WebKitGTK >/dev/null 2>&1 && [ -n \"$EBROOTWEBKITGTK\" ] && env -0",
    ]);
    let raw = run_within(cmd, Duration::from_secs(20))?;
    Some(
        raw.split(|b| *b == 0)
            .filter_map(|kv| {
                let kv = String::from_utf8_lossy(kv);
                let (k, v) = kv.split_once('=')?;
                // Lmod's own bookkeeping and bash's exported functions stay out.
                (!k.is_empty() && !k.starts_with("BASH_FUNC_") && !k.starts_with("__LMOD")).then(|| (k.to_owned(), v.to_owned()))
            })
            .collect(),
    )
}

fn starts(bin: &Path, env: &[(String, String)], limit: Duration) -> bool {
    let mut cmd = Command::new(bin);
    cmd.arg("--version").envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())));
    run_within(cmd, limit).is_some()
}

/// What a start of `bin` with `env` depends on: the binary (path, size, mtime)
/// and the WebKitGTK it loads (`ml WebKitGTK`'s install path, which names its
/// version). A change in either is a new probe.
fn desk_key(bin: &Path, env: &[(String, String)]) -> String {
    let meta = std::fs::metadata(bin).ok();
    let mtime = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let root = env.iter().find(|(k, _)| k == "EBROOTWEBKITGTK").map_or("", |(_, v)| v.as_str());
    format!("{} {} {mtime} {root}", bin.display(), meta.map_or(0, |m| m.len()))
}

fn ok_file() -> Option<PathBuf> {
    Some(PathBuf::from(std::env::var_os("HOME")?).join(".cache/cove/desk-ok"))
}

fn exec(bin: &Path, args: &[String], env: &[(String, String)]) {
    let err = Command::new(bin)
        .args(&args[1..])
        .envs(env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
        .env("COVE_DESK", "1")
        .exec();
    eprintln!("cove: could not start {}: {err}", bin.display());
}

/// lib/cove/bin beside this binary's bin/.
fn desk_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    Some(exe.parent()?.parent()?.join("lib/cove/bin"))
}

/// Become the build with the embedded browser, when one starts here. Returns
/// only when none does.
pub fn hand_over(args: &[String]) {
    if std::env::var_os("COVE_DESK").is_some() {
        return;
    }
    if std::env::var_os("DISPLAY").is_none() && std::env::var_os("WAYLAND_DISPLAY").is_none() {
        return;
    }
    let Some(dir) = desk_dir() else { return };
    let stack = dir.join("cove-desk-stack");
    if stack.is_file() {
        if let Some(env) = stack_env() {
            // Its first load fetches WebKitGTK and GTK from CVMFS - over 200 MB
            // on a cold cache, more than ten seconds (2026-11-24: a 10 s probe
            // timed out and the system build was used). So: up to 90 s once,
            // remembered while the binary and the WebKitGTK it loads are the
            // same, and later starts go straight to it.
            let key = desk_key(&stack, &env);
            let known = ok_file().and_then(|f| std::fs::read_to_string(f).ok()).is_some_and(|k| k == key);
            if known || starts(&stack, &env, Duration::from_secs(90)) {
                if !known {
                    if let Some(f) = ok_file() {
                        let _ = f.parent().map(std::fs::create_dir_all);
                        let _ = std::fs::write(f, &key);
                    }
                }
                exec(&stack, args, &env);
            }
        }
    }
    let system = dir.join("cove-desk");
    if system.is_file() && starts(&system, &[], Duration::from_secs(10)) {
        exec(&system, args, &[]);
    }
}
