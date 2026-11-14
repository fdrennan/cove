//! In-place self-update, the same shape as `ndexr update --remote`: read a
//! pointer, download, sha256-verify, swap, relaunch.
//!
//!   <feed>/<platform>/latest/release.json      {"version","file","sha256",…}
//!   <feed>/<platform>/<version>/<file>         tar.gz — Cove.app on macOS,
//!                                              <name>/bin/cove (+ lib/…/ndexr) on Linux
//!
//! The feed is repo.ndexr.io/ndexr/cove — the one place ndexr software is
//! delivered, beside the CLI at repo.ndexr.io/ndexr/<arch>/. app/publish.sh
//! writes it. The payload comes from the immutable <version>/ dir, so a
//! publish racing a download can't swap the file under a verified sha256.
//! Everything is staged next to the install so
//! the final swap is a same-directory rename: the running process keeps its
//! old inode, and the next launch is the new build. Read-only installs
//! (EasyBuild on /cvmfs, a root-owned prefix) are never touched.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use serde::Deserialize;

use crate::runner::shell_quote;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
const DEFAULT_FEED: &str = "https://repo.ndexr.io/ndexr/cove";
const RECHECK: Duration = Duration::from_secs(6 * 60 * 60);

#[derive(Clone, PartialEq)]
pub enum Status {
    Checking,
    Current,
    Downloading(String),
    /// Swapped on disk; the next launch is this version.
    Ready(String),
    /// Not ours to replace — the reason is shown on hover.
    Managed(String),
    /// Nothing published for this platform yet; keeps rechecking.
    Unpublished,
    Failed(String),
}

#[derive(Deserialize)]
struct Release {
    version: String,
    file: String,
    sha256: String,
}

/// What gets replaced: the .app bundle on macOS, the binary (+ bundled CLI) elsewhere.
enum Install {
    Bundle(PathBuf),
    Binary { exe: PathBuf, cli: Option<PathBuf> },
}

impl Install {
    fn dir(&self) -> &Path {
        match self {
            Install::Bundle(app) => app.parent().unwrap_or(Path::new("/")),
            Install::Binary { exe, .. } => exe.parent().unwrap_or(Path::new("/")),
        }
    }
}

pub struct Updater {
    status: Arc<Mutex<Status>>,
    busy: Arc<AtomicBool>,
}

impl Updater {
    /// Checks now, then every six hours until an update is staged.
    pub fn start(ctx: &egui::Context) -> Self {
        let u = Updater {
            status: Arc::new(Mutex::new(Status::Checking)),
            busy: Arc::new(AtomicBool::new(false)),
        };
        let (status, busy, ctx) = (u.status.clone(), u.busy.clone(), ctx.clone());
        std::thread::spawn(move || loop {
            run(&status, &busy, &ctx);
            if !matches!(*status.lock().unwrap(), Status::Current | Status::Unpublished | Status::Failed(_)) {
                break;
            }
            std::thread::sleep(RECHECK);
        });
        u
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap().clone()
    }

    pub fn check_now(&self, ctx: &egui::Context) {
        // Checking from the click on, so what the strip shows is this check's.
        if !self.busy.load(Ordering::Relaxed) {
            *self.status.lock().unwrap() = Status::Checking;
        }
        let (status, busy, ctx) = (self.status.clone(), self.busy.clone(), ctx.clone());
        std::thread::spawn(move || run(&status, &busy, &ctx));
    }

