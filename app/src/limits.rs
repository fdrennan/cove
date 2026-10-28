//! What one person may use of a shared machine (operator, 2026-10-25: "each
//! user of cove.ndexr.io will need a limit to the memory and cpu and such they
//! can consume").
//!
//! The limit is a cgroup per account, and everything the account's sessions
//! start lives in it: the session, its consoles and terminals, the apps `up.sh`
//! starts, every `cargo` and `julia` a build runs. One person's runaway
//! compile is killed inside their own memory, and waits on their own cores,
//! instead of taking the machine from everyone else.
//!
//!   <unit>/supervisor      `cove multiuser` itself (systemd DelegateSubgroup)
//!   <unit>/people/u<uid>   one account's sessions; memory.max, cpu.max,
//!                          pids.max from multiuser.conf person-*
//!
//! `<unit>` is the multiuser service's cgroup, delegated to it
//! (onprem/cove-onprem-multiuser.service Delegate=, DelegateSubgroup=). A
//! server whose cgroup is not delegated places nobody and limits nobody, and
//! says so at start.
//!
//! The session reads the same files back for the person (`mine`): what they
//! may use, what they use, and whether the kernel has killed anything of
//! theirs for memory - measured, never stored.

use std::path::{Path, PathBuf};

const CG: &str = "/sys/fs/cgroup";

/// The ceiling for one account; 0 = no ceiling on that resource.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Limits {
    pub memory: u64,
    pub cpus: u32,
    pub tasks: u32,
}

impl Limits {
    pub const NONE: Limits = Limits { memory: 0, cpus: 0, tasks: 0 };

    pub fn is_none(&self) -> bool {
        *self == Self::NONE
    }

    /// One line for the server's start banner and the audit log.
    pub fn describe(&self) -> String {
        if self.is_none() {
            return "no per-person limits".into();
        }
        let part = |set: bool, s: String| if set { s } else { "no limit".into() };
        format!(
            "per person: memory {} · cpus {} · tasks {}",
            part(self.memory > 0, bytes(self.memory)),
            part(self.cpus > 0, self.cpus.to_string()),
            part(self.tasks > 0, self.tasks.to_string())
        )
    }
}

/// "6G", "512M", "1.5G", "2048" (bytes) -> bytes.
pub fn parse_size(v: &str) -> Option<u64> {
    let v = v.trim();
    let (num, mult) = match v.chars().last()?.to_ascii_uppercase() {
        'K' => (&v[..v.len() - 1], 1u64 << 10),
        'M' => (&v[..v.len() - 1], 1 << 20),
        'G' => (&v[..v.len() - 1], 1 << 30),
        'T' => (&v[..v.len() - 1], 1 << 40),
        _ => (v, 1),
    };
    let n: f64 = num.trim().parse().ok()?;
    (n >= 0.0).then(|| (n * mult as f64) as u64)
}

/// 6442450944 -> "6.0 GB".
pub fn bytes(n: u64) -> String {
    let g = n as f64 / (1u64 << 30) as f64;
    if g >= 1.0 {
        format!("{g:.1} GB")
    } else {
        format!("{} MB", n >> 20)
    }
}

/// This process's cgroup, from /proc/self/cgroup's v2 line ("0::/a/b").
fn own_cgroup() -> Option<PathBuf> {
    let t = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let rel = t.lines().find_map(|l| l.strip_prefix("0::"))?.trim();
    Some(Path::new(CG).join(rel.trim_start_matches('/')))
}

/// Where the people go: `<unit>/people`, made ready for limits - or why not.
/// Run once by `cove multiuser`, as root, before it starts or adopts anyone.
/// Processes still in `<unit>` itself (sessions a server from before the
/// delegation left running) are moved to their account's group first: the
/// kernel lets a group hand out memory and cpu only once it holds no
/// processes of its own.
pub fn people_root() -> Result<PathBuf, String> {
    let own = own_cgroup().ok_or("no cgroup v2 line in /proc/self/cgroup")?;
    if own.file_name().and_then(|n| n.to_str()) != Some("supervisor") {
        return Err(format!("{} is not a delegated supervisor group (Delegate= and DelegateSubgroup=supervisor on the unit)", own.display()));
    }
    let unit = own.parent().ok_or("no parent cgroup")?.to_path_buf();
    let people = unit.join("people");
    mkdir(&people)?;
    let strays = std::fs::read_to_string(unit.join("cgroup.procs")).unwrap_or_default();
    for pid in strays.lines().filter_map(|l| l.trim().parse::<u32>().ok()) {
        let Some(uid) = uid_of(pid) else { continue };
        let to = if uid == 0 { own.clone() } else { people.join(format!("u{uid}")) };
        let _ = mkdir(&to);
        let _ = std::fs::write(to.join("cgroup.procs"), pid.to_string());
    }
    for g in [&unit, &people] {
        write(g, "cgroup.subtree_control", "+cpu +memory +pids")?;
    }
    Ok(people)
}

