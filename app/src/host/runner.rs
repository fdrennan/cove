//! The machine's commands: short, time-boxed probes (`capture`) and handing a
//! command to the person's own terminal (`open_in_terminal`). Each runs in a
//! login shell so an app launched from Finder, a desktop menu or an Open
//! OnDemand session sees the same PATH as a terminal (Homebrew, ~/.local/bin,
//! Lmod modules, ndexr). The kernels (kernel.rs) start through the same shell.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

/// An ndexr CLI shipped beside this binary, if any: Contents/Resources/bin in
/// the macOS .app, <prefix>/lib/cove/bin in the Linux tarball, or the
/// same bin/ in an EasyBuild prefix. It goes on the END of PATH: a
/// user-installed ndexr wins, the bundled one is the fallback that makes a
/// fresh machine work out of the box.
pub fn bundled_bin_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let here = exe.parent()?.to_path_buf();
    let up = here.parent()?;
    // Never this binary itself: when ndexr.app's executable was named `ndexr`
    // and no CLI was bundled, Contents/MacOS matched, every `ndexr …` the app ran
    // launched another window, and neo opened ndexr in a loop (2026-10-11).
    let me = exe.canonicalize().ok();
    [up.join("Resources").join("bin"), up.join("lib").join("cove").join("bin"), here]
        .into_iter()
        .find(|d| {
            let cli = d.join("ndexr");
            cli.is_file() && cli.canonicalize().ok() != me
        })
}

pub use crate::api::shell_quote;

/// Set by `cove serve`: this process answers a browser, so nothing it
/// runs may prompt on this machine's screen (stack.rs's pkexec).
static SERVING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn set_serving() {
    SERVING.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn serving() -> bool {
    SERVING.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn login_shell() -> String {
    std::env::var("SHELL")
        .ok()
        .filter(|s| Path::new(s).is_file())
        .unwrap_or_else(|| if cfg!(target_os = "macos") { "/bin/zsh".into() } else { "/bin/bash".into() })
}

/// `cmd` for a login shell. The bundled CLI goes first: it ships with this
/// Cove, and the laptop's own /usr/local/bin/ndexr was 0.0.49 against the
/// bundle's 0.0.142 and won (2026-11-02). What the shell's /etc/profile drops
/// goes back at the end: NDEXR_SHELL_PATH, the PATH a cell (cell-entry.sh) or
/// code.ndexr.io started with, which its terminals restore the same way
/// (ndexr-shell.sh). Without it a cell's Cove server found no `ndexr` or
/// `claude` in /opt/ndexr/bin, and glimpse and the manifests said "the ndexr
/// CLI is not on this machine".
pub fn with_path(cmd: &str) -> String {
    let first = bundled_bin_dir().map(|d| format!("{}:", d.display())).unwrap_or_default();
    let last = std::env::var("NDEXR_SHELL_PATH").ok().filter(|p| !p.is_empty()).map(|p| format!(":{p}")).unwrap_or_default();
    if first.is_empty() && last.is_empty() {
        return cmd.to_owned();
    }
    format!("export PATH=\"{first}$PATH{last}\"; {cmd}")
}

/// Lmod's record of what is loaded, as inherited from whatever started this
/// process. In a cell, cell-entry.sh loads R (eb-R-env.sh) before it starts
/// `cove serve`; a kernel's login shell then resets PATH (/etc/profile)
/// but kept that record, so `ml env/r4.6_py3.13-release` skipped R as
/// "already loaded" and the R console said "no R here" while Python, never
/// loaded, came up (2026-10-13). Every shell that loads modules starts
/// without it; MODULEPATH and LMOD_CMD stay, so Lmod itself still works.
pub fn forget_loaded_modules(command: &mut Command) {
    for (k, _) in std::env::vars_os() {
        let k = k.to_string_lossy();
        if k == "LOADEDMODULES" || k == "_LMFILES_" || k.starts_with("_ModuleTable") || k.starts_with("__LMOD_REF_COUNT") || k.starts_with("__LMOD_STACK") {
            command.env_remove(k.as_ref());
        }
    }
}

/// Run `cmd` once in a login shell and return trimmed stdout (for probes).
/// Never longer than 20 s: a probe that waits on a network or a database
/// (`ndexr whoami` on a laptop with no postgres) must not hang the UI.
pub fn capture(cmd: &str) -> Option<String> {
    capture_within(cmd, Duration::from_secs(20))
}

/// `capture` with an explicit deadline; the whole process group is killed at it.
pub fn capture_within(cmd: &str, limit: Duration) -> Option<String> {
    let mut command = Command::new(login_shell());
    forget_loaded_modules(&mut command);
    command
        .args(["-lc", &with_path(cmd)])
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stdout.read_to_end(&mut buf);
        buf
    });
    let deadline = Instant::now() + limit;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = Command::new("kill").args(["-KILL", &format!("-{}", child.id())]).status();
                let _ = child.wait();
                return None;
            }
        }
    };
    let s = String::from_utf8_lossy(&reader.join().ok()?).trim().to_owned();
    (status.success() && !s.is_empty()).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_single_quotes() {
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
    }
}

