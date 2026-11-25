//! The env manifests - what each environment of the stack holds - and
//! installing them on the person's own machine (Software › Packages ›
//! Manifests, the Packages manifest filter, and Software › Install).
//!
//! Operator, 2026-10-15: "the different manifests really do become an
//! important filter and viewer in the software sections ... we need to create
//! manifests of combinations of language packages for different purposes on
//! the stack we have built below, so we are in easybuild territory as far as
//! file structure is concerned with these environments". So a manifest's name
//! may carry slashes the way an EasyBuild module does: `manifests/<Name>/
//! <version>/manifest.dcf` is the module `env/<Name>/<version>`
//! (env/R/4.6.1-bioc3.23, env/Python/3.13.15, ndexr-app/4.6.1), beside svbt's
//! R+Python combinations (r4.6_py3.13-release). Each may carry a `set.json`
//! naming the modules it builds on (`{"modules": [...], "bioc": "..."}`).
//!
//! And: "the desktop version of cove will use the ndexr commands to install
//! the stack locally on their system ... users should be able to select all or
//! individually which environments they would like in their stack ... the
//! users install sends build stats back to ndexr ... just like n-boxes do, but
//! they wouldnt set up s3 or push or anything like that yet". The install is
//! the ndexr CLI's own commands, composed here (`install_script`), and never
//! one that publishes, promotes or pushes (`FORBIDDEN`, checked before a job
//! starts and in the tests).
//!
//! Where the manifests come from - the ndexr CLI, never files the UI reaches
//! for itself. `ndexr pkgs sets --json` names the sets this ndexr's kit
//! carries and lays the kit's manifest half while it does; run with
//! NDEXR_PKGS_ROOT at Cove's own cache, that lands where any machine can write
//! (a fresh desktop cannot write /opt/ndexr/site-library), and the records are
//! read from there. A named set the kit does not carry (ndexr-app-r4.6.1) is
//! the repo's: https://repo.ndexr.io/env/manifests/<set>/{manifest.dcf,
//! set.json}, the same place `ndexr pkgs build` fetches it from. Build state
//! is `ndexr pkgs status --json`. The CLI has no command that prints one
//! manifest's records; that ask went to ndexr.ndexr.io (CONTEXT.md).
//!
//! This half is shared: the types travel between the UI and the machine
//! (api.rs `Query::Manifests` …), and the parsers run in a browser too. The
//! machine's half is `host`, native only.

// The browser only reads and draws; the script and its guard are the machine's.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

// ── what a manifest says ───────────────────────────────────────────────────

/// One manifest record, svbt's DCF fields (manifest/run.R reads the same).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Record {
    pub package: String,
    /// "R", "Python" or "Rust".
    pub language: String,
    pub source: String,
    pub version: String,
    pub group: String,
    pub description: String,
    pub url: String,
    #[serde(rename = "ref")]
    pub reference: String,
    pub note: String,
    pub report: String,
    pub skip: String,
    pub arch: String,
    pub import: String,
    /// `SiteLibrary: yes`: synced back into a box's site library with what it
    /// needs; otherwise it lives in S3 only (repo.ndexr.io/env) - the default.
    pub site_library: bool,
}

/// The languages a set can hold: R and Python, since 2026-10-18 Rust
/// (operator: "Rust is going to be a third supported ... language. Just like
/// R and Python"), whose sets are crates from crates.io, and Julia's General
/// registry packages (the CLI builds `Language: Julia` sets since 2026-10-21).
/// Julia used to read as R here, so Julia/1.11.7 showed "37 R records".
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Lang {
    R,
    Python,
    Rust,
    Julia,
}

impl Lang {
    pub const ALL: [Lang; 4] = [Lang::R, Lang::Python, Lang::Rust, Lang::Julia];

    /// A manifest's `Language:` field (anything unknown reads as R, the
    /// records' original language).
    pub fn of(language: &str) -> Lang {
        Lang::ALL.into_iter().find(|l| language.eq_ignore_ascii_case(l.word())).unwrap_or(Lang::R)
    }

    pub fn word(self) -> &'static str {
        match self {
            Lang::R => "R",
            Lang::Python => "Python",
            Lang::Rust => "Rust",
            Lang::Julia => "Julia",
        }
    }
}

/// Records per language, in `Lang::ALL` order.
pub type Counts = [usize; 4];

impl Record {
    pub fn lang(&self) -> Lang {
        Lang::of(&self.language)
    }

    /// The name a catalog row matches on: R exactly, Python as pip
    /// normalizes it (PEP 503: case, and - _ . alike), a crate as Cargo does
    /// (case, and - _ alike).
    pub fn key(&self) -> String {
        match self.lang() {
            Lang::Python => py_norm(&self.package),
            Lang::Rust => crate_norm(&self.package),
            Lang::R | Lang::Julia => self.package.to_lowercase(),
        }
    }
}

/// Cargo's view of a crate name: `Serde_JSON` and `serde-json` are one crate.
pub fn crate_norm(name: &str) -> String {
    name.trim().to_lowercase().replace('_', "-")
}

/// PEP 503's normal form: `Foo_Bar.baz` and `foo-bar-baz` are one project.
pub fn py_norm(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut dash = false;
    for c in name.chars() {
        if matches!(c, '-' | '_' | '.') {
            if !dash {
                out.push('-');
            }
            dash = true;
        } else {
            out.extend(c.to_lowercase());
            dash = false;
        }
    }
    out
}

/// A DCF file as its records, each a field map. Records are separated by
/// blank lines; a line that starts with white space continues the field
/// before it (R's read.dcf); `#` lines are comments. Keys keep their case.
pub fn parse_dcf(text: &str) -> Vec<BTreeMap<String, String>> {
    let mut out = Vec::new();
    let mut rec: BTreeMap<String, String> = BTreeMap::new();
    let mut last: Option<String> = None;
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r');
        if line.trim().is_empty() {
            if !rec.is_empty() {
                out.push(std::mem::take(&mut rec));
            }
            last = None;
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some(v) = last.as_ref().and_then(|k| rec.get_mut(k)) {
                let more = line.trim();
                // A lone "." is DCF's blank line inside a field.
                if more == "." {
                    v.push('\n');
                } else {
                    if !v.is_empty() && !v.ends_with('\n') {
                        v.push(' ');
                    }
                    v.push_str(more);
                }
            }
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim().to_owned();
            rec.insert(k.clone(), v.trim().to_owned());
            last = Some(k);
        }
    }
    if !rec.is_empty() {
        out.push(rec);
    }
    out
}

/// A manifest.dcf as its records; one without a Package is no record.
pub fn records(text: &str) -> Vec<Record> {
    parse_dcf(text)
        .into_iter()
        .filter_map(|mut m| {
            let mut take = |k: &str| m.remove(k).unwrap_or_default();
            let package = take("Package");
            if package.is_empty() {
                return None;
            }
            Some(Record {
                package,
                language: take("Language"),
                source: take("Source"),
                version: take("Version"),
                group: take("Group"),
                description: take("Description"),
                url: take("Url"),
                reference: take("Ref"),
                note: take("Note"),
                report: take("Report"),
                skip: take("Skip"),
                arch: take("Arch"),
                import: take("Import"),
                site_library: matches!(take("SiteLibrary").trim().to_ascii_lowercase().as_str(), "yes" | "true" | "1"),
            })
        })
        .collect()
}

