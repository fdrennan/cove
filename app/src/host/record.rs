//! A project's record: the exact software it runs on, so it can be reopened
//! or deployed later on the same artifacts - step 5 of "how we build" on
//! ndexr.io ("Every project records the exact software versions, toolchain,
//! platform and CVMFS revision it uses"; operator, 2026-11-18: "improve run
//! and record in cove and ndexr").
//!
//! Read off the running app, never off what a script meant to load: its
//! process's own environment says which modules Lmod loaded (LOADEDMODULES),
//! from which stack (MODULEPATH), and its language is asked, in that same
//! environment, which packages it can load and from where. So the record is
//! the same whoever started the app - Cove's Up, the systemd unit, a cell
//! waking a site - and a script that changed is caught by what it did.
//!
//! Written to `.cove/record.json`, beside the contract, and so saved with
//! the project's versions (the 30-minute autosave): an older version carries
//! the record of what it ran on. Rewritten only when something in it changed,
//! so a version shows a record change only when the software did. The stack
//! fields are the image record's (ndexr image `io.ndexr.stack.*`).

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use crate::api::{ProjectRecord, RecordContract, RecordLock, RecordPackage, RecordPlatform, RecordSource, RecordStack, RecordView};

pub const FILE: &str = "record.json";
const LOCKS: [&str; 7] = ["Cargo.lock", "Manifest.toml", "package-lock.json", "renv.lock", "requirements.txt", "uv.lock", "poetry.lock"];

