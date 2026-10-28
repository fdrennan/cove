//! The source behind any star (operator, 2026-10-17: "every single one of
//! those stars, every single one of those packages is a place in the chain of
//! dependencies where we could modify the source code and apply a patch ... to
//! be able to view the source code for that entire project, and to be able to
//! take that source code, modify it, and apply it to a new branch of the tree
//! or replace it altogether").
//!
//! This is the first branch of that: Source. A star's exact source lands in a
//! domain project of the person's own (names.rs: `<name>.<paradigm>.<their
//! domain>`, 2026-10-17), a git repository in their owner folder - so Save
//! pushes it and Cove opens it by name like any domain. Each version opened is
//! two branches: `upstream/<version>`, the pristine source, and
//! `work/<version>`, where it is changed; one is checked out at a time, and
//! opening another version first checkpoints what is pending. What a person
//! changed is always `git diff upstream/<version>`: the patch the next
//! branches (Patch, Apply) take to a new branch of the tree or put in place of
//! the original. Workspaces from before (`~/cove-source/<kind>/<name>/<ver>/`,
//! branches `upstream` and `work`) still read.
//!
//! Where each kind's source is, for the version the stack has:
//!
//!   R          the source tarball: CRAN, then CRAN's archive, then Bioconductor
//!              (the release the star's sets name, then release and devel)
//!   Python     the sdist PyPI lists for that version
//!   easybuild  the installed module's own easybuild/ folder on the stack: the
//!              easyconfig it was built from, its patches and EasyBuild's
//!              reproducibility copy (<stack>/<triple>/eb/software/<name>/<ver>)
//!   prefix     the ebuild from the Prefix's own portage tree, its files/, and
//!              the distfiles the Manifest names for that version, unpacked
//!              under src/ (<prefix>/var/db/repos/gentoo, var/cache/distfiles)
//!
//! Run as the person (their session), into their home; a second open of the
//! same star finds the repository already there and leaves their work alone.
//!
//! THE SECOND BRANCH: CUSTOMIZE ONE COMPONENT COMPLETELY (operator, 2026-10-17:
//! "open the exact source behind this component, customize it, and use the
//! resulting variant in a recorded environment"). In a workspace:
//!
//!   checkpoint  the work so far, committed on `work` - it need not build yet
//!   candidate   the checkpoint built in the session's own environment (the one
//!               the kernels run in), into ~/cove-candidates/<kind>/<name>/
//!               <version>/<commit>/, with candidate.json: which source commit,
//!               upstream and origin, which environment, the artifact and its
//!               sha256, when, and whether it built. Never into the published
//!               environment, which does not change.
//!   test        the person's own snippet, in two fresh processes - the
//!               environment as published, then with the candidate ahead of it
//!               - each saying which copy of the package it loaded
//!
//! R and Python packages build here; an EasyBuild or Prefix star's workspace is
//! source to read and change until its build adapter exists.

use serde::{Deserialize, Serialize};

/// What opened: the folder, where the source came from, whether this open
/// fetched it or found it from before, and the project's domain.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceView {
    pub path: String,
    pub origin: String,
    pub fresh: bool,
    /// `data-table.r.fdrennan.ndexr.io`, and the paradigm parent registered
    /// before it (`r.fdrennan.ndexr.io`).
    pub host: String,
    pub parent: String,
}

/// The kinds a star can be (the catalog's `k`).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub const KINDS: [&str; 5] = ["prefix", "easybuild", "R", "Python", "Rust"];

/// A name or version a script may be handed: package, atom and module
/// characters only, no empty, `.` or `..` segment. Everything is also
/// shell-quoted; this keeps a path built from it inside the workspace.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn safe(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 160
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "._+-/".contains(c))
        && s.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
}

/// The Bioconductor releases a star's sets name (`R/4.6.1-bioc3.23` → 3.23).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
pub fn biocs(sets: &[String]) -> Vec<String> {
    let mut v: Vec<String> = sets
        .iter()
        .filter_map(|s| s.split("-bioc").nth(1))
        .map(|b| b.chars().take_while(|c| c.is_ascii_digit() || *c == '.').collect::<String>())
        .filter(|b| b.contains('.'))
        .collect();
    v.sort();
    v.dedup();
    v.reverse();
    v
}

#[cfg(not(target_arch = "wasm32"))]
const FETCH: &str = r#"set -uo pipefail
kind="$1"; name="$2"; ver="$3"; ws="$4"; stack="$5"; biocs="$6"
host="${7:-}"; paradigm="${8:-}"; digest="${9:-}"; authority="${10:-}"; native="${11:-$name}"; env="${12:-}"
sub="${13:-}"; template="${14:-}"
say() { printf 'COVE_SOURCE %s\n' "$*"; }
up="upstream/$ver"; wk="work/$ver"
mkdir -p "$ws" || { say "fail cannot make $ws"; exit 0; }
# One open at a time per project: a second waits, then finds the first's work.
exec 9>"$ws/.cove-lock"; flock 9 2>/dev/null || true
g() { git -C "$ws" -c user.name="${USER:-cove}" -c user.email="${USER:-cove}@ndexr.io" -c core.hooksPath=/dev/null "$@"; }
# A fresh project that fails leaves nothing; one with versions keeps them.
fail() { [ -d "$ws/.git" ] || rm -rf "$ws"; say "fail $*"; exit 0; }
# Pending work is checkpointed on the branch it is on before leaving it.
keep() {
  [ -z "$(g status --porcelain 2>/dev/null)" ] && return 0
  g add -A && g commit -qm "checkpoint before opening $ver" || fail "could not checkpoint the work in progress"
}
if [ -d "$ws/.git" ]; then
  # From before versions: one version, on branches upstream and work.
  if g rev-parse -q --verify refs/heads/work >/dev/null 2>&1; then say "ok existing $ws"; exit 0; fi
  if g rev-parse -q --verify "refs/heads/$wk" >/dev/null 2>&1; then
    if [ "$(g branch --show-current)" != "$wk" ]; then keep; g checkout -q "$wk" || fail "could not switch to $wk"; fi
    say "ok existing $ws $(sed -n 's/^origin=//p' "$ws/.cove-source" 2>/dev/null | head -n1)"; exit 0
  fi