/// Records back as a manifest.dcf, in the fields and order `records` reads
/// them (Url and Ref as the kit writes them; SiteLibrary only when yes): a
/// draft of a manifest is written with this, so what is proposed is what
/// the CLI will read.
pub fn to_dcf(recs: &[Record]) -> String {
    let mut out = String::new();
    for r in recs {
        let fields = [
            ("Package", r.package.as_str()),
            ("Language", r.language.as_str()),
            ("Source", r.source.as_str()),
            ("Version", r.version.as_str()),
            ("Group", r.group.as_str()),
            ("Description", r.description.as_str()),
            ("Url", r.url.as_str()),
            ("Ref", r.reference.as_str()),
            ("Note", r.note.as_str()),
            ("Report", r.report.as_str()),
            ("Skip", r.skip.as_str()),
            ("Arch", r.arch.as_str()),
            ("Import", r.import.as_str()),
        ];
        for (k, v) in fields.iter().filter(|(_, v)| !v.trim().is_empty()) {
            // DCF joins continuation lines with a space; a line break is a
            // lone "." (parse_dcf), so each one is written as one.
            let mut lines = v.split('\n');
            out.push_str(&format!("{k}: {}\n", lines.next().unwrap_or("").trim()));
            for l in lines {
                out.push_str("  .\n");
                if !l.trim().is_empty() {
                    out.push_str(&format!("  {}\n", l.trim()));
                }
            }
        }
        if r.site_library {
            out.push_str("SiteLibrary: yes\n");
        }
        out.push('\n');
    }
    out
}

/// Records per language.
pub fn counts(recs: &[Record]) -> Counts {
    Lang::ALL.map(|l| recs.iter().filter(|r| r.lang() == l).count())
}

/// A set's records in words, in its own languages only: "253 R",
/// "108 Python", "40 Rust", "37 Julia", "590 R · 108 Python" for svbt's
/// combined sets - never "R 0 · Py 108" (Software › Manifests, 2026-10-18).
pub fn count_words(n: Counts) -> String {
    let parts: Vec<String> = Lang::ALL.iter().zip(n).filter(|(_, n)| *n > 0).map(|(l, n)| format!("{n} {}", l.word())).collect();
    if parts.is_empty() {
        "no records".into()
    } else {
        parts.join(" · ")
    }
}

/// A set's set.json: the modules it builds on, its Bioconductor, and - the
/// repo's published sets - the one module it builds in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SetMeta {
    pub modules: Vec<String>,
    pub module: String,
    pub bioc: String,
}

impl SetMeta {
    /// The module the set is used through: what set.json names, else
    /// `env/<name>` (svbt's sets, and the EasyBuild-style names alike).
    pub fn env_module(&self, name: &str) -> String {
        if self.module.trim().is_empty() {
            format!("env/{name}")
        } else {
            self.module.trim().to_owned()
        }
    }
}

/// A module's easyconfig file name, as build-suite.sh's `--only` matches it:
/// `R/4.6.1-foss-2024a` → `R-4.6.1-foss-2024a.eb`, `env/r4.6_py3.13-release`
/// → `env-r4.6_py3.13-release.eb`. A deeper EasyBuild-style name joins every
/// segment the same way (`env/R/4.6.1-bioc3.23` → `env-R-4.6.1-bioc3.23.eb`).
pub fn easyconfig(module: &str) -> String {
    format!("{}.eb", module.trim().trim_matches('/').replace('/', "-"))
}

/// The easyconfigs a set needs: the modules set.json says it builds on, then
/// its own env module - whole file names, so the substring match selects
/// exactly these (and `--robot` brings their toolchains).
pub fn set_easyconfigs(name: &str, meta: &SetMeta) -> Vec<String> {
    let mut v: Vec<String> = meta.modules.iter().filter(|m| !m.trim().is_empty()).map(|m| easyconfig(m)).collect();
    v.push(easyconfig(&meta.env_module(name)));
    let mut seen = std::collections::HashSet::new();
    v.retain(|e| seen.insert(e.clone()));
    v
}

/// A manifest name the machine will act on: EasyBuild-style segments
/// (letters, digits, `._+-`), up to four deep, none of them `.` or `..`.
pub fn valid_name(name: &str) -> bool {
    let segs: Vec<&str> = name.split('/').collect();
    !name.is_empty()
        && name.len() <= 120
        && segs.len() <= 4
        && segs.iter().all(|s| {
            !s.is_empty() && *s != "." && *s != ".." && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-".contains(c))
        })
}

/// What kind of environment a name is: an R+Python combination (svbt's), a
/// language on its own (env/R/…, env/Python/…), or a purpose set.
pub fn family(name: &str) -> &'static str {
    let first = name.split('/').next().unwrap_or(name);
    if first.starts_with('r') && first.contains("_py") {
        "R + Python"
    } else if first.eq_ignore_ascii_case("R") {
        "R"
    } else if first.eq_ignore_ascii_case("Python") {
        "Python"
    } else if first.eq_ignore_ascii_case("Rust") {
        "Rust"
    } else if first.eq_ignore_ascii_case("Julia") {
        "Julia"
    } else {
        "purpose"
    }
}

// ── what travels ───────────────────────────────────────────────────────────

/// A set as built on this machine (`ndexr pkgs status --json`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Built {
    pub r: usize,
    pub python: usize,
    pub rust: usize,
    pub julia: usize,
    /// Some `ndexr pkgs` run holds the set's lock - a build or a sync.
    pub busy: bool,
    /// Build steps running: busy with none is a sync or a stale hold, not a build.
    pub running: usize,
    pub done: usize,
    pub failed: usize,
    pub published_at: String,
}

impl Built {
    pub fn counts(&self) -> Counts {
        [self.r, self.python, self.rust, self.julia]
    }
}

/// One manifest in the list.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Summary {
    pub name: String,
    pub env_module: String,
    pub modules: Vec<String>,
    pub bioc: String,
    pub r: usize,
    pub python: usize,
    pub rust: usize,
    pub julia: usize,
    /// In the CLI's pkgs.sets: the combinations the operator kept
    /// (2026-10-15) and the named sets.
    pub kept: bool,
    /// "kit" (this ndexr's) or "repo" (repo.ndexr.io/env/manifests).
    pub from: String,
    /// Its shared library on this machine, when there is one.
    pub built: Option<Built>,
}

impl Summary {
    pub fn counts(&self) -> Counts {
        [self.r, self.python, self.rust, self.julia]
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Listing {
    pub sets: Vec<Summary>,
    pub triple: String,
    /// The ndexr version that answered.
    pub cli: String,
    /// What could not be read, said once (no CLI, a set the repo lacks).
    pub note: String,
}

/// One manifest with its records.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Detail {
    pub name: String,
    pub env_module: String,
    pub modules: Vec<String>,
    pub bioc: String,
    pub from: String,
    pub records: Vec<Record>,
}

/// Two manifests side by side: what `b` adds, drops and moves, by language
/// and package.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Diff {
    pub added: Vec<Record>,
    pub removed: Vec<Record>,
    /// (in a, in b) - the same package at another version.
    pub changed: Vec<(Record, Record)>,
    pub same: usize,
}