fn run(cmd: &mut Command) -> Option<String> {
    let o = cmd.output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// The process listening on 127.0.0.1:`port`, as `ss` names it.
fn listener(port: u16) -> Option<u32> {
    let out = run(Command::new("ss").args(["-ltnpH", &format!("sport = :{port}")]))?;
    let i = out.find("pid=")? + 4;
    out[i..].split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

fn environ(pid: u32) -> Option<HashMap<String, String>> {
    let raw = std::fs::read(format!("/proc/{pid}/environ")).ok()?;
    Some(
        raw.split(|b| *b == 0)
            .filter_map(|kv| {
                let kv = String::from_utf8_lossy(kv);
                let (k, v) = kv.split_once('=')?;
                Some((k.to_owned(), v.to_owned()))
            })
            .collect(),
    )
}

/// The stack a MODULEPATH is under: (mount, triple), from its
/// `<mount>/<triple>/...` entries.
fn stack_of(modulepath: &str) -> Option<(String, String)> {
    modulepath.split(':').find_map(|p| {
        let rest = p.strip_prefix("/opt/ndexr/stack/")?;
        let triple = rest.split('/').next().filter(|t| t.contains("-linux-"))?;
        Some(("/opt/ndexr/stack".to_owned(), triple.to_owned()))
    })
}

/// The CVMFS revision of the stack at `mount`, when it is CVMFS.
pub fn stack_revision(mount: &str) -> Option<String> {
    run(Command::new("getfattr").args(["--only-values", "-n", "user.revision", mount])).filter(|r| !r.is_empty())
}

/// The toolchain of a language's module: `R/4.6.1-foss-2024a` -> foss-2024a.
fn toolchain(modules: &[String], language: &str) -> String {
    let name = match language {
        "r" => "R/",
        "python" => "Python/",
        "julia" => "Julia/",
        "rust" => "Rust/",
        "node" => "nodejs/",
        _ => return String::new(),
    };
    modules
        .iter()
        .rev()
        .find_map(|m| m.strip_prefix(name))
        .and_then(|v| v.split_once('-').map(|(_, tc)| tc.to_owned()))
        .unwrap_or_default()
}

fn platform() -> RecordPlatform {
    let os = std::fs::read_to_string("/etc/os-release")
        .ok()
        .and_then(|t| t.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=").map(|v| v.trim_matches('"').to_owned())))
        .unwrap_or_default();
    RecordPlatform {
        arch: std::env::consts::ARCH.to_owned(),
        os,
        kernel: std::fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default().trim().to_owned(),
        glibc: run(Command::new("getconf").arg("GNU_LIBC_VERSION")).map(|g| g.trim_start_matches("glibc ").to_owned()).unwrap_or_default(),
    }
}

/// The R packages the app names (APP_NEEDS r:…) and everything they load,
/// as the app's own R sees them: first on its library path wins.
const R_PACKAGES: &str = r#"a <- commandArgs(TRUE)
have <- installed.packages()
have <- have[!duplicated(have[, "Package"]), , drop = FALSE]
rownames(have) <- have[, "Package"]
want <- intersect(a, rownames(have))
deps <- unique(c(want, unlist(tools::package_dependencies(want, db = have, recursive = TRUE))))
deps <- sort(intersect(deps, rownames(have)))
deps <- deps[is.na(have[deps, "Priority"]) | have[deps, "Priority"] != "base"]
cat(sprintf("%s\t%s\t%s\n", deps, have[deps, "Version"], have[deps, "LibPath"]), sep = "")"#;

fn packages(dir: &Path, env: &HashMap<String, String>, language: &str, needs: &str) -> Vec<RecordPackage> {
    let mut cmd = match language {
        "r" => {
            let want: Vec<&str> = needs.split_whitespace().filter_map(|n| n.strip_prefix("r:")).collect();
            if want.is_empty() {
                return Vec::new();
            }
            let mut c = Command::new("Rscript");
            c.args(["--vanilla", "-e", R_PACKAGES, "--args"]).args(want);
            c
        }
        "python" => {
            let mut c = Command::new("python3");
            c.args(["-m", "pip", "list", "--format=freeze", "--disable-pip-version-check"]);
            c
        }
        _ => return Vec::new(),
    };
    cmd.env_clear().envs(env).current_dir(dir);
    let Some(out) = run(&mut cmd) else { return Vec::new() };
    out.lines()
        .filter_map(|l| {
            let (name, version, from) = match language {
                "r" => {
                    let mut f = l.split('\t');
                    (f.next()?, f.next()?, f.next().unwrap_or(""))
                }
                _ => {
                    let (n, v) = l.split_once("==")?;
                    (n, v, "")
                }
            };
            Some(RecordPackage { language: language.to_owned(), name: name.to_owned(), version: version.to_owned(), from: from.to_owned() })
        })
        .collect()
}

fn locks(dir: &Path) -> Vec<RecordLock> {
    LOCKS
        .iter()
        .filter(|f| dir.join(f).is_file())
        .filter_map(|f| {
            let h = run(Command::new("sha256sum").arg(dir.join(f)))?;
            Some(RecordLock { file: (*f).to_owned(), sha256: h.split_whitespace().next()?.to_owned() })
        })
        .collect()
}

fn source(dir: &Path) -> RecordSource {
    let git = |args: &[&str]| run(Command::new("git").arg("-C").arg(dir).args(args));
    RecordSource {
        commit: git(&["rev-parse", "HEAD"]).unwrap_or_default(),
        // The record itself does not count: writing it is not a change.
        dirty: git(&["status", "--porcelain", "--", ".", ":!.cove/record.json", ":!.cove/run"]).is_some_and(|s| !s.is_empty()),
    }
}

/// Read the record off the app running for the project at `dir`.
pub fn capture(dir: &Path) -> Result<ProjectRecord, String> {
    let cdir = crate::project::dotcove::contract_dir(dir);
    let env_text = std::fs::read_to_string(cdir.join("app.env")).map_err(|_| "no .cove/app.env: a project made from a starter has one".to_owned())?;
    let app: HashMap<String, String> = crate::host::apps::read_env(&env_text).into_iter().collect();
    let get = |k: &str| app.get(k).cloned().unwrap_or_default();
    let (language, kind) = (get("APP_LANGUAGE"), get("APP_KIND"));
    if kind != "port" {
        return Err(format!("a {} app: the platform's shared R runs it, so its software is the platform's, not this project's", if kind.is_empty() { "module" } else { &kind }));
    }
    let port: u16 = get("APP_PORT").parse().map_err(|_| "APP_PORT in .cove/app.env is not a port".to_owned())?;
    let pid = listener(port).ok_or_else(|| format!("nothing of yours is listening on 127.0.0.1:{port}: Up it first"))?;
    let env = environ(pid).ok_or_else(|| format!("cannot read process {pid}'s environment"))?;
    let modules: Vec<String> = env.get("LOADEDMODULES").map(|m| m.split(':').filter(|x| !x.is_empty()).map(str::to_owned).collect()).unwrap_or_default();
    let (mount, triple) = env.get("MODULEPATH").and_then(|m| stack_of(m)).unwrap_or_default();
    let revision = if mount.is_empty() { None } else { stack_revision(&mount) };
    Ok(ProjectRecord {
        schema: 1,
        project: dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        recorded_at: run(Command::new("date").args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])).unwrap_or_default(),
        recorded_by: format!("cove {}", crate::host::update::VERSION),
        source: source(dir),
        contract: RecordContract { language: language.clone(), kind, port: Some(port) },
        platform: platform(),
        stack: RecordStack { repository: if mount.is_empty() { String::new() } else { "stack.ndexr.io".into() }, mount, revision, triple },
        toolchain: toolchain(&modules, &language),
        packages: packages(dir, &env, &language, &get("APP_NEEDS")),
        modules,
        locks: locks(dir),
    })
}

/// The saved record, if there is one.
pub fn saved(dir: &Path) -> Option<ProjectRecord> {
    serde_json::from_str(&std::fs::read_to_string(crate::project::dotcove::contract_dir(dir).join(FILE)).ok()?).ok()
}

/// Capture and save; the file is left alone when only the time differs.
pub fn save(dir: &Path) -> Result<(ProjectRecord, bool), String> {
    let new = capture(dir)?;
    if let Some(old) = saved(dir) {
        if (ProjectRecord { recorded_at: new.recorded_at.clone(), source: new.source.clone(), ..old.clone() }) == new {
            return Ok((old, false));
        }
    }
    let path = crate::project::dotcove::contract_dir(dir).join(FILE);
    let text = serde_json::to_string_pretty(&new).map_err(|e| e.to_string())? + "\n";
    std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok((new, true))
}

/// Settings › Record: the saved record (or a fresh one), and the stack now.
pub fn view(dir: &Path, capture_now: bool) -> RecordView {
    let (record, note) = if capture_now {
        match save(dir) {
            Ok((r, _)) => (Some(r), String::new()),
            Err(e) => (saved(dir), e),
        }
    } else {
        (saved(dir), String::new())
    };
    let mount = record.as_ref().map(|r| r.stack.mount.clone()).filter(|m| !m.is_empty()).unwrap_or_else(|| "/opt/ndexr/stack".into());
    RecordView { record, stack_now: stack_revision(&mount), note }
}

/// `cove record [dir]`: capture the record of the app running for a project
/// (default: the folder you are in). Up runs it once the app answers.
pub fn cli(args: &[String]) -> i32 {
    let dir = args.first().cloned().unwrap_or_else(|| ".".into());
    let dir = Path::new(&dir).canonicalize().unwrap_or_else(|_| Path::new(&dir).to_path_buf());
    match save(&dir) {
        Ok((r, wrote)) => {
            println!(
                "record: {} · {} modules · {} packages · stack {} revision {} · {}{}",
                r.project,
                r.modules.len(),
                r.packages.len(),
                if r.stack.repository.is_empty() { "none" } else { &r.stack.repository },
                r.stack.revision.as_deref().unwrap_or("?"),
                r.toolchain,
                if wrote { " (saved .cove/record.json)" } else { " (unchanged)" }
            );
            0
        }
        Err(e) => {
            eprintln!("record: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_stack_and_toolchain() {
        let mp = "/home/a/.mod:/opt/ndexr/stack/x86_64-pc-linux-gnu/eb/modules/all:/usr/share/lmod";
        assert_eq!(stack_of(mp), Some(("/opt/ndexr/stack".into(), "x86_64-pc-linux-gnu".into())));
        assert_eq!(stack_of("/usr/share/lmod"), None);
        let m: Vec<String> = ["GCCcore/13.3.0", "R/4.6.1-foss-2024a", "env/r4.6_py3.13-release"].map(String::from).to_vec();
        assert_eq!(toolchain(&m, "r"), "foss-2024a");
        assert_eq!(toolchain(&m, "python"), "");
    }
}
