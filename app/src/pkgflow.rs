//! Packages: where every language set is in the cycle that makes it, read
//! live from each place the cycle leaves a record (operator, 2026-10-24:
//! "determining where ... over multiple systems (on prem where app runs and
//! read sys paths, s3 where packages may or may not be yet, servers where
//! builds run and push to s3, and the repos available through nginx and which
//! are currently in use by cvmfs ml languages now)"):
//!
//! ```text
//! plan      the kit's manifest       ndexr.ndexr.io stack-kit/manifest/manifests/<set>/manifest.dcf
//! build     the build machines       /srv/ndexr/software/catalog.json (boxes, runs)
//! publish   S3, served by nginx      <repo>/<triple>/<set>/current.json  (repo.ndexr.io/env)
//! sync      this host's library      /opt/ndexr/site-library/<triple>/<set>
//! module    the stack, on CVMFS      /opt/ndexr/stack/<triple>/eb/modules/all/env/<set>.lua
//! load      a running language       the module's R_LIBS_SITE / PYTHONPATH / JULIA_* / PATH
//! use       Cove's sessions          multiuser's live sessions, by module (an admin's view)
//! ```
//!
//! What each language does with it (Rust compiles its crates in before the
//! program runs; Python, R and Julia find theirs at run time through the
//! paths the module sets) is in `load`, taken from the set's easyconfig, not
//! written here.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::api::{PackageFlow, PfLive, PfLiveMachine, PfMachine, PfPublished, PfRecord, PfRow, PfSet, PfStage, PfTrace};

const LIB_ROOT: &str = "/opt/ndexr/site-library";
const STACK: &str = "/opt/ndexr/stack";
const CATALOG: &str = "/srv/ndexr/software/catalog.json";
const TRIPLES: [&str; 2] = ["x86_64-pc-linux-gnu", "aarch64-unknown-linux-gnu"];
const LANGS: [&str; 4] = ["R", "Python", "Julia", "Rust"];

/// The public repo (ndexr's `pkgs.public_url`).
pub fn repo() -> String {
    std::env::var("COVE_PKGS_PUBLIC_URL").unwrap_or_else(|_| "https://repo.ndexr.io/env".into()).trim_end_matches('/').to_owned()
}

fn kit() -> PathBuf {
    Path::new(crate::host::PLATFORM_DOMAINS).join("ndexr.ndexr.io/crate/assets/stack-kit")
}

pub fn this_triple() -> &'static str {
    if std::env::consts::ARCH == "aarch64" { TRIPLES[1] } else { TRIPLES[0] }
}

fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

fn mtime(p: &Path) -> i64 {
    std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64)
}

/// The language a set is in: its first segment, or the combined sets'
/// two (`r4.6_py3.13-release`) and the app's own R set (`ndexr-app-r4.6.1`).
fn language(set: &str) -> &'static str {
    match set.split('/').next().unwrap_or("") {
        "R" => "R",
        "Python" => "Python",
        "Julia" => "Julia",
        "Rust" => "Rust",
        _ if set.starts_with('r') && set.contains("_py") => "R + Python",
        _ => "R",
    }
}

/// A DCF file's records (blank-line separated `Key: value` blocks).
fn dcf(text: &str) -> Vec<BTreeMap<String, String>> {
    text.split("\n\n")
        .map(|b| {
            b.lines()
                .filter(|l| !l.starts_with('#') && !l.starts_with(char::is_whitespace))
                .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned())))
                .collect::<BTreeMap<_, _>>()
        })
        .filter(|r| r.contains_key("Package"))
        .collect()
}

/// Python's name normalisation (PEP 503), so `Pillow` and `pillow` meet.
fn norm(name: &str) -> String {
    name.to_lowercase().replace(['_', '.'], "-")
}

// ── installed, read as the ndexr CLI reads them (ndexr.ndexr.io pkgs.rs) ──

fn r_installed(lib: &Path) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for e in std::fs::read_dir(lib).into_iter().flatten().flatten() {
        if e.file_name().to_string_lossy().starts_with("00LOCK") {
            continue;
        }
        let Ok(t) = std::fs::read_to_string(e.path().join("DESCRIPTION")) else { continue };
        let f = dcf(&format!("{t}\n")).into_iter().next().unwrap_or_default();
        if f.contains_key("Built") {
            if let (Some(p), Some(v)) = (f.get("Package"), f.get("Version")) {
                m.insert(p.clone(), v.clone());
            }
        }
    }
    m
}

fn py_installed(lib: &Path) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for e in std::fs::read_dir(lib).into_iter().flatten().flatten() {
        if !e.file_name().to_string_lossy().ends_with(".dist-info") {
            continue;
        }
        let Ok(t) = std::fs::read_to_string(e.path().join("METADATA")) else { continue };
        let (mut n, mut v) = (None, None);
        for l in t.lines().take_while(|l| !l.is_empty()) {
            if let Some(x) = l.strip_prefix("Name: ") {
                n = Some(x.trim().to_owned());
            } else if let Some(x) = l.strip_prefix("Version: ") {
                v = Some(x.trim().to_owned());
            }
        }
        if let (Some(n), Some(v)) = (n, v) {
            m.insert(norm(&n), v);
        }
    }
    m
}