    /// Wait for this process to exit, then launch the (already swapped) install.
    pub fn restart(&self, ctx: &egui::Context) {
        // Windows: the new .exe already holds the name (the running one was
        // renamed aside), so it starts now and this window closes.
        #[cfg(windows)]
        {
            if let Ok(Install::Binary { exe, .. }) = locate() {
                if Command::new(&exe).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().is_ok() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            return;
        }
        #[allow(unreachable_code)]
        let launch = match locate() {
            Ok(Install::Bundle(app)) => format!("open {}", shell_quote(&app.display().to_string())),
            Ok(Install::Binary { exe, .. }) => format!("{} >/dev/null 2>&1", shell_quote(&exe.display().to_string())),
            Err(_) => return,
        };
        let script = format!(
            "while kill -0 {} 2>/dev/null; do sleep 0.2; done; exec {launch}",
            std::process::id()
        );
        let mut cmd = Command::new("/bin/sh");
        cmd.args(["-c", &script]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        if cmd.spawn().is_ok() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

/// `cove --update`: the same check-and-swap, headless. Exit 0 when
/// current or updated, 1 on failure, 2 when the install is not ours to touch.
/// Check and, if a newer release is published, swap it in. Ok(Some(v)) when
/// v is now on disk (the running process is still the old one); Ok(None) when
/// current, unpublished, or not ours to replace (a managed or dev install).
pub fn swap_if_newer() -> Result<Option<String>, String> {
    let Ok(install) = locate() else { return Ok(None) };
    let Some(rel) = fetch_release()? else { return Ok(None) };
    if !newer(&rel.version, VERSION) {
        return Ok(None);
    }
    stage_and_swap(&install, &rel)?;
    Ok(Some(rel.version))
}

pub fn cli() -> i32 {
    let install = match locate() {
        Ok(i) => i,
        Err(why) => {
            eprintln!("cove v{VERSION}: {why}");
            return 2;
        }
    };
    let rel = match fetch_release() {
        Ok(Some(r)) => r,
        Ok(None) => {
            println!("cove v{VERSION}: nothing published for {} yet", platform());
            return 0;
        }
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    if !newer(&rel.version, VERSION) {
        println!("cove v{VERSION} is current ({})", platform());
        return 0;
    }
    println!("updating v{VERSION} → v{} …", rel.version);
    match stage_and_swap(&install, &rel) {
        Ok(()) => {
            println!("✓ v{} installed in place; restart ndexr to run it", rel.version);
            0
        }
        Err(e) => {
            eprintln!("update failed: {e}");
            1
        }
    }
}

/// darwin is one universal build; Linux and Windows are per arch.
pub fn platform() -> String {
    if cfg!(target_os = "macos") {
        "darwin".into()
    } else if cfg!(windows) {
        format!("windows-{}", std::env::consts::ARCH)
    } else {
        format!("linux-{}", std::env::consts::ARCH)
    }
}

/// The app's file name inside a package's bin/.
const EXE: &str = if cfg!(windows) { "cove.exe" } else { "cove" };

fn feed() -> String {
    std::env::var("COVE_FEED")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_FEED.to_owned())
        .trim_end_matches('/')
        .to_owned()
}

fn run(status: &Mutex<Status>, busy: &AtomicBool, ctx: &egui::Context) {
    if busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let set = |s: Status| {
        *status.lock().unwrap() = s;
        ctx.request_repaint();
    };
    set(Status::Checking);
    let next = match locate() {
        // Airgap mode: no feed to ask; a new Cove arrives the way the stack
        // copy did (Global Options turns it off, then Check for updates).
        _ if crate::host::saved_settings().airgap => Status::Managed("airgap mode: updates are installed by hand".into()),
        Err(why) => Status::Managed(why),
        Ok(install) => match fetch_release() {
            Err(e) => Status::Failed(e),
            Ok(None) => Status::Unpublished,
            Ok(Some(rel)) if !newer(&rel.version, VERSION) => Status::Current,
            Ok(Some(rel)) => {
                set(Status::Downloading(rel.version.clone()));
                match stage_and_swap(&install, &rel) {
                    Ok(()) => Status::Ready(rel.version),
                    Err(e) => Status::Failed(e),
                }
            }
        },
    };
    set(next);
    busy.store(false, Ordering::SeqCst);
}

fn locate() -> Result<Install, String> {
    let exe = std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|e| format!("cannot locate the running app: {e}"))?;
    let path = exe.to_string_lossy();
    if path.starts_with("/cvmfs/") || path.contains("/easybuild/") || path.contains("/software/cove/") {
        return Err("managed install — update via `module load cove`".into());
    }
    if path.contains("/target/debug/") || path.contains("/target/release/") {
        return Err("development build — rebuild to update".into());
    }

    let install = if cfg!(target_os = "macos") {
        let app = exe
            .ancestors()
            .find(|p| p.extension().is_some_and(|e| e == "app"))
            .ok_or("not running from an app bundle")?;
        Install::Bundle(app.to_path_buf())
    } else {
        let cli = exe
            .parent()
            .and_then(Path::parent)
            .map(|p| p.join("lib/cove/bin/ndexr"))
            .filter(|p| p.is_file());
        Install::Binary { exe, cli }
    };

    // Writable install dir, or it is somebody else's to update.
    let probe = install.dir().join(format!(".ndexr-write-probe-{}", std::process::id()));
    std::fs::create_dir(&probe)
        .and_then(|_| std::fs::remove_dir(&probe))
        .map_err(|_| format!("{} is read-only — reinstall to update", install.dir().display()))?;
    Ok(install)
}

/// Ok(None): the feed answered 404 — no release for this platform yet.
fn fetch_release() -> Result<Option<Release>, String> {
    let url = format!("{}/{}/latest/release.json", feed(), platform());
    let out = Command::new("curl")
        .args(["-fsSL", "--max-time", "15", "-H", "Cache-Control: no-cache", &url])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    match out.status.code() {
        Some(0) => {}
        Some(22) if String::from_utf8_lossy(&out.stderr).contains("404") => return Ok(None),
        Some(37) if url.starts_with("file://") => return Ok(None), // tests: missing local feed file
        _ => return Err(format!("cannot reach {url}: {}", String::from_utf8_lossy(&out.stderr).trim())),
    }
    let rel: Release = serde_json::from_slice(&out.stdout).map_err(|e| format!("bad feed {url}: {e}"))?;
    let safe = |s: &str| !s.is_empty() && !s.starts_with('.') && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c));
    if !safe(&rel.version) || !safe(&rel.file) {
        return Err(format!("refusing feed entry {:?} / {:?}", rel.version, rel.file));
    }
    Ok(Some(rel))
}

/// Numeric dotted compare; anything after '-' or '+' is ignored.
fn newer(remote: &str, local: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split(['-', '+'])
            .next()
            .unwrap_or("")
            .split('.')
            .map(|n| n.parse().unwrap_or(0))
            .collect()
    };
    parse(remote) > parse(local)
}