pub fn diff(a: &[Record], b: &[Record]) -> Diff {
    let key = |r: &Record| (r.lang(), r.key());
    let am: BTreeMap<_, &Record> = a.iter().map(|r| (key(r), r)).collect();
    let bm: BTreeMap<_, &Record> = b.iter().map(|r| (key(r), r)).collect();
    let mut d = Diff::default();
    for (k, rb) in &bm {
        match am.get(k) {
            None => d.added.push((*rb).clone()),
            Some(ra) if ra.version != rb.version => d.changed.push(((*ra).clone(), (*rb).clone())),
            Some(_) => d.same += 1,
        }
    }
    for (k, ra) in &am {
        if !bm.contains_key(k) {
            d.removed.push((*ra).clone());
        }
    }
    d
}

/// Can this machine install, and how it would go.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InstallInfo {
    /// Why not here, in words; empty when it can.
    pub unsupported: String,
    /// The stack folder it builds (`<stack.root>/<triple>`) and the shared
    /// libraries' root (`pkgs.root`).
    pub dir: String,
    pub pkgs_root: String,
    pub cli: String,
    /// How the one root step (making those folders yours) would get root:
    /// "root", "sudo", "pkexec", or "" (then it is shown to run by hand).
    pub root_by: String,
    /// The folders that need that step; empty when they are yours already.
    pub needs_root: Vec<String>,
    /// An API key is here, so the builds report to the person's account.
    pub key: bool,
}

/// What the person picked.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InstallRequest {
    pub sets: Vec<String>,
    /// Fetch the prebuilt binaries first (`ndexr pkgs sync`, HTTPS, read
    /// only), then build what is still missing.
    pub prebuilt: bool,
    /// Send the build steps to the person's ndexr account.
    pub report: bool,
}

/// The person's machines as their account has them (api.ndexr.io/v1/builds).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MyBuilds {
    pub owner: String,
    /// This machine's id as the CLI reports it (~/.config/ndexr/telemetry-id).
    pub this_machine: String,
    pub machines: Vec<Machine>,
    pub error: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Machine {
    pub id: String,
    pub run: String,
    pub host: String,
    pub system: String,
    pub steps: i64,
    pub done: i64,
    pub failed: i64,
    pub running: i64,
    pub started: f64,
    pub until: f64,
    pub seen: f64,
}

// ── the install, as ndexr commands ─────────────────────────────────────────

/// Words no install may carry: publishing, promoting, assembling, a manifest
/// push, a stage, S3. The operator's line (2026-10-15): "they wouldnt set up
/// s3 or push or anything like that yet - that comes later as an option".
pub const FORBIDDEN: [&str; 10] = [" publish", "--publish", "promote", "assemble", "pkgs manifest", "--then-publish", "--to-stage", "--from-stage", "s3://", "cvmfs publish"];

/// Why `script` must not run, if it would publish anything.
pub fn refuse(script: &str) -> Option<&'static str> {
    FORBIDDEN.into_iter().find(|w| script.contains(w))
}

/// Everything `install_script` needs, decided by the machine (host::install).
#[derive(Clone, Debug, Default)]
pub struct Plan {
    /// The ndexr CLI, as an absolute path.
    pub cli: String,
    /// `<stack.root>` and the folder built in, `<stack.root>/<triple>`.
    pub root: String,
    pub dir: String,
    /// Each set and the easyconfigs it needs (set_easyconfigs).
    pub sets: Vec<(String, Vec<String>)>,
    pub prebuilt: bool,
    pub report: bool,
    /// The root step that makes the folders the person's, already wrapped
    /// for how this machine gets root; None when they are theirs.
    pub make_dirs: Option<String>,
}

fn q(s: &str) -> String {
    crate::api::shell_quote(s)
}

/// The install as one bash script of ndexr commands, the steps `stack up`
/// would run for a machine of one's own, but for the picked environments
/// only: lay the kit, build the Prefix + EasyBuild layer if missing, build the
/// sets' modules, then their packages. Each step announces itself the way
/// `stack up` does (`[cove install] step i/n …`, read by `progress`).
/// Reporting is the CLI's own: `ndexr stack report` while it builds (`stack
/// up` reports on its minute tick; the steps used here do not), and `pkgs
/// build` reports its packages unless `--no-report`.
pub fn install_script(p: &Plan) -> String {
    let n = 4 + p.sets.len() + usize::from(p.report);
    let only: Vec<String> = {
        let mut v: Vec<String> = Vec::new();
        for e in p.sets.iter().flat_map(|(_, e)| e) {
            if !v.contains(e) {
                v.push(e.clone());
            }
        }
        v
    };
    let mut s = String::new();
    s.push_str("set -u\n");
    s.push_str(&format!("cli={}\ndir={}\nexport NDEXR_STACK_ROOT={}\n", q(&p.cli), q(&p.dir), q(&p.root)));
    s.push_str(&format!("n={n}\nrc=0\n"));
    s.push_str("step() { echo; echo \"[cove install] step $1/$n $2 — $(date -u +%H:%M:%SZ)\"; }\n");
    s.push_str("note() { echo \"  · $*\"; }\n");
    s.push_str(&format!(
        "echo '[cove install] {} environment(s): {}'\n",
        p.sets.len(),
        p.sets.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ").replace('\'', "")
    ));
    if !p.report {
        s.push_str("note 'not reporting: no build steps go to your ndexr account'\n");
    }
    // 1. The folders: the stack is built at the path every binary is built
    // for, so it is /opt, and the one root step is making it the person's.
    s.push_str("step 1 folders\n");
    match &p.make_dirs {
        Some(cmd) => s.push_str(&format!("{cmd} || {{ echo '[FAIL] could not make the stack folders yours'; exit 1; }}\n")),
        None => s.push_str("note \"$dir is yours already\"\n"),
    }
    // 2. The kit: the scripts and easyconfigs this ndexr carries.
    s.push_str("step 2 kit\n\"$cli\" stack init --dir \"$dir\" || { echo '[FAIL] kit'; exit 1; }\n");
    if p.report {
        // A Prefix takes hours: send the timeline every five minutes so the
        // account sees it move, as an n-box's does.
        s.push_str("( while sleep 300; do \"$cli\" stack report --dir \"$dir\" >/dev/null 2>&1; done ) &\ntick=$!\ntrap 'kill $tick 2>/dev/null' EXIT\n");
    }
    // 3. Gentoo Prefix + EasyBuild + Lmod, unless the CLI's own done-marker
    // (stack/mod.rs prefix_done) says it is built.
    s.push_str("step 3 prefix\n");
    s.push_str("if grep -q '__EB_DONE__ rc=0' \"$dir/.build/easybuild.log\" 2>/dev/null && [ -e \"$dir/.state/venv/bin/eb\" ]; then note 'Prefix + EasyBuild already built'\n");
    s.push_str("else \"$cli\" stack prefix --dir \"$dir\" --foreground || { echo '[FAIL] prefix'; exit 1; }; fi\n");
    // 4. The modules the sets stand on; failing forward from here, as `stack
    // up` does - one set's failure still installs the others.
    s.push_str("step 4 modules\n");
    s.push_str(&format!("\"$cli\" stack build --dir \"$dir\" --only {} || {{ rc=1; echo '[FAIL] modules'; }}\n", q(&only.join(","))));
    for (i, (set, _)) in p.sets.iter().enumerate() {
        s.push_str(&format!("step {} {}\n", 5 + i, q(&format!("packages {set}"))));
        if p.prebuilt {
            s.push_str(&format!("\"$cli\" pkgs sync {} || note 'no prebuilt binaries for this one here - building them'\n", q(set)));
        }
        s.push_str(&format!(
            "\"$cli\" pkgs build {}{} || {{ rc=1; echo {}; }}\n",
            q(set),
            if p.report { "" } else { " --no-report" },
            q(&format!("[FAIL] packages {set}"))
        ));
    }
    if p.report {
        s.push_str(&format!("step {} report\n\"$cli\" stack report --dir \"$dir\" || note 'the report did not go - the next install sends it'\n", n));
    }
    s.push_str("echo\nif [ $rc -eq 0 ]; then echo '[cove install] done'; else echo \"[cove install] finished with failures (rc=$rc)\"; fi\nexit $rc\n");
    s
}