fn julia_installed(lib: &Path) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    if !lib.join("depot/packages").is_dir() {
        return m;
    }
    let mut name: Option<String> = None;
    for l in std::fs::read_to_string(lib.join("Manifest.toml")).unwrap_or_default().lines() {
        let l = l.trim();
        if let Some(n) = l.strip_prefix("[[deps.").and_then(|r| r.strip_suffix("]]")) {
            name = Some(n.trim_matches('"').to_owned());
        } else if let Some(v) = l.strip_prefix("version = \"").and_then(|r| r.strip_suffix('"')) {
            if let Some(n) = name.take() {
                m.insert(n, v.to_owned());
            }
        }
    }
    m
}

fn rust_installed(lib: &Path) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    for e in std::fs::read_dir(lib.join("vendor")).into_iter().flatten().flatten() {
        let toml = std::fs::read_to_string(e.path().join("Cargo.toml")).unwrap_or_default();
        let (mut n, mut v, mut inside) = (None, None, false);
        for l in toml.lines().map(str::trim) {
            if l.starts_with('[') {
                inside = l == "[package]";
                continue;
            }
            let val = |l: &str| l.split_once('=').map(|(_, x)| x.trim().trim_matches('"').to_owned());
            if inside && l.starts_with("name") && l[4..].trim_start().starts_with('=') {
                n = val(l);
            } else if inside && l.starts_with("version") && l[7..].trim_start().starts_with('=') {
                v = val(l);
            }
        }
        if let (Some(n), Some(v)) = (n, v) {
            // Several versions of one crate: keep the highest-sorting one.
            let e = m.entry(n).or_insert_with(String::new);
            if v > *e {
                *e = v;
            }
        }
    }
    // Programs `cargo install` put in bin/, by its own record.
    let record: serde_json::Value = std::fs::read(lib.join(".crates2.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    for k in record["installs"].as_object().into_iter().flatten().map(|(k, _)| k) {
        let mut w = k.split_whitespace();
        if let (Some(n), Some(v)) = (w.next(), w.next()) {
            m.entry(n.to_owned()).or_insert_with(|| v.to_owned());
        }
    }
    m
}

/// Every package installed in a set's library, by its language's folder;
/// keys normalised as `published` keys are (Python by PEP 503).
fn installed(set: &str, lib: &Path) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    match language(set) {
        "Python" => m.extend(py_installed(&lib.join("python"))),
        "Julia" => m.extend(julia_installed(&lib.join("julia"))),
        "Rust" => m.extend(rust_installed(&lib.join("rust"))),
        "R + Python" => {
            m.extend(r_installed(&lib.join("R")));
            m.extend(py_installed(&lib.join("python")));
        }
        _ => m.extend(r_installed(&lib.join("R"))),
    }
    m
}

/// Scans keyed by the library folder and when it last changed: a set's
/// library is read again only after a sync touched it.
fn installed_cached(set: &str, lib: &Path) -> BTreeMap<String, String> {
    static CACHE: Mutex<Option<HashMap<PathBuf, (i64, BTreeMap<String, String>)>>> = Mutex::new(None);
    let stamp = ["R", "python", "julia", "rust"].iter().map(|d| mtime(&lib.join(d))).max().unwrap_or(0).max(mtime(lib));
    if let Some((s, m)) = CACHE.lock().unwrap().get_or_insert_with(HashMap::new).get(lib) {
        if *s == stamp {
            return m.clone();
        }
    }
    let m = installed(set, lib);
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(lib.to_path_buf(), (stamp, m.clone()));
    m
}

/// `<repo>/<triple>/<set>/current.json`, asked again at most every minute and
/// then with its ETag, so an unchanged publish costs one 304.
fn published(triple: &str, set: &str) -> Option<serde_json::Value> {
    type Entry = (Instant, String, Option<serde_json::Value>);
    static CACHE: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);
    let url = format!("{}/{triple}/{set}/current.json", repo());
    let old = CACHE.lock().unwrap().get_or_insert_with(HashMap::new).get(&url).cloned();
    if let Some((at, _, v)) = &old {
        if at.elapsed() < Duration::from_secs(60) {
            return v.clone();
        }
    }
    let mut req = ehttp::Request::get(&url);
    if let Some((_, etag, Some(_))) = &old {
        if !etag.is_empty() {
            req.headers.insert("If-None-Match", etag.clone());
        }
    }
    let (etag, v) = match ehttp::fetch_blocking(&req) {
        Ok(r) if r.status == 304 => (old.as_ref().map(|o| o.1.clone()).unwrap_or_default(), old.and_then(|o| o.2)),
        Ok(r) if r.ok => (r.headers.get("etag").unwrap_or_default().to_owned(), serde_json::from_slice(&r.bytes).ok()),
        // 403/404: nothing there. Unreachable: say so rather than keep a stale copy.
        _ => (String::new(), None),
    };
    CACHE.lock().unwrap().get_or_insert_with(HashMap::new).insert(url, (Instant::now(), etag, v.clone()));
    v
}