fn uid_of(pid: u32) -> Option<u32> {
    let t = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    t.lines().find_map(|l| l.strip_prefix("Uid:"))?.split_whitespace().next()?.parse().ok()
}

fn mkdir(p: &Path) -> Result<(), String> {
    match std::fs::create_dir(p) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(format!("{}: {e}", p.display())),
    }
}

fn write(dir: &Path, file: &str, v: &str) -> Result<(), String> {
    std::fs::write(dir.join(file), v).map_err(|e| format!("{}/{file} <- {v}: {e}", dir.display()))
}

/// The account's group under `people`, its limits written (again, every
/// time: a changed multiuser.conf reaches running people at their next
/// session). Returns the group's `cgroup.procs`, for the session to enter
/// before it drops root.
pub fn place(people: &Path, uid: u32, l: Limits) -> Result<PathBuf, String> {
    let g = people.join(format!("u{uid}"));
    mkdir(&g)?;
    let max = |n: u64| if n == 0 { "max".to_owned() } else { n.to_string() };
    write(&g, "memory.max", &max(l.memory))?;
    // Reclaim and throttle at 90 %, so a build slows down before it is killed.
    write(&g, "memory.high", &max(l.memory / 10 * 9))?;
    write(&g, "cpu.max", &if l.cpus == 0 { "max 100000".to_owned() } else { format!("{} 100000", l.cpus as u64 * 100_000) })?;
    write(&g, "pids.max", &max(l.tasks as u64))?;
    Ok(g.join("cgroup.procs"))
}

/// The environment that sizes builds to the cores a person has: cargo,
/// make, Julia and OpenMP otherwise start one worker per core of the
/// machine and fight over the few they may use.
pub fn job_env(l: Limits) -> Vec<(&'static str, String)> {
    if l.cpus == 0 {
        return Vec::new();
    }
    let n = l.cpus.to_string();
    vec![("CARGO_BUILD_JOBS", n.clone()), ("MAKEFLAGS", format!("-j{n}")), ("JULIA_NUM_THREADS", n.clone()), ("OMP_NUM_THREADS", n)]
}

/// This process's own group, as the session sees it. None outside a
/// limited group (the operator's desktop Cove, `cove serve` alone).
pub fn mine() -> Option<crate::api::Quota> {
    let g = own_cgroup()?;
    if g.parent()?.file_name()?.to_str()? != "people" {
        return None;
    }
    let read = |f: &str| std::fs::read_to_string(g.join(f)).unwrap_or_default();
    let num = |s: &str| s.trim().parse::<u64>().unwrap_or(0);
    let cpus = {
        let t = read("cpu.max");
        let mut it = t.split_whitespace();
        match (it.next().and_then(|q| q.parse::<f64>().ok()), it.next().and_then(|p| p.parse::<f64>().ok())) {
            (Some(q), Some(p)) if p > 0.0 => q / p,
            _ => 0.0,
        }
    };
    let oom_kills = read("memory.events").lines().find_map(|l| l.strip_prefix("oom_kill ")).map(num).unwrap_or(0);
    Some(crate::api::Quota { memory_max: num(&read("memory.max")), memory_now: num(&read("memory.current")), cpus, oom_kills })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_read_as_people_write_them() {
        assert_eq!(parse_size("6G"), Some(6 << 30));
        assert_eq!(parse_size("512m"), Some(512 << 20));
        assert_eq!(parse_size("1.5G"), Some(3 << 29));
        assert_eq!(parse_size("2048"), Some(2048));
        assert_eq!(parse_size("lots"), None);
        assert_eq!(parse_size("-1G"), None);
        assert_eq!(bytes(6 << 30), "6.0 GB");
        assert_eq!(bytes(300 << 20), "300 MB");
    }

    #[test]
    fn builds_are_sized_to_the_cores_a_person_has() {
        assert!(job_env(Limits::NONE).is_empty());
        let e = job_env(Limits { memory: 0, cpus: 4, tasks: 0 });
        assert!(e.contains(&("CARGO_BUILD_JOBS", "4".into())) && e.contains(&("MAKEFLAGS", "-j4".into())));
        assert_eq!(Limits { memory: 6 << 30, cpus: 4, tasks: 0 }.describe(), "per person: memory 6.0 GB · cpus 4 · tasks no limit");
    }
}