// ── reading the log ────────────────────────────────────────────────────────

/// How far an install has got, read from its log.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// (i, n, what): `[cove install] step 3/7 prefix — …`, or `stack up`'s
    /// `[ndexr up] step 3/5 suite — …`.
    pub step: Option<(usize, usize, String)>,
    /// (package, language, i, n): svbt's `* now installing x (R) - 12 of 559`.
    pub package: Option<(String, String, usize, usize)>,
    /// The easyconfig build-suite.sh is on (`* [build] R-4.6.1-foss-2024a.eb`).
    pub easyconfig: Option<String>,
    pub built: usize,
    pub skipped: usize,
    /// `[ok]` and `[FAIL]` lines.
    pub ok: usize,
    pub failed: usize,
}

/// `i/n` or `i of n` → (i, n).
fn frac(s: &str, sep: &str) -> Option<(usize, usize)> {
    let (a, b) = s.split_once(sep)?;
    let a = a.trim().rsplit(' ').next()?.parse().ok()?;
    let b = b.trim().split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()?;
    Some((a, b))
}

pub fn progress<S: AsRef<str>>(lines: &[S]) -> Progress {
    let mut p = Progress::default();
    for l in lines {
        let l = l.as_ref().trim();
        if (l.starts_with("[cove install] step ") || l.starts_with("[ndexr up] step ")) && l.contains('/') {
            let rest = l.split_once("] step ").map_or("", |x| x.1);
            let (num, what) = rest.split_once(' ').unwrap_or((rest, ""));
            if let Some((i, n)) = frac(num, "/") {
                let what = what.split(" — ").next().unwrap_or(what).trim().to_owned();
                p.step = Some((i, n, what));
                // A new step's package count starts again.
                p.package = None;
            }
        } else if let Some(rest) = l.strip_prefix("* now installing ") {
            // "ggplot2 (R) - 12 of 559"
            let (head, count) = rest.rsplit_once(" - ").unwrap_or((rest, ""));
            let (pkg, lang) = match head.rsplit_once(" (") {
                Some((p, l)) => (p.trim(), l.trim_end_matches(')').trim()),
                None => (head.trim(), ""),
            };
            if let Some((i, n)) = frac(count, " of ") {
                p.package = Some((pkg.to_owned(), lang.to_owned(), i, n));
            }
        } else if let Some(ec) = l.strip_prefix("* [build] ").or_else(|| l.strip_prefix("* [rebuild] ").map(|r| r.split(" - ").next().unwrap_or(r))) {
            p.easyconfig = Some(ec.trim().to_owned());
            p.built += 1;
        } else if l.starts_with("* [skip]") {
            p.skipped += 1;
        } else if l.starts_with("[ok]") {
            p.ok += 1;
        } else if l.starts_with("[FAIL]") {
            p.failed += 1;
        }
    }
    p
}

// ── the machine's half ─────────────────────────────────────────────────────

#[cfg(not(target_arch = "wasm32"))]
pub mod host {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use crate::api::JobView;
    use crate::host::runner::capture_within;

    /// The ndexr CLI's pkgs.sets default (ndexr.ndexr.io crate/src/config/
    /// site.rs): the language sets, one language each, since 0.0.85 - the
    /// R+Python combinations are compositions of two of these and have no
    /// manifest of their own, so naming them here only made a "not found"
    /// warning. Rust joined 2026-10-18. The CLI prints no machine-readable
    /// form of its settings, so this is read the way the CLI reads it:
    /// NDEXR_PKGS_SETS, else this.
    const KEPT: &str = "R/4.6.1-bioc3.24,R/4.6.1-bioc3.23,R/4.5.3-bioc3.22,R/4.4.3-bioc3.20,Python/3.13.15,Python/3.12.14,Python/3.10.21,Rust/1.96.0";
    const REPO: &str = "https://repo.ndexr.io/env";

    fn kept() -> Vec<String> {
        let v = std::env::var("NDEXR_PKGS_SETS").ok().filter(|s| !s.trim().is_empty()).unwrap_or_else(|| KEPT.into());
        v.split(',').map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect()
    }

    fn pkgs_root() -> PathBuf {
        std::env::var_os("NDEXR_PKGS_ROOT").filter(|v| !v.is_empty()).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/opt/ndexr/site-library"))
    }

    /// Cove's own cache: where the CLI lays the kit's manifests for reading.
    fn cache() -> PathBuf {
        let base = std::env::var_os("XDG_CACHE_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
            .unwrap_or_else(std::env::temp_dir);
        base.join("cove").join("manifests")
    }

    fn cli() -> Option<String> {
        capture_within("command -v ndexr", Duration::from_secs(5))
    }

    /// Every folder under `dir` that holds a manifest.dcf, as its path below
    /// `dir` - one segment for svbt's sets, more for the EasyBuild-style ones.
    fn walk(dir: &Path, rel: &str, depth: usize, out: &mut Vec<String>) {
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut kids: Vec<_> = rd.flatten().filter(|e| e.path().is_dir()).collect();
        kids.sort_by_key(|e| e.file_name());
        for e in kids {
            let name = e.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() { name } else { format!("{rel}/{name}") };
            if e.path().join("manifest.dcf").is_file() {
                out.push(r.clone());
            }
            if depth < 3 {
                walk(&e.path(), &r, depth + 1, out);
            }
        }
    }

    struct Laid {
        at: Instant,
        names: Vec<String>,
        kit: PathBuf,
        version: String,
        error: String,
    }

    /// The kit's sets, newest first, and where their manifests were laid:
    /// `ndexr pkgs sets --json` with NDEXR_PKGS_ROOT at the cache. Kept a
    /// minute - the list is read on every visit to the view.
    fn laid() -> &'static Mutex<Option<Laid>> {
        static L: OnceLock<Mutex<Option<Laid>>> = OnceLock::new();
        L.get_or_init(Default::default)
    }