/// A published set's packages: name (normalised) → (version, file, sha256,
/// size). R and Python name an object each; Rust and Julia name a version
/// (their files are one archive per set).
fn published_rows(cur: &serde_json::Value) -> BTreeMap<String, (String, String, String, u64)> {
    let mut m = BTreeMap::new();
    for lang in LANGS {
        let archive = &cur[format!("{}_archive", lang.to_lowercase())];
        for (name, v) in cur[lang].as_object().into_iter().flatten() {
            let key = if lang == "Python" { norm(v["name"].as_str().unwrap_or(name)) } else { name.clone() };
            let row = match v.as_str() {
                Some(ver) => (ver.to_owned(), archive["file"].as_str().unwrap_or("").to_owned(), archive["sha256"].as_str().unwrap_or("").to_owned(), 0),
                None => (
                    v["version"].as_str().unwrap_or("").to_owned(),
                    v["file"].as_str().unwrap_or("").to_owned(),
                    v["sha256"].as_str().unwrap_or("").to_owned(),
                    v["size"].as_u64().unwrap_or(0),
                ),
            };
            m.insert(key, row);
        }
    }
    m
}

/// How the language finds this set's library: the variables the set's
/// module sets toward it, read from its module file on the stack, else
/// from the kit's easyconfig.
fn load(set: &str, triple: &str) -> String {
    let module = Path::new(STACK).join(triple).join("eb/modules/all/env").join(format!("{set}.lua"));
    let (lang, ver) = set.split_once('/').unwrap_or((set, ""));
    let ec = kit().join(format!("easybuild-suite/easyconfigs/e/env-{lang}/env-{lang}-{ver}.eb"));
    let text = std::fs::read_to_string(&module).or_else(|_| std::fs::read_to_string(&ec)).unwrap_or_default();
    let mut out: Vec<String> = Vec::new();
    for l in text.lines().map(str::trim) {
        let Some(rest) = ["prepend_path(\"", "pushenv(\"", "setenv(\"NDEXR_RUST_VENDOR"].iter().find_map(|p| l.strip_prefix(p).map(|r| (p, r))) else { continue };
        if !l.contains("_site") {
            continue;
        }
        let var = if rest.0.starts_with("setenv") { "NDEXR_RUST_VENDOR".to_owned() } else { rest.1.split('"').next().unwrap_or("").to_owned() };
        // The path after `_site`: pathJoin(_site, "julia", "depot") → julia/depot.
        let inside = l.split("_site").nth(1).unwrap_or("").split(')').next().unwrap_or("");
        let tail: Vec<&str> = inside.split('"').skip(1).step_by(2).collect();
        let entry = format!("{var} → {}", if tail.is_empty() { "the set".to_owned() } else { tail.join("/") });
        if !out.contains(&entry) {
            out.push(entry);
        }
    }
    out.join(" · ")
}

/// Every set: the kit's language sets, and whatever this host's library holds
/// (the combined sets the stack's modules load today among them).
fn sets(triple: &str) -> Vec<String> {
    let mut all = BTreeSet::new();
    let manifests = kit().join("manifest/manifests");
    for lang in LANGS {
        for e in std::fs::read_dir(manifests.join(lang)).into_iter().flatten().flatten() {
            if e.path().join("manifest.dcf").is_file() {
                all.insert(format!("{lang}/{}", e.file_name().to_string_lossy()));
            }
        }
    }
    let lib = Path::new(LIB_ROOT).join(triple);
    for e in std::fs::read_dir(&lib).into_iter().flatten().flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if n.starts_with('.') {
            continue;
        }
        if LANGS.contains(&n.as_str()) {
            for v in std::fs::read_dir(e.path()).into_iter().flatten().flatten() {
                all.insert(format!("{n}/{}", v.file_name().to_string_lossy()));
            }
        } else {
            all.insert(n);
        }
    }
    let mut v: Vec<String> = all.into_iter().collect();
    // Language sets first (R, Python, Julia, Rust; newest version first), then the combined ones.
    v.sort_by_key(|s| {
        let rank = LANGS.iter().position(|l| s.starts_with(&format!("{l}/"))).unwrap_or(LANGS.len());
        (rank, std::cmp::Reverse(s.clone()))
    });
    v
}

