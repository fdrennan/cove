//! Storage against the person's plan (operator, 2026-10-14): "users will get
//! 5gb storage for their app and once they go over 5gb they have to subscribe
//! or ask the agent to remove something (they can help you do it, until
//! under)". The allowance and who is paid are billing.ndexr.io's
//! (pricing.r `storage_cap_bytes`, r/billing/tier.r); Cove only measures and
//! holds the line.
//!
//! Two halves:
//! - `keeper` runs in `cove multiuser`, as root: for everyone with a Cove
//!   session it asks billing their plan (every 10 min, so a new subscription
//!   lifts the cap without a new session) and measures their home plus their
//!   owner folder on the platform checkout, which their own account cannot
//!   read. The answer goes to /run/cove/storage/<owner>.json, root-owned and
//!   readable by the person's group only: a session cannot write its own number.
//! - The session (`cove serve`, as the person) reads it (`mine`). Over the cap
//!   it takes no uploads and makes no new domains, and every agent turn is
//!   about freeing space (`agent_brief`). Saving an open file still works: a
//!   few KB of text is not the problem, and losing typed work would be.
//!
//! Operator, 2026-10-18: "limit non-paying users to .5gb code storage and if
//! go over either a) allow agent to clear it b) give user 5 minutes to clear
//! c) pay to upgrade through account". The cap (1.5 GB since 2026-10-28) is
//! pricing.r's; the three
//! ways out are Cove's storage strip: (a) `clear`, which removes only what
//! rebuilds itself, (b) `STORAGE_GRACE_SECS` from the keeper's first sight of
//! them over (`over_since`) before anything is held back, (c) Subscribe.

use crate::api::{gb, StorageView, STORAGE_GRACE_SECS, SUBSCRIBE_URL};
use std::path::PathBuf;

/// Where `keeper` leaves each person's measure.
pub const DIR: &str = "/run/cove/storage";

fn file_for(owner: &str) -> PathBuf {
    PathBuf::from(DIR).join(format!("{owner}.json"))
}

/// The signed-in person's storage, as the multiuser server last measured it.
/// None outside `cove multiuser` (a desktop, a cell): nothing measures there,
/// so nothing is held back.
pub fn mine() -> Option<StorageView> {
    of(&std::env::var("COVE_OWNER").ok()?)
}

/// `owner`'s storage as the multiuser server last measured it.
pub fn of(owner: &str) -> Option<StorageView> {
    if owner.is_empty() || owner.contains(['/', '.']) {
        return None;
    }
    let mut v: StorageView = serde_json::from_slice(&std::fs::read(file_for(owner)).ok()?).ok()?;
    v.grace_left = grace_left(&v, now_secs());
    Some(v)
}

/// Seconds of grace `v` has left at `now`: the whole grace until the keeper
/// has stamped `over_since`, none once it has run out or when under the cap.
fn grace_left(v: &StorageView, now: u64) -> u64 {
    if !v.exceeded() {
        return 0;
    }
    if v.over_since == 0 {
        return STORAGE_GRACE_SECS;
    }
    STORAGE_GRACE_SECS.saturating_sub(now.saturating_sub(v.over_since))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Ask for a new measure now (after removing something). The keeper looks for
/// this file's time every 10 s; touching it more often only costs a walk.
fn poke_file() -> PathBuf {
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| "/".into())).join(".cache/cove/storage.poke")
}

pub fn poke() {
    let p = poke_file();
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(&p, b"");
}

/// Err for anything that would add files while the person is over their cap.
pub fn refuse_growth() -> Result<(), String> {
    match mine() {
        Some(v) if v.over() => Err(format!(
            "your files use {} - over the {} your free plan includes. Subscribe ({SUBSCRIBE_URL}) or ask the agent to help you remove something first",
            gb(v.used),
            gb(v.cap)
        )),
        _ => Ok(()),
    }
}

/// Put in front of every agent turn while the person is over the cap: the
/// turn is for getting back under, whatever was asked. A restricted agent
/// has no shell and sees only the domain, so it answers with the commands and
/// the person runs them (each ```bash block shows ▶ Terminal) - they decide
/// what goes.
/// `agent_brief` for a named owner (`cove multiuser` running a turn for them).
pub fn agent_brief_for(owner: &str) -> Option<String> {
    brief_of(of(owner))
}