    fn lay() -> (Vec<String>, PathBuf, String, String) {
        let mut slot = laid().lock().unwrap();
        if let Some(l) = slot.as_ref().filter(|l| l.at.elapsed() < Duration::from_secs(60)) {
            return (l.names.clone(), l.kit.clone(), l.version.clone(), l.error.clone());
        }
        let root = cache().join("kit");
        let kit = root.join(".build").join("kit-manifest").join("manifests");
        let (mut names, mut version, mut error) = (Vec::new(), String::new(), String::new());
        match cli() {
            None => error = "the ndexr CLI is not on this machine, so its manifests cannot be read - cvmfs.ndexr.io has the install".into(),
            Some(cli) => {
                let c = crate::api::shell_quote(&cli);
                version = capture_within(&format!("{c} --version"), Duration::from_secs(10)).unwrap_or_default().trim_start_matches("ndexr ").to_owned();
                let cmd = format!("NDEXR_PKGS_ROOT={} {c} pkgs sets --json", crate::api::shell_quote(&root.display().to_string()));
                match capture_within(&cmd, Duration::from_secs(60)).and_then(|o| serde_json::from_str::<Vec<String>>(o.lines().last().unwrap_or("")).ok()) {
                    Some(v) => names = v,
                    None => error = format!("`ndexr pkgs sets --json` did not answer (ndexr {version})"),
                }
            }
        }
        // The nested, EasyBuild-style names, whether or not this CLI lists
        // them yet.
        let mut nested = Vec::new();
        walk(&kit, "", 0, &mut nested);
        for n in nested {
            if !names.contains(&n) {
                names.push(n);
            }
        }
        *slot = Some(Laid { at: Instant::now(), names: names.clone(), kit: kit.clone(), version: version.clone(), error: error.clone() });
        (names, kit, version, error)
    }

    /// A file from the repo, kept in the cache an hour.
    fn from_repo(name: &str, file: &str) -> Result<String, String> {
        let p = cache().join("repo").join(name).join(file);
        let fresh = std::fs::metadata(&p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|e| e < Duration::from_secs(3600));
        if fresh {
            if let Ok(t) = std::fs::read_to_string(&p) {
                return Ok(t);
            }
        }
        let url = format!("{REPO}/manifests/{name}/{file}");
        match ehttp::fetch_blocking(&ehttp::Request::get(&url)) {
            Ok(r) if r.ok && !r.bytes.trim_ascii_start().starts_with(b"<") => {
                let t = String::from_utf8_lossy(&r.bytes).into_owned();
                let _ = std::fs::create_dir_all(p.parent().unwrap()).and_then(|_| std::fs::write(&p, &t));
                Ok(t)
            }
            Ok(r) => Err(format!("{url}: {}", r.status)),
            Err(e) => std::fs::read_to_string(&p).map_err(|_| format!("{url}: {e}")),
        }
    }

    /// One set's manifest text and set.json, from the kit or the repo.
    fn read_set(name: &str, kit: &Path) -> Result<(String, SetMeta, &'static str), String> {
        if !valid_name(name) {
            return Err(format!("{name:?} is not a manifest name"));
        }
        let d = kit.join(name);
        if let Ok(t) = std::fs::read_to_string(d.join("manifest.dcf")) {
            let meta = std::fs::read_to_string(d.join("set.json")).ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
            return Ok((t, meta, "kit"));
        }
        let t = from_repo(name, "manifest.dcf")?;
        let meta = from_repo(name, "set.json").ok().and_then(|j| serde_json::from_str(&j).ok()).unwrap_or_default();
        Ok((t, meta, "repo"))
    }