/// The build machines and each one's last run.
fn machines() -> Vec<PfMachine> {
    static CACHE: Mutex<Option<(i64, Vec<PfMachine>)>> = Mutex::new(None);
    let stamp = mtime(Path::new(CATALOG));
    if let Some((s, m)) = CACHE.lock().unwrap().as_ref() {
        if *s == stamp {
            return m.clone();
        }
    }
    let c: serde_json::Value = std::fs::read(CATALOG).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut out: Vec<PfMachine> = c["boxes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|b| PfMachine { name: b["name"].as_str().unwrap_or("").trim_start_matches("four-box-").to_owned(), arch: b["lane"].as_str().unwrap_or("").to_owned(), ..Default::default() })
        .collect();
    for r in c["runs"].as_array().into_iter().flatten() {
        let Some(m) = r["box"].as_u64().and_then(|b| out.get_mut(b as usize)) else { continue };
        let end = r["end"].as_i64().unwrap_or(0);
        if end >= m.last_end {
            m.last_end = end;
            m.last_built = r["n"].as_u64().unwrap_or(0) as u32;
            m.itype = r["itype"].as_str().unwrap_or("").to_owned();
        }
    }
    *CACHE.lock().unwrap() = Some((stamp, out.clone()));
    out
}

/// "1792633427" seconds → "2026-10-22T02:23:47" (UTC), so build times and
/// publish times compare as text.
fn iso(secs: i64) -> String {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}", rem / 3600, rem % 3600 / 60, rem % 60)
}

/// A current.json's head, from a file the CLI kept.
fn record_at(path: &Path) -> Option<PfRecord> {
    let v: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some(record(&v))
}

fn record(v: &serde_json::Value) -> PfRecord {
    PfRecord {
        host: v["host"].as_str().unwrap_or("").to_owned(),
        at: v["published_at"].as_str().unwrap_or("").chars().take(19).collect(),
        manifest_sha256: v["manifest_sha256"].as_str().unwrap_or("").to_owned(),
        packages: published_rows(v).len() as u32,
        failed: 0,
    }
}

/// The set's build directory (ndexr's `pkgs::build_dir`).
fn build_dir(triple: &str, set: &str) -> PathBuf {
    Path::new(LIB_ROOT).join(".build").join(triple).join(set)
}

