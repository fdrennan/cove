//! Which software stack a project's modules come from (operator, 2026-11-26,
//! answering "why not just use EESSI or Compute Canada's existing CVMFS
//! stack?" on r/HPC: Cove should "learn how to be able to consume either the
//! ndexr stack, the EESSI stack or the Compute Canada stack using ml as a
//! framework to build domains and frameworks against").
//!
//! The three share one model - Lmod over a read-only CernVM-FS repository -
//! and differ only in how a shell enters them and how they spell a program.
//! A stack is one row of `PROFILES`; the choice is one word, kept in
//! Settings (`stack_source`) and mirrored to `~/.config/ndexr/cove-stack`
//! so a project's shell scripts (templates/starters/_cove/stack.sh) read the
//! same answer without parsing JSON. "" picks the first one mounted, the
//! ndexr stack first.

use std::path::PathBuf;

pub use crate::api::{StackProfile as Profile, STACKS as PROFILES};

pub fn profile(key: &str) -> Option<&'static Profile> {
    PROFILES.iter().find(|p| p.key == key)
}

/// The file stack.sh reads: in the home folder itself, not under
/// XDG_CONFIG_HOME - Cove gives each session its own of that, and a choice
/// made in one session would be gone in the next.
pub fn choice_file() -> PathBuf {
    PathBuf::from(crate::host::home()).join(".config").join("ndexr").join("cove-stack")
}

/// Is `p` here? The ndexr stack is wherever Cove finds it (a picked folder,
/// the mount, a local build); the others only at their CVMFS mounts.
pub fn mounted(p: &Profile) -> bool {
    if p.key == "ndexr" {
        return crate::host::use_sh().is_some();
    }
    std::path::Path::new(p.init).is_file()
}

/// The stack in use: COVE_STACK, else the saved choice, else the first one
/// mounted, else the ndexr stack (what setup offers to mount).
pub fn chosen(saved: &str) -> &'static Profile {
    let env = std::env::var("COVE_STACK").unwrap_or_default();
    pick(&env, saved, mounted)
}

fn pick(env: &str, saved: &str, here: impl Fn(&Profile) -> bool) -> &'static Profile {
    if let Some(p) = profile(env.trim()).or_else(|| profile(saved.trim())) {
        return p;
    }
    PROFILES.iter().find(|p| here(p)).unwrap_or(&PROFILES[0])
}

/// Write the choice where stack.sh reads it; "" removes it (automatic).
pub fn save_choice(key: &str) -> Result<(), String> {
    let f = choice_file();
    if key.is_empty() {
        let _ = std::fs::remove_file(&f);
        return Ok(());
    }
    if profile(key).is_none() {
        return Err(format!("not a stack: {key} (ndexr, eessi or computecanada)"));
    }
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&f, format!("{key}\n")).map_err(|e| format!("{}: {e}", f.display()))
}

/// What the Terminal sources to enter the chosen stack: the ndexr stack's
/// own use.sh, or for another a small script that sources its init and runs
/// its `after`, written beside Cove's other helpers. None: it is not here.
pub fn enter_script(saved: &str, dir: &std::path::Path) -> Option<PathBuf> {
    let p = chosen(saved);
    if p.key == "ndexr" {
        return crate::host::use_sh();
    }
    if !mounted(p) {
        return None;
    }
    let f = dir.join(format!("stack-{}.sh", p.key));
    let body = format!("# Cove: enter {} (host/stacks.rs)\n. {} >/dev/null 2>&1\n{}\n", p.name, p.init, if p.after.is_empty() { ":".to_owned() } else { format!("{} >/dev/null 2>&1", p.after) });
    if std::fs::read_to_string(&f).ok().as_deref() != Some(body.as_str()) {
        std::fs::create_dir_all(dir).ok()?;
        std::fs::write(&f, body).ok()?;
    }
    Some(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_wins_then_what_is_mounted_then_ndexr() {
        assert_eq!(pick("", "", |_| false).key, "ndexr");
        assert_eq!(pick("", "", |p| p.key == "eessi").key, "eessi");
        assert_eq!(pick("", "computecanada", |p| p.key == "eessi").key, "computecanada");
        assert_eq!(pick("eessi", "computecanada", |_| false).key, "eessi", "COVE_STACK over the saved choice");
        assert_eq!(pick("", "nonsense", |p| p.key == "ndexr").key, "ndexr", "an unknown word is automatic");
    }

    #[test]
    fn the_shell_helper_knows_every_profile() {
        let sh = crate::host::starter_files("_cove").into_iter().find(|(n, _)| *n == "stack.sh").map(|(_, b)| b).expect("_cove/stack.sh");
        for p in PROFILES {
            assert!(sh.contains(p.key) && sh.contains(p.init), "stack.sh lacks {}", p.key);
            if !p.after.is_empty() {
                assert!(sh.contains(p.after), "stack.sh lacks {}'s {}", p.key, p.after);
            }
        }
    }

    #[test]
    fn stack_sh_parses_and_maps_names() {
        let sh = crate::host::starter_files("_cove").into_iter().find(|(n, _)| *n == "stack.sh").map(|(_, b)| b).unwrap();
        let d = std::env::temp_dir().join(format!("cove-stacksh-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("stack.sh");
        std::fs::write(&f, sh).unwrap();
        let out = std::process::Command::new("bash")
            .arg("-c")
            .arg(format!(
                ". {}; for m in env/r4.6_py3.13-release env/Python/3.13.15 env/R/4.6.1-bioc3.23 nodejs/22.22.2-GCCcore-13.3.0 Rust/1.96.0-GCCcore-13.3.0 Julia/1.11.7; do printf '%s|' \"$(cove_programs $m)\"; done",
                f.display()
            ))
            .env("COVE_STACK", "eessi")
            .output()
            .unwrap();
        let _ = std::fs::remove_dir_all(&d);
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        assert_eq!(String::from_utf8_lossy(&out.stdout), "R Python|Python|R|nodejs|Rust|Julia|");
    }
}