fn sh(cmd: &mut Command, what: &str) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("{what}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!("{what}: {}", String::from_utf8_lossy(&out.stderr).trim()))
    }
}

fn stage_and_swap(install: &Install, rel: &Release) -> Result<(), String> {
    let stage = install.dir().join(format!(".ndexr-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir_all(&stage).map_err(|e| format!("stage: {e}"))?;
    let result = stage_into(&stage, install, rel);
    let _ = std::fs::remove_dir_all(&stage);
    result
}

fn stage_into(stage: &Path, install: &Install, rel: &Release) -> Result<(), String> {
    let pkg = stage.join(&rel.file);
    let url = format!("{}/{}/{}/{}", feed(), platform(), rel.version, rel.file);
    sh(
        Command::new("curl").args(["-fsSL", "--max-time", "900", "-o"]).arg(&pkg).arg(&url),
        "download",
    )?;

    // Windows 10+ ships curl.exe and tar.exe (bsdtar, reads .tar.gz), but no
    // sha256sum: certutil prints the hash alone on its second line.
    let digest = if cfg!(target_os = "macos") {
        sh(Command::new("shasum").args(["-a", "256"]).arg(&pkg), "shasum")?
    } else if cfg!(windows) {
        let out = sh(Command::new("certutil").args(["-hashfile"]).arg(&pkg).arg("SHA256"), "certutil")?;
        out.lines().nth(1).unwrap_or("").replace(' ', "")
    } else {
        sh(Command::new("sha256sum").arg(&pkg), "sha256sum")?
    };
    let got = digest.split_whitespace().next().unwrap_or("");
    if !got.eq_ignore_ascii_case(rel.sha256.trim()) {
        return Err(format!("sha256 mismatch for {}: got {got}", rel.file));
    }

    sh(Command::new("tar").arg("-xzf").arg(&pkg).arg("-C").arg(stage), "extract")?;

    match install {
        Install::Bundle(app) => {
            let new = stage.join("Cove.app");
            if !new.join("Contents/MacOS").is_dir() {
                return Err("package has no Cove.app".into());
            }
            let _ = Command::new("xattr").args(["-dr", "com.apple.quarantine"]).arg(&new).status();
            swap_dir(&new, app)
        }
        Install::Binary { exe, cli } => {
            let root = std::fs::read_dir(stage)
                .map_err(|e| e.to_string())?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.join("bin").join(EXE).is_file())
                .ok_or(format!("package has no bin/{EXE}"))?;
            // Never swap in a build this machine cannot start. From 0.1.5 the
            // Linux build links WebKitGTK 4.1 (the App panel's webview); a
            // machine without it would get an app that does not open at all.
            #[cfg(target_os = "linux")]
            if let Ok(out) = Command::new("ldd").arg(root.join("bin").join(EXE)).output() {
                let missing: Vec<String> = String::from_utf8_lossy(&out.stdout)
                    .lines()
                    .filter(|l| l.contains("not found"))
                    .filter_map(|l| l.split_whitespace().next().map(str::to_owned))
                    .collect();
                if !missing.is_empty() {
                    let hint = if missing.iter().any(|m| m.contains("webkit2gtk-4.1") || m.contains("javascriptcoregtk-4.1")) {
                        " - install it with: sudo apt install libwebkit2gtk-4.1-0"
                    } else {
                        ""
                    };
                    return Err(format!("v{} needs {}{hint}; staying on v{VERSION}", rel.version, missing.join(", ")));
                }
            }
            if let Some(cli) = cli {
                let new_cli = root.join("lib/cove/bin/ndexr");
                if new_cli.is_file() {
                    replace_file(&new_cli, cli)?;
                }
            }
            replace_file(&root.join("bin").join(EXE), exe)
        }
    }
}

/// Rename the old bundle aside, the new one in; roll back if the second step fails.
fn swap_dir(new: &Path, target: &Path) -> Result<(), String> {
    let old = target.with_file_name(format!(".Cove.app.old-{}", std::process::id()));
    std::fs::rename(target, &old).map_err(|e| format!("move old app aside: {e}"))?;
    if let Err(e) = std::fs::rename(new, target) {
        let _ = std::fs::rename(&old, target);
        return Err(format!("install new app: {e}"));
    }
    let _ = std::fs::remove_dir_all(&old);
    Ok(())
}

/// Copy beside the target, then rename over it — atomic, and safe while it runs.
#[cfg(unix)]
fn replace_file(new: &Path, target: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let tmp = target.with_file_name(format!(
        ".{}.new-{}",
        target.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    std::fs::copy(new, &tmp).map_err(|e| format!("copy {}: {e}", target.display()))?;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("replace {}: {e}", target.display())
    })
}

/// Windows will not overwrite a running .exe, but it will rename one: the old
/// one moves aside (removed at the next swap), the new one takes its name.
#[cfg(windows)]
fn replace_file(new: &Path, target: &Path) -> Result<(), String> {
    let name = target.file_name().unwrap_or_default().to_string_lossy().into_owned();
    if let Ok(dir) = std::fs::read_dir(target.parent().unwrap_or(Path::new("."))) {
        for e in dir.flatten() {
            if e.file_name().to_string_lossy().starts_with(&format!(".{name}.old-")) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let tmp = target.with_file_name(format!(".{name}.new-{}", std::process::id()));
    let old = target.with_file_name(format!(".{name}.old-{}", std::process::id()));
    std::fs::copy(new, &tmp).map_err(|e| format!("copy {}: {e}", target.display()))?;
    if target.exists() {
        std::fs::rename(target, &old).map_err(|e| format!("move old {} aside: {e}", target.display()))?;
    }
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::rename(&old, target);
        let _ = std::fs::remove_file(&tmp);
        format!("replace {}: {e}", target.display())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions_numerically() {
        assert!(newer("0.1.10", "0.1.9"));
        assert!(newer("v1.0.0", "0.9.9"));
        assert!(!newer("0.1.0", "0.1.0"));
        assert!(!newer("0.1.0-rc1", "0.1.0"));
        assert!(newer("0.2", "0.1.9"));
    }

    #[test]
    fn swaps_a_bundle_in_place() {
        let root = std::env::temp_dir().join(format!("cove-swap-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (app, new) = (root.join("Cove.app"), root.join("stage/Cove.app"));
        std::fs::create_dir_all(app.join("Contents")).unwrap();
        std::fs::write(app.join("Contents/v"), "old").unwrap();
        std::fs::create_dir_all(new.join("Contents")).unwrap();
        std::fs::write(new.join("Contents/v"), "new").unwrap();
        swap_dir(&new, &app).unwrap();
        assert_eq!(std::fs::read_to_string(app.join("Contents/v")).unwrap(), "new");
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2); // Cove.app + stage, no .old left
        std::fs::remove_dir_all(&root).unwrap();
    }
}