fi
cache="${XDG_CACHE_HOME:-$HOME/.cache}"; mkdir -p "$cache"
tmp="$(mktemp -d "$cache/cove-source.XXXXXX")" || fail "no scratch folder in $cache"
trap 'rm -rf "$tmp"' EXIT
src="$tmp/src"; mkdir -p "$src"
# Unpack an archive into a folder, without the single top folder most carry.
unpack() {  # archive dest
  local x="$tmp/x.$RANDOM"; mkdir -p "$x" "$2"
  case "$1" in
    *.zip|*.whl) unzip -q "$1" -d "$x" ;;
    *) tar -xf "$1" -C "$x" ;;
  esac || return 1
  set -- "$x"/* "$2"
  if [ $# -eq 2 ] && [ -d "$1" ]; then cp -a "$1"/. "$2"/; else cp -a "$x"/. "${@: -1}"/; fi
}
origin=""; sum=""
case "$kind" in
  R)
    urls="https://cloud.r-project.org/src/contrib/${name}_${ver}.tar.gz https://cloud.r-project.org/src/contrib/Archive/${name}/${name}_${ver}.tar.gz"
    for b in $biocs release devel; do
      for r in bioc data/annotation data/experiment workflows; do urls="$urls https://bioconductor.org/packages/$b/$r/src/contrib/${name}_${ver}.tar.gz"; done
    done
    for u in $urls; do
      if curl -fsSL --max-time 90 -o "$tmp/src.tar.gz" "$u" 2>/dev/null; then origin="$u"; break; fi
    done
    [ -n "$origin" ] || fail "no source tarball for $name $ver on CRAN or Bioconductor"
    sum="$(sha256sum "$tmp/src.tar.gz" | cut -d' ' -f1)"
    unpack "$tmp/src.tar.gz" "$src" || fail "could not unpack $origin" ;;
  Python)
    j="$(curl -fsSL --max-time 30 "https://pypi.org/pypi/$name/$ver/json" 2>/dev/null)" || fail "PyPI has no $name $ver"
    u="$(printf '%s' "$j" | jq -r '[.urls[] | select(.packagetype == "sdist")][0].url // empty' 2>/dev/null)"
    [ -n "$u" ] || fail "PyPI has no source distribution for $name $ver - wheels only"
    f="$tmp/${u##*/}"
    curl -fsSL --max-time 300 -o "$f" "$u" 2>/dev/null || fail "could not download $u"
    origin="$u"; sum="$(sha256sum "$f" | cut -d' ' -f1)"
    unpack "$f" "$src" || fail "could not unpack $u" ;;
  Rust)
    # A crate's published source: the .crate crates.io serves (a tar.gz).
    u="https://static.crates.io/crates/$name/$name-$ver.crate"
    curl -fsSL --max-time 300 -o "$tmp/src.crate" "$u" 2>/dev/null || fail "crates.io has no $name $ver"
    origin="$u"; sum="$(sha256sum "$tmp/src.crate" | cut -d' ' -f1)"
    unpack "$tmp/src.crate" "$src" || fail "could not unpack $u" ;;
  easybuild)
    d="$stack/eb/software/$name/$ver/easybuild"
    [ -d "$d" ] || fail "this stack has no $name/$ver installed ($d)"
    cp -a "$d"/. "$src"/ || fail "could not copy $d"
    # The easyconfig, its patches and the reproducibility copy - not the logs.
    rm -f "$src"/easybuild-*.log "$src"/*_test_report.md
    origin="$d" ;;
  prefix)
    pre="$stack/prefix/${stack##*/}"
    pn="${name#*/}"; tree="$pre/var/db/repos/gentoo/$name"
    [ -f "$tree/$pn-$ver.ebuild" ] || fail "no $pn-$ver.ebuild in this Prefix's portage tree ($tree)"
    mkdir -p "$src/ebuild"
    cp -a "$tree/$pn-$ver.ebuild" "$src/ebuild/" || fail "could not copy the ebuild"
    for x in metadata.xml Manifest files; do [ -e "$tree/$x" ] && cp -a "$tree/$x" "$src/ebuild/"; done
    # The distfiles the Manifest names for this version, unpacked under src/.
    dist="$pre/var/cache/distfiles"
    if [ -f "$tree/Manifest" ]; then
      while read -r t f _; do
        [ "$t" = DIST ] || continue
        case "$f" in *"$ver"*) ;; *) continue ;; esac
        [ -f "$dist/$f" ] || continue
        case "$f" in
          *.tar*|*.tgz|*.tbz2|*.txz|*.zip) unpack "$dist/$f" "$src/src/${f%%.t*}" || cp -a "$dist/$f" "$src/src/" ;;
          *) mkdir -p "$src/src"; cp -a "$dist/$f" "$src/src/" ;;
        esac
      done < "$tree/Manifest"
    fi
    origin="$tree" ;;
  *) fail "not a kind of star: $kind" ;;
esac
# Into the project: a new repository, or an orphan branch beside the versions
# already in it - the work in progress checkpointed first, never lost.
# upstream/<v>: the source as it came. work/<v>: where it is changed. The
# patch is always the difference between them.
# The site the person made of it (module.r) is theirs across versions: kept
# from the version in front, put back on the new one.
[ -f "$ws/module.r" ] && cp -a "$ws/module.r" "$tmp/module.r.keep"
if [ -d "$ws/.git" ]; then
  keep
  g checkout -q --orphan "$up" || fail "could not start $up"
  g rm -rqf --cached . >/dev/null 2>&1
  find "$ws" -mindepth 1 -maxdepth 1 ! -name .git ! -name .cove-lock -exec rm -rf {} +
else
  find "$ws" -mindepth 1 -maxdepth 1 ! -name .cove-lock -exec rm -rf {} +
  g -c init.defaultBranch="$up" init -q || fail "git could not start the project"
fi
grep -qx '.cove-lock' "$ws/.git/info/exclude" 2>/dev/null || echo .cove-lock >> "$ws/.git/info/exclude"
# The source in a folder of its proper name; the project's own top level is
# a site about it (operator, 2026-10-18: "the project itself goes into a sub
# folder ... a template shows up with the project name and details ...
# routing with a ui is set"). A project from before has no $sub: as it was.
if [ -n "$sub" ]; then
  mkdir -p "$ws/$sub" && cp -a "$src"/. "$ws/$sub"/ || fail "could not place the source"
  if [ -f "$tmp/module.r.keep" ]; then cp -a "$tmp/module.r.keep" "$ws/module.r"
  elif [ -n "$template" ] && [ -f "$template" ]; then cp "$template" "$ws/module.r" || fail "could not write module.r"
  fi
  printf '.Rhistory\n.RData\n*.log\n' > "$ws/.gitignore"
else
  cp -a "$src"/. "$ws"/ || fail "could not place the source"
fi
cat > "$ws/.cove-source" <<EOF || fail "could not write .cove-source"
kind=$kind
name=$name
version=$ver
origin=$origin
sha256=$sum
fetched=$(date -u +%Y-%m-%dT%H:%M:%SZ)
host=$host
paradigm=$paradigm
authority=$authority
native=$native
digest=$digest
env=$env
branch=$wk
dir=$sub
EOF
if [ -n "$host" ]; then
  # The domain's own page: what this project is and where it came from.
  cat > "$ws/CONTEXT.md" <<EOF || fail "could not write CONTEXT.md"
# $name · $host
The source of $name $ver ($kind), opened from the ndexr stack as a project of
its own. Branch upstream/$ver is the source exactly as it came; work/$ver is
where it is changed, and git diff upstream/$ver is the patch.

## Source
- identity: $paradigm · $authority · $native
- origin: $origin
- sha256: ${sum:-n/a}
- opened in: ${env:-the session environment}

## Files
- ${sub:-.}/ - the source; git diff upstream/$ver -- ${sub:-.} is the patch
- module.r - this domain's site: the project's name and details, read live from .cove-source
- .cove-source - the identity and provenance Cove reads (names.rs, source.rs)
EOF
  if [ -n "$env" ]; then printf '# the environment this source was opened from (Cove)\n%s\n' "$env" > "$ws/.ndexr-env"; fi
fi
g add -A && g commit -qm "upstream: $kind $name $ver ($origin)" && g checkout -qb "$wk" \
  || fail "git could not record the upstream source"
say "ok new $ws $origin"
"#;