fn brief_of(v: Option<StorageView>) -> Option<String> {
    let v = v.filter(StorageView::over)?;
    let top: String = v.top.iter().take(15).map(|i| format!("- {}  {}\n", gb(i.bytes), i.path)).collect();
    Some(format!(
        "[Storage - from Cove, not typed by the person]\n\
         Their files on ndexr use {used}; the free plan includes {cap}, so they are {over} over. \
         Until they are under it or subscribe ({SUBSCRIBE_URL}), this turn is ONLY about freeing space, \
         whatever they ask below: help them see what takes the room and choose what to remove. \
         Do not create, download or install anything. For each removal they agree to, give one ```bash \
         block with the exact command and absolute paths (rm -rf, or e.g. `git gc`, clearing a cache); \
         Cove shows it with ▶ Terminal and they run it. Never pick for them what to delete. \
         Caches (~/.cache, renv/pip caches, build output) are the usual first wins.\n\
         Biggest, two levels down (folders include what is inside them):\n{top}\n---\n",
        used = gb(v.used),
        cap = gb(v.cap),
        over = gb(v.used - v.cap),
    ))
}

/// Folders that rebuild themselves - what "Let the agent clear it" may take
/// without asking: package installs, compiler and test caches, notebook
/// checkpoints. A cargo `target` counts only with the CACHEDIR.TAG cargo
/// writes into it, so a folder someone named target is never touched.
fn regenerable(dir: &std::path::Path) -> bool {
    match dir.file_name().and_then(|n| n.to_str()) {
        Some("node_modules" | "__pycache__" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache" | ".ipynb_checkpoints" | ".sass-cache" | ".parcel-cache") => true,
        Some("target") => dir.join("CACHEDIR.TAG").is_file(),
        _ => false,
    }
}

/// Option (a): remove, as the person, everything regenerable under their
/// home and their owner folder, plus the contents of ~/.cache (pip, renv,
/// R and the like re-download what they need) - never .git, never another
/// filesystem, never through a symlink - and ask for a new measure. What is
/// left over the cap is theirs to choose; the agent helps with that.
#[cfg(unix)]
pub fn clear() -> Result<crate::api::StorageCleared, String> {
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};
    let owner = std::env::var("COVE_OWNER").unwrap_or_default();
    let v = mine().ok_or("nothing measures storage here")?;
    if !v.exceeded() {
        return Err("you are under your storage - nothing to clear".into());
    }
    let home = PathBuf::from(std::env::var("HOME").map_err(|_| "no HOME")?);
    let mut roots = vec![home.clone()];
    let owned = Path::new(crate::host::PLATFORM_DOMAINS).join(&owner);
    if !owner.is_empty() && !owner.contains(['/', '.']) && owned.is_dir() && !owned.starts_with(&home) {
        roots.push(owned);
    }
    let mut found: Vec<PathBuf> = Vec::new();
    fn walk(p: &Path, dev: u64, found: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
            let path = e.path();
            let Ok(m) = std::fs::symlink_metadata(&path) else { continue };
            if !m.is_dir() || m.dev() != dev || e.file_name() == ".git" {
                continue;
            }
            if regenerable(&path) {
                found.push(path);
            } else {
                walk(&path, dev, found);
            }
        }
    }
    for r in &roots {
        let Ok(m) = std::fs::symlink_metadata(r) else { continue };
        // ~/.cache whole, but Cove's own folder (the measure poke) stays.
        if r == &home {
            for e in std::fs::read_dir(home.join(".cache")).into_iter().flatten().flatten() {
                if e.file_name() != "cove" {
                    found.push(e.path());
                }
            }
        }
        walk(r, m.dev(), &mut found);
    }
    let mut out = crate::api::StorageCleared::default();
    for p in found {
        let (bytes, _) = keep::measure(std::slice::from_ref(&p));
        let gone = match std::fs::symlink_metadata(&p) {
            Ok(m) if m.is_dir() => std::fs::remove_dir_all(&p),
            Ok(_) => std::fs::remove_file(&p),
            Err(e) => Err(e),
        };
        if gone.is_ok() {
            out.freed += bytes;
            out.removed.push(p.display().to_string());
        }
    }
    poke();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grace_runs_five_minutes_from_the_first_measure_over() {
        let mut v = StorageView { cap: 500_000_000, used: 600_000_000, ..Default::default() };
        assert_eq!(grace_left(&v, 1_000), STORAGE_GRACE_SECS, "not stamped yet: the whole grace");
        v.over_since = 1_000;
        assert_eq!(grace_left(&v, 1_100), STORAGE_GRACE_SECS - 100);
        v.grace_left = grace_left(&v, 1_100);
        assert!(v.exceeded() && !v.over(), "in grace nothing is held back");
        v.grace_left = grace_left(&v, 1_000 + STORAGE_GRACE_SECS);
        assert!(v.over());
        v.used = 400_000_000;
        assert_eq!(grace_left(&v, 9_999), 0);
        assert!(!v.over());
    }

    #[test]
    fn only_cargo_targets_count_as_build_output() {
        let root = std::env::temp_dir().join(format!("cove-regen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("a/target")).unwrap();
        std::fs::create_dir_all(root.join("b/target")).unwrap();
        std::fs::write(root.join("b/target/CACHEDIR.TAG"), b"Signature: 8a477f597d28d172789f06886806bc55").unwrap();
        assert!(!regenerable(&root.join("a/target")));
        assert!(regenerable(&root.join("b/target")));
        assert!(regenerable(&root.join("x/node_modules")));
        assert!(!regenerable(&root.join("x/data")));
        let _ = std::fs::remove_dir_all(&root);
    }
}

/// The root half, for `cove multiuser`.
#[cfg(unix)]
pub mod keep {
    use crate::api::{StorageItem, StorageView};
    use std::collections::{HashMap, HashSet};
    use std::os::unix::fs::MetadataExt;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    /// What one walk adds up: bytes on disk (blocks, not lengths - a sparse
    /// file costs what it holds), each hard-linked file once, and the
    /// biggest things one and two levels below each root.
    struct Walk {
        dev: u64,
        seen: HashSet<(u64, u64)>,
        items: Vec<StorageItem>,
    }

    fn walk(w: &mut Walk, p: &Path, depth: usize) -> u64 {
        let Ok(m) = std::fs::symlink_metadata(p) else { return 0 };
        // Another filesystem mounted inside (a stack, a bind mount) is not theirs.
        if m.dev() != w.dev {
            return 0;
        }
        if !m.is_dir() && m.nlink() > 1 && !w.seen.insert((m.dev(), m.ino())) {
            return 0;
        }
        let mut n = m.blocks() * 512;
        if m.is_dir() {
            for e in std::fs::read_dir(p).into_iter().flatten().flatten() {
                n += walk(w, &e.path(), depth + 1);
            }
        }
        if (1..=2).contains(&depth) {
            w.items.push(StorageItem { path: p.display().to_string(), bytes: n });
        }
        n
    }

    /// Total bytes under `roots` and the biggest 25 things in them.
    pub fn measure(roots: &[PathBuf]) -> (u64, Vec<StorageItem>) {
        let mut total = 0;
        let mut items = Vec::new();
        for r in roots {
            let Ok(m) = std::fs::symlink_metadata(r) else { continue };
            let mut w = Walk { dev: m.dev(), seen: HashSet::new(), items: Vec::new() };
            total += walk(&mut w, r, 0);
            items.append(&mut w.items);
        }
        items.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        items.truncate(25);
        (total, items)
    }

    /// One person, as the keeper knows them.
    #[derive(Default)]
    struct Known {
        view: StorageView,
        /// When billing was last asked; None = never.
        asked: Option<Instant>,
        measured: Option<SystemTime>,
    }

    /// Someone with a Cove session: their ndexr username, and the home and
    /// group of the account the session runs as.
    pub struct Person {
        pub owner: String,
        pub home: PathBuf,
        pub gid: u32,
    }

    fn now_secs() -> u64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
    }

    fn write(owner: &str, gid: u32, v: &StorageView) -> std::io::Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let dir = Path::new(super::DIR);
        std::fs::create_dir_all(dir)?;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755))?;
        let tmp = dir.join(format!(".{owner}.json.tmp"));
        std::fs::write(&tmp, serde_json::to_vec(v).unwrap_or_default())?;
        std::os::unix::fs::chown(&tmp, Some(0), Some(gid))?;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o640))?;
        std::fs::rename(&tmp, super::file_for(owner))
    }

    /// Every 10 s: for each person `people()` names, ask billing their plan
    /// when 10 min old, and measure when due - every 5 min, every minute once
    /// they are within 10% of the cap or over it (so getting back under shows
    /// soon), or at once when their session poked. No cap, no walk: the
    /// platform account's home is the whole repository.
    pub fn keeper(people: impl Fn() -> Vec<Person> + Send + 'static) {
        std::thread::spawn(move || {
            let mut known: HashMap<String, Known> = HashMap::new();
            loop {
                for p in people() {
                    // A restart keeps the grace clock: start from the last answer.
                    let k = known.entry(p.owner.clone()).or_insert_with(|| Known {
                        view: super::of(&p.owner).map(|v| StorageView { grace_left: 0, ..v }).unwrap_or_default(),
                        ..Default::default()
                    });
                    let mut changed = false;
                    if k.asked.is_none_or(|t| t.elapsed() > Duration::from_secs(600)) {
                        k.asked = Some(Instant::now());
                        // A failed lookup keeps the last answer; with none, no
                        // cap - a database blip must not lock anyone out.
                        if let Some((plan, cap)) = crate::host::plan_lookup(&p.owner) {
                            changed |= plan != k.view.plan || cap != k.view.cap;
                            k.view.plan = plan;
                            k.view.cap = cap;
                        }
                    }
                    if k.view.cap > 0 {
                        let poked = std::fs::metadata(p.home.join(".cache/cove/storage.poke")).and_then(|m| m.modified()).ok();
                        let every = if k.view.used * 10 >= k.view.cap * 9 { 60 } else { 300 };
                        let due = match k.measured {
                            None => true,
                            Some(at) => at.elapsed().map_or(true, |e| e > Duration::from_secs(every)) || poked.is_some_and(|t| t > at),
                        };
                        if due {
                            let mut roots = vec![p.home.clone()];
                            // Their domains on the platform checkout, when they
                            // are not already under the home.
                            let owned = Path::new(crate::host::PLATFORM_DOMAINS).join(&p.owner);
                            if owned.is_dir() && !owned.starts_with(&p.home) {
                                roots.push(owned);
                            }
                            let (used, top) = measure(&roots);
                            k.view.used = used;
                            k.view.top = top;
                            k.view.measured = now_secs();
                            k.measured = Some(SystemTime::now());
                            // The grace clock: from the first measure over, until under.
                            if !k.view.exceeded() {
                                k.view.over_since = 0;
                            } else if k.view.over_since == 0 {
                                k.view.over_since = k.view.measured;
                            }
                            changed = true;
                        }
                    } else if k.view.measured != 0 {
                        k.view = StorageView { plan: k.view.plan.clone(), ..Default::default() };
                        changed = true;
                    }
                    if changed || !super::file_for(&p.owner).exists() {
                        if let Err(e) = write(&p.owner, p.gid, &k.view) {
                            eprintln!("multiuser: storage for {}: {e}", p.owner);
                        }
                    }
                }
                std::thread::sleep(Duration::from_secs(10));
            }
        });
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn counts_blocks_once_per_inode_and_ranks_the_biggest() {
            let root = std::env::temp_dir().join(format!("cove-storage-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("big/inner")).unwrap();
            std::fs::create_dir_all(root.join("small")).unwrap();
            std::fs::write(root.join("big/inner/a"), vec![1u8; 300_000]).unwrap();
            std::fs::hard_link(root.join("big/inner/a"), root.join("big/inner/b")).unwrap();
            std::fs::write(root.join("small/c"), vec![1u8; 10_000]).unwrap();
            let (total, top) = measure(std::slice::from_ref(&root));
            assert!(total >= 310_000 && total < 600_000, "hard link counted twice? {total}");
            assert_eq!(top[0].path, root.join("big").display().to_string());
            assert!(top.iter().any(|i| i.path.ends_with("big/inner")));
            assert!(!top.iter().any(|i| i.path.ends_with("inner/a")), "three levels down is not listed");
            let _ = std::fs::remove_dir_all(&root);
        }
    }
}