/// The newest build of a set: this host's own publish of it (stage/), a
/// build machine's that this host pulled (sync/, when another host wrote
/// it), or the steps of a build here (pkgs.timeline.json) - whichever is
/// latest.
fn built(triple: &str, set: &str, me: &str) -> PfRecord {
    let dir = build_dir(triple, set);
    let mut all: Vec<PfRecord> = Vec::new();
    all.extend(record_at(&dir.join("stage/current.json")));
    all.extend(record_at(&dir.join("sync/current.json")).filter(|r| r.host != me));
    if let Some(t) = std::fs::read(dir.join("pkgs.timeline.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()) {
        let steps = t["steps"].as_array().cloned().unwrap_or_default();
        all.push(PfRecord {
            host: t["host"].as_str().unwrap_or("").to_owned(),
            at: iso(t["generated"].as_i64().unwrap_or(0)),
            manifest_sha256: String::new(),
            packages: steps.iter().filter(|s| s["status"] == "done").count() as u32,
            failed: steps.iter().filter(|s| s["status"] == "failed").count() as u32,
        });
    }
    all.into_iter().max_by(|a, b| a.at.cmp(&b.at)).unwrap_or_default()
}

/// Judge a set's six stages on this host's triple. Each says what is there,
/// what is not, and the one operation that closes a gap (the ndexr CLI's).
pub fn stages(s: &PfSet, triple: &str, stack_revision: &str) -> Vec<PfStage> {
    let st = |key: &str, state: &str, value: String, note: String, fix: String| PfStage { key: key.into(), state: state.into(), value, note, fix };
    let set = &s.set;
    let planned = s.records > 0;
    let kit = &s.manifest_sha256;
    let short = |h: &str| h.get(..8).unwrap_or("").to_owned();
    let mut out = Vec::new();
    out.push(if planned {
        st("planned", "ok", format!("{} records", s.records), format!("the kit's manifest {}", short(kit)), String::new())
    } else {
        st("planned", "none", "-".into(), "no manifest: a combined set the older modules load".into(), String::new())
    });
    let b = &s.built;
    out.push(if b.at.is_empty() {
        st("built", if planned { "gap" } else { "none" }, "-".into(), "no build record on this host".into(), if planned { format!("ndexr pkgs build {set}") } else { String::new() })
    } else if b.failed > 0 {
        st("built", "behind", format!("{} built", b.packages), format!("{} failed · {} · {}", b.failed, b.host, b.at), format!("ndexr pkgs build {set}"))
    } else if planned && !b.manifest_sha256.is_empty() && &b.manifest_sha256 != kit {
        st("built", "behind", format!("{}", b.packages), format!("built from manifest {}, the kit's is {}", short(&b.manifest_sha256), short(kit)), format!("ndexr pkgs build {set}"))
    } else {
        st("built", "ok", format!("{}", b.packages), format!("{} · {}", b.host, b.at), String::new())
    });
    let p = s.published.iter().find(|p| p.triple == triple).cloned().unwrap_or_default();
    out.push(if !p.found {
        st("published", "gap", "none".into(), "nothing in the public repo for this triple".into(), format!("ndexr pkgs publish {set}"))
    } else if planned && &p.manifest_sha256 != kit && &b.manifest_sha256 == kit && b.at > p.published_at {
        st("published", "behind", format!("{}", p.packages), format!("a newer build ({} · {}) is not in the public repo", b.host, b.at), format!("ndexr pkgs promote {set}"))
    } else if planned && &p.manifest_sha256 != kit {
        st("published", "behind", format!("{}", p.packages), format!("published from manifest {}, the kit's is {}", short(&p.manifest_sha256), short(kit)), format!("ndexr pkgs build {set} && ndexr pkgs publish {set}"))
    } else {
        st("published", "ok", format!("{}", p.packages), format!("{} · {}", p.host, p.published_at), String::new())
    });
    let y = &s.synced;
    out.push(if y.at.is_empty() {
        st("synced", "gap", "never".into(), "this host has not synchronized it".into(), format!("ndexr pkgs sync {set}"))
    } else if p.found && y.at < p.published_at {
        st("synced", "behind", format!("{}", y.packages), format!("synchronized from a publish older than the public one ({})", y.at), format!("ndexr pkgs sync {set}"))
    } else if p.found && y.at > p.published_at {
        st("synced", "behind", format!("{}", y.packages), format!("from {}'s staged build ({}), newer than the public repo", y.host, y.at), format!("ndexr pkgs promote {set}"))
    } else {
        st("synced", "ok", format!("{}", y.packages), format!("matches the public repo · {}", y.at), String::new())
    });
    let want = if y.packages > 0 { y.packages } else { p.packages };
    out.push(if s.local == 0 {
        st("installed", "gap", "0".into(), format!("nothing in {}", s.library), format!("ndexr pkgs sync {set}"))
    } else if want > 0 && s.local != want {
        let note = if s.local < want { format!("{} of the {want} synchronized are installed", s.local) } else { format!("{} installed, {} more than the {want} synchronized", s.local, s.local - want) };
        st("installed", "behind", format!("{}", s.local), note, format!("ndexr pkgs sync {set}"))
    } else {
        st("installed", "ok", format!("{}", s.local), s.library.clone(), String::new())
    });
    out.push(if s.module_on_stack {
        st("cvmfs", "ok", if stack_revision.is_empty() { "module".into() } else { format!("r{stack_revision}") }, format!("{} on stack.ndexr.io", s.module), String::new())
    } else {
        st("cvmfs", "gap", "no module".into(), format!("no {} on CVMFS: `ml` cannot load it", s.module), "ndexr stack cvmfs publish --from-s3".into())
    });
    out
}

/// A package's dependencies as its install records name them.
fn deps_of(set: &str, lib: &Path, name: &str) -> Vec<String> {
    let clean = |x: &str| x.split(|c: char| c == '(' || c == ' ' || c == '[' || c == ';' || c == '<' || c == '>' || c == '=' || c == '!' || c == '~').next().unwrap_or("").trim().to_owned();
    match language(set) {
        "Python" => {
            let dir = std::fs::read_dir(lib.join("python")).into_iter().flatten().flatten().find(|e| {
                let f = e.file_name().to_string_lossy().into_owned();
                f.ends_with(".dist-info") && norm(f.split('-').next().unwrap_or("")) == norm(name)
            });
            let meta = dir.and_then(|d| std::fs::read_to_string(d.path().join("METADATA")).ok()).unwrap_or_default();
            meta.lines().take_while(|l| !l.is_empty()).filter_map(|l| l.strip_prefix("Requires-Dist: ")).filter(|l| !l.contains("extra ==")).map(|l| norm(&clean(l))).collect()
        }
        "Julia" => {
            let text = std::fs::read_to_string(lib.join("julia/Manifest.toml")).unwrap_or_default();
            let mut inside = false;
            for l in text.lines().map(str::trim) {
                if l.starts_with("[[deps.") {
                    inside = l.trim_start_matches("[[deps.").trim_end_matches("]]").trim_matches('"') == name;
                } else if inside && l.starts_with("deps = [") {
                    return l.trim_start_matches("deps = [").trim_end_matches(']').split(',').map(|d| d.trim().trim_matches('"').to_owned()).filter(|d| !d.is_empty()).collect();
                }
            }
            Vec::new()
        }
        "Rust" => {
            let dir = std::fs::read_dir(lib.join("rust/vendor")).into_iter().flatten().flatten().find(|e| e.file_name().to_string_lossy().rsplit_once('-').is_some_and(|(n, _)| n == name));
            let toml = dir.and_then(|d| std::fs::read_to_string(d.path().join("Cargo.toml")).ok()).unwrap_or_default();
            let mut inside = false;
            let mut out = Vec::new();
            for l in toml.lines().map(str::trim) {
                if l.starts_with('[') {
                    inside = l == "[dependencies]";
                    if let Some(d) = l.strip_prefix("[dependencies.").and_then(|r| r.strip_suffix(']')) {
                        out.push(d.to_owned());
                    }
                } else if inside {
                    if let Some((k, _)) = l.split_once('=') {
                        out.push(k.trim().to_owned());
                    }
                }
            }
            out
        }
        _ => {
            let desc = std::fs::read_to_string(lib.join("R").join(name).join("DESCRIPTION")).unwrap_or_default();
            let f = dcf(&format!("{}
", desc.replace("
 ", " ").replace("
	", " "))).into_iter().next().unwrap_or_default();
            ["Depends", "Imports", "LinkingTo"].iter().filter_map(|k| f.get(*k)).flat_map(|v| v.split(',').map(|x| clean(x.trim()))).filter(|x| !x.is_empty() && x != "R").collect()
        }
    }
}

/// The modules under a set: its module file's `depends_on`, else its
/// easyconfig's dependencies and toolchain.
fn toolchain(set: &str, triple: &str) -> Vec<String> {
    let module = Path::new(STACK).join(triple).join("eb/modules/all/env").join(format!("{set}.lua"));
    if let Ok(text) = std::fs::read_to_string(&module) {
        return text.lines().filter_map(|l| l.trim().strip_prefix("depends_on(\"").and_then(|r| r.split('"').next()).map(str::to_owned)).collect();
    }
    let (lang, ver) = set.split_once('/').unwrap_or((set, ""));
    let ec = std::fs::read_to_string(kit().join(format!("easybuild-suite/easyconfigs/e/env-{lang}/env-{lang}-{ver}.eb"))).unwrap_or_default();
    let mut out = Vec::new();
    let mut inside = false;
    for l in ec.lines().map(str::trim) {
        if l.starts_with("toolchain") {
            out.push(l.split('=').nth(1).unwrap_or("").trim().replace(['{', '}', '\'', ' '], "").replace("name:", "").replace(",version:", "/"));
        }
        if l.starts_with("dependencies") {
            inside = true;
            continue;
        }
        if inside {
            if l.starts_with(']') {
                inside = false;
            } else if let Some(t) = l.strip_prefix("('") {
                let parts: Vec<&str> = t.split('\'').collect();
                if parts.len() > 2 {
                    out.push(format!("{}/{}", parts[0], parts[2]));
                }
            }
        }
    }
    out
}

/// The ndexr operations running on this host, read from /proc: which stage
/// each moves and for which set, and since when (the process's start).
pub fn live_here() -> Vec<PfLive> {
    let mut out = Vec::new();
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Ok(pid) = e.file_name().to_string_lossy().parse::<u32>() else { continue };
        let Ok(raw) = std::fs::read(e.path().join("cmdline")) else { continue };
        let args: Vec<String> = raw.split(|b| *b == 0).filter(|a| !a.is_empty()).map(|a| String::from_utf8_lossy(a).into_owned()).collect();
        let Some(i) = args.iter().position(|a| a == "ndexr" || a.ends_with("/ndexr")) else { continue };
        let rest: Vec<&str> = args[i + 1..].iter().map(String::as_str).collect();
        let (stage, what) = match rest.as_slice() {
            ["pkgs", "build", ..] => ("built", "ndexr pkgs build"),
            ["pkgs", "publish", ..] => ("published", "ndexr pkgs publish"),
            ["pkgs", "promote", ..] => ("published", "ndexr pkgs promote"),
            ["pkgs", "sync", ..] => ("synced", "ndexr pkgs sync"),
            ["stack", "cvmfs", "publish", ..] => ("cvmfs", "ndexr stack cvmfs publish"),
            ["stack", "build", ..] => ("cvmfs", "ndexr stack build"),
            _ => continue,
        };
        // The set: the first argument after the verb that names one.
        let set = rest.iter().skip(2).find(|a| a.contains('/') || (a.starts_with('r') && a.contains("_py"))).map_or(String::new(), |s| (*s).to_owned());
        out.push(PfLive { stage: stage.into(), set, what: what.into(), pid, since: mtime(&e.path()) });
    }
    // One line per operation: a CLI and the children it forks share the cmdline.
    out.sort_by_key(|l| l.since);
    out.dedup_by(|a, b| a.what == b.what && a.set == b.set);
    out
}

/// The build machines as builds.json has them now (the platform's database,
/// which the root server reads).
pub fn live_machines(builds: &[u8]) -> Vec<PfLiveMachine> {
    let v: serde_json::Value = serde_json::from_slice(builds).unwrap_or_default();
    v["eta"]["machines"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|m| {
            let r = &m["running"];
            let running = if r.is_null() {
                String::new()
            } else {
                r["name"].as_str().or_else(|| r["item"].as_str()).or_else(|| r.as_str()).map_or_else(|| r.to_string(), str::to_owned)
            };
            PfLiveMachine {
                name: m["name"].as_str().unwrap_or("").to_owned(),
                kind: m["kind"].as_str().unwrap_or("").to_owned(),
                arch: m["arch"].as_str().unwrap_or("").to_owned(),
                active: m["active"].as_bool().unwrap_or(false),
                running,
                done: m["counts"]["done"].as_u64().unwrap_or(0) as u32,
                failed: m["counts"]["failed"].as_u64().unwrap_or(0) as u32,
                total: m["counts"]["total"].as_u64().unwrap_or(0) as u32,
                remaining_s: m["remaining_s"].as_i64(),
            }
        })
        .collect()
}

/// The whole cycle, `want`'s packages on this host's triple, and `package` traced.
pub fn flow(want: &str, package: &str) -> PackageFlow {
    let triple = this_triple();
    let manifests = kit().join("manifest/manifests");
    let mut rows = Vec::new();
    let mut trace = None;
    let me = std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned();
    let names = sets(triple);
    // Every current.json at once: one after another, 32 of them took 41 s.
    std::thread::scope(|sc| {
        for set in &names {
            for t in TRIPLES {
                sc.spawn(move || published(t, set));
            }
        }
    });
    let sets = names
        .into_iter()
        .map(|set| {
            let mf = manifests.join(&set).join("manifest.dcf");
            let text = std::fs::read(&mf).unwrap_or_default();
            let records = dcf(&String::from_utf8_lossy(&text));
            let lib = Path::new(LIB_ROOT).join(triple).join(&set);
            let local = installed_cached(&set, &lib);
            let pubs: Vec<(String, Option<serde_json::Value>)> = TRIPLES.iter().map(|t| ((*t).to_owned(), published(t, &set))).collect();
            if set == want {
                let here = pubs.iter().find(|p| p.0 == triple).and_then(|p| p.1.as_ref()).map(published_rows).unwrap_or_default();
                let key = |n: &str| if language(&set) == "Python" { norm(n) } else { n.to_owned() };
                let asked: BTreeMap<String, (String, String)> = records
                    .iter()
                    .map(|r| (key(&r["Package"]), (r.get("Version").cloned().unwrap_or_default(), r.get("Source").cloned().unwrap_or_default())))
                    .collect();
                let names: BTreeSet<&String> = asked.keys().chain(here.keys()).chain(local.keys()).collect();
                let synced = record_at(&build_dir(triple, &set).join("sync/current.json"))
                    .and_then(|_| std::fs::read(build_dir(triple, &set).join("sync/current.json")).ok())
                    .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                    .map(|v| published_rows(&v))
                    .unwrap_or_default();
                rows = names
                    .into_iter()
                    .map(|n| {
                        let (requested, source) = asked.get(n).cloned().unwrap_or_default();
                        let (published, file, sha256, size) = here.get(n).cloned().unwrap_or_default();
                        PfRow {
                            name: n.clone(),
                            requested,
                            source,
                            published,
                            file,
                            sha256,
                            size,
                            local: local.get(n).cloned().unwrap_or_default(),
                            synced: synced.get(n).map(|r| r.0.clone()).unwrap_or_default(),
                        }
                    })
                    .collect();
                if !package.is_empty() {
                    let deps = deps_of(&set, &lib, package);
                    let needed_by: Vec<String> = local.keys().filter(|k| k.as_str() != package && deps_of(&set, &lib, k).iter().any(|d| d == package)).take(200).cloned().collect();
                    let step = std::fs::read(build_dir(triple, &set).join("pkgs.timeline.json"))
                        .ok()
                        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                        .and_then(|t| t["steps"].as_array().and_then(|a| a.iter().find(|x| x["name"] == package).map(|x| format!("{} · {}", x["status"].as_str().unwrap_or(""), iso(x["finished"].as_i64().unwrap_or(0))))))
                        .unwrap_or_default();
                    trace = Some(PfTrace {
                        name: package.to_owned(),
                        deps: deps.into_iter().map(|d| (d.clone(), local.get(&d).cloned().unwrap_or_default())).collect(),
                        needed_by,
                        toolchain: toolchain(&set, triple),
                        built: step,
                    });
                }
            }
            PfSet {
                language: language(&set).to_owned(),
                module: format!("env/{set}"),
                records: records.len() as u32,
                manifest_sha256: if text.is_empty() { String::new() } else { crate::names::sha256(&text).iter().map(|b| format!("{b:02x}")).collect() },
                published: pubs
                    .iter()
                    .map(|(t, v)| match v {
                        Some(v) => PfPublished {
                            triple: t.clone(),
                            found: true,
                            packages: published_rows(v).len() as u32,
                            published_at: v["published_at"].as_str().unwrap_or("").chars().take(19).collect(),
                            host: v["host"].as_str().unwrap_or("").to_owned(),
                            cli_version: v["cli_version"].as_str().unwrap_or("").to_owned(),
                            manifest_sha256: v["manifest_sha256"].as_str().unwrap_or("").to_owned(),
                        },
                        None => PfPublished { triple: t.clone(), ..Default::default() },
                    })
                    .collect(),
                library: lib.display().to_string(),
                local: local.len() as u32,
                local_at: ["R", "python", "julia", "rust"].iter().map(|d| mtime(&lib.join(d))).max().unwrap_or(0),
                module_on_stack: Path::new(STACK).join(triple).join("eb/modules/all/env").join(format!("{set}.lua")).is_file(),
                load: load(&set, triple),
                built: built(triple, &set, &me),
                synced: record_at(&build_dir(triple, &set).join("sync/current.json")).unwrap_or_default(),
                stages: Vec::new(),
                set,
            }
        })
        .collect();
    let stack_revision = revision(Path::new(STACK));
    let mut sets: Vec<PfSet> = sets;
    for s in sets.iter_mut() {
        s.stages = stages(s, triple, &stack_revision);
    }
    let kit_line = |k: &str| {
        std::fs::read_to_string(Path::new(STACK).join(triple).join(".ndexr-stack-kit"))
            .unwrap_or_default()
            .lines()
            .find_map(|l| l.strip_prefix(&format!("{k}=")).map(str::to_owned))
            .unwrap_or_default()
    };
    PackageFlow {
        triple: triple.to_owned(),
        repo: repo(),
        host: std::fs::read_to_string("/proc/sys/kernel/hostname").unwrap_or_default().trim().to_owned(),
        stack_revision,
        stack_synced: kit_line("synced").chars().take(19).collect(),
        sets,
        machines: machines(),
        sessions: Vec::new(),
        rows,
        trace,
        live: live_here(),
        live_machines: Vec::new(),
        at: now(),
    }
}

/// A CVMFS mount's revision (its `user.revision` attribute); "" elsewhere.
fn revision(p: &Path) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(p.as_os_str().as_bytes()).unwrap_or_default();
        let name = std::ffi::CString::new("user.revision").unwrap_or_default();
        let mut buf = [0u8; 64];
        let n = unsafe { libc::getxattr(path.as_ptr(), name.as_ptr(), buf.as_mut_ptr().cast(), buf.len()) };
        if n > 0 {
            return String::from_utf8_lossy(&buf[..n as usize]).trim().to_owned();
        }
    }
    let _ = p;
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_and_names() {
        assert_eq!(language("R/4.6.1-bioc3.23"), "R");
        assert_eq!(language("Julia/1.11.7"), "Julia");
        assert_eq!(language("r4.6_py3.13-release"), "R + Python");
        assert_eq!(language("ndexr-app-r4.6.1"), "R");
        assert_eq!(norm("Typing_Extensions"), "typing-extensions");
    }

    #[test]
    fn rust_and_julia_publish_one_archive() {
        let cur = serde_json::json!({
            "Rust": {"tokio": "1.53.3"}, "rust_archive": {"file": "rust/set.tar.gz", "sha256": "ab", "size": 9},
            "Python": {"pillow": {"name": "Pillow", "version": "12.0", "file": "p.whl", "sha256": "cd", "size": 3}},
        });
        let m = published_rows(&cur);
        assert_eq!(m["tokio"], ("1.53.3".into(), "rust/set.tar.gz".into(), "ab".into(), 0));
        assert_eq!(m["pillow"].0, "12.0");
    }

    /// Measured against this host, where the stack and the kit are.
    #[test]
    fn the_live_cycle() {
        if !Path::new(LIB_ROOT).is_dir() || !kit().is_dir() {
            eprintln!("skipped: not on the platform host");
            return;
        }
        let f = flow("R/4.6.1-bioc3.23", "dplyr");
        let t = f.trace.as_ref().expect("dplyr traced");
        assert!(t.deps.iter().any(|(d, v)| d == "rlang" && !v.is_empty()), "{t:?}");
        assert!(!t.needed_by.is_empty() && !t.toolchain.is_empty(), "{t:?}");
        let r = f.sets.iter().find(|s| s.set == "R/4.6.1-bioc3.23").expect("the R set");
        assert!(r.records > 100 && r.local > 100, "{r:?}");
        assert!(r.load.contains("R_LIBS_SITE"), "{}", r.load);
        assert!(f.rows.iter().any(|x| !x.requested.is_empty() && !x.local.is_empty()));
        assert!(!f.host.is_empty() && !f.stack_synced.is_empty(), "host {:?} synced {:?}", f.host, f.stack_synced);
        eprintln!("host {} · revision {:?} · synced {}", f.host, f.stack_revision, f.stack_synced);
        for s in &f.sets {
            assert_eq!(s.stages.len(), 6, "{}", s.set);
            let line: Vec<String> = s.stages.iter().map(|g| format!("{}={}", g.key, g.state)).collect();
            eprintln!("{:<22} {}", s.set, line.join(" "));
            for g in s.stages.iter().filter(|g| g.state == "gap" || g.state == "behind") {
                eprintln!("    {} {}: {} -> {}", g.key, g.state, g.note, g.fix);
            }
        }
        eprintln!("live here {:?}; trace {:?}", f.live, f.trace);
    }
}