/// Where the person's projects live - their owner folder: `~/domains` in a
/// multiuser session (a bind mount of it), else the platform checkout's
/// `<owner>/` - and the domain they own there that projects hang from.
#[cfg(not(target_arch = "wasm32"))]
pub fn software_root() -> Result<(std::path::PathBuf, String), String> {
    let owner = crate::host::session_owner();
    if owner.is_empty() {
        return Err("no signed-in account to make the project for".into());
    }
    let mut roots = Vec::new();
    if let Ok(h) = std::env::var("HOME") {
        roots.push(std::path::Path::new(&h).join("domains"));
    }
    roots.push(std::path::Path::new(crate::host::PLATFORM_DOMAINS).join(&owner));
    let root = roots.into_iter().find(|r| r.is_dir()).ok_or_else(|| format!("no domains folder for {owner} on this machine"))?;
    // Their personal domain, as their account made it: what every project of
    // theirs hangs from.
    let own = format!("{owner}.ndexr.io");
    if !root.join(&own).is_dir() {
        return Err(format!("{own}, your personal domain, is not in {} - it comes with your account", root.display()));
    }
    Ok((root, own))
}

/// The identity digest a folder's `.cove-source` records, if it has one.
#[cfg(not(target_arch = "wasm32"))]
fn digest_at(dir: &std::path::Path) -> Option<String> {
    let t = std::fs::read_to_string(dir.join(".cove-source")).ok()?;
    t.lines().find_map(|l| l.strip_prefix("digest=")).map(|d| d.trim().to_owned()).filter(|d| !d.is_empty())
}

/// The project for `id` under `root`: the one already holding its identity,
/// else the first of its names nobody holds - reserved before anything is
/// fetched (`create_dir` is the atomic step, the stub `.cove-source` the
/// claim). A folder holding another identity, or any other domain, is never
/// taken over.
#[cfg(not(target_arch = "wasm32"))]
pub fn allocate(root: &std::path::Path, owned: &str, id: &crate::names::Identity) -> Result<(String, std::path::PathBuf), String> {
    let digest = id.digest();
    let hosts = id.hosts(owned);
    // An existing assignment wins, whichever of its names it holds.
    for h in &hosts {
        if digest_at(&root.join(h)).as_deref() == Some(digest.as_str()) {
            return Ok((h.clone(), root.join(h)));
        }
    }
    for h in &hosts {
        let d = root.join(h);
        match std::fs::create_dir(&d) {
            Ok(()) => {
                let stub = format!("digest={digest}\nhost={h}\nparadigm={}\nnative={}\n", id.paradigm, id.native);
                std::fs::write(d.join(".cove-source"), stub).map_err(|e| format!("{}: {e}", d.display()))?;
                return Ok((h.clone(), d));
            }
            // Something else's - or this same identity an instant ago, in a race.
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if digest_at(&d).as_deref() == Some(digest.as_str()) {
                    return Ok((h.clone(), d));
                }
            }
            Err(e) => return Err(format!("{}: {e}", d.display())),
        }
    }
    Err(format!("every name for {} under {owned} is held by something else", id.native))
}

/// The folder a project's source goes in: the software's own name - the
/// package's (`data.table`), the module's (`BLAST+`), a Prefix package
/// without its category (`sys-devel/gcc` → `gcc`).
pub fn source_folder(kind: &str, name: &str) -> String {
    let n = if kind == "prefix" { name.rsplit('/').next().unwrap_or(name) } else { name };
    let n: String = n.chars().filter(|c| c.is_ascii_alphanumeric() || "._+-".contains(*c)).collect();
    if n.is_empty() || n.starts_with('.') { "source".to_owned() } else { n }
}

/// A software project's site from the moment it exists: routing and a page
/// about the project - its name, kind, version, origin and checksum, read
/// live from `.cove-source` so it follows the version checked out - and the
/// source folder's contents. The person makes it into their own application
/// from here; Cove never writes over a module.r that is already there.
pub fn module_template(host: &str) -> String {
    MODULE_TEMPLATE.replace("__HOST__", host)
}

const MODULE_TEMPLATE: &str = r#"# __HOST__ - a software project opened in Cove.
#
# The source is in the folder named in .cove-source (dir=); this page reads
# that file each time it is drawn, so it shows the version checked out. Make
# it an application of your own: everything below is yours to change.

.meta <- function() {
  f <- box::file(".cove-source")
  if (!file.exists(f)) return(list())
  kv <- strsplit(readLines(f, warn = FALSE), "=", fixed = TRUE)
  kv <- kv[lengths(kv) >= 2]
  stats::setNames(lapply(kv, function(x) paste(x[-1], collapse = "=")), vapply(kv, `[[`, "", 1))
}

#' @export
ui_module <- function(id, request) {
  box::use(
    shiny = shiny[NS, tags],
    r / prelude / prelude[shell, inputs]
  )
  ns <- shiny$NS(id)
  m <- .meta()
  g <- function(k) if (is.null(m[[k]]) || !nzchar(m[[k]])) "-" else m[[k]]
  folder <- if (is.null(m$dir) || !nzchar(m$dir)) "." else m$dir
  files <- sort(list.files(box::file(folder), all.files = FALSE))
  row <- function(k, v) tags$tr(tags$th(class = "text-body-secondary fw-normal pe-3", k), tags$td(tags$code(v)))
  shell$ui_shell(
    ns        = ns,
    request   = request,
    head_args = list(title = paste(g("name"), "·", "__HOST__"), domain = "__HOST__", stylesheet = "css/home.css"),
    body      = inputs$page(
      inputs$hero(g("name"), eyebrow = paste(g("kind"), g("version")),
                  lead = "Opened from the ndexr stack as a project of its own - built from source, changed on its own branch, and this site is yours to make into an application."),
      inputs$card(
        "The project",
        tags$table(
          class = "table table-sm mb-0",
          row("name", g("name")), row("kind", g("kind")), row("version", g("version")),
          row("identity", paste(g("paradigm"), g("authority"), g("native"), sep = " · ")),
          row("origin", g("origin")), row("sha256", g("sha256")), row("fetched", g("fetched")),
          row("branch", g("branch")), row("environment", g("env")), row("source folder", folder)
        )
      ),
      inputs$card(
        paste0("In ", folder, "/"),
        tags$p(class = "small text-body-secondary",
               paste0(length(files), " entries. upstream/", g("version"), " is the source as it came; the patch is git diff upstream/",
                      g("version"), " -- ", folder, ".")),
        tags$ul(class = "small mb-0", lapply(utils::head(files, 60), function(f) tags$li(tags$code(f))))
      )
    )
  )
}

.server_inner <- function(id = "__HOST__", site_session) {
  box::use(r / prelude / prelude[shell])
  shell$server_shell(id, site_session)
}