    /// `ndexr pkgs status --json`: each set's shared library here.
    fn built() -> BTreeMap<String, Built> {
        let Some(cli) = cli() else { return BTreeMap::new() };
        let out = capture_within(&format!("{} pkgs status --json", crate::api::shell_quote(&cli)), Duration::from_secs(30)).unwrap_or_default();
        let v: serde_json::Value = serde_json::from_str(&out).unwrap_or_default();
        v["sets"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|s| {
                        let n = |k: &str| s[k].as_u64().unwrap_or(0) as usize;
                        Some((
                            s["set"].as_str()?.to_owned(),
                            Built {
                                r: n("r"),
                                python: n("python"),
                                rust: n("rust"),
                                julia: n("julia"),
                                busy: s["busy"].as_bool().unwrap_or(false),
                                running: s["steps"]["running"].as_u64().unwrap_or(0) as usize,
                                done: s["steps"]["done"].as_u64().unwrap_or(0) as usize,
                                failed: s["steps"]["failed"].as_u64().unwrap_or(0) as usize,
                                published_at: s["published_at"].as_str().unwrap_or("").to_owned(),
                            },
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every manifest: the kit's (the CLI's order, newest first), the kept
    /// named sets from the repo, with counts and what is built here.
    pub fn list() -> Listing {
        let (mut names, kit, cli, mut note) = lay();
        let kept = kept();
        for k in &kept {
            if !names.contains(k) {
                names.push(k.clone());
            }
        }
        let built = built();
        let mut sets = Vec::new();
        let mut missing = Vec::new();
        for name in names {
            match read_set(&name, &kit) {
                Ok((text, meta, from)) => {
                    let [r, python, rust, julia] = counts(&records(&text));
                    sets.push(Summary {
                        env_module: meta.env_module(&name),
                        modules: meta.modules.clone(),
                        bioc: meta.bioc.clone(),
                        r,
                        python,
                        rust,
                        julia,
                        kept: kept.contains(&name) || name.contains('/'),
                        from: from.into(),
                        built: built.get(&name).cloned(),
                        name,
                    });
                }
                Err(_) => missing.push(name),
            }
        }
        if !missing.is_empty() && note.is_empty() {
            note = format!("not found in this ndexr's kit or the repo: {}", missing.join(", "));
        }
        Listing { sets, triple: crate::host::own_triple(), cli, note }
    }

    pub fn detail(name: &str) -> Result<Detail, String> {
        let (_, kit, _, _) = lay();
        let (text, meta, from) = read_set(name, &kit)?;
        Ok(Detail { name: name.into(), env_module: meta.env_module(name), modules: meta.modules, bioc: meta.bioc, from: from.into(), records: records(&text) })
    }

    // ── install ────────────────────────────────────────────────────────

    /// A CVMFS mount at or above `dir`: the stack is ndexr's there already,
    /// and a local build under it would be hidden (or refused, read-only).
    fn cvmfs_over(dir: &Path) -> Option<String> {
        let mounts = std::fs::read_to_string("/proc/mounts").unwrap_or_default();
        cvmfs_mount(&mounts, dir)
    }

    pub(crate) fn cvmfs_mount(mounts: &str, dir: &Path) -> Option<String> {
        mounts.lines().find_map(|l| {
            let f: Vec<&str> = l.split_whitespace().collect();
            let (dev, at, fs) = (*f.first()?, *f.get(1)?, *f.get(2)?);
            let cvmfs = dev.starts_with("cvmfs2") || fs.contains("cvmfs") || (fs == "autofs" && at.contains("cvmfs"));
            (cvmfs && at != "/" && dir.starts_with(at)).then(|| at.to_owned())
        })
    }

    fn writable(d: &Path) -> bool {
        if !d.is_dir() {
            return false;
        }
        let p = d.join(".cove-write-test");
        let ok = std::fs::write(&p, b"").is_ok();
        let _ = std::fs::remove_file(&p);
        ok
    }

    pub fn info() -> InstallInfo {
        let root = crate::host::stack_root();
        let dir = root.join(crate::host::own_triple());
        let pr = pkgs_root();
        let mut v = InstallInfo {
            dir: dir.display().to_string(),
            pkgs_root: pr.display().to_string(),
            key: crate::host::account::find_key().is_some(),
            ..Default::default()
        };
        // A cell runs `cove serve` trusting its loopback (cell-entry.sh).
        let cell = std::env::var("COVE_TRUST_LOOPBACK").as_deref() == Ok("1");
        v.unsupported = if crate::host::runner::serving() && cell {
            "This window runs on your cell, whose stack is ndexr's own, mounted read-only from CVMFS: every environment is there already. To build environments on your own Linux machine, switch the desktop app to it (account menu › Run on: this computer) and open Install there.".into()
        } else if crate::host::runner::serving() {
            // A server's (or a cell's) stack is ndexr's own, from CVMFS:
            // installing is the person's desktop app's job, on their machine.
            "This is a Cove server: its stack is ndexr's own, mounted from CVMFS, so there is nothing to install here. Install is for the Cove desktop app on your own Linux machine - it builds the environments you pick there with the ndexr CLI.".into()
        } else if !cfg!(target_os = "linux") {
            "The ndexr stack is built for Linux. On this computer, run on your cell (account menu › my cell), where the stack is already mounted.".into()
        } else if !crate::host::arch_supported() {
            format!("The ndexr stack is built for Linux on {}; this machine is {}, so there is nothing to build here. Run on your cell (account menu › my cell), where the stack is already mounted.", crate::host::STACK_ARCHES.join(" and "), std::env::consts::ARCH)
        } else if let Some(at) = cvmfs_over(&dir) {
            format!("The ndexr stack is already mounted here from CVMFS at {at}, so every environment is here already - nothing to build. A local build would sit under that mount; unmount it first if you mean to build your own.")
        } else {
            String::new()
        };
        match cli() {
            Some(c) => v.cli = c,
            None if v.unsupported.is_empty() => v.unsupported = "The ndexr CLI is not on this machine: install it first (`curl -fsSL https://ndexr.io/install | sh`, cvmfs.ndexr.io has the guide), then reopen this tab.".into(),
            None => {}
        }
        if v.unsupported.is_empty() {
            v.needs_root = [&dir, &pr].iter().filter(|d| !writable(d)).map(|d| d.display().to_string()).collect();
            if !v.needs_root.is_empty() {
                v.root_by = crate::host::stack::root_by();
            }
        }
        v
    }

    /// The root step: create the folders as the person (`install -d -o`),
    /// the one thing that needs root - the stack itself never builds as root
    /// (the CLI refuses to).
    fn make_dirs_cmd(how: &str, dirs: &[String]) -> Option<String> {
        if dirs.is_empty() {
            return None;
        }
        let ds = dirs.iter().map(|d| crate::api::shell_quote(d)).collect::<Vec<_>>().join(" ");
        let user = capture_within("id -un", Duration::from_secs(5)).unwrap_or_default();
        let group = capture_within("id -gn", Duration::from_secs(5)).unwrap_or_default();
        let script = format!("install -d -o {} -g {} {ds}", crate::api::shell_quote(&user), crate::api::shell_quote(&group));
        Some(crate::host::stack::as_root(how, &script))
    }

    pub fn install(req: &InstallRequest) -> Result<JobView, String> {
        let info = info();
        if !info.unsupported.is_empty() {
            return Err(info.unsupported);
        }
        if req.sets.is_empty() {
            return Err("pick at least one environment".into());
        }
        if !info.needs_root.is_empty() && info.root_by.is_empty() {
            let dirs = info.needs_root.join(" ");
            return Err(format!("the stack folders need making yours once, which needs root: run `sudo install -d -o $(id -un) -g $(id -gn) {dirs}` in a terminal, then Install again"));
        }
        let (_, kit, _, _) = lay();
        let mut sets = Vec::new();
        for s in &req.sets {
            let (_, meta, _) = read_set(s, &kit)?;
            sets.push((s.clone(), set_easyconfigs(s, &meta)));
        }
        let plan = Plan {
            cli: info.cli.clone(),
            root: crate::host::stack_root().display().to_string(),
            dir: info.dir.clone(),
            sets,
            prebuilt: req.prebuilt,
            report: req.report && info.key,
            make_dirs: make_dirs_cmd(&info.root_by, &info.needs_root),
        };
        let script = install_script(&plan);
        if let Some(w) = refuse(&script) {
            return Err(format!("refused: the install would have run {w:?} - Cove never publishes or pushes"));
        }
        let what = format!("Installing {}", if req.sets.len() == 1 { req.sets[0].clone() } else { format!("{} environments", req.sets.len()) });
        crate::host::stack::jobs::start("install", what, format!("bash -c {}", crate::api::shell_quote(&script)), None)
    }

    pub fn job() -> JobView {
        crate::host::stack::jobs::view("install")
    }

    /// Stop the install: its process group, then whatever builds it left in
    /// the folder (`ndexr stack stop`, this machine's builds there only).
    pub fn stop() -> JobView {
        if crate::host::stack::jobs::stop("install") {
            if let Some(c) = cli() {
                let dir = crate::host::stack_root().join(crate::host::own_triple());
                let _ = capture_within(&format!("{} stack stop --dir {}", crate::api::shell_quote(&c), crate::api::shell_quote(&dir.display().to_string())), Duration::from_secs(30));
            }
        }
        job()
    }

    // ── the person's builds, as their account has them ──────────────────

    fn telemetry_id() -> String {
        let base = std::env::var_os("XDG_CONFIG_HOME").filter(|v| !v.is_empty()).map(PathBuf::from).or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")));
        base.and_then(|b| std::fs::read_to_string(b.join("ndexr/telemetry-id")).ok()).map(|s| s.trim().to_owned()).unwrap_or_default()
    }

    /// api.ndexr.io/v1/builds with the person's key: their machines, newest
    /// first, each with its latest run's counts. The Software catalog leaves
    /// owners' rows out, so this is the one place a person sees their own.
    pub fn my_builds() -> MyBuilds {
        let mut out = MyBuilds { this_machine: telemetry_id(), ..Default::default() };
        let Some(key) = crate::host::account::find_key() else {
            out.error = "not signed in - sign in (account menu) and your installs report here".into();
            return out;
        };
        let mut req = ehttp::Request::get(format!("{}/v1/builds", crate::host::account::api_base()));
        req.headers.insert("Authorization", format!("Bearer {key}"));
        req.headers.insert("Accept", "application/json");
        match ehttp::fetch_blocking(&req) {
            Ok(r) if r.ok => match serde_json::from_slice::<serde_json::Value>(&r.bytes) {
                Ok(v) => {
                    out.owner = v["owner"].as_str().unwrap_or("").to_owned();
                    out.machines = machines(&v);
                }
                Err(e) => out.error = format!("api.ndexr.io/v1/builds did not read: {e}"),
            },
            Ok(r) => out.error = format!("api.ndexr.io/v1/builds answered {}: {}", r.status, crate::backend::readable(&r.bytes)),
            Err(e) => out.error = e,
        }
        out
    }

    pub(crate) fn machines(v: &serde_json::Value) -> Vec<Machine> {
        v["machines"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|m| {
                        let mc = &m["machine"];
                        let sys = &mc["system"];
                        let system = ["os", "arch"].iter().filter_map(|k| sys[*k].as_str()).collect::<Vec<_>>().join(" ");
                        let system = match sys["cpus"].as_u64() {
                            Some(c) => format!("{system} · {c} cpus"),
                            None => system,
                        };
                        Machine {
                            id: m["id"].as_str().unwrap_or("").to_owned(),
                            run: m["run"].as_str().unwrap_or("").to_owned(),
                            host: mc["host"].as_str().unwrap_or("").to_owned(),
                            system,
                            steps: m["steps"].as_i64().unwrap_or(0),
                            done: m["done"].as_i64().unwrap_or(0),
                            failed: m["failed"].as_i64().unwrap_or(0),
                            running: m["running"].as_i64().unwrap_or(0),
                            started: m["started"].as_f64().unwrap_or(0.0),
                            until: m["until"].as_f64().unwrap_or(0.0),
                            seen: m["seen"].as_f64().unwrap_or(0.0),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A draft is written with to_dcf and read back with records: every
    /// field, a value over lines, and SiteLibrary come through as they were.
    #[test]
    fn a_manifest_written_reads_back_the_same() {
        let mut recs = records(DCF);
        recs[0].site_library = true;
        recs[1].note = "first line\n\nafter a blank".into();
        let again = records(&to_dcf(&recs));
        assert_eq!(again, recs);
    }

    const DCF: &str = "# svbt manifest\nPackage: ggplot2\nLanguage: R\nSource: CRAN\nVersion: 3.5.1\nGroup: plotting\nDescription: Create Elegant Data\n  Visualisations Using the Grammar of Graphics\n\nPackage: Biostrings\nLanguage: R\nSource: Bioconductor\nVersion: 2.74.0\nGroup: bioc\nArch: x86_64\n\n\nPackage: scikit_learn\nLanguage: Python\nSource: PyPI\nVersion: 1.5.2\nImport: sklearn\nSkip: aarch64\n\nLanguage: R\nNote: a record without a Package is none\n";

    #[test]
    fn a_manifest_reads_as_its_records() {
        let r = records(DCF);
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].package, "ggplot2");
        assert_eq!(r[0].description, "Create Elegant Data Visualisations Using the Grammar of Graphics");
        assert_eq!(r[1].source, "Bioconductor");
        assert_eq!(r[1].arch, "x86_64");
        assert_eq!((r[2].language.as_str(), r[2].import.as_str(), r[2].skip.as_str()), ("Python", "sklearn", "aarch64"));
        assert_eq!(counts(&r), [2, 1, 0, 0]);
        let rs = records("Package: Serde_JSON\nLanguage: Rust\nSource: crates.io\nVersion: 1.0.140\n");
        assert_eq!((rs[0].lang(), rs[0].key()), (Lang::Rust, "serde-json".to_owned()));
        assert_eq!(counts(&rs), [0, 0, 1, 0]);
        assert_eq!(counts(&records("Package: DuckDB\nLanguage: Julia\nVersion: 1.5.5\n")), [0, 0, 0, 1]);
        assert_eq!(count_words([253, 0, 0, 0]), "253 R");
        assert_eq!(count_words([0, 0, 40, 0]), "40 Rust");
        assert_eq!(count_words([0, 0, 0, 37]), "37 Julia");
        assert_eq!(count_words([590, 108, 0, 0]), "590 R · 108 Python");
        assert_eq!(count_words([0, 0, 0, 0]), "no records");
        assert_eq!(family("Julia/1.11.7"), "Julia");
        assert_eq!(r[2].key(), "scikit-learn");
        assert_eq!(py_norm("Foo__Bar.baz"), "foo-bar-baz");
        // CRLF, a lone "." continuation, no trailing newline.
        let m = parse_dcf("Package: a\r\nNote: one\r\n .\r\n two\r\n\r\nPackage: b");
        assert_eq!(m.len(), 2);
        assert_eq!(m[0]["Note"], "one\ntwo");
        assert_eq!(m[1]["Package"], "b");
    }

    #[test]
    fn names_are_easybuild_shaped() {
        for ok in ["r4.6_py3.13-release", "R/4.6.1-bioc3.23", "Python/3.13.15", "ndexr-app/4.6.1", "ndexr-app-r4.6.1"] {
            assert!(valid_name(ok), "{ok}");
        }
        for bad in ["", "../etc", "R/../x", "/R", "R/", "a b", "R/4.6;rm -rf", "a/b/c/d/e", ".hidden"] {
            assert!(!valid_name(bad), "{bad}");
        }
        assert_eq!(family("r4.5_py3.12-devel"), "R + Python");
        assert_eq!(family("R/4.6.1-bioc3.23"), "R");
        assert_eq!(family("Python/3.13.15"), "Python");
        assert_eq!(family("Rust/1.96.0"), "Rust");
        assert!(valid_name("Rust/1.96.0"));
        assert_eq!(family("ndexr-app/4.6.1"), "purpose");
    }

    #[test]
    fn a_set_needs_its_modules_then_its_own() {
        assert_eq!(set_easyconfigs("r4.6_py3.13-release", &SetMeta::default()), ["env-r4.6_py3.13-release.eb"]);
        let m = SetMeta { modules: vec!["R/4.6.1-foss-2024a".into(), "Python/3.13.5-GCCcore-14.2.0".into()], bioc: "3.23".into(), module: String::new() };
        assert_eq!(set_easyconfigs("R/4.6.1-bioc3.23", &m), ["R-4.6.1-foss-2024a.eb", "Python-3.13.5-GCCcore-14.2.0.eb", "env-R-4.6.1-bioc3.23.eb"]);
        // The repo's named sets build in a module of another set's.
        let app = SetMeta { module: "env/r4.6_py3.13-release".into(), ..Default::default() };
        assert_eq!(set_easyconfigs("ndexr-app-r4.6.1", &app), ["env-r4.6_py3.13-release.eb"]);
    }

    fn plan(report: bool, prebuilt: bool, make_dirs: Option<String>) -> Plan {
        Plan {
            cli: "/home/me/.local/bin/ndexr".into(),
            root: "/opt/ndexr/stack".into(),
            dir: "/opt/ndexr/stack/x86_64-pc-linux-gnu".into(),
            sets: vec![
                ("r4.6_py3.13-release".into(), vec!["env-r4.6_py3.13-release.eb".into()]),
                ("R/4.6.1-bioc3.23".into(), vec!["R-4.6.1-foss-2024a.eb".into(), "env-R-4.6.1-bioc3.23.eb".into()]),
                ("ndexr-app-r4.6.1".into(), vec!["env-r4.6_py3.13-release.eb".into()]),
            ],
            prebuilt,
            report,
            make_dirs,
        }
    }

    #[test]
    fn the_install_never_publishes_or_pushes() {
        for (report, prebuilt, dirs) in [(true, true, Some("sudo -n sh -c 'install -d -o me -g me /opt/ndexr/stack/x86_64-pc-linux-gnu'".to_owned())), (false, false, None), (true, false, None)] {
            let s = install_script(&plan(report, prebuilt, dirs));
            assert_eq!(refuse(&s), None, "{s}");
            for w in ["publish", "promote", "assemble", "s3://", "push", "--to", "manifest "] {
                assert!(!s.contains(w), "{w} in:\n{s}");
            }
            assert!(s.contains("stack init --dir \"$dir\""));
            assert!(s.contains("stack prefix --dir \"$dir\" --foreground"));
            // The modules once each, as whole file names.
            assert!(s.contains("stack build --dir \"$dir\" --only 'env-r4.6_py3.13-release.eb,R-4.6.1-foss-2024a.eb,env-R-4.6.1-bioc3.23.eb'"), "{s}");
            assert!(s.contains("pkgs build 'R/4.6.1-bioc3.23'"));
            assert_eq!(s.contains("pkgs sync 'r4.6_py3.13-release'"), prebuilt);
            assert_eq!(s.contains("--no-report"), !report);
            assert_eq!(s.contains("stack report --dir"), report);
            assert!(s.contains(&format!("n={}", 4 + 3 + usize::from(report))));
        }
        assert_eq!(refuse("ndexr stack up --publish"), Some("--publish"));
        assert_eq!(refuse("ndexr pkgs publish x"), Some(" publish"));
        assert_eq!(refuse("ndexr pkgs promote"), Some("promote"));
        assert_eq!(refuse("ndexr stack assemble"), Some("assemble"));
        assert_eq!(refuse("ndexr pkgs sync x --then-publish"), Some("--then-publish"));
    }

    #[test]
    fn a_hostile_set_name_stays_one_word() {
        let mut p = plan(true, true, None);
        p.sets = vec![("x'; rm -rf ~; '".into(), vec!["a.eb".into()])];
        let s = install_script(&p);
        assert!(s.contains(r"pkgs build 'x'\''; rm -rf ~; '\'''"), "{s}");
    }

    #[test]
    fn the_log_says_how_far_it_got() {
        let log = [
            "[cove install] 2 environment(s): r4.6_py3.13-release, ndexr-app-r4.6.1",
            "[cove install] step 3/7 prefix — 10:02:11Z",
            "* [skip]  GCCcore-13.3.0.eb",
            "* [build] R-4.6.1-foss-2024a.eb",
            "[ndexr up] step 4/5 suite — 2026-10-15T10:00:00Z",
            "* [rebuild] env-r4.6_py3.13-release.eb - easyconfig changed since it was built",
            "* now installing ggplot2 (R) - 12 of 559",
            "  [ok] R        ggplot2",
            "[FAIL] packages R/4.6.1-bioc3.23",
            "* now installing scikit-learn (Python) - 3 of 269",
        ];
        let p = progress(&log);
        assert_eq!(p.step, Some((4, 5, "suite".to_owned())));
        assert_eq!(p.package, Some(("scikit-learn".into(), "Python".into(), 3, 269)));
        assert_eq!(p.easyconfig.as_deref(), Some("env-r4.6_py3.13-release.eb"));
        assert_eq!((p.built, p.skipped, p.ok, p.failed), (2, 1, 1, 1));
        // A new step forgets the last step's package count.
        let p = progress(&["* now installing a (R) - 1 of 2", "[cove install] step 6/7 packages ndexr-app-r4.6.1 — 11:00:00Z"]);
        assert_eq!(p.package, None);
        assert_eq!(p.step.map(|s| s.2), Some("packages ndexr-app-r4.6.1".to_owned()));
        assert_eq!(progress::<&str>(&[]), Progress::default());
    }

    #[test]
    fn two_manifests_compare() {
        let a = records("Package: a\nLanguage: R\nVersion: 1\n\nPackage: b\nLanguage: R\nVersion: 1\n\nPackage: Foo_Bar\nLanguage: Python\nVersion: 2\n");
        let b = records("Package: a\nLanguage: R\nVersion: 2\n\nPackage: foo-bar\nLanguage: Python\nVersion: 2\n\nPackage: c\nLanguage: Python\nVersion: 9\n");
        let d = diff(&a, &b);
        assert_eq!(d.added.iter().map(|r| r.package.as_str()).collect::<Vec<_>>(), ["c"]);
        assert_eq!(d.removed.iter().map(|r| r.package.as_str()).collect::<Vec<_>>(), ["b"]);
        assert_eq!(d.changed.len(), 1);
        assert_eq!((d.changed[0].0.version.as_str(), d.changed[0].1.version.as_str()), ("1", "2"));
        assert_eq!(d.same, 1);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_cvmfs_mount_over_the_stack_is_seen() {
        use std::path::Path;
        let m = "/dev/nvme0n1p2 / ext4 rw 0 0\ncvmfs2 /opt/ndexr/stack fuse rw,nosuid 0 0\n";
        assert_eq!(host::cvmfs_mount(m, Path::new("/opt/ndexr/stack/x86_64-pc-linux-gnu")).as_deref(), Some("/opt/ndexr/stack"));
        assert_eq!(host::cvmfs_mount("/dev/sda1 / ext4 rw 0 0\n", Path::new("/opt/ndexr/stack/x")), None);
        assert_eq!(host::cvmfs_mount("cvmfs2 /cvmfs/other fuse rw 0 0\n", Path::new("/opt/ndexr/stack/x")), None);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn the_accounts_machines_read() {
        let v: serde_json::Value = serde_json::from_str(r#"{"owner":"a@b.c","machines":[{"id":"ab12","run":"1760000000","machine":{"host":"desk","system":{"os":"linux","arch":"x86_64","cpus":16}},"steps":40,"done":38,"failed":1,"running":1,"started":1760000000.0,"until":1760003600.0,"seen":1760003600.5}]}"#).unwrap();
        let m = host::machines(&v);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].host.as_str(), m[0].system.as_str(), m[0].done, m[0].failed), ("desk", "linux x86_64 · 16 cpus", 38, 1));
    }
}