#' @export
server_module <- local({
  box::use(r / prelude / prelude[timing])
  wrapped <- timing$measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
"#;

/// Fetch (or find) the source of one star into its domain project. `env` is
/// the environment it is opened from - recorded, and the project's
/// `.ndexr-env` - or "" for the session's own.
#[cfg(not(target_arch = "wasm32"))]
pub fn open(kind: &str, name: &str, version: &str, sets: &[String], env: &str) -> Result<SourceView, String> {
    use std::time::Duration;
    if !KINDS.contains(&kind) {
        return Err(format!("{kind}: not a kind of star"));
    }
    if !safe(name) || !safe(version) {
        return Err(format!("{name} {version}: not a name this can fetch"));
    }
    let env = env.trim();
    if !env.is_empty() && !crate::kernel::valid_module(env) {
        return Err(format!("{env}: not an environment module"));
    }
    let id = crate::names::Identity::of(kind, name).ok_or_else(|| format!("{name}: no project name for a {kind} star"))?;
    let (root, owned) = software_root()?;
    let (host, ws) = allocate(&root, &owned, &id)?;
    let parent = format!("{}.{owned}", id.paradigm);
    let home = std::env::var("HOME").map_err(|_| "no home folder".to_owned())?;
    let arch = std::env::consts::ARCH;
    let triple = format!("{arch}-{}-linux-gnu", if arch == "aarch64" { "unknown" } else { "pc" });
    let stack = std::env::var("NDEXR_STACK_DIR").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/opt/ndexr/stack".into());
    let stack = std::path::Path::new(&stack).join(&triple);
    let script = std::path::Path::new(&home).join(".cache").join("cove").join("source.sh");
    std::fs::create_dir_all(script.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&script, FETCH).map_err(|e| e.to_string())?;
    let sub = source_folder(kind, name);
    let template = script.with_file_name(format!("module-{}.r", std::process::id()));
    std::fs::write(&template, module_template(&host)).map_err(|e| e.to_string())?;
    let q = crate::api::shell_quote;
    let cmd = format!(
        "bash {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
        q(&script.display().to_string()),
        q(kind),
        q(name),
        q(version),
        q(&ws.display().to_string()),
        q(&stack.display().to_string()),
        q(&biocs(sets).join(" ")),
        q(&host),
        q(id.paradigm),
        q(&id.digest()),
        q(id.authority),
        q(&id.native),
        q(env),
        q(&sub),
        q(&template.display().to_string()),
    );
    let out = crate::runner::capture_within(&cmd, Duration::from_secs(600));
    let _ = std::fs::remove_file(&template);
    let out = out.ok_or("fetching the source took too long or did not answer")?;
    let line = out.lines().rev().find_map(|l| l.strip_prefix("COVE_SOURCE ")).ok_or_else(|| format!("unexpected answer: {}", out.chars().take(300).collect::<String>()))?;
    match line.split_once(' ') {
        Some(("ok", rest)) => {
            let mut w = rest.splitn(3, ' ');
            let fresh = w.next() == Some("new");
            let path = w.next().unwrap_or_default().to_owned();
            let origin = w.next().unwrap_or_default().to_owned();
            Ok(SourceView { path, origin, fresh, host, parent })
        }
        Some(("fail", why)) => Err(why.to_owned()),
        _ => Err(format!("unexpected answer: {line}")),
    }
}

// ── the second branch: checkpoints and candidates ──────────────────────────

/// A checkpoint on `work`: the commit, when, and what it says.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Checkpoint {
    pub sha: String,
    pub when: String,
    pub message: String,
}

/// A built variant of a star (its candidate.json).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Candidate {
    /// Its folder, ~/cove-candidates/<kind>/<name>/<version>/<commit12>.
    pub dir: String,
    pub kind: String,
    pub name: String,
    pub version: String,
    /// The `work` commit it was built from, and the upstream it descends from.
    pub commit: String,
    pub upstream: String,
    pub origin: String,
    /// The environment it was built in ("" = the R and python3 on PATH).
    pub env: String,
    pub built_at: String,
    pub ok: bool,
    /// The built package (R source tarball, Python wheel) and its sha256.
    pub artifact: String,
    pub sha256: String,
    /// Where it is installed: the R library or the Python site folder.
    pub lib: String,
}

/// A workspace as the Files strip shows it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceStatus {
    pub root: String,
    pub kind: String,
    pub name: String,
    pub version: String,
    pub origin: String,
    pub branch: String,
    /// Files that differ from `upstream`, saved or not.
    pub changed: Vec<String>,
    /// Changes not yet in a checkpoint.
    pub dirty: bool,
    /// Newest first.
    pub checkpoints: Vec<Checkpoint>,
    /// Newest first.
    pub candidates: Vec<Candidate>,
    /// Whether a candidate can be built here (R, Python).
    pub buildable: bool,
}

#[cfg(not(target_arch = "wasm32"))]
mod work {
    use super::*;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    fn home() -> Result<PathBuf, String> {
        std::env::var("HOME").map(PathBuf::from).map_err(|_| "no home folder".to_owned())
    }

    /// The person's own workspaces and candidates, and nowhere else.
    fn under(base: &str, p: &str) -> Result<PathBuf, String> {
        let base = home()?.join(base);
        let base = base.canonicalize().map_err(|_| format!("no {} yet", base.display()))?;
        let p = Path::new(p.trim()).canonicalize().map_err(|e| format!("{p}: {e}"))?;
        if !p.starts_with(&base) {
            return Err(format!("{}: not in {}", p.display(), base.display()));
        }
        Ok(p)
    }

    /// The workspace a path is in: the folder holding .cove-source - a
    /// project in the person's domains, or one from before under ~/cove-source.
    pub fn root(path: &str) -> Result<(PathBuf, std::collections::BTreeMap<String, String>), String> {
        let in_domains = super::software_root().ok().and_then(|(r, _)| r.canonicalize().ok()).and_then(|r| {
            let p = Path::new(path.trim()).canonicalize().ok()?;
            p.starts_with(&r).then_some(p)
        });
        let mut d = match in_domains {
            Some(p) => p,
            None => under("cove-source", path)?,
        };
        loop {
            let f = d.join(".cove-source");
            if f.is_file() {
                let meta = std::fs::read_to_string(&f).map_err(|e| e.to_string())?;
                let m = meta.lines().filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned())).collect();
                return Ok((d, m));
            }
            if !d.pop() {
                return Err(format!("{path}: not inside a source workspace"));
            }
        }
    }

    pub fn git(ws: &Path, args: &[&str]) -> Result<String, String> {
        let who = std::env::var("USER").unwrap_or_else(|_| "cove".into());
        let out = Command::new("git")
            .arg("-C")
            .arg(ws)
            .args(["-c", &format!("user.name={who}"), "-c", &format!("user.email={who}@ndexr.io"), "-c", "core.hooksPath=/dev/null"])
            .args(args)
            .output()
            .map_err(|e| format!("git: {e}"))?;
        if !out.status.success() {
            return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    fn candidates_dir(m: &std::collections::BTreeMap<String, String>) -> Result<PathBuf, String> {
        let g = |k: &str| m.get(k).cloned().unwrap_or_default();
        let (kind, name, version) = (g("kind"), g("name"), g("version"));
        if !KINDS.contains(&kind.as_str()) || !safe(&name) || !safe(&version) {
            return Err("this workspace's .cove-source is not one Cove wrote".into());
        }
        Ok(home()?.join("cove-candidates").join(kind).join(name).join(version))
    }

    /// The pristine branch the checked-out work stands on: `upstream/<v>` for
    /// `work/<v>`, `upstream` for a workspace from before versions.
    pub fn upstream_of(branch: &str) -> String {
        match branch.strip_prefix("work/") {
            Some(v) => format!("upstream/{v}"),
            None => "upstream".into(),
        }
    }

    pub fn status(path: &str) -> Result<SourceStatus, String> {
        let (ws, m) = root(path)?;
        let g = |k: &str| m.get(k).cloned().unwrap_or_default();
        let branch = git(&ws, &["branch", "--show-current"])?.trim().to_owned();
        let up = upstream_of(&branch);
        let porcelain = git(&ws, &["status", "--porcelain=v1", "-uall"])?;
        let dirty = !porcelain.trim().is_empty();
        // The patch is the source folder's alone (dir=); the site around it
        // (module.r, CONTEXT.md) is the person's own and not part of it.
        let sub = g("dir");
        let scope = if sub.is_empty() { ".".to_owned() } else { sub.clone() };
        let mut changed: Vec<String> = git(&ws, &["diff", "--name-only", &up, "--", &scope])?.lines().map(str::to_owned).collect();
        changed.extend(porcelain.lines().filter_map(|l| l.get(3..)).map(|s| s.trim().to_owned()).filter(|f| sub.is_empty() || f.starts_with(&format!("{sub}/"))));
        changed.retain(|f| f != ".cove-source");
        changed.sort();
        changed.dedup();
        let checkpoints = git(&ws, &["log", "--format=%H%x09%cI%x09%s", &format!("{up}..HEAD")])?
            .lines()
            .filter_map(|l| {
                let mut f = l.splitn(3, '\t');
                Some(Checkpoint { sha: f.next()?.to_owned(), when: f.next()?.to_owned(), message: f.next().unwrap_or("").to_owned() })
            })
            .collect();
        let mut candidates: Vec<Candidate> = std::fs::read_dir(candidates_dir(&m)?)
            .map(|rd| rd.flatten().filter_map(|e| std::fs::read(e.path().join("candidate.json")).ok()).filter_map(|b| serde_json::from_slice(&b).ok()).collect())
            .unwrap_or_default();
        candidates.sort_by(|a, b| b.built_at.cmp(&a.built_at));
        let kind = g("kind");
        Ok(SourceStatus {
            root: ws.display().to_string(),
            buildable: matches!(kind.as_str(), "R" | "Python"),
            kind,
            name: g("name"),
            version: g("version"),
            origin: g("origin"),
            branch,
            changed,
            dirty,
            checkpoints,
            candidates,
        })
    }

    pub fn checkpoint(path: &str, message: &str) -> Result<String, String> {
        let (ws, _) = root(path)?;
        if git(&ws, &["status", "--porcelain=v1", "-uall"])?.trim().is_empty() {
            return Err("nothing has changed since the last checkpoint".into());
        }
        let msg = message.trim();
        let msg = if msg.is_empty() { "checkpoint" } else { msg };
        git(&ws, &["add", "-A"])?;
        git(&ws, &["commit", "-qm", msg])?;
        Ok(git(&ws, &["rev-parse", "HEAD"])?.trim().to_owned())
    }

    pub const JOB: &str = "candidate";

    pub fn build(path: &str) -> Result<crate::api::JobView, String> {
        let st = status(path)?;
        if !st.buildable {
            return Err(format!("a {} star's candidate is built by its own adapter, which comes next - for now its workspace is source to read and change", st.kind));
        }
        let ws = PathBuf::from(&st.root);
        if st.dirty {
            checkpoint(path, "before building a candidate")?;
        }
        let sha = git(&ws, &["rev-parse", "HEAD"])?.trim().to_owned();
        let upstream = git(&ws, &["rev-parse", &upstream_of(&st.branch)])?.trim().to_owned();
        let (_, m) = root(path)?;
        let cand = candidates_dir(&m)?.join(&sha[..12.min(sha.len())]);
        // Only the source folder is the package (dir=); a project from before
        // is its whole tree.
        let tree = match m.get("dir").map(|d| d.trim()).filter(|d| !d.is_empty()) {
            Some(d) if safe(d) => format!("HEAD:{d}"),
            Some(d) => return Err(format!("{d}: not a source folder this can build")),
            None => "HEAD".to_owned(),
        };
        let module = crate::host::saved_settings().module.trim().to_owned();
        let prefix = if module.is_empty() { String::new() } else { crate::kernel::module_prefix(&module)? };
        let q = crate::api::shell_quote;
        let (c, w, n) = (q(&cand.display().to_string()), q(&ws.display().to_string()), q(&st.name));
        let steps = if st.kind == "R" {
            format!(
                "echo \"* R CMD build\"; R CMD build --no-build-vignettes --no-manual src; \
                 art=\"$(ls -1 {n}_*.tar.gz | head -n1)\"; [ -n \"$art\" ] || {{ echo 'no package tarball was built'; exit 1; }}; \
                 echo \"* R CMD INSTALL into the candidate's own library\"; R CMD INSTALL --no-docs -l lib \"$art\""
            )
        } else {
            "echo \"* pip wheel\"; python3 -m pip wheel --no-deps -w dist src; \
             art=\"$(ls -1 dist/*.whl | head -n1)\"; [ -n \"$art\" ] || { echo 'no wheel was built'; exit 1; }; \
             echo \"* pip install into the candidate's own site folder\"; python3 -m pip install --no-deps --no-compile --target lib \"$art\"".to_owned()
        };
        let cmd = format!(
            "set -e; rm -rf {c}; mkdir -p {c}/src {c}/lib; git -C {w} archive {tree} | tar -x -C {c}/src; rm -f {c}/src/CONTEXT.md {c}/src/.cove-source {c}/src/.ndexr-env {c}/src/module.r; cd {c}; \
             echo \"* {kind} {name} {version} at {short} in {env}\"; {prefix} {steps}; \
             sha256sum \"$art\" > artifact.sha256; echo \"* built: $art\"",
            kind = st.kind,
            name = st.name,
            version = st.version,
            short = &sha[..12.min(sha.len())],
            env = if module.is_empty() { "the R and python3 on PATH".to_owned() } else { module.clone() },
            tree = q(&tree),
        );
        let rec = Candidate {
            dir: cand.display().to_string(),
            kind: st.kind.clone(),
            name: st.name.clone(),
            version: st.version.clone(),
            commit: sha,
            upstream,
            origin: st.origin.clone(),
            env: module,
            built_at: chrono_now(),
            lib: cand.join("lib").display().to_string(),
            ..Default::default()
        };
        let done: crate::stack::jobs::Done = Box::new(move |ok, view| {
            let mut r = rec;
            r.ok = ok;
            if let Ok(t) = std::fs::read_to_string(Path::new(&r.dir).join("artifact.sha256")) {
                let mut f = t.split_whitespace();
                r.sha256 = f.next().unwrap_or("").to_owned();
                r.artifact = f.next().map(|a| Path::new(&r.dir).join(a).display().to_string()).unwrap_or_default();
            }
            match serde_json::to_vec_pretty(&r).map(|b| std::fs::write(Path::new(&r.dir).join("candidate.json"), b)) {
                Ok(Ok(())) => view.lines.push(format!("* recorded {}/candidate.json", r.dir)),
                _ => view.lines.push("* could not record candidate.json".into()),
            }
        });
        crate::stack::jobs::start(JOB, format!("candidate {} {}", st.name, st.version), cmd, Some(done))
    }

    /// The snippet in two fresh processes: as published, then with the
    /// candidate ahead of it; each first says which copy it loaded.
    pub fn test(dir: &str, code: &str) -> Result<crate::api::JobView, String> {
        let d = under("cove-candidates", dir)?;
        let c: Candidate = serde_json::from_slice(&std::fs::read(d.join("candidate.json")).map_err(|_| "not a candidate folder".to_owned())?).map_err(|e| e.to_string())?;
        if !c.ok {
            return Err("this candidate did not build".into());
        }
        let code = code.trim();
        if code.is_empty() || code.len() > 8000 {
            return Err("say what to run - a short snippet".into());
        }
        let prefix = if c.env.is_empty() { String::new() } else { crate::kernel::module_prefix(&c.env)? };
        let q = crate::api::shell_quote;
        let (lib, dq, n) = (q(&c.lib), q(&d.display().to_string()), q(&c.name));
        let (file, run, which, with) = if c.kind == "R" {
            ("test.R", "Rscript --no-init-file test.R", format!("Rscript --no-init-file -e 'p <- commandArgs(TRUE)[1]; cat(\"loaded\", p, format(packageVersion(p)), \"from\", dirname(find.package(p)), \"\\n\")' {n}"), format!("R_LIBS={lib}"))
        } else {
            ("test.py", "python3 test.py", String::new(), format!("PYTHONPATH={lib}${{PYTHONPATH:+:$PYTHONPATH}}"))
        };
        std::fs::write(d.join(file), code).map_err(|e| e.to_string())?;
        let cmd = format!(
            "cd {dq}; {prefix} echo \"── as published ({env})\"; {which}; {run} || echo \"(exit $?)\"; \
             echo; echo \"── with candidate {short}\"; export {with}; {which}; {run} || echo \"(exit $?)\"",
            env = if c.env.is_empty() { "PATH" } else { c.env.as_str() },
            short = &c.commit[..12.min(c.commit.len())],
        );
        crate::stack::jobs::start(JOB, format!("test {} candidate", c.name), cmd, None)
    }

    fn chrono_now() -> String {
        crate::runner::capture("date -u +%Y-%m-%dT%H:%M:%SZ").map(|s| s.trim().to_owned()).unwrap_or_default()
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub use work::{build, checkpoint, status, test, JOB};

// ── the third branch: an environment revision from accepted candidates ──────
// (operator, 2026-10-17: "a successful customization becomes a new, preserved
// environment ... revision" - "published environments never change".) A
// revision is an Lmod module of the person's own, written once and never
// rewritten: cove-env/<base>/<stamp>-<digest> loads the published base
// environment as it is, then puts each accepted candidate's library ahead of
// its own (R_LIBS_SITE, PYTHONPATH). revision.json beside it records the base,
// every candidate (commit, upstream, origin, artifact sha256) and the revision
// it was derived from. Made while the session is already in a revision, the
// new one carries that one's candidates forward - a chain of customizations,
// each still reopenable, the published environment always one pick away.

/// A recorded environment revision.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Revision {
    /// The Lmod module that enters it: cove-env/<base>/<version>.
    pub module: String,
    /// The published environment it stands on (env/r4.6_py3.13-release).
    pub base: String,
    /// The revision it was made from, if any.
    pub derived_from: String,
    /// Each component it replaces, newest last; one per kind + name.
    pub candidates: Vec<Candidate>,
    pub created_at: String,
    pub created_by: String,
    /// sha256 of the base and every candidate's commit and artifact digest:
    /// what the revision is, whatever it is named.
    pub digest: String,
}

/// Where a person's revisions live, and the modules that enter them.
#[cfg(not(target_arch = "wasm32"))]
pub fn envs_dir() -> Option<std::path::PathBuf> {
    std::env::var("HOME").ok().map(|h| std::path::Path::new(&h).join("cove-envs"))
}

/// `module use` for the person's revisions, when they have any - added to
/// every module load and listing (envs.rs, host.rs), so a revision is entered
/// and picked like any other environment.
#[cfg(not(target_arch = "wasm32"))]
pub fn module_use_line() -> String {
    let mut line = match envs_dir().map(|d| d.join("modules")).filter(|d| d.is_dir()) {
        Some(d) => format!("module use {} >/dev/null 2>&1; ", crate::api::shell_quote(&d.display().to_string())),
        None => String::new(),
    };
    // The single-language env/ modules a host without them yet gets from
    // scripts/env-modules-local.py - after the stack's own, so its published
    // modules win once a republish brings them.
    if let Some(d) = env_local_modules() {
        line.push_str(&format!("module use --append {} >/dev/null 2>&1; ", crate::api::shell_quote(&d.display().to_string())));
    }
    line
}

/// /opt/ndexr/env-local/<triple>/eb/modules/all, where it is on this machine.
#[cfg(not(target_arch = "wasm32"))]
pub fn env_local_modules() -> Option<std::path::PathBuf> {
    let d = std::path::Path::new("/opt/ndexr/env-local").join(crate::pkgflow::this_triple()).join("eb/modules/all");
    d.is_dir().then_some(d)
}

/// cove-env/<base>/<version> → its revision.json.
#[cfg(not(target_arch = "wasm32"))]
fn revision_record(module: &str) -> Option<Revision> {
    let rest = module.strip_prefix("cove-env/")?;
    if !safe(rest) {
        return None;
    }
    let f = envs_dir()?.join("revisions").join(format!("{rest}.json"));
    serde_json::from_slice(&std::fs::read(f).ok()?).ok()
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
/// The base's short name in a module path: env/r4.6_py3.13-release →
/// r4.6_py3.13-release; env/R/4.6.1-bioc3.23 → R-4.6.1-bioc3.23.
fn base_short(base: &str) -> String {
    base.trim_start_matches("env/").replace('/', "-")
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
/// Lua for a revision's module: the base, then each candidate ahead of it.
fn module_text(r: &Revision) -> String {
    let mut s = format!(
        "-- {module}: a Cove environment revision, written once (Cove source.rs).\n\
         -- {base}, with {n} customised component(s) ahead of its own; derived from {from}.\n\
         -- Its record: revision.json under ~/cove-envs/revisions/.\n\
         whatis(\"Cove environment revision of {base}\")\n\
         load(\"{base}\")\n",
        module = r.module,
        base = r.base,
        n = r.candidates.len(),
        from = if r.derived_from.is_empty() { "the published environment" } else { r.derived_from.as_str() },
    );
    for c in &r.candidates {
        let var = if c.kind == "R" { "R_LIBS_SITE" } else { "PYTHONPATH" };
        let short = &c.commit[..12.min(c.commit.len())];
        s.push_str(&format!("-- {} {} at {} (sha256 {})\nprepend_path(\"{var}\", \"{}\")\n", c.name, c.version, short, c.sha256, c.lib));
    }
    s.push_str(&format!("setenv(\"NDEXR_ENV_REVISION\", \"{}\")\nsetenv(\"NDEXR_ENV_REVISION_DIGEST\", \"{}\")\n", r.module, r.digest));
    s
}

/// Make a revision from an accepted candidate, on the published environment
/// it was built in - or, when the session is in a revision of that same
/// environment, on top of that revision's candidates.
#[cfg(not(target_arch = "wasm32"))]
pub fn revise(dir: &str) -> Result<Revision, String> {
    let d = std::path::Path::new(dir.trim()).canonicalize().map_err(|e| format!("{dir}: {e}"))?;
    let home = envs_dir().and_then(|e| e.parent().map(|h| h.join("cove-candidates"))).ok_or("no home folder")?;
    if !d.starts_with(home.canonicalize().map_err(|_| "no candidates yet".to_owned())?) {
        return Err(format!("{}: not one of your candidates", d.display()));
    }
    let c: Candidate = serde_json::from_slice(&std::fs::read(d.join("candidate.json")).map_err(|_| "not a candidate folder".to_owned())?).map_err(|e| e.to_string())?;
    if !c.ok {
        return Err("this candidate did not build".into());
    }
    if c.env.is_empty() || !c.env.starts_with("env/") {
        return Err("this candidate was built outside an environment - pick one in the toolbar and build it again".into());
    }
    let session = crate::host::saved_settings().module.trim().to_owned();
    let (base, mut carried, derived_from) = match revision_record(&session) {
        Some(r) if r.base == c.env => (r.base, r.candidates, r.module),
        Some(r) => return Err(format!("this candidate was built in {}, and the session is in a revision of {} - build it there, or pick {} first", c.env, r.base, c.env)),
        None => (c.env.clone(), Vec::new(), String::new()),
    };
    carried.retain(|o| !(o.kind == c.kind && o.name == c.name));
    carried.push(c);
    let mut h = std::process::Command::new("sha256sum").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    {
        use std::io::Write;
        let mut w = h.stdin.take().ok_or("sha256sum")?;
        let _ = writeln!(w, "{base}");
        for x in &carried {
            let _ = writeln!(w, "{} {} {} {}", x.kind, x.name, x.commit, x.sha256);
        }
    }
    let out = h.wait_with_output().map_err(|e| e.to_string())?;
    let digest = String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap_or("").to_owned();
    let stamp = crate::runner::capture("date -u +%Y%m%d-%H%M%S").map(|s| s.trim().to_owned()).unwrap_or_default();
    let short = base_short(&base);
    let version = format!("{stamp}-{}", &digest[..8.min(digest.len())]);
    let r = Revision {
        module: format!("cove-env/{short}/{version}"),
        base,
        derived_from,
        candidates: carried,
        created_at: crate::runner::capture("date -u +%Y-%m-%dT%H:%M:%SZ").map(|s| s.trim().to_owned()).unwrap_or_default(),
        created_by: std::env::var("USER").unwrap_or_default(),
        digest,
    };
    let root = envs_dir().ok_or("no home folder")?;
    let rec = root.join("revisions").join(&short).join(format!("{version}.json"));
    let lua = root.join("modules").join("cove-env").join(&short).join(format!("{version}.lua"));
    for f in [&rec, &lua] {
        std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    }
    // Written once: never over an existing revision.
    let write_new = |f: &std::path::Path, b: &[u8]| -> Result<(), String> {
        use std::io::Write;
        let mut o = std::fs::OpenOptions::new().write(true).create_new(true).open(f).map_err(|e| format!("{}: {e}", f.display()))?;
        o.write_all(b).map_err(|e| e.to_string())
    };
    write_new(&rec, &serde_json::to_vec_pretty(&r).map_err(|e| e.to_string())?)?;
    write_new(&lua, module_text(&r).as_bytes())?;
    Ok(r)
}


#[cfg(test)]
mod tests {
    #[test]
    fn a_project_keeps_its_source_in_a_folder_of_its_name() {
        assert_eq!(super::source_folder("R", "data.table"), "data.table");
        assert_eq!(super::source_folder("easybuild", "BLAST+"), "BLAST+");
        assert_eq!(super::source_folder("prefix", "sys-devel/gcc"), "gcc");
        // Never a path out of the project, never a hidden folder.
        assert_eq!(super::source_folder("Python", "../x"), "source");
        let t = super::module_template("data-table.r.u.ndexr.io");
        assert!(t.contains("timing$measured(\"data-table.r.u.ndexr.io\"") && !t.contains("__HOST__"));
    }

    use super::*;

    #[test]
    fn a_revision_loads_its_base_then_puts_its_candidates_first() {
        let c = |kind: &str, name: &str, lib: &str| Candidate { kind: kind.into(), name: name.into(), version: "1".into(), commit: "0123456789abcdef".into(), sha256: "ab".into(), lib: lib.into(), ok: true, ..Default::default() };
        let r = Revision { module: "cove-env/r4.6_py3.13-release/20261017-000000-deadbeef".into(), base: "env/r4.6_py3.13-release".into(), candidates: vec![c("R", "abind", "/h/c/R/lib"), c("Python", "cmdstanpy", "/h/c/P/lib")], ..Default::default() };
        let t = module_text(&r);
        let at = |x: &str| t.find(x).unwrap();
        assert!(at("load(\"env/r4.6_py3.13-release\")") < at("prepend_path(\"R_LIBS_SITE\", \"/h/c/R/lib\")"));
        assert!(t.contains("prepend_path(\"PYTHONPATH\", \"/h/c/P/lib\")"));
        assert_eq!(base_short("env/R/4.6.1-bioc3.23"), "R-4.6.1-bioc3.23");
        assert!(crate::kernel::valid_module(&r.module));
    }

    /// The whole second branch against a real R: a workspace, an edit, a
    /// checkpoint, a candidate built and recorded, and the two-process test
    /// telling the published copy from the candidate. Ignored by default: it
    /// compiles a package and points HOME elsewhere for the run.
    /// `cargo test candidate_loop -- --ignored --test-threads=1`
    #[cfg(unix)]
    #[test]
    #[ignore]
    fn candidate_loop() {
        let home = std::env::temp_dir().join(format!("cove-cand-home-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        let ws = home.join("cove-source/R/covetiny/0.1");
        std::fs::create_dir_all(ws.join("R")).unwrap();
        std::fs::write(ws.join("DESCRIPTION"), "Package: covetiny\nVersion: 0.1\nTitle: Tiny\nDescription: Tiny.\nAuthor: Cove\nMaintainer: Cove <c@ndexr.io>\nLicense: MIT\n").unwrap();
        std::fs::write(ws.join("NAMESPACE"), "export(hi)\n").unwrap();
        std::fs::write(ws.join("R/hi.R"), "hi <- function() 'upstream'\n").unwrap();
        std::fs::write(ws.join(".cove-source"), "kind=R\nname=covetiny\nversion=0.1\norigin=test\n").unwrap();
        std::env::set_var("HOME", &home);
        let g = |a: &[&str]| work::git(&ws, a).unwrap();
        g(&["-c", "init.defaultBranch=upstream", "init", "-q"]);
        g(&["add", "-A"]);
        g(&["commit", "-qm", "upstream"]);
        g(&["checkout", "-qb", "work"]);
        let p = ws.display().to_string();
        assert!(checkpoint(&p, "").is_err(), "nothing changed yet");
        std::fs::write(ws.join("R/hi.R"), "hi <- function() 'customised'\n").unwrap();
        let st = status(&p).unwrap();
        assert!(st.dirty && st.changed == ["R/hi.R"] && st.buildable, "{st:?}");
        checkpoint(&p, "say customised").unwrap();
        assert_eq!(status(&p).unwrap().checkpoints[0].message, "say customised");
        let wait = || loop {
            let v = crate::stack::jobs::view(JOB);
            if !v.running && v.ok.is_some() {
                return v;
            }
            std::thread::sleep(std::time::Duration::from_millis(300));
        };
        build(&p).unwrap();
        let v = wait();
        assert_eq!(v.ok, Some(true), "{}", v.lines.join("\n"));
        let c = status(&p).unwrap().candidates.remove(0);
        assert!(c.ok && c.sha256.len() == 64 && c.artifact.ends_with("covetiny_0.1.tar.gz"), "{c:?}");
        test(&c.dir, "cat(covetiny::hi(), '\\n')").unwrap();
        let v = wait();
        let out = v.lines.join("\n");
        assert!(out.contains("with candidate") && out.contains("customised"), "{out}");
        let _ = std::fs::remove_dir_all(&home);
    }

    /// One identity, one project: found again wherever it is; a name held by
    /// another identity or by any other domain is passed over, never taken.
    #[cfg(unix)]
    #[test]
    fn an_identity_reopens_its_own_project() {
        let root = std::env::temp_dir().join(format!("cove-alloc-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let dt = crate::names::Identity::of("R", "data.table").unwrap();
        let (h, d) = allocate(&root, "u.ndexr.io", &dt).unwrap();
        assert_eq!(h, "data-table.r.u.ndexr.io");
        assert_eq!(allocate(&root, "u.ndexr.io", &dt).unwrap(), (h.clone(), d));
        // An unrelated domain already at the readable name of another identity.
        std::fs::create_dir_all(root.join("blast-plus.eb.u.ndexr.io")).unwrap();
        std::fs::write(root.join("blast-plus.eb.u.ndexr.io/module.r"), "").unwrap();
        let b = crate::names::Identity::of("easybuild", "BLAST+").unwrap();
        let (hb, _) = allocate(&root, "u.ndexr.io", &b).unwrap();
        assert!(hb.starts_with("blast-plus-") && hb.ends_with(".eb.u.ndexr.io") && hb != "blast-plus.eb.u.ndexr.io", "{hb}");
        assert_eq!(allocate(&root, "u.ndexr.io", &b).unwrap().0, hb, "stable once allocated");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_plain_names_reach_a_script() {
        assert!(safe("sys-devel/gnuconfig") && safe("2.42-GCCcore-13.3.0") && safe("data.table") && safe("1.4-8"));
        assert!(!safe("") && !safe("../etc") && !safe("a/../b") && !safe("x y") && !safe("x;rm") && !safe("/abs") && !safe("$(id)"));
    }

    #[test]
    fn bioconductor_releases_come_from_the_sets() {
        let sets: Vec<String> = ["R/4.6.1-bioc3.23", "R/4.6.1-bioc3.24", "r4.6_py3.13-release", "Python/3.13.15"].iter().map(|s| s.to_string()).collect();
        assert_eq!(biocs(&sets), ["3.24", "3.23"]);
        assert!(biocs(&[]).is_empty());
    }

    /// The script against a fake stack: an EasyBuild module and a Prefix
    /// ebuild with its distfile land as `upstream`, with `work` checked out.
    #[cfg(unix)]
    #[test]
    fn a_star_lands_as_upstream_with_work_checked_out() {
        let root = std::env::temp_dir().join(format!("cove-source-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let stack = root.join("stack/x86_64-pc-linux-gnu");
        let eb = stack.join("eb/software/zlib/1.3.1/easybuild");
        std::fs::create_dir_all(&eb).unwrap();
        std::fs::write(eb.join("zlib-1.3.1.eb"), "name = 'zlib'\n").unwrap();
        std::fs::write(eb.join("easybuild-zlib-1.log"), "log\n").unwrap();
        let pre = stack.join("prefix/x86_64-pc-linux-gnu");
        let tree = pre.join("var/db/repos/gentoo/sys-libs/zlib");
        std::fs::create_dir_all(&tree).unwrap();
        std::fs::write(tree.join("zlib-1.3.1.ebuild"), "EAPI=8\n").unwrap();
        std::fs::write(tree.join("Manifest"), "DIST zlib-1.3.1.tar.xz 1 BLAKE2B x\n").unwrap();
        let dist = pre.join("var/cache/distfiles");
        std::fs::create_dir_all(dist.join("zlib-1.3.1")).unwrap();
        std::fs::write(dist.join("zlib-1.3.1/zlib.c"), "int main(){}\n").unwrap();
        assert!(std::process::Command::new("tar").args(["-cJf", "zlib-1.3.1.tar.xz", "zlib-1.3.1"]).current_dir(&dist).status().unwrap().success());
        let script = root.join("source.sh");
        std::fs::write(&script, FETCH).unwrap();
        let run = |kind: &str, name: &str, ver: &str| {
            let ws = root.join("ws").join(kind).join(name).join(ver);
            let out = std::process::Command::new("bash")
                .arg(&script)
                .args([kind, name, ver, &ws.display().to_string(), &stack.display().to_string(), ""])
                .env("XDG_CACHE_HOME", root.join("cache"))
                .output()
                .unwrap();
            (ws, String::from_utf8_lossy(&out.stdout).into_owned())
        };
        let (ws, out) = run("easybuild", "zlib", "1.3.1");
        assert!(out.contains("COVE_SOURCE ok new"), "{out}");
        assert!(ws.join("zlib-1.3.1.eb").is_file() && !ws.join("easybuild-zlib-1.log").exists());
        let git = |ws: &std::path::Path, a: &[&str]| String::from_utf8_lossy(&std::process::Command::new("git").arg("-C").arg(ws).args(a).output().unwrap().stdout).trim().to_owned();
        assert_eq!(git(&ws, &["branch", "--show-current"]), "work/1.3.1");
        let (_, again) = run("easybuild", "zlib", "1.3.1");
        assert!(again.contains("COVE_SOURCE ok existing"), "{again}");
        // A second version joins the same project as two more branches; the
        // first's pending work is checkpointed, then switched back to.
        let eb2 = stack.join("eb/software/zlib/1.3.2/easybuild");
        std::fs::create_dir_all(&eb2).unwrap();
        std::fs::write(eb2.join("zlib-1.3.2.eb"), "name = 'zlib'\nversion = '1.3.2'\n").unwrap();
        std::fs::write(ws.join("zlib-1.3.1.eb"), "name = 'zlib'  # mine\n").unwrap();
        let script2 = |ver: &str| {
            let out = std::process::Command::new("bash")
                .arg(&script)
                .args(["easybuild", "zlib", ver, &ws.display().to_string(), &stack.display().to_string(), "", "zlib.eb.u.ndexr.io", "eb", "d", "ndexr-easyconfigs", "zlib", ""])
                .env("XDG_CACHE_HOME", root.join("cache"))
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).into_owned()
        };
        let out = script2("1.3.2");
        assert!(out.contains("COVE_SOURCE ok new"), "{out}");
        assert_eq!(git(&ws, &["branch", "--show-current"]), "work/1.3.2");
        assert!(ws.join("zlib-1.3.2.eb").is_file() && !ws.join("zlib-1.3.1.eb").exists());
        let ctx = std::fs::read_to_string(ws.join("CONTEXT.md")).unwrap();
        assert!(ctx.starts_with("# zlib · zlib.eb.u.ndexr.io") && ctx.contains("opened in: the session environment"), "{ctx}");
        assert!(git(&ws, &["log", "-1", "--format=%s", "work/1.3.1"]).starts_with("checkpoint before opening 1.3.2"));
        assert!(script2("1.3.1").contains("COVE_SOURCE ok existing"));
        assert_eq!(git(&ws, &["branch", "--show-current"]), "work/1.3.1");
        assert!(std::fs::read_to_string(ws.join("zlib-1.3.1.eb")).unwrap().contains("# mine"));
        assert_eq!(work::upstream_of("work/1.3.1"), "upstream/1.3.1");
        let (ws, out) = run("prefix", "sys-libs/zlib", "1.3.1");
        assert!(out.contains("COVE_SOURCE ok new"), "{out}");
        assert!(ws.join("ebuild/zlib-1.3.1.ebuild").is_file() && ws.join("src/zlib-1.3.1/zlib.c").is_file(), "{out}");
        let (ws, out) = run("easybuild", "nope", "1.0");
        assert!(out.contains("COVE_SOURCE fail") && !ws.exists(), "{out}");
        let _ = std::fs::remove_dir_all(&root);
    }
}
