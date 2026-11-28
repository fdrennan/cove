//! The machine side: answers a Query by looking at this machine. The desktop
//! app calls it in-process (backend.rs, Local); `cove serve` calls it
//! for /api/q (server.rs). Native only — a browser has no machine to look at.

// What the dispatcher answers with: each subsystem of this machine.
pub(crate) mod account;
pub(crate) mod apps;
pub(crate) mod boxcomplete;
pub(crate) mod boxes;
pub(crate) mod brief;
pub(crate) mod envs;
pub(crate) mod git;
pub(crate) mod kernel;
#[cfg(unix)]
pub(crate) mod limits;
pub(crate) mod localagent;
pub(crate) mod lsp;
pub(crate) mod messages;
pub(crate) mod patches;
pub(crate) mod pkgflow;
pub(crate) mod recap;
#[cfg(unix)]
pub(crate) mod record;
pub(crate) mod runner;
#[cfg(unix)]
pub(crate) mod shares;
pub(crate) mod stack;
pub(crate) mod stacks;
pub(crate) mod storage;
pub(crate) mod update;
pub(crate) mod vitals;
pub(crate) mod zippack;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use serde_json::Value;

use crate::api::{b64_encode, shell_quote, DirEntry, FileText, JobView, Lang, Listing, Modules, Package, PlotData, Probe, Query, Settings, SettingsView, StackView};
use crate::host::runner::{capture, capture_within};

/// Files the source pane opens: text, and not huge.
const MAX_FILE: u64 = 5 << 20;

pub fn answer(q: &Query) -> Result<Value, String> {
    let v = match q {
        Query::Probe => serde_json::to_value(probe()),
        Query::Settings => {
            let v = settings_view();
            // Resolved before the first console asks (envs.rs; once per stack revision).
            crate::host::envs::warm(&v.settings.module);
            serde_json::to_value(v)
        }
        Query::SaveSettings { settings } => {
            save_settings(settings)?;
            serde_json::to_value(settings_view())
        }
        Query::PinEnv { dir, modules } => {
            let dir = resolve(dir)?;
            crate::host::envs::pin(&dir, modules)?;
            serde_json::to_value(crate::host::envs::pinned(&dir).unwrap_or_default())
        }
        Query::Modules => serde_json::to_value(modules()),
        Query::Eval { lang, code, at } => {
            kernel::eval_at(*lang, code, at.as_ref())?;
            serde_json::to_value(kernel::poll(*lang, usize::MAX))
        }
        Query::Poll { lang, since } => serde_json::to_value(kernel::poll(*lang, *since)),
        Query::Stop { lang } => {
            kernel::stop(*lang);
            serde_json::to_value(kernel::poll(*lang, usize::MAX))
        }
        Query::Complete { lang, line, cursor } => serde_json::to_value(kernel::complete(*lang, line, *cursor)?),
        Query::CodeComplete { path, lang, line, col, text, row } => serde_json::to_value(code_complete(path, lang, line, *col, text, *row)?),
        Query::Assist { lang, ask, context } => serde_json::to_value(assist(*lang, ask, context)?),
        Query::Interrupt { lang } => serde_json::to_value(kernel::interrupt(*lang)?),
        Query::Restart { lang } => {
            kernel::restart(*lang)?;
            serde_json::to_value(kernel::poll(*lang, usize::MAX))
        }
        Query::Plot { lang, n } => serde_json::to_value(PlotData { png_b64: b64_encode(&kernel::plot(*lang, *n)?) }),
        Query::Packages { lang } => serde_json::to_value(packages(*lang)?),
        Query::View { lang, name } => {
            kernel::view(*lang, name)?;
            serde_json::to_value(kernel::poll(*lang, usize::MAX))
        }
        Query::StackSetup => serde_json::to_value(stack::setup(&saved_settings().cvmfs)?),
        Query::StackBuild { report } => serde_json::to_value(stack::build(*report)?),
        Query::StackApply { cvmfs } => serde_json::to_value(stack::apply(cvmfs)?),
        Query::StackTest { proxy } => serde_json::to_value(stack::test(proxy)?),
        Query::StackJob => serde_json::to_value(stack::jobs::view("stack")),
        Query::StackPkgs { set } => serde_json::to_value(stack::pkgs_sync(set)?),
        Query::LocalInstall => serde_json::to_value(stack::local_install()),
        Query::Render { path } => serde_json::to_value(render(path)?),
        Query::RenderJob => serde_json::to_value(stack::jobs::view("render")),
        Query::ListDomains => serde_json::to_value(list_domains()),
        Query::Inbox { all, chat, find } => serde_json::to_value(crate::host::messages::inbox(&session_owner(), *all, chat, find)?),
        Query::InboxAct { act } => Ok(crate::host::messages::act(&session_owner(), act)?),
        Query::ProjectNames { names } => serde_json::to_value(crate::host::messages::project_names(&session_owner(), names)?),
        Query::DeployApp { .. } | Query::DeployJob { .. } | Query::AppRoute { .. } | Query::ProjectRelease { .. } | Query::AppAccess { .. } | Query::SitePassword { .. } => return Err("apps are routed from cove.ndexr.io, where they are served".into()),
        // Build / Up / Down (apps.rs): the project's own scripts, run here as
        // the person - in a cove.ndexr.io session and on a desktop alike.
        Query::AppStatus { host } => serde_json::to_value(crate::host::apps::status(Path::new(&app_dir(host)?))),
        Query::AppAction { host, verb } => serde_json::to_value(crate::host::apps::action(host, Path::new(&app_dir(host)?), verb)?),
        Query::AppType { host, language } => serde_json::to_value(app_type(Path::new(&app_dir(host)?), language)?),
        Query::AppJob { host } => serde_json::to_value(crate::host::apps::job(host)),
        Query::Versions { host } => serde_json::to_value(versions_at(Path::new(&app_dir(host)?))?),
        Query::VersionRestore { host, sha } => serde_json::to_value(version_restore_at(Path::new(&app_dir(host)?), sha, &session_owner())?),
        Query::Image { host, verb } => serde_json::to_value(image_at(&session_owner(), Path::new(&app_dir(host)?), host, verb)?),
        #[cfg(unix)]
        Query::Record { host, capture } => serde_json::to_value(crate::host::record::view(Path::new(&app_dir(host)?), *capture)),
        #[cfg(not(unix))]
        Query::Record { .. } => return Err("a project's record is read on Linux".into()),
        Query::GitChanges { host, sha } => serde_json::to_value(git::changes_at(Path::new(&app_dir(host)?), sha)?),
        Query::GitDiff { host, sha, path, old, area, full } => serde_json::to_value(git::diff_at(Path::new(&app_dir(host)?), sha, path, old, area, *full)?),
        Query::GitStatus { host } => serde_json::to_value(git::status_at(Path::new(&app_dir(host)?))?),
        Query::GitFileAt { host, sha, path } => serde_json::to_value(git::file_at(Path::new(&app_dir(host)?), sha, path)?),
        Query::GitPatch { host, action, patch } => serde_json::to_value(git::patch_at(Path::new(&app_dir(host)?), action, patch)?),
        Query::RunCommand { dir, command, timeout_secs } => Ok(crate::host::runner::run_command(&resolve(dir)?.display().to_string(), command, *timeout_secs)?),
        // Connectors live on the multi-person server, which answers these itself.
        Query::McpGrants | Query::McpRevoke { .. } => return Err("Claude connectors are kept by cove.ndexr.io's server".into()),
        Query::Easyconfig { name, version } => serde_json::to_value(easyconfig(name, version)?),
        Query::DmFile { id } => serde_json::to_value(crate::host::messages::dm_file(&session_owner(), *id)?),
        Query::Serving { apexes, targets } => {
            if !admin_cached() {
                return Err("the serving path is for ndexr admins".into());
            }
            serde_json::to_value(serving(apexes, targets))
        }
        // A desktop or single-person server: the cycle as this machine sees
        // it, no sessions (multiuser.rs answers it with them).
        Query::PackageFlow { set, package } => {
            let mut f = crate::host::pkgflow::flow(set, package);
            if let Ok(b) = builds_live() {
                f.live_machines = crate::host::pkgflow::live_machines(&b);
            }
            serde_json::to_value(f)
        }
        Query::Recap { minutes } => {
            if !ndexr_admin(&session_owner()) {
                return Err("the recap is for ndexr admins".into());
            }
            serde_json::to_value(crate::host::recap::recap(*minutes))
        }
        Query::Spoken { asked, reply } => serde_json::to_value(crate::host::brief::spoken(asked, reply)?),
        Query::ReadAloud { text } => serde_json::to_value(crate::host::brief::read_aloud(text)?),
        Query::RecapBrief { minutes, fresh } => {
            if !ndexr_admin(&session_owner()) {
                return Err("the briefing is for ndexr admins".into());
            }
            serde_json::to_value(crate::host::brief::brief(*minutes, *fresh))
        }
        Query::ConvertDomain { host, starter } => serde_json::to_value(convert_domain(host, starter)?),
        Query::CreateDomain { host, parent, rust, starter } => {
            let starter = match starter.as_str() {
                "" if *rust => crate::project::starters::RUST_APP,
                "" => crate::project::starters::SHINY_CHAOS,
                s => s,
            };
            // The host decides, not the client's guess (`parent`, the folder
            // of the open project): every new project goes to one folder.
            let _ = parent;
            let root = projects_root(&saved_settings());
            std::fs::create_dir_all(&root).map_err(|e| format!("cannot make the projects folder {}: {e}", root.display()))?;
            serde_json::to_value(create_domain(host, &root.display().to_string(), starter)?)
        }
        Query::PushAll { root } => serde_json::to_value(push_all(&own_domains().map(|p| p.display().to_string()).unwrap_or_else(|| root.clone()))?),
        Query::PushJob => serde_json::to_value(stack::jobs::view("push")),
        // Never from a session or the desktop app: only `cove multiuser`
        // answers these, for an ndexr admin (multiuser.rs on_behalf).
        Query::Boxes | Query::BoxAct { .. } | Query::BoxJob { .. } => return Err("the build machines are managed by admins signed in at cove.ndexr.io".into()),
        Query::HostVitals => return Err("the machine's vitals are for ndexr admins signed in at cove.ndexr.io".into()),
        Query::Archive { host } => serde_json::to_value(archive(host)?),
        Query::ArchiveJob => serde_json::to_value(stack::jobs::view("archive")),
        Query::SiteGate { host } => serde_json::to_value(site_gate(host)?),
        Query::AgentRun { domain, prompt, model, cwd, mode, .. } => serde_json::to_value(agent_run(domain, prompt, model, cwd.as_deref(), mode.as_deref())?),
        Query::AgentPoll { id, offset } => serde_json::to_value(agent_poll(id, *offset)?),
        Query::AgentStop { id } => serde_json::to_value(agent_stop(id)?),
        Query::AgentHistory { domain } => serde_json::to_value(agent_history(domain)?),
        Query::AgentBusy { domains } => serde_json::to_value(agent_busy(&session_owner(), domains)),
        Query::AppViewer { host, on } => serde_json::to_value(app_viewer(&host.trim().to_ascii_lowercase(), *on)?),
        Query::Term { id, offset, rows, cols } => serde_json::to_value(crate::ui::ide::term::pty::read(*id, *offset, *rows, *cols)?),
        Query::TermWrite { id, input } => serde_json::to_value(crate::ui::ide::term::pty::write(*id, input)?),
        Query::TermKill { id } => serde_json::to_value(crate::ui::ide::term::pty::kill(*id)),
        Query::ClaudeStatus => serde_json::to_value(claude_as(&session_owner(), "status", "")?),
        Query::ClaudeConnect { token } => serde_json::to_value(claude_as(&session_owner(), "connect", token)?),
        Query::ClaudeForget => serde_json::to_value(claude_as(&session_owner(), "forget", "")?),
        Query::OpenAiStatus => serde_json::to_value(localagent::codex::view()),
        Query::OpenAiConnect { key } => {
            localagent::codex::connect(key)?;
            serde_json::to_value(localagent::codex::view())
        }
        Query::OpenAiForget => {
            localagent::codex::forget();
            serde_json::to_value(localagent::codex::view())
        }
        Query::CodexInstall => serde_json::to_value(localagent::codex::install()?),
        Query::Account => serde_json::to_value(account_as(&session_owner(), "")?),
        Query::RevokeKey { key_id } => serde_json::to_value(account_as(&session_owner(), key_id)?),
        Query::SendRequest { domain, what, trying, blocked, files } => serde_json::to_value(send_request(domain, what, trying, *blocked, files)?),
        Query::ListDir { path } => serde_json::to_value(list_dir(path)?),
        Query::Grep { root, query, case, regex } => serde_json::to_value(grep(root, query, *case, *regex)?),
        Query::Glimpse { path } => serde_json::to_value(glimpse(path)?),
        Query::ReadFile { path } => serde_json::to_value(read_file(path)?),
        Query::AppLog { path, build } => serde_json::to_value(app_log(path, *build)?),
        Query::SiteRequests { host } => serde_json::to_value(site_requests(host)?),
        Query::Stamps { paths } => serde_json::to_value(paths.iter().take(500).map(|p| stamp(p)).collect::<Vec<String>>()),
        Query::SiteStamp { path } => serde_json::to_value(site_stamp(path)?),
        Query::SourceOpen { kind, name, version, sets, env } => serde_json::to_value(crate::catalog::source::open(kind, name, version, sets, env)?),
        Query::DomainClaim { hosts } => serde_json::to_value(claim_as(&session_owner(), hosts)?),
        Query::PublishDomain { host } => serde_json::to_value(publish_as(&session_owner(), host)?),
        Query::DomainRestore { host } => serde_json::to_value(restore_as(&session_owner(), host)?),
        Query::SourceStatus { path } => serde_json::to_value(crate::catalog::source::status(path)?),
        Query::SourceCheckpoint { path, message } => serde_json::to_value(crate::catalog::source::checkpoint(path, message)?),
        Query::CandidateBuild { path } => serde_json::to_value(crate::catalog::source::build(path)?),
        Query::CandidateTest { dir, code } => serde_json::to_value(crate::catalog::source::test(dir, code)?),
        Query::CandidateJob => serde_json::to_value(stack::jobs::view(crate::catalog::source::JOB)),
        Query::EnvRevise { dir } => serde_json::to_value(crate::catalog::source::revise(dir)?),
        Query::WriteFile { path, text } => serde_json::to_value(write_file(path, text)?),
        Query::DbSchema { path } => serde_json::to_value(crate::ui::ide::db::schema(path)?),
        Query::DbRows { path, table, offset, limit, filter, sort, desc, count } => {
            serde_json::to_value(crate::ui::ide::db::rows(path, table, *offset, *limit, filter, sort.as_deref(), *desc, *count)?)
        }
        Query::DbRow { path, table, rowid } => serde_json::to_value(crate::ui::ide::db::row(path, table, *rowid)?),
        Query::DbSql { path, sql } => serde_json::to_value(crate::ui::ide::db::sql(path, sql)?),
        Query::Storage { refresh } => {
            if *refresh {
                crate::host::storage::poke();
            }
            serde_json::to_value(crate::host::storage::mine())
        }
        #[cfg(unix)]
        Query::StorageClear => serde_json::to_value(crate::host::storage::clear()?),
        #[cfg(not(unix))]
        Query::StorageClear => return Err("nothing measures storage here".into()),
        Query::Manifests => serde_json::to_value(crate::catalog::manifest::host::list()),
        Query::Manifest { name } => serde_json::to_value(crate::catalog::manifest::host::detail(name)?),
        Query::InstallInfo => serde_json::to_value(crate::catalog::manifest::host::info()),
        Query::Install { install } => serde_json::to_value(crate::catalog::manifest::host::install(install)?),
        Query::InstallJob => serde_json::to_value(crate::catalog::manifest::host::job()),
        Query::InstallStop => serde_json::to_value(crate::catalog::manifest::host::stop()),
        Query::MyBuilds => serde_json::to_value(crate::catalog::manifest::host::my_builds()),
    };
    v.map_err(|e| e.to_string())
}

/// Where `ndexr aggregate`'s `software` spec writes the Software tab's catalog
/// (ndexr.ndexr.io crate/src/aggregate/software.rs): `<platform.state_dir>/software`.
pub fn software_dir() -> PathBuf {
    std::env::var_os("COVE_SOFTWARE_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/srv/ndexr/software"))
}

/// The catalog itself, for the desktop app reading this machine: the ndexr
/// host's own file; anywhere else the public one (cove.ndexr.io/view), kept
/// in the cache so the atlas and Packages work offline after one connection
/// (standalone, 2026-10-19). Fetched at most every two minutes.
pub fn software_catalog() -> Result<Vec<u8>, String> {
    if let Ok(b) = std::fs::read(software_dir().join("catalog.json")) {
        return Ok(b);
    }
    let cache = crate::host::kernel::cache_dir().join("catalog.json");
    if saved_settings().airgap {
        return std::fs::read(&cache).map_err(|_| "airgap mode: no software catalog on this machine - the atlas and Packages stay empty".to_owned());
    }
    let age = std::fs::metadata(&cache).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok());
    if age.is_some_and(|a| a < std::time::Duration::from_secs(120)) {
        if let Ok(b) = std::fs::read(&cache) {
            return Ok(b);
        }
    }
    let url = std::env::var("COVE_PUBLIC_CATALOG").unwrap_or_else(|_| "https://cove.ndexr.io/view/software.json".into());
    let _ = std::fs::create_dir_all(crate::host::kernel::cache_dir());
    let tmp = cache.with_extension("json.part");
    let got = Command::new("curl").args(["-fsSL", "--max-time", "30", "-o"]).arg(&tmp).arg(&url).status().is_ok_and(|s| s.success());
    if got && std::fs::rename(&tmp, &cache).is_ok() {
        return std::fs::read(&cache).map_err(|e| e.to_string());
    }
    let _ = std::fs::remove_file(&tmp);
    std::fs::read(&cache).map_err(|_| format!("no software catalog yet: {url} could not be reached - the atlas and Packages fill in once Cove has been online once"))
}

/// One query against the platform's database with its own login (.env), as
/// the root multiuser server can and a session cannot: stdout, or why not.
fn platform_psql(sql: &str) -> Result<Vec<u8>, String> {
    platform_psql_vars(sql, &[])
}

/// `platform_psql` with values: each `(name, value)` is a psql variable the
/// SQL names as `:'name'` (a quoted literal) - psql does the quoting, so
/// what a person typed never becomes SQL. The script goes in on stdin, the
/// only way psql interpolates variables; `-q` keeps INSERT/UPDATE tags out
/// of the answer, which is what the last statement selects.
/// Live Development on the public view (operator, 2026-11-20: ndexr.io as
/// "the public, real-time status dashboard for community-driven
/// development"): every requested change as the backend recorded it
/// (code_changes, written by `ndexr change`), and the publishing pipeline.
/// What an agent says it did is never shown - only states, risk levels,
/// counts and check results the code wrote. Sanitized here, in SQL: no
/// requester, no file paths, no diff; a domain that is not public in the
/// registry is "a private project", with no title.
pub fn development_status() -> Result<Vec<u8>, String> {
    const SQL: &str = r#"
WITH c AS (
  SELECT c.*, COALESCE(r.access_level = 'public', false) AS open
    FROM code_changes c LEFT JOIN domain_registry r ON r.domain = c.domain
   ORDER BY c.id DESC LIMIT 100)
SELECT json_build_object(
  'generated', now(),
  'counts', COALESCE((SELECT json_object_agg(status, n) FROM (SELECT status, count(*) AS n FROM code_changes GROUP BY status) s), '{}'::json),
  'changes', COALESCE((SELECT json_agg(json_build_object(
      'id', id, 'status', status, 'level', risk_level,
      'project', CASE WHEN open THEN domain ELSE 'a private project' END,
      'title', CASE WHEN open THEN left(split_part(request, E'
', 1), 120) END,
      'files', jsonb_array_length(files),
      'checks', (SELECT COALESCE(json_agg(json_build_object('name', x->>'name', 'ok', (x->>'ok')::boolean)), '[]'::json) FROM jsonb_array_elements(checks) x),
      'created_at', created_at, 'updated_at', updated_at, 'decided_at', decided_at,
      'installed', left(merged_commit, 12)) ORDER BY id DESC) FROM c), '[]'::json))"#;
    let out = platform_psql_vars(SQL, &[])?;
    let mut v: Value = serde_json::from_slice(&out).map_err(|e| format!("development status: {e}"))?;
    v["pipeline"] = development_pipeline();
    serde_json::to_vec(&v).map_err(|e| e.to_string())
}

/// The release this server publishes and the public source behind it, read
/// from the files run.sh and onprem/public-push.sh keep: the current release
/// link, and the push log's last line and the public clone's last commit.
fn development_pipeline() -> Value {
    let unix = |p: &Path| std::fs::symlink_metadata(p).ok().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
    let shared = Path::new("/usr/local/lib/cove/current");
    let release = std::fs::read_link(shared).ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
    let state = platform_root().parent().map(|h| h.join(".local/state/cove-onprem")).unwrap_or_default();
    let last_push = std::fs::read_to_string(state.join("public-push.log")).ok().and_then(|t| t.lines().rev().find(|l| l.contains(" pushed ") || l.contains(" HELD ") || l.contains(" failed")).map(str::to_owned)).unwrap_or_default();
    // The server runs as root and the clone is ubuntu's: git refuses it as
    // "dubious ownership" unless this one directory is named safe.
    let clone = state.join("public");
    let public = Command::new("git").arg("-c").arg(format!("safe.directory={}", clone.display())).arg("-C").arg(&clone).args(["log", "-1", "--format=%h%x09%ct%x09%s"]).output().ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
    let mut p = public.splitn(3, '\t');
    let (commit, at, subject) = (p.next().unwrap_or(""), p.next().and_then(|t| t.parse::<u64>().ok()), p.next().unwrap_or(""));
    serde_json::json!({
        "release": release,
        "release_at": unix(shared),
        "source": "https://github.com/fdrennan/cove",
        "source_commit": commit,
        "source_at": at,
        "source_release": subject.strip_prefix("Cove: release ").unwrap_or(subject),
        // Held or failed is shown as such: the page reports the pipeline, not a hope.
        "last_push": last_push.split_once(' ').map(|(_, r)| r.to_owned()).unwrap_or_default(),
        "push_held": last_push.contains(" HELD ") || last_push.contains(" failed"),
    })
}

pub(crate) fn platform_psql_vars(sql: &str, vars: &[(&str, &str)]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let env = std::fs::read_to_string(platform_root().join(".env")).map_err(|e| format!("no build database here ({e}) - the Builds views read it on the ndexr host"))?;
    let var = |k: &str| {
        env.lines()
            .filter_map(|l| l.trim().strip_prefix("export ").unwrap_or(l.trim()).split_once('='))
            .filter(|(n, _)| n.trim() == k)
            .map(|(_, v)| v.trim().trim_matches(['"', '\'']).to_owned())
            .last()
            .unwrap_or_default()
    };
    let port = Some(var("POSTGRES_DIRECT_PORT")).filter(|p| !p.is_empty()).unwrap_or_else(|| "5432".into());
    let mut cmd = Command::new("psql");
    cmd.args(["-h", "127.0.0.1", "-p", &port, "-U", &var("POSTGRES_USER"), "-d", &var("POSTGRES_DB"), "-qAtX", "-v", "ON_ERROR_STOP=1"]);
    for (k, v) in vars {
        // psql cuts a variable at a NUL; nothing typed needs one.
        cmd.arg("-v").arg(format!("{k}={}", v.replace('\0', "")));
    }
    let mut child = cmd
        .args(["-f", "-"])
        .env("PGPASSWORD", var("POSTGRES_PASSWORD"))
        .env("PGCONNECT_TIMEOUT", "5")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("psql: {e}"))?;
    child.stdin.take().ok_or("psql: no stdin")?.write_all(sql.as_bytes()).map_err(|e| format!("psql: {e}"))?;
    let out = child.wait_with_output().map_err(|e| format!("psql: {e}"))?;
    if !out.status.success() {
        return Err(format!("the build database did not answer: {}", String::from_utf8_lossy(&out.stderr).lines().next().unwrap_or("")));
    }
    Ok(out.stdout)
}

/// The domains `viewer` (an ndexr username; None = not signed in) may see
/// in the atlas's canopy - the same levels the platform's own gates enforce
/// (operator, 2026-10-18: "user levels admin, logged in, owner only, public
/// are consistent across what cove visualization shows ... sites with
/// password ... should never be shown"):
///
///   never      a site password (site_passwords), or archived
///   public     everyone, signed in or not (an unknown or empty level is
///              public, as registry.r `domain_access_level` reads it)
///   login      anyone signed in
///   admin      admins (admin_users.is_admin)
///   owner      only the owner - user_domains first, else
///              domain_registry.owner_email (registry.r `domain_owner`);
///              admins do not get through it either
///
///   shared     not otherwise visible, but its owner shared it with the
///              viewer and they accepted (people_sharing.sql `shares`, to them
///              or to a group they are a member of) - what auth_svc
///              `lookup_domain_member` lets through the gate. A password site
///              is never shared this way.
///
/// The viewer's OWN password sites come back too, as `target: "password"`,
/// so the atlas can take them out of the person's own folder list
/// (app.rs atlas_view) - never to draw them. Nobody else's are ever sent.
///
/// As api::DomainInfo with no path: they are looked at, not opened. Cached
/// 30 s per viewer, so a new domain is in within a minute.
/// Each row carries its owner's username (`owner`), never the email, and only
/// for a signed-in viewer: the atlas's person view (operator, 2026-10-23:
/// "atlas shows the whole system everyone has worked on, and to see
/// individual user systems as well").
pub fn visible_domains(viewer: Option<&str>) -> Result<Vec<u8>, String> {
    use std::collections::HashMap;
    static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<String, (std::time::Instant, Vec<u8>)>>> = std::sync::OnceLock::new();
    // Only a plain username ever reaches the SQL (it is inlined: platform_psql
    // takes no parameters); anything else is treated as not signed in.
    let who = viewer.filter(|u| !u.is_empty() && u.len() <= 64 && u.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-' || b == b'.')).unwrap_or("");
    let cache = CACHE.get_or_init(Default::default);
    if let Some((at, b)) = cache.lock().unwrap().get(who) {
        if at.elapsed() < Duration::from_secs(30) {
            return Ok(b.clone());
        }
    }
    let sql = format!(
        "with v as (select lower(u.email) as email, \
                           coalesce((select a.is_admin from admin_users a where lower(a.email) = lower(u.email) limit 1), false) as admin \
                      from ndexr_users u where u.username = '{who}' and '{who}' <> '' limit 1), \
              d as (select d.domain, d.title, d.label, d.owner_email, d.created_at, \
                           case when exists (select 1 from site_passwords p where p.domain = d.domain) then 'password' \
                                when d.access_level in ('public', 'login', 'admin', 'owner') then d.access_level else 'public' end as level, \
                           coalesce((select lower(ud.email) from user_domains ud where lower(ud.domain) = lower(d.domain) order by ud.added_at limit 1), \
                                    nullif(lower(d.owner_email), '')) as owner \
                      from domain_registry d \
                     where d.archived_at is null), \
              s as (select distinct lower(sh.domain) as domain from shares sh, v \
                     where sh.status = 'accepted' \
                       and (lower(sh.to_email) = v.email \
                            or sh.to_group in (select gm.group_id from people_group_members gm \
                                                where lower(gm.email) = v.email and gm.status = 'member'))), \
              e as (select d.*, \
                           (d.level = 'public' \
                            or (d.level = 'login' and exists (select 1 from v)) \
                            or (d.level = 'admin' and exists (select 1 from v where v.admin)) \
                            or (d.level in ('owner', 'password') and exists (select 1 from v where v.email = d.owner))) as seen, \
                           (d.level <> 'password' and lower(d.domain) in (select domain from s)) as shared \
                      from d) \
         select coalesce(json_agg(json_build_object('host', e.domain, 'path', '', \
                'title', coalesce(nullif(e.title, ''), nullif(e.label, ''), ''), 'module', true, \
                'target', case when e.seen then e.level else 'shared' end, 'env', '', \
                'created', coalesce(extract(epoch from e.created_at)::bigint, 0), \
                'owner', case when exists (select 1 from v) then coalesce((select u.username from ndexr_users u where lower(u.email) = e.owner limit 1), '') else '' end) \
                order by e.domain), '[]') \
           from e \
          where e.seen or e.shared"
    );
    let b = platform_psql(&sql)?.trim_ascii().to_vec();
    // Each with the kind of application behind it (the atlas's tint) and
    // what it is built with (its mark, in Connect and beside a project).
    let b = match serde_json::from_slice::<Vec<crate::api::DomainInfo>>(&b) {
        Ok(mut rows) => {
            // Signed out - the public viewer (/view/, ndexr.io's front page) -
            // a project shows only when its owner has said so (Settings ›
            // Access; operator, 2026-11-14: "users should be able to opt in
            // to show their domain in the viewer"). Signed in, the atlas and
            // Connect still show every app the person may open.
            if who.is_empty() {
                rows.retain(|r| in_viewer(&r.host));
            }
            let facts = app_kinds();
            for r in &mut rows {
                if let Some(f) = facts.get(&r.host) {
                    (r.app, r.language, r.framework) = (f.kind.to_owned(), f.language.clone(), f.framework.to_owned());
                }
            }
            serde_json::to_vec(&rows).map_err(|e| e.to_string())?
        }
        Err(_) => b,
    };
    let mut c = cache.lock().unwrap();
    c.retain(|_, (at, _)| at.elapsed() < Duration::from_secs(300));
    c.insert(who.to_owned(), (std::time::Instant::now(), b.clone()));
    Ok(b)
}

/// A project's folder on this machine: `<platform domains>/<owner>/<host>`,
/// or a platform domain's own `<platform domains>/<host>` - the one holding
/// its `.cove/app.env`.
pub fn project_folder(host: &str) -> Option<PathBuf> {
    if !is_host_name(host) {
        return None;
    }
    let root = Path::new(PLATFORM_DOMAINS);
    let has = |d: &Path| d.join(crate::project::dotcove::DIR).join("app.env").is_file();
    let flat = root.join(host);
    if has(&flat) {
        return Some(flat);
    }
    std::fs::read_dir(root).ok()?.flatten().map(|e| e.path().join(host)).find(|d| has(d))
}

/// Whether a project is listed on the public viewer: APP_VIEWER=yes in its
/// .cove/app.env, its owner's choice (Query::AppViewer). Not said: no.
pub fn in_viewer(host: &str) -> bool {
    project_folder(host).is_some_and(|d| viewer_line(&std::fs::read_to_string(d.join(crate::project::dotcove::DIR).join("app.env")).unwrap_or_default()))
}

fn viewer_line(env: &str) -> bool {
    env.lines().find_map(|l| l.trim().strip_prefix("APP_VIEWER=")).is_some_and(|v| v.trim().trim_matches('"') == "yes")
}

/// Read, and with `on` set, the project's APP_VIEWER (Query::AppViewer):
/// the line rewritten in place, or added under a comment saying what it is.
/// The file keeps its owner and mode (written in place).
pub fn app_viewer(host: &str, on: Option<bool>) -> Result<bool, String> {
    let dir = project_folder(host).ok_or_else(|| format!("{host}: no .cove/app.env here - only a Cove project can be listed"))?;
    let path = dir.join(crate::project::dotcove::DIR).join("app.env");
    let env = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let Some(on) = on else { return Ok(viewer_line(&env)) };
    std::fs::write(&path, set_viewer_line(&env, on)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(on)
}

fn set_viewer_line(env: &str, on: bool) -> String {
    let line = format!("APP_VIEWER={}", if on { "yes" } else { "no" });
    if env.lines().any(|l| l.trim().starts_with("APP_VIEWER=")) {
        let mut out: String = env.lines().map(|l| if l.trim().starts_with("APP_VIEWER=") { line.clone() } else { l.to_owned() }).collect::<Vec<_>>().join("\n");
        out.push('\n');
        out
    } else {
        let mut out = env.to_owned();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str("# Listed on the public viewer (ndexr.io's front page, /view/): the owner's\n# choice, in Cove's Settings › Access.\n");
        out.push_str(&line);
        out.push('\n');
        out
    }
}

/// The Software tab's Builds views' live half (`/builds.json`): what each
/// machine is building and has left (`ndexr aggregate`'s `builds.eta` rollup,
/// as build.ndexr.io's "to build" page reads it) and what an hour of each
/// instance type costs (`ec2_instance_prices`, as its cost window prices it).
/// The catalog has the history; this is the part that moves. Read from the
/// platform's postgres with the checkout's .env - which `cove multiuser`, as
/// root, can read for everyone - and kept 15 s, so many tabs make one query.
/// An env set as the tour shows it (operator, 2026-10-18: "whenever we get
/// into the R environments ... where we set the environment variables for
/// where certain things live, we can maybe show those in the tour as well,
/// like the site libraries and the repository options, repos, settings").
/// Read from the set's own Lmod module and its R's ndexr-repos.R - never
/// typed into a slide, so it says what the stack actually does. One line each.
pub fn env_tour_facts(set: &str) -> Result<Vec<String>, String> {
    if set.is_empty() || set.len() > 80 || !set.chars().all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c)) || set.contains("..") {
        return Err(format!("{set}: not an env set"));
    }
    let arch = std::env::consts::ARCH;
    let eb = format!("/opt/ndexr/stack/{arch}-{}-linux-gnu/eb", if arch == "aarch64" { "unknown" } else { "pc" });
    let lua = std::fs::read_to_string(format!("{eb}/modules/all/env/{set}.lua")).map_err(|_| format!("{set}: no such env module on this stack"))?;
    let quoted = |l: &str, key: &str| -> Option<String> {
        let rest = l.trim().strip_prefix(key)?;
        Some(rest.split('"').nth(1)?.to_owned())
    };
    let setenv = |name: &str| lua.lines().find_map(|l| quoted(l, &format!("setenv(\"{name}\""))).or_else(|| lua.lines().find_map(|l| l.trim().strip_prefix(&format!("setenv(\"{name}\", \"")).map(|r| r.trim_end_matches("\")").to_owned())));
    let deps: Vec<String> = lua.lines().filter_map(|l| quoted(l, "depends_on(")).collect();
    let mut out = Vec::new();
    let langs: Vec<String> = [("R", "NDEXR_ENV_R"), ("Python", "NDEXR_ENV_PYTHON"), ("Rust", "NDEXR_ENV_RUST")].iter().filter_map(|(l, var)| setenv(var).map(|v| format!("{l} {v}"))).collect();
    if !langs.is_empty() {
        out.push(langs.join(" · "));
    }
    let site = format!("{eb}/site-library/{set}");
    if lua.contains("R_LIBS_SITE") {
        out.push(format!("R_LIBS_SITE   {site}/R"));
    }
    if lua.contains("PYTHONPATH") {
        out.push(format!("PYTHONPATH    {site}/python"));
    }
    // A Rust set: its crates vendored for offline builds, its tools on PATH.
    if lua.contains("NDEXR_RUST_VENDOR") {
        out.push(format!("crates        {site}/rust/vendor  (NDEXR_RUST_VENDOR: a directory source for cargo)"));
        out.push(format!("PATH          {site}/rust/bin  (the set's cargo-installed tools)"));
    }
    // The repos: R's own etc/ndexr-repos.R, its defaults as written there.
    if let Some(r) = deps.iter().find(|d| d.starts_with("R/")) {
        let repos = std::fs::read_to_string(format!("{eb}/software/{r}/lib64/R/etc/ndexr-repos.R")).unwrap_or_default();
        let default = |var: &str| {
            repos.lines().find_map(|l| {
                let at = l.find(&format!("Sys.getenv(\"{var}\", \""))?;
                l[at..].split('"').nth(3).map(str::to_owned)
            })
        };
        if let (Some(ppm), Some(snap)) = (default("NDEXR_PPM"), default("NDEXR_CRAN_SNAPSHOT")) {
            out.push(format!("repos CRAN    {ppm}/cran/{snap}  (NDEXR_CRAN_SNAPSHOT pins a date)"));
        }
        if let Some(b) = default("NDEXR_BIOC_VERSION") {
            out.push(format!("repos BioC    https://bioconductor.org/packages/{b}"));
        }
        if repos.contains("NDEXR_BINARY_REPO") {
            out.push("repos first   $NDEXR_BINARY_REPO, when a host serves its own prebuilt packages".to_owned());
        }
    }
    let extra: Vec<&str> = deps.iter().filter(|d| !d.starts_with("R/") && !d.starts_with("Python/") && !d.starts_with("Rust/")).map(|d| d.split('/').next().unwrap_or(d)).collect();
    if !extra.is_empty() {
        out.push(format!("also loads    {}", extra.join(", ")));
    }
    if lua.contains("R_MAKEVARS_SITE") {
        out.push(format!("Makevars      {eb}/software/env/{set}/etc/Makevars.site  (each package's .so finds its libraries)"));
    }
    Ok(out)
}

/// The kernel each built stack can stand on (operator, 2026-10-18: "the
/// kernel, like a star should have its name and ... whatever the minimum
/// assumption is where the user clones the stack and it is guaranteed, those
/// will be different singularities and we will need to prove those"). One
/// per triple under the stack: the oldest Linux its own glibc was built for
/// (the libc's ELF ABI note - what the dynamic loader itself enforces), that
/// glibc and the kernel headers it was built against, and what it is proven
/// on so far: the kernel this machine runs, when it runs that triple.
/// Measured once per process: the stack under /opt changes by publish.
pub fn kernel_floors() -> Vec<u8> {
    static ONCE: std::sync::OnceLock<Vec<u8>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        let root = std::env::var("NDEXR_STACK_DIR").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "/opt/ndexr/stack".into());
        let here = std::env::consts::ARCH;
        let ran = std::fs::read_to_string("/proc/sys/kernel/osrelease").map(|s| s.trim().to_owned()).unwrap_or_default();
        let pkg = |pre: &Path, cat: &str, name: &str| -> String {
            std::fs::read_dir(pre.join("var/db/pkg").join(cat))
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_prefix(&format!("{name}-")).map(str::to_owned)))
                .max()
                .unwrap_or_default()
        };
        let mut out = Vec::new();
        let mut triples: Vec<String> = std::fs::read_dir(&root).into_iter().flatten().flatten().filter_map(|e| e.file_name().to_str().map(str::to_owned)).filter(|t| t.ends_with("-linux-gnu")).collect();
        triples.sort();
        for t in triples {
            let pre = Path::new(&root).join(&t).join("prefix").join(&t);
            let libc = ["lib64/libc.so.6", "lib/libc.so.6"].iter().map(|p| pre.join(p)).find(|p| p.is_file());
            let Some(min) = libc.and_then(|p| std::fs::read(p).ok()).and_then(|b| abi_tag(&b)) else { continue };
            let arch = t.split('-').next().unwrap_or("").to_owned();
            let proven: Vec<String> = if arch == here && !ran.is_empty() { vec![ran.clone()] } else { Vec::new() };
            out.push(serde_json::json!({
                "triple": t, "arch": arch, "min_kernel": min,
                "glibc": pkg(&pre, "sys-libs", "glibc"), "headers": pkg(&pre, "sys-kernel", "linux-headers"),
                "proven_on": proven,
            }));
        }
        serde_json::to_vec(&out).unwrap_or_else(|_| b"[]".to_vec())
    })
    .clone()
}

/// The oldest kernel a glibc runs on: its NT_GNU_ABI_TAG note (namesz 4,
/// descsz 16, type 1, "GNU\0", then os 0 = Linux, major, minor, patch).
fn abi_tag(elf: &[u8]) -> Option<String> {
    const HEAD: [u8; 16] = [4, 0, 0, 0, 16, 0, 0, 0, 1, 0, 0, 0, b'G', b'N', b'U', 0];
    let at = elf.windows(16).position(|w| w == HEAD)? + 16;
    let w = |k: usize| elf.get(at + 4 * k..at + 4 * k + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]));
    (w(0)? == 0).then(|| format!("{}.{}.{}", w(1).unwrap_or(0), w(2).unwrap_or(0), w(3).unwrap_or(0)))
}

pub fn builds_live() -> Result<Vec<u8>, String> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<(std::time::Instant, Vec<u8>)>>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((at, b)) = cache.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(15) {
            return Ok(b.clone());
        }
    }
    const SQL: &str = "select json_build_object(\
        'generated', extract(epoch from now())::bigint, \
        'eta', (select payload from stat_rollups where key = 'builds.eta'), \
        'eta_at', (select extract(epoch from computed_at)::bigint from stat_rollups where key = 'builds.eta'), \
        'prices', (select json_object_agg(instance_type, price_per_hour) from ec2_instance_prices where region = 'us-east-2'))";
    let out = platform_psql(SQL)?;
    // The package lane, from the logs (build_lanes): builds.eta counts only
    // the EasyBuild plan, so the Queue sat on "200 of 201" for a day while the
    // machines worked through hundreds of R and Python packages (2026-10-15).
    let mut v: Value = serde_json::from_slice(out.trim_ascii()).map_err(|e| format!("the build database answered oddly: {e}"))?;
    v["lanes"] = Value::from(build_lanes());
    let bytes = serde_json::to_vec(&v).map_err(|e| e.to_string())?;
    *cache.lock().unwrap() = Some((std::time::Instant::now(), bytes.clone()));
    Ok(bytes)
}

/// Each build machine's package lane as its log says it now: the last
/// "* now installing duckdb (R) - 182 of 321", when the log last grew, and the
/// run's state - read from the end of the file only.
pub fn build_lanes() -> Vec<Value> {
    use std::io::{Read, Seek, SeekFrom};
    let dir = build_logs_dir();
    let mut out = Vec::new();
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(k) = name.strip_suffix(".log").filter(|k| !k.is_empty() && !k.starts_with('.')) else { continue };
        let Ok(mut f) = std::fs::File::open(e.path()) else { continue };
        let Ok(md) = f.metadata() else { continue };
        let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        // The last MiB: one big compile (duckdb's) writes more than 64 KB
        // after its "now installing" line, and the lane went blank.
        let _ = f.seek(SeekFrom::Start(md.len().saturating_sub(1 << 20)));
        let mut buf = Vec::new();
        let _ = f.take(1 << 20).read_to_end(&mut buf);
        let text = String::from_utf8_lossy(&buf);
        let state: Value = std::fs::read(dir.join(format!("{k}.json"))).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        // The logs here are four-box's (build_logs_dir): its boxes are four-box-<key>.
        let (power, status) = box_power(&dir, "four-box", k);
        let mut lane = serde_json::json!({ "key": k, "mtime": mtime, "state": state.get("state").cloned().unwrap_or(Value::Null), "step": state.get("step").cloned().unwrap_or(Value::Null), "power": power, "status": status });
        if let Some((pkg, kind, n, m)) = text.lines().rev().find_map(parse_installing) {
            lane["now"] = Value::from(pkg);
            lane["kind"] = Value::from(kind);
            lane["n"] = Value::from(n);
            lane["of"] = Value::from(m);
        }
        // Where the run is, for the atlas's machines panel (operator,
        // 2026-10-24: "if we are running any of the boxes ... let them know
        // what is going on, like are packages building and where, what stage").
        for k in ["steps", "step_index", "started", "finished", "host", "publish_id"] {
            lane[k] = state.get(k).cloned().unwrap_or(Value::Null);
        }
        let p = run_progress(&text);
        lane["set"] = Value::from(p.set);
        lane["set_n"] = Value::from(p.set_n);
        lane["sets"] = Value::from(p.sets);
        lane["tail"] = Value::from(p.tail);
        out.push(lane);
    }
    out.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
    out
}

/// One answer, everywhere, to "is this box on, and what is it doing"
/// (operator, 2026-10-18: "seems like differing indicators across the site
/// for the status of the boxes, is gpu running or not"). The Queue, the
/// Machines panel, the atlas and the front's slides each read a different
/// signal - a run file's last state (gpu's from four days before), an
/// address left behind, a log's age - and disagreed. In order of trust:
///
///   1. `ndexr aws server watch`'s .watch/<role>-<stem>.server.json, checked
///      in the last 10 minutes: the instance state from AWS and whether it
///      answered. Stopped is off, whatever the run file says.
///   2. Else the log: written in the last 15 minutes, and not the mirror's
///      "is stopped" note, is on but unconfirmed; anything else is off.
///
/// Returns (power, status): power `on` · `off` · `unconfirmed`; status the
/// run's state when on (running · starting · done · failed · stale), else `off`.
pub fn box_power(dir: &Path, role: &str, stem: &str) -> (String, String) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let age = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(i64::MAX, |d| now - d.as_secs() as i64);
    let run: Value = std::fs::read(dir.join(format!("{stem}.json"))).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
    let log = dir.join(format!("{stem}.log"));
    let log_age = age(&log);
    let watch = dir.join(".watch").join(format!("{role}-{stem}.server.json"));
    let power = match std::fs::read(&watch).ok().and_then(|b| serde_json::from_slice::<Value>(&b).ok()).filter(|_| age(&watch) < 600) {
        Some(w) => match w["state"].as_str() {
            Some("running") => "on",
            Some("pending") => "unconfirmed",
            _ => "off",
        },
        None => {
            let tail = std::fs::read(&log).map(|b| String::from_utf8_lossy(&b[b.len().saturating_sub(4096)..]).into_owned()).unwrap_or_default();
            if log_age < 15 * 60 && !tail.contains("is stopped") && !tail.contains("no public IP") { "unconfirmed" } else { "off" }
        }
    };
    let status = if power == "off" {
        "off".to_owned()
    } else {
        match run["state"].as_str().unwrap_or("") {
            // Running with a quiet log is a hung run (n-box `.shown`).
            // Hung means quiet everywhere: the log AND the timeline the box
            // reports each minute. One long compile (linearmodels, 40 min of
            // pip building numpy) writes no log line, and "stale" sat beside
            // "compiling" for a box hard at work (operator, 2026-10-20: "this
            // is confusing").
            "running" if log_age > 15 * 60 && age(&dir.join(format!("{stem}.timeline.json"))) > 15 * 60 => "stale".to_owned(),
            "" => "idle".to_owned(),
            s => s.to_owned(),
        }
    };
    (power.to_owned(), status)
}

/// "* now installing duckdb (R) - 182 of 321" → (duckdb, R, 182, 321).
/// What a box's log says about its run beyond the package it installs: the
/// env set it is on (`· packages <set>: pull ...`), which of how many (`·
/// packages: N sets`), and its last lines, secrets masked as /buildlog.json
/// masks them - the same text the public Logs view shows.
#[derive(Default, Debug, PartialEq)]
pub struct RunProgress {
    pub set: String,
    pub set_n: u64,
    pub sets: u64,
    pub tail: Vec<String>,
}

pub fn run_progress(text: &str) -> RunProgress {
    let mut p = RunProgress::default();
    for l in text.lines() {
        let l = l.trim();
        if let Some(rest) = l.strip_prefix("· packages: ") {
            p.sets = rest.split_whitespace().next().and_then(|n| n.parse().ok()).unwrap_or(0);
            p.set_n = 0;
            p.set.clear();
        } else if let Some((set, _)) = l.strip_prefix("· packages ").and_then(|r| r.split_once(": pull")) {
            p.set = set.to_owned();
            p.set_n += 1;
        } else if l.starts_with("[ndexr up]") && l.contains(" step 1/") {
            p = RunProgress::default();
        }
    }
    p.tail = text
        .lines()
        .rev()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .take(6)
        .map(|l| mask_secrets(&l.chars().take(200).collect::<String>()))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    p
}

fn parse_installing(line: &str) -> Option<(String, String, u64, u64)> {
    let rest = line.trim().strip_prefix("* now installing ")?;
    let (pkg, rest) = rest.split_once(" (")?;
    let (kind, rest) = rest.split_once(')')?;
    let (n, m) = rest.trim().trim_start_matches('-').trim().split_once(" of ")?;
    Some((pkg.trim().to_owned(), kind.to_owned(), n.trim().parse().ok()?, m.trim().parse().ok()?))
}

/// Where the build machines' logs land on this host: `ndexr aws server watch`
/// (user units `ndexr-server-watch@<server>`) appends each box's run log by
/// offset into `.watch/`, and four-box's `<key>.log` / `<key>.json` sit
/// beside it (the log a symlink into `.watch/`, or a plain file for a box no
/// watcher follows). `COVE_BUILD_LOGS_DIR` moves it.
pub fn build_logs_dir() -> PathBuf {
    std::env::var_os("COVE_BUILD_LOGS_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/srv/ndexr/host_runs/four-box"))
}

/// The Builds › Logs view (`/buildlog.json?key=<box>&from=<byte>`): every
/// box's log with its run state, and - for `key` - the bytes from `from` on
/// (at most 512 KB a call; without `from`, the last 128 KB from a line
/// start). The client keeps the offset it is given back, so a poll costs only
/// what was written since. Read from disk, never over ssh, so a box that has
/// gone away still shows what it said.
pub fn build_log(key: Option<&str>, from: Option<u64>) -> Result<Vec<u8>, String> {
    use std::io::{Read, Seek, SeekFrom};
    const TAIL: u64 = 128 << 10;
    const CHUNK: u64 = 512 << 10;
    let dir = build_logs_dir();
    let rd = std::fs::read_dir(&dir).map_err(|e| format!("no build logs on this machine ({}: {e}) - they are kept on the ndexr host", dir.display()))?;
    let mut logs: Vec<Value> = Vec::new();
    let mut paths: Vec<(String, PathBuf)> = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Some(k) = name.strip_suffix(".log").filter(|k| !k.is_empty() && !k.starts_with('.')) else { continue };
        // metadata() follows the symlink: a dangling one is no log.
        let Ok(md) = std::fs::metadata(e.path()) else { continue };
        let state: Value = std::fs::read(dir.join(format!("{k}.json"))).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        let mtime = md.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
        let (power, status) = box_power(&dir, "four-box", k);
        logs.push(serde_json::json!({
            "key": k,
            "power": power,
            "status": status,
            "size": md.len(),
            "mtime": mtime,
            "state": state.get("state").cloned().unwrap_or(Value::Null),
            "step": state.get("step").cloned().unwrap_or(Value::Null),
            "started": state.get("started").cloned().unwrap_or(Value::Null),
            "finished": state.get("finished").cloned().unwrap_or(Value::Null),
            "host": state.get("host").cloned().unwrap_or(Value::Null),
        }));
        paths.push((k.to_owned(), e.path()));
    }
    logs.sort_by(|a, b| a["key"].as_str().cmp(&b["key"].as_str()));
    let mut out = serde_json::json!({ "generated": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs()), "logs": logs });
    // Only a name the listing holds is opened: the key never becomes a path.
    if let Some((k, p)) = key.and_then(|k| paths.iter().find(|(n, _)| n == k)) {
        let mut f = std::fs::File::open(p).map_err(|e| format!("{k}.log: {e}"))?;
        let size = f.metadata().map_err(|e| e.to_string())?.len();
        // A log that shrank was started again: begin at its tail.
        let tailing = from.is_none_or(|f| f > size);
        let mut at = if tailing { size.saturating_sub(TAIL) } else { from.unwrap() };
        f.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        f.take(CHUNK).read_to_end(&mut buf).map_err(|e| e.to_string())?;
        if tailing && at > 0 {
            let cut = buf.iter().position(|&b| b == b'\n').map_or(0, |i| i + 1);
            buf.drain(..cut);
            at += cut as u64;
        }
        // Hand back whole lines only; the rest comes with the next poll.
        let keep = match buf.iter().rposition(|&b| b == b'\n') {
            Some(i) => i + 1,
            None if buf.len() as u64 >= CHUNK => buf.len(),
            None => 0,
        };
        buf.truncate(keep);
        let next = at + buf.len() as u64;
        out["key"] = Value::from(k.as_str());
        out["from"] = Value::from(at);
        out["next"] = Value::from(next);
        out["size"] = Value::from(size);
        out["reset"] = Value::from(tailing);
        out["text"] = Value::from(mask_secrets(&String::from_utf8_lossy(&buf)));
    }
    serde_json::to_vec(&out).map_err(|e| e.to_string())
}

/// Build logs go to everyone signed in, so anything shaped like a credential
/// is blanked first: an AWS access key id, and the value of a `KEY=value`
/// whose name says secret, token, password or api key (a long value only -
/// compiler flags like `-DHAVE_CURLOPT_PASSWORD=1` stay as they are).
pub(crate) fn mask_secrets(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for line in s.split_inclusive('\n') {
        let mut l = line.to_owned();
        while let Some(i) = l.find("AKIA") {
            let id = &l[i..];
            let n = id.bytes().take_while(|b| b.is_ascii_uppercase() || b.is_ascii_digit()).count();
            if n < 20 {
                break;
            }
            l.replace_range(i + 4..i + n, &"*".repeat(n - 4));
        }
        let lower = l.to_ascii_lowercase();
        let mut cuts = Vec::new();
        for (eq, _) in lower.match_indices('=') {
            let name_start = lower[..eq].rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                // Past the whole character, not one byte: a curly quote before
                // `token=` put the cut inside it, the slice panicked, and the
                // release build aborts on any panic - cove multiuser restarted
                // every 5 s for everyone (2026-10-18, request #35).
                .map_or(0, |i| i + lower[i..].chars().next().map_or(1, char::len_utf8));
            let name = &lower[name_start..eq];
            if !["secret", "token", "password", "passwd", "api_key", "apikey"].iter().any(|w| name.contains(w)) {
                continue;
            }
            let v = &l[eq + 1..];
            let n = v.find(|c: char| c.is_whitespace() || c == '"' || c == '\'' || c == ',' || c == ';').unwrap_or(v.len());
            if n >= 12 {
                cuts.push((eq + 1, eq + 1 + n));
            }
        }
        for (a, b) in cuts.into_iter().rev() {
            l.replace_range(a..b, "****");
        }
        out.push_str(&l);
    }
    out
}

pub(crate) fn home() -> String {
    std::env::var("HOME").unwrap_or_else(|_| "/".into())
}

/// Fast and local only: where ndexr is and which version.
fn probe() -> Probe {
    let ndexr = capture_within("command -v ndexr", Duration::from_secs(5));
    Probe {
        version: ndexr.as_ref().and_then(|_| capture_within("ndexr --version", Duration::from_secs(5))),
        ndexr,
        home: home(),
        host: capture("hostname").unwrap_or_default(),
        os: std::env::consts::OS.into(),
    }
}

// ── settings: asked once, kept on the machine, never assumed ────────────────

fn settings_file() -> PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(home()).join(".config"))
        .join("ndexr/cove.json")
}

pub fn saved_settings() -> Settings {
    std::fs::read(settings_file()).ok().and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).unwrap_or_default().one_module()
}

/// Where new projects are made, one folder each (operator, 2026-11-20: "there
/// needs to be a default location for the projects which the user can
/// choose. All projects will be stored in that folder"). On ndexr's own
/// servers the platform's folder (`own_domains`: nginx, autosave and cells
/// read it). Elsewhere - a desktop, a customer's own server - the person's
/// choice, else ~/.cove/projects. Before this a desktop project was made
/// beside whichever project happened to be open.
pub fn projects_root(s: &Settings) -> PathBuf {
    if let Some(p) = own_domains() {
        return p;
    }
    let chosen = s.projects_dir.trim();
    if Path::new(chosen).is_absolute() {
        return PathBuf::from(chosen);
    }
    Path::new(&home()).join(".cove").join("projects")
}

/// Where the kernels start and Files opens: the chosen project, else home.
pub fn project_dir(s: &Settings) -> String {
    let p = s.project_dir.trim();
    if Path::new(p).is_absolute() && Path::new(p).is_dir() {
        p.to_owned()
    } else {
        home()
    }
}

/// The ndexr stack: stack.ndexr.io on CVMFS at /opt/ndexr/stack, always
/// (operator, 2026-10-13: "the ide will be highly coupled to our cvmfs
/// distribution for cleanliness" - no stack folder, no paths to set).
/// COVE_STACK_ROOT exists only to point a test instance at a scratch
/// Lmod tree; it is not a setting.
///
/// Standalone (2026-10-19): a folder picked in Global Options (`stack_dir`),
/// else the mount, else a local build at ~/.ndexr/stack. With none of them
/// the kernels run what is on PATH (envs.rs `prefix`).
pub fn stack_root() -> PathBuf {
    if let Some(p) = std::env::var_os("COVE_STACK_ROOT") {
        return PathBuf::from(p);
    }
    let env_dir = std::env::var_os("NDEXR_STACK_DIR").filter(|d| !d.is_empty()).map(PathBuf::from);
    find_stack_root(&saved_settings().stack_dir, Path::new("/opt/ndexr/stack"), Path::new(&home()), env_dir, &own_triple())
}

/// Found wherever it was set up (operator, 2026-11-14: "if it has been set up
/// locally it should auto detect"): the folder picked in Settings, the CVMFS
/// mount, Cove's own build (~/.ndexr/stack), and where `ndexr stack up` and
/// `ndexr stack clone` put one by default ($NDEXR_STACK_DIR, else ~/.svbt) -
/// the first that holds a use.sh. A picked folder with no stack in it no
/// longer hides one that is here. None: the picked folder (an airgapped copy
/// on its way), else the mount - what setup and the messages name.
fn find_stack_root(picked: &str, mount: &Path, home: &Path, env_dir: Option<PathBuf>, triple: &str) -> PathBuf {
    let picked = picked.trim();
    let picked = picked.starts_with('/').then(|| PathBuf::from(picked));
    let has = |p: &Path| p.join("use.sh").is_file() || p.join(triple).join("use.sh").is_file();
    let candidates = picked.iter().cloned().chain([mount.to_path_buf(), home.join(".ndexr/stack")]).chain(env_dir).chain([home.join(".svbt")]);
    for c in candidates {
        if has(&c) {
            return c;
        }
    }
    picked.unwrap_or_else(|| mount.to_path_buf())
}

/// Some ndexr stack is here - mounted, built locally, or picked - so modules
/// load; else R, Python and the shell are this computer's own, from PATH.
pub fn stack_present() -> bool {
    use_sh().is_some()
}

/// The easyconfig for EasyBuild module `name`/`version` (Packages, operator
/// 2026-10-19: "if we have an easybuild configuration for something we need
/// to be able to see it"): the installed module's own copy under
/// eb/software/<name>/<version>/easybuild, else the suite's recipe,
/// easybuild-suite/easyconfigs/<n>/<name>/<name>-<version>.eb, else any
/// file of that name in the suite.
pub fn easyconfig(name: &str, version: &str) -> Result<crate::api::EasyconfigView, String> {
    // A module name may hold a slash: env/R/4.6.1-bioc3.24 is EasyBuild's
    // `env-R` at 4.6.1-bioc3.24 (its modaltsoftname = 'env/R'). Each part is
    // checked; no part may climb out.
    let part = |s: &str| !s.is_empty() && !s.contains(['\\', '\0']) && s.split('/').all(|p| !p.is_empty() && p != "." && p != "..");
    if !part(name) || !part(version) || version.contains('/') {
        return Err(format!("{name}/{version}: not a module name"));
    }
    let root = stack_root().join(own_triple());
    let eb_name = name.replace('/', "-");
    let file = format!("{eb_name}-{version}.eb");
    let first = eb_name.chars().next().map(|c| c.to_ascii_lowercase().to_string()).unwrap_or_default();
    let suite = root.join("easybuild-suite/easyconfigs");
    // The ndexr CLI's own kit: language sets' recipes land there first.
    let kit = Path::new(PLATFORM_DOMAINS).join("ndexr.ndexr.io/crate/assets/stack-kit/easybuild-suite/easyconfigs");
    let installed = |n: &str| root.join("eb/software").join(n).join(version).join("easybuild").join(&file);
    let mut found = [
        (installed(&eb_name), true),
        (installed(name), true),
        (suite.join(&first).join(&eb_name).join(&file), false),
        (kit.join(&first).join(&eb_name).join(&file), false),
    ]
    .into_iter()
    .find(|(p, _)| p.is_file());
    if found.is_none() {
        let mut stack = vec![suite.clone()];
        while let Some(d) = stack.pop() {
            for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if e.file_name().to_string_lossy() == file {
                    found = Some((p, false));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
    }
    let (path, built) = found.ok_or_else(|| format!("no easyconfig {file} in this stack ({})", root.display()))?;
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(crate::api::EasyconfigView { path: path.display().to_string(), text, built })
}

/// How every package in this machine's site libraries came to be there, by
/// what each install recorded: a Python wheel's `Tag:` (manylinux/musllinux
/// = built elsewhere and downloaded; linux_<arch> = built here; none-any =
/// no machine code), an R package's DESCRIPTION (a package manager's binary
/// says so in `Repository:`/`Packaged:`; compiled code in libs/ built for
/// this platform = built here; no libs/ = no machine code). One row per
/// package and way, with every set it came that way in. Kept ten minutes.
pub fn provenance() -> Vec<crate::api::Provenance> {
    use std::time::{Duration, Instant};
    static LAST: std::sync::Mutex<Option<(Instant, Vec<crate::api::Provenance>)>> = std::sync::Mutex::new(None);
    if let Some((at, v)) = LAST.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(600) {
            return v.clone();
        }
    }
    let lib = stack_root().join(own_triple()).join("eb/site-library");
    let mut rows: std::collections::BTreeMap<(String, String, String, String), (String, Vec<String>)> = std::collections::BTreeMap::new();
    let sets: Vec<(String, PathBuf)> = std::fs::read_dir(&lib)
        .into_iter()
        .flatten()
        .flatten()
        .flat_map(|top| {
            let t = top.file_name().to_string_lossy().into_owned();
            // A language set is two deep (Python/3.13.15); a composition one.
            let kids: Vec<(String, PathBuf)> = std::fs::read_dir(top.path()).into_iter().flatten().flatten().map(|k| (format!("{t}/{}", k.file_name().to_string_lossy()), k.path())).collect();
            if ["R", "Python", "Rust", "Julia"].contains(&t.as_str()) { kids } else { vec![(t, top.path())] }
        })
        .collect();
    for (set, dir) in sets {
        for e in std::fs::read_dir(dir.join("python")).into_iter().flatten().flatten() {
            let f = e.file_name().to_string_lossy().into_owned();
            let Some(stem) = f.strip_suffix(".dist-info") else { continue };
            let (name, version) = stem.rsplit_once('-').unwrap_or((stem, ""));
            let tag = std::fs::read_to_string(e.path().join("WHEEL")).unwrap_or_default().lines().find_map(|l| l.strip_prefix("Tag:")).map(|t| t.trim().to_owned()).unwrap_or_default();
            let how = if tag.contains("manylinux") || tag.contains("musllinux") {
                "external"
            } else if tag.ends_with("-any") {
                "pure"
            } else if tag.is_empty() {
                continue;
            } else {
                "built"
            };
            let r = rows.entry(("Python".into(), crate::api::pkg_key(name), version.into(), how.into())).or_insert_with(|| (format!("wheel tag {tag}"), Vec::new()));
            r.1.push(set.clone());
        }
        for e in std::fs::read_dir(dir.join("R")).into_iter().flatten().flatten() {
            let Ok(d) = std::fs::read_to_string(e.path().join("DESCRIPTION")) else { continue };
            let field = |k: &str| d.lines().find_map(|l| l.strip_prefix(k).and_then(|v| v.strip_prefix(':'))).map(|v| v.trim().to_owned()).unwrap_or_default();
            let (repo, packaged, built) = (field("Repository"), field("Packaged"), field("Built"));
            let libs = e.path().join("libs").is_dir();
            let manager = ["RSPM", "PPM", "Posit Package Manager", "RStudio Package Manager"].iter().find(|m| repo.contains(*m) || packaged.contains(*m));
            let (how, evidence) = match manager {
                Some(m) => ("external", format!("a {m} binary (DESCRIPTION Repository/Packaged)")),
                None if libs => ("built", format!("compiled code, Built: {built}")),
                None => ("pure", "no compiled code (no libs/)".to_owned()),
            };
            let r = rows.entry(("R".into(), e.file_name().to_string_lossy().into_owned(), field("Version"), how.into())).or_insert_with(|| (evidence, Vec::new()));
            r.1.push(set.clone());
        }
    }
    // Julia: a `_jll` package wraps a binary built by Yggdrasil/BinaryBuilder
    // and downloaded as an artifact - machine code not compiled here. The
    // rest are Julia source, compiled on the machine that runs them.
    let jl = stack_root().join(own_triple()).join("eb/site-library/Julia");
    for v in std::fs::read_dir(&jl).into_iter().flatten().flatten() {
        let set = format!("Julia/{}", v.file_name().to_string_lossy());
        for e in std::fs::read_dir(v.path().join("julia/depot/packages")).into_iter().flatten().flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let (how, evidence) = if name.ends_with("_jll") {
                ("external", "a JLL package: its artifact is a prebuilt binary from JuliaBinaryWrappers (Yggdrasil, BinaryBuilder), downloaded")
            } else {
                ("pure", "Julia source, compiled where it runs")
            };
            let r = rows.entry(("Julia".into(), crate::api::pkg_key(&name), String::new(), how.into())).or_insert_with(|| (evidence.to_owned(), Vec::new()));
            r.1.push(set.clone());
        }
    }
    // Rust: the programs a set installs are compiled here by `cargo install`
    // (.crates2.json names each, its version and registry); every other crate
    // is vendored source (vendor/<crate>-<version>), compiled wherever a
    // project uses it. No crate arrives as machine code.
    let rs = stack_root().join(own_triple()).join("eb/site-library/Rust");
    for v in std::fs::read_dir(&rs).into_iter().flatten().flatten() {
        let set = format!("Rust/{}", v.file_name().to_string_lossy());
        let root = v.path().join("rust");
        let installed: serde_json::Value = std::fs::read(root.join(".crates2.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        let mut programs = std::collections::HashSet::new();
        for key in installed["installs"].as_object().into_iter().flat_map(|o| o.keys()) {
            let mut w = key.split_whitespace();
            let (Some(name), Some(version)) = (w.next(), w.next()) else { continue };
            programs.insert(crate::api::pkg_key(name));
            let r = rows.entry(("Rust".into(), crate::api::pkg_key(name), version.into(), "built".into()))
                .or_insert_with(|| (format!("compiled here by cargo install from crates.io ({key})"), Vec::new()));
            r.1.push(set.clone());
        }
        for e in std::fs::read_dir(root.join("vendor")).into_iter().flatten().flatten() {
            let f = e.file_name().to_string_lossy().into_owned();
            let Some((name, version)) = f.rsplit_once('-') else { continue };
            if programs.contains(&crate::api::pkg_key(name)) {
                continue;
            }
            let r = rows.entry(("Rust".into(), crate::api::pkg_key(name), version.into(), "pure".into()))
                .or_insert_with(|| ("crate source from crates.io, vendored; compiled where a project uses it".to_owned(), Vec::new()));
            r.1.push(set.clone());
        }
    }
    // The kit's list of wheels taken on purpose, each with its reason
    // (stack-kit/manifest/wheel-exceptions.tsv: pattern <tab> reason).
    let listed: Vec<(regex_lite::Regex, String)> = std::fs::read_to_string(Path::new(PLATFORM_DOMAINS).join("ndexr.ndexr.io/crate/assets/stack-kit/manifest/wheel-exceptions.tsv"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("pattern\t"))
        .filter_map(|l| l.split_once('\t'))
        .filter_map(|(p, r)| regex_lite::Regex::new(p.trim()).ok().map(|re| (re, r.trim().to_owned())))
        .collect();
    // What the next release does about each downloaded binary
    // (stack-kit/manifest/binary-plan.tsv: language, pattern, fate, plan).
    let kit = Path::new(PLATFORM_DOMAINS).join("ndexr.ndexr.io/crate/assets/stack-kit/manifest");
    let plan = binary_plan(&std::fs::read_to_string(kit.join("binary-plan.tsv")).unwrap_or_default());
    // The Gentoo Prefix's own downloaded binaries: a plan row names them
    // (category/package), the Prefix's package database says if one is here.
    let vdb = stack_root().join(own_triple()).join("prefix").join(own_triple()).join("var/db/pkg");
    for (_, re, _, _) in plan.iter().filter(|p| p.0 == "Prefix") {
        for cat in std::fs::read_dir(&vdb).into_iter().flatten().flatten() {
            let c = cat.file_name().to_string_lossy().into_owned();
            for p in std::fs::read_dir(cat.path()).into_iter().flatten().flatten() {
                let pv = p.file_name().to_string_lossy().into_owned();
                // name-version: the version starts at the first "-<digit>".
                let cut = pv.char_indices().find(|&(i, ch)| ch == '-' && pv[i + 1..].starts_with(|d: char| d.is_ascii_digit())).map_or(pv.len(), |x| x.0);
                let atom = format!("{c}/{}", &pv[..cut]);
                if re.is_match(&atom) {
                    let r = rows.entry(("Prefix".into(), atom, pv[cut..].trim_start_matches('-').to_owned(), "external".into())).or_insert_with(|| ("a prebuilt binary package in the Gentoo Prefix".into(), Vec::new()));
                    r.1.push("Prefix".into());
                }
            }
        }
    }
    let v: Vec<crate::api::Provenance> = rows
        .into_iter()
        .map(|((language, name, version, how), (evidence, sets))| {
            let reason = if how == "external" && language == "Python" { listed.iter().find(|(re, _)| re.is_match(&name)).map(|x| x.1.clone()).unwrap_or_default() } else { String::new() };
            let (fate, plan) = if how == "external" {
                plan.iter().find(|(l, re, _, _)| *l == language && re.is_match(&name)).map(|p| (p.2.clone(), p.3.clone())).unwrap_or_default()
            } else {
                Default::default()
            };
            crate::api::Provenance { language, name, version, how, evidence, sets, reason, fate, plan }
        })
        .collect();
    *LAST.lock().unwrap() = Some((Instant::now(), v.clone()));
    v
}

/// binary-plan.tsv's rows: (language, pattern, fate, plan). A row whose fate is
/// not source or remove - the deployment is binaryless, so nothing is kept - or
/// whose pattern does not compile, is left out, and its binary reads undecided.
fn binary_plan(text: &str) -> Vec<(String, regex_lite::Regex, String, String)> {
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("language\t"))
        .filter_map(|l| {
            let f: Vec<&str> = l.splitn(4, '\t').collect();
            let [lang, pat, fate, plan] = f.as_slice() else { return None };
            let fate = fate.trim();
            if !["source", "remove"].contains(&fate) {
                return None;
            }
            Some((lang.trim().to_owned(), regex_lite::Regex::new(pat.trim()).ok()?, fate.to_owned(), plan.trim().to_owned()))
        })
        .collect()
}

/// The `dependencies` of an easyconfig, each as the module version EasyBuild
/// gives it: name, then version + suffix + "-<toolchain>-<version>" unless
/// the toolchain is SYSTEM. One tuple per line, as every easyconfig the kit
/// generates writes them.
pub fn easyconfig_deps(text: &str) -> Vec<crate::api::EnvUse> {
    let Some(start) = text.find("\ndependencies = [") else { return Vec::new() };
    let body = &text[start + 1..];
    let body = &body[..body.find("\n]").unwrap_or(body.len())];
    body.lines()
        .filter(|l| l.trim_start().starts_with('('))
        .filter_map(|l| {
            let q: Vec<&str> = l.split('\'').skip(1).step_by(2).collect();
            let (name, ver) = (*q.first()?, *q.get(1)?);
            let suffix = q.get(2).copied().unwrap_or("");
            let tc = match (q.get(3), q.get(4)) {
                (Some(t), Some(v)) if *t != "system" => format!("-{t}-{v}"),
                _ => String::new(),
            };
            Some(crate::api::EnvUse { name: name.to_owned(), version: format!("{ver}{suffix}{tc}") })
        })
        .collect()
}

/// What each env set's module loads (`module`s like env/R/4.6.1-bioc3.24,
/// at most 64), from its easyconfig in this stack or the kit
/// (`easyconfig`). A set whose easyconfig is not found says so.
pub fn env_uses(modules: &[&str]) -> Vec<crate::api::EnvUses> {
    modules
        .iter()
        .take(64)
        .map(|m| {
            let mut out = crate::api::EnvUses { module: (*m).to_owned(), ..Default::default() };
            match m.rsplit_once('/').ok_or_else(|| format!("{m}: not a module")).and_then(|(n, v)| easyconfig(n, v)) {
                Ok(ec) => {
                    out.uses = easyconfig_deps(&ec.text);
                    out.path = ec.path;
                }
                Err(e) => out.error = e,
            }
            out
        })
        .collect()
}

/// The kit's record sets (manifests/<Language>/<version>/set.json) and
/// combinations (manifests/combos.txt), with the packages of the sets in
/// `with` read from their manifest.dcf. The kit is the source the stack is
/// built from, so a set it defines that is not built yet is here too.
pub fn env_sets(with: &[&str]) -> crate::api::EnvSets {
    let root = Path::new(PLATFORM_DOMAINS).join("ndexr.ndexr.io/crate/assets/stack-kit/manifest/manifests");
    let mut out = crate::api::EnvSets::default();
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&root).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    dirs.sort();
    for lang in dirs {
        let mut vs: Vec<PathBuf> = std::fs::read_dir(&lang).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.join("set.json").is_file()).collect();
        vs.sort();
        for v in vs {
            let Ok(j) = std::fs::read(v.join("set.json")).map_err(|_| ()).and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).map_err(|_| ())) else { continue };
            let set = j["set"].as_str().unwrap_or_default().to_owned();
            if set.is_empty() {
                continue;
            }
            let packages = if with.contains(&set.as_str()) { manifest_packages(&std::fs::read_to_string(v.join("manifest.dcf")).unwrap_or_default()) } else { Vec::new() };
            out.sets.push(crate::api::KitSet { language: j["language"].as_str().unwrap_or_default().to_owned(), records: j["records"].as_u64().unwrap_or(0) as u32, set, packages });
        }
    }
    out.repos = julia_repos();
    for l in std::fs::read_to_string(root.join("combos.txt")).unwrap_or_default().lines() {
        let f: Vec<&str> = l.split_whitespace().collect();
        if f.len() >= 2 && !l.trim_start().starts_with('#') {
            out.combos.push(crate::api::KitCombo { name: f[0].to_owned(), sets: f[1..].iter().map(|s| (*s).to_owned()).collect() });
        }
    }
    out
}

/// Every installed Julia package's repository, by name: the packages in each
/// Julia set's depot (`site-library/Julia/<v>/julia/depot/packages`), their
/// `repo` read from the depot's own General registry (one tarball of
/// Package.toml files, read through `tar`). Kept ten minutes.
pub fn julia_repos() -> std::collections::HashMap<String, String> {
    use std::time::{Duration, Instant};
    static LAST: std::sync::Mutex<Option<(Instant, std::collections::HashMap<String, String>)>> = std::sync::Mutex::new(None);
    if let Some((at, m)) = LAST.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(600) {
            return m.clone();
        }
    }
    let mut out = std::collections::HashMap::new();
    let lib = stack_root().join(own_triple()).join("eb/site-library/Julia");
    for v in std::fs::read_dir(&lib).into_iter().flatten().flatten() {
        let depot = v.path().join("julia/depot");
        let installed: std::collections::HashSet<String> = std::fs::read_dir(depot.join("packages")).into_iter().flatten().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        let reg = depot.join("registries/General.tar.gz");
        if installed.is_empty() || !reg.is_file() {
            continue;
        }
        let Ok(o) = std::process::Command::new("tar").args(["xzf"]).arg(&reg).args(["--wildcards", "*/Package.toml", "-O"]).output() else { continue };
        let text = String::from_utf8_lossy(&o.stdout);
        let mut name = String::new();
        for l in text.lines() {
            let val = |l: &str| l.split_once('=').map(|(_, v)| v.trim().trim_matches('"').to_owned()).unwrap_or_default();
            if l.starts_with("name ") || l.starts_with("name=") {
                name = val(l);
            } else if (l.starts_with("repo ") || l.starts_with("repo=")) && installed.contains(&name) {
                out.insert(name.clone(), val(l));
            }
        }
    }
    // Julia's standard library ships inside Julia itself (no depot entry):
    // JuliaLang's, from the Julia modules' own share/julia/stdlib.
    for j in std::fs::read_dir(stack_root().join(own_triple()).join("eb/software/Julia")).into_iter().flatten().flatten() {
        // share/julia/stdlib/v1.11/<Name>/
        for v in std::fs::read_dir(j.path().join("share/julia/stdlib")).into_iter().flatten().flatten() {
            for p in std::fs::read_dir(v.path()).into_iter().flatten().flatten().filter(|p| p.path().is_dir()) {
                out.entry(p.file_name().to_string_lossy().into_owned()).or_insert_with(|| "https://github.com/JuliaLang/julia".to_owned());
            }
        }
    }
    *LAST.lock().unwrap() = Some((Instant::now(), out.clone()));
    out
}

/// A manifest.dcf's packages: Package, Version and Group of each record.
pub fn manifest_packages(dcf: &str) -> Vec<crate::api::KitPackage> {
    let mut out = Vec::new();
    for rec in dcf.split("\n\n") {
        let field = |k: &str| rec.lines().find_map(|l| l.strip_prefix(k).and_then(|r| r.strip_prefix(':'))).map(|v| v.trim().to_owned()).unwrap_or_default();
        let name = field("Package");
        if !name.is_empty() {
            out.push(crate::api::KitPackage { name, version: field("Version"), group: field("Group") });
        }
    }
    out
}

/// Find in files: each line of each text file under `root` that matches,
/// as the session's own account reads them. What is not the person's work
/// is not searched - git's store, build output, installed packages, hidden
/// folders - nor a file over 2 MB or one that is binary (a NUL early on).
/// At most 2,000 lines from 20,000 files.
pub fn grep(root: &str, query: &str, case: bool, regex: bool) -> Result<crate::api::GrepView, String> {
    const SKIP: [&str; 6] = ["node_modules", "renv", "dist", "__pycache__", "venv", "site-library"];
    if !root.starts_with('/') {
        return Err(format!("{root}: not a folder"));
    }
    let m = crate::project::find::Matcher::new(query, case, regex)?;
    let mut out = crate::api::GrepView { root: root.to_owned(), ..Default::default() };
    let mut stack = vec![PathBuf::from(root)];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut entries: Vec<_> = rd.flatten().collect();
        entries.sort_by_key(|e| e.file_name());
        for e in entries.into_iter().rev() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !name.starts_with('.') && !name.starts_with("target") && !SKIP.contains(&name.as_str()) {
                    stack.push(e.path());
                }
                continue;
            }
            if !ft.is_file() || e.metadata().map_or(true, |md| md.len() > 2 << 20) {
                continue;
            }
            if out.files >= 20_000 || out.hits.len() >= 2_000 {
                out.truncated = true;
                return Ok(out);
            }
            let Ok(bytes) = std::fs::read(e.path()) else { continue };
            if bytes[..bytes.len().min(8000)].contains(&0) {
                continue;
            }
            out.files += 1;
            let text = String::from_utf8_lossy(&bytes);
            if !m.is_match(&text) {
                continue;
            }
            let path = e.path().display().to_string();
            for (i, l) in text.lines().enumerate() {
                if m.is_match(l) {
                    out.hits.push(crate::api::GrepHit { path: path.clone(), line: i + 1, text: l.chars().take(300).collect() });
                    if out.hits.len() >= 2_000 {
                        out.truncated = true;
                        return Ok(out);
                    }
                }
            }
        }
    }
    // Files in the order a person reads a tree: by path, then line.
    out.hits.sort_by(|a, b| a.path.cmp(&b.path).then(a.line.cmp(&b.line)));
    Ok(out)
}

/// The use.sh that puts Lmod and the stack's modules in a shell (it picks
/// this machine's platform), when the stack is mounted - or, for a stack the
/// person built here (Software › Install), the platform folder's own: only
/// `clone` and `assemble` write the root one, and `ndexr pkgs` falls back to
/// the folder's the same way (pkgs.rs stack_use).
pub fn use_sh() -> Option<PathBuf> {
    let root = stack_root();
    [root.join("use.sh"), root.join(own_triple()).join("use.sh")].into_iter().find(|p| p.is_file())
}

/// This machine's platform directory under the root.
pub fn own_triple() -> String {
    let arch = std::env::consts::ARCH;
    format!("{arch}-{}-linux-gnu", if arch == "aarch64" { "unknown" } else { "pc" })
}

/// The catalog revision this machine's CVMFS client is on ("" off CVMFS).
pub fn stack_revision() -> String {
    xattr(&stack_root(), "user.revision")
}

/// One CVMFS client attribute of the mount (user.revision, user.logbuffer, …).
fn xattr(root: &Path, name: &str) -> String {
    Command::new("getfattr")
        .args(["--only-values", "-n", name])
        .arg(root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default()
}

/// Why modules cannot load right now, when this machine's platform has no
/// module tree: a CVMFS publish removes it for about an hour.
pub fn stack_gap() -> Option<String> {
    gap(&stack_root(), &own_triple())
}

fn gap(root: &Path, mine: &str) -> Option<String> {
    let dir = root.join(mine).join("eb/modules/all");
    (!dir.is_dir()).then(|| {
        let rev = xattr(root, "user.revision");
        format!(
            "the ndexr stack (revision {}) has no {mine} modules right now - a publish is replacing them, usually back within the hour; the next command tries again",
            if rev.is_empty() { "?" } else { &rev },
        )
    })
}

/// Why this machine has no modules, said plainly. A Mac never has the stack -
/// its R and Python belong on the cell - and only a platform folder without
/// its modules is a publish in flight (2026-10-14: a Mac said "not mounted at
/// /opt/ndexr/stack (or is mid-publish)", which it never is).
pub fn stack_absent() -> String {
    absent(&stack_root(), &own_triple(), cfg!(target_os = "linux") && arch_supported())
}

/// The CPUs the stack is built for (one CVMFS triple each, own_triple). A
/// Linux machine on another arch is a remote front end, as a Mac is.
pub const STACK_ARCHES: [&str; 2] = ["x86_64", "aarch64"];

pub fn arch_supported() -> bool {
    STACK_ARCHES.contains(&std::env::consts::ARCH)
}

/// `linux`: Linux on an arch the stack is built for.
fn absent(root: &Path, mine: &str, linux: bool) -> String {
    if !linux {
        return "no ndexr stack on this computer (it is built for Linux on x86_64 and aarch64): R, Python and the shell run from its own PATH - sign in to run on your cell for the stack's modules".into();
    }
    let published = std::fs::read_dir(root).is_ok_and(|mut d| d.any(|e| e.is_ok_and(|e| e.file_name().to_string_lossy().contains("-linux-"))));
    match gap(root, mine) {
        Some(g) if published => g,
        _ => format!("no ndexr stack at {} - R, Python and the shell run from this computer's PATH; Tools › Global Options mounts the stack or points at a local build, and Software › Install builds one here from source", root.display()),
    }
}

pub fn stack_view() -> StackView {
    let root = stack_root();
    let mine = own_triple();
    let mut triples: Vec<String> = std::fs::read_dir(&root)
        .map(|d| d.flatten().filter(|e| e.path().is_dir()).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains("-linux-")).collect())
        .unwrap_or_default();
    triples.sort();
    let updated = xattr(&root, "user.logbuffer").lines().rev().find(|l| l.contains("revision")).unwrap_or_default().to_owned();
    StackView {
        root: root.display().to_string(),
        repo: xattr(&root, "user.fqrn"),
        revision: xattr(&root, "user.revision"),
        updated,
        // The platform folder appears early in a publish; its modules last.
        ok: use_sh().is_some() && root.join(&mine).join("eb/modules/all").is_dir(),
        triples,
        mine,
        ..Default::default()
    }
}

fn settings_view() -> SettingsView {
    let saved: Option<Settings> = std::fs::read(settings_file()).ok().and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).map(Settings::one_module);
    let settings = saved.clone().unwrap_or_default();
    let stacks_here = stacks::PROFILES.iter().filter(|p| stacks::mounted(p)).map(|p| p.key.to_owned()).collect();
    let stack_in_use = stacks::chosen(&settings.stack_source).key.to_owned();
    SettingsView { chosen: saved.is_some(), settings, stack: stack::view(), platform: crate::server::has_platform(), served_as: crate::server::served_as(), stacks_here, stack_in_use }
}

/// A session `cove multiuser` starts arrives with its environment and
/// project (COVE_SEED, JSON Settings): written once, the first time this
/// session's settings folder has none, so the person's later choices stand.
pub fn seed_settings(json: &str) -> Result<(), String> {
    if settings_file().exists() {
        return Ok(());
    }
    let s: Settings = serde_json::from_str(json).map_err(|e| format!("seed: {e}"))?;
    save_settings(&s)
}

fn save_settings(s: &Settings) -> Result<(), String> {
    // Absolute by the platform's rule, not "starts with /": a Windows desktop's
    // folder is C:\... (2026-10-19, Open Folder on the desktop).
    if !s.project_dir.is_empty() && !Path::new(&s.project_dir).is_absolute() {
        return Err("the project folder must be an absolute path".into());
    }
    if !s.project_dir.is_empty() && !Path::new(&s.project_dir).is_dir() {
        return Err(format!("{} is not a folder", s.project_dir));
    }
    // The projects folder may not exist yet: it is made here, so a mistyped
    // path fails now, not at the next New project.
    let projects = s.projects_dir.trim();
    if !projects.is_empty() {
        if !Path::new(projects).is_absolute() {
            return Err("the projects folder must be an absolute path".into());
        }
        std::fs::create_dir_all(projects).map_err(|e| format!("cannot make the projects folder {projects}: {e}"))?;
    }
    if let Some(bad) = s.module.split_whitespace().find(|m| !kernel::valid_module(m)) {
        return Err(format!("not a module name: {bad}"));
    }
    let s = &s.clone().one_module();
    s.cvmfs.check()?;
    crate::host::stacks::save_choice(s.stack_source.trim())?;
    crate::host::envs::warm(&s.module);
    let f = settings_file();
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    let body = serde_json::to_vec_pretty(s).map_err(|e| e.to_string())?;
    std::fs::write(&f, body).map_err(|e| format!("{}: {e}", f.display()))?;
    // A live `ml()` list was layered on the old pick; the new pick replaces it.
    kernel::forget_live();
    Ok(())
}

// ── the module picker (Inlet 0004) ────────────────────────────────

/// "R/4.10.0" after "R/4.9.1": numbers compare as numbers.
fn natural_key(s: &str) -> Vec<(u64, String)> {
    let mut out = Vec::new();
    let mut num = String::new();
    let mut txt = String::new();
    for c in s.chars() {
        if c.is_ascii_digit() {
            if !txt.is_empty() {
                out.push((0, std::mem::take(&mut txt)));
            }
            num.push(c);
        } else {
            if !num.is_empty() {
                out.push((num.parse().unwrap_or(0), String::new()));
                num.clear();
            }
            txt.push(c);
        }
    }
    if !num.is_empty() {
        out.push((num.parse().unwrap_or(0), String::new()));
    }
    if !txt.is_empty() {
        out.push((0, txt));
    }
    out
}

fn parse_modules(listing: &str) -> (Vec<String>, Vec<String>, Vec<String>) {
    let (mut env, mut r, mut py) = (Vec::new(), Vec::new(), Vec::new());
    for l in listing.lines().map(|l| l.trim().trim_end_matches("(default)")).filter(|l| !l.ends_with(':') && kernel::valid_module(l)) {
        let l = l.to_owned();
        if l.starts_with("env/") {
            env.push(l);
        } else if l.starts_with("R/") {
            r.push(l);
        } else if l.starts_with("Python/") {
            py.push(l);
        }
    }
    for v in [&mut env, &mut r, &mut py] {
        v.sort_by_key(|m| std::cmp::Reverse(natural_key(m)));
        v.dedup();
    }
    (env, r, py)
}

/// Every module on the stack (`ml -t av`), sorted: env/ sets with a package
/// library built for this machine (operator, 2026-10-15: "for ndexr builds we
/// only use those envs" - an env/ module with no site library behind it is an
/// R and a Python with none of the set's packages, which is how 203 sessions
/// came to sit on env/r4.6_py3.14-release), then R/ and Python/, then the rest,
/// which layer on top of the env (`other`). With no library folder to read
/// (a client that does not carry it), every env/ module is offered.
/// The environment a language runs in on this machine: its own env/ set when
/// one is installed - the set the combined default uses first (R 4.6.1 with
/// Bioconductor 3.23, Python 3.13.15), else the newest of the family - and
/// for Node the stack's nodejs. "" for an unknown language or when the
/// machine has none, so the project keeps the person's default.
pub fn language_env(lang: &str) -> String {
    let (family, prefer): (&str, &[&str]) = match lang {
        "r" => ("env/R/", &["env/R/4.6.1-bioc3.23"]),
        // Python runs inside R (reticulate) until it has a console of its own:
        // a Python project opens in the combined env, which brings both, not in
        // env/Python - which has no R, so its Python console would not start.
        "python" => ("env/r", &["env/r4.6_py3.13-release"]),
        "julia" => ("env/Julia/", &["env/Julia/1.11.7"]),
        "rust" => ("env/Rust/", &["env/Rust/1.96.0"]),
        "node" => ("nodejs/", &["nodejs/22.22.2-GCCcore-13.3.0"]),
        _ => return String::new(),
    };
    let have = installed_modules();
    // The list could not be read: the language's own environment by name,
    // which every stack carries - never none, which meant the last project's.
    if have.is_empty() {
        return prefer.first().map(|p| (*p).to_owned()).unwrap_or_default();
    }
    if let Some(p) = prefer.iter().find(|p| have.iter().any(|m| m == *p)) {
        return (*p).to_owned();
    }
    have.iter().filter(|m| m.starts_with(family)).max().cloned().unwrap_or_default()
}

/// Every module this machine lists (`ml -t av`, the local env/ tree too),
/// asked at most every ten minutes: domain_at asks it once per domain.
fn installed_modules() -> Vec<String> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<(std::time::Instant, Vec<String>)>>> = std::sync::OnceLock::new();
    let mut c = CACHE.get_or_init(Default::default).lock().unwrap();
    if let Some((at, v)) = c.as_ref() {
        if at.elapsed() < Duration::from_secs(600) {
            return v.clone();
        }
    }
    let Some(u) = use_sh() else { return Vec::new() };
    let cmd = format!(". {} >/dev/null 2>&1 && {}ml -t av 2>&1", shell_quote(&u.display().to_string()), crate::catalog::source::module_use_line());
    let v: Vec<String> = capture_within(&cmd, Duration::from_secs(30))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().trim_end_matches("(default)").to_owned())
        .filter(|l| !l.is_empty() && !l.ends_with(':') && !l.ends_with('/'))
        .collect();
    // An empty answer (`ml` timed out, the stack still mounting) is never
    // kept: kept, it named no environment for ten minutes, and every R
    // project opened in the last project's - env/Rust, nodejs - with no R
    // console and a Source that did nothing (operator, 2026-11-17,
    // box.biomarker.ndexr.io).
    if !v.is_empty() {
        *c = Some((std::time::Instant::now(), v.clone()));
    }
    v
}

pub fn modules() -> Modules {
    let Some(u) = use_sh() else {
        return Modules { source: stack_absent(), ..Default::default() };
    };
    let cmd = format!(". {} >/dev/null 2>&1 && {}ml -t av 2>&1", shell_quote(&u.display().to_string()), crate::catalog::source::module_use_line());
    match capture_within(&cmd, Duration::from_secs(30)) {
        Some(out) => {
            let (mut env, r, python) = parse_modules(&out);
            if let Some(built) = built_sets(&stack_root().join(own_triple()).join("eb/site-library")) {
                env.retain(|m| built.iter().any(|b| m.strip_prefix("env/") == Some(b.as_str())));
            }
            // The person's own environment revisions (source.rs) first: newest first.
            let mut revs: Vec<String> = out.lines().map(|l| l.trim().trim_end_matches("(default)").to_owned()).filter(|l| l.starts_with("cove-env/") && !l.ends_with('/') && kernel::valid_module(l)).collect();
            revs.sort_by(|a, b| b.cmp(a));
            revs.dedup();
            revs.extend(env);
            let env = revs;
            Modules { env, r, python, other: parse_other(&out), source: u.display().to_string() }
        }
        None => Modules { source: format!("{} gave no module list", u.display()), ..Default::default() },
    }
}

/// The env sets with a package library under `lib`: a folder holding `R/`,
/// `python/` or `rust/` is one - `r4.6_py3.13-release`, or two deep for the
/// EasyBuild-shaped sets, `R/4.6.1-bioc3.23`, `Python/3.13.15` and
/// `Rust/1.96.0`. None when the folder cannot be read or holds no set.
fn built_sets(lib: &std::path::Path) -> Option<Vec<String>> {
    let is_set = |d: &std::path::Path| d.join("R").is_dir() || d.join("python").is_dir() || d.join("rust").is_dir() || d.join("julia").is_dir();
    let mut out = Vec::new();
    for e in std::fs::read_dir(lib).ok()?.flatten() {
        let d = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if !d.is_dir() {
            continue;
        }
        if is_set(&d) {
            out.push(name);
            continue;
        }
        for c in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if c.path().is_dir() && is_set(&c.path()) {
                out.push(format!("{name}/{}", c.file_name().to_string_lossy()));
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// The modules that are not env/, R/ or Python/: what layers on an env (GDAL,
/// CmdStan, Apptainer …), sorted by name, newest version first.
fn parse_other(listing: &str) -> Vec<String> {
    let mut v: Vec<String> = listing
        .lines()
        .map(|l| l.trim().trim_end_matches("(default)").to_owned())
        .filter(|l| !l.ends_with(':') && !l.ends_with('/') && kernel::valid_module(l) && !["env/", "R/", "Python/", "cove-env/"].iter().any(|p| l.starts_with(p)))
        .collect();
    v.sort_by(|a, b| {
        let (na, nb) = (a.split('/').next().unwrap_or(a).to_ascii_lowercase(), b.split('/').next().unwrap_or(b).to_ascii_lowercase());
        na.cmp(&nb).then_with(|| natural_key(b).cmp(&natural_key(a)))
    });
    v.dedup();
    v
}

/// The picked interpreter's packages, read by a fresh process in the same
/// module the kernel starts in.
fn packages(lang: Lang) -> Result<Vec<Package>, String> {
    let s = saved_settings();
    let pre = kernel::module_prefix(s.module(lang))?;
    let list = match lang {
        Lang::Shell => return Err("the shell has no packages - its programs are the stack's modules (ml av)".into()),
        Lang::Julia => r#"julia --startup-file=no -e 'import Pkg; for (_, p) in Pkg.dependencies(); p.is_direct_dep && println(p.name, "\t", something(p.version, "")); end'"#,
        Lang::R => r#"Rscript --no-save --no-restore -e 'ip <- utils::installed.packages()[, c("Package", "Version"), drop = FALSE]; cat(paste(ip[, 1], ip[, 2], sep = "\t"), sep = "\n")'"#,
        Lang::Python => r#"python3 -c 'import importlib.metadata as m; print("\n".join(sorted({"%s\t%s" % (d.metadata["Name"], d.version) for d in m.distributions() if d.metadata["Name"]})))'"#,
    };
    let out = capture_within(&format!("{pre}{list}"), Duration::from_secs(60)).ok_or_else(|| format!("{} gave no package list", lang.word()))?;
    let mut v: Vec<Package> = out
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(n, ver)| Package { name: n.to_owned(), version: ver.to_owned() })
        .collect();
    v.sort_by_key(|p| p.name.to_lowercase());
    v.dedup_by(|a, b| a.name == b.name);
    Ok(v)
}

// ── documents: quarto render, shown in the Viewer ───────────────────────────

/// The HTML pages renders wrote: the only files `serve` hands out as pages
/// (/view), so the Viewer cannot be turned into a way to read any file.
fn rendered() -> &'static std::sync::Mutex<std::collections::HashSet<PathBuf>> {
    static R: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<PathBuf>>> = std::sync::OnceLock::new();
    R.get_or_init(Default::default)
}

/// A page this server rendered, by its path (server.rs's /view).
pub fn rendered_page(path: &str) -> Option<Vec<u8>> {
    let p = PathBuf::from(path).canonicalize().ok()?;
    if !rendered().lock().unwrap().contains(&p) {
        return None;
    }
    std::fs::read(p).ok()
}

/// "Output created: report.html" - quarto's own word for where it wrote, so
/// an output-dir or output-file in the project is followed.
fn render_output(lines: &[String], dir: &Path, file: &Path) -> Option<PathBuf> {
    let named = lines.iter().rev().find_map(|l| l.trim().strip_prefix("Output created: ")).map(|o| dir.join(o.trim()));
    let guess = file.with_extension("html");
    named.into_iter().chain([guess]).find(|p| p.is_file())
}

/// The Quarto project `dir` is in: the nearest folder up with a _quarto.yml.
fn quarto_project(dir: &Path) -> Option<PathBuf> {
    dir.ancestors().take(8).find(|d| d.join("_quarto.yml").is_file() || d.join("_quarto.yaml").is_file()).map(Path::to_path_buf)
}

/// Render (Ctrl+Shift+K) for the Viewer, as RStudio's Render and Quarto
/// preview do, with every chunk's language at hand: the module the whole IDE
/// runs in (QUARTO_PYTHON and RETICULATE_PYTHON name its Python), and the
/// stack's Quarto unless the module brings one.
/// - a lone .qmd / .Rmd / .ipynb / .md: one self-contained HTML page;
/// - a file in a Quarto project, or its _quarto.yml: the project's way
///   (website, book), into its output folder;
/// - a Jupyter Book's _toc.yml: `jupyter-book build`, into _build/html;
/// - mkdocs.yml: `mkdocs build`, into site/;
/// - a page already written (.html): shown as it is.
/// The Viewer shows the result with everything beside it (`site_address`), so
/// a site's other pages, styles and search work.
fn render(path: &str) -> Result<JobView, String> {
    let file = resolve(path)?;
    let name = file.display().to_string();
    if !crate::api::renders(&name) {
        return Err(format!("{name}: Render takes .qmd, .Rmd, .ipynb, .md, .html, _quarto.yml, _toc.yml or mkdocs.yml"));
    }
    let dir = file.parent().filter(|d| d.is_dir()).ok_or_else(|| format!("{name}: no such folder"))?.to_path_buf();
    let short = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = short.to_ascii_lowercase();
    if lower.ends_with(".html") || lower.ends_with(".htm") {
        let view = site_address(&dir, &file)?;
        return Ok(JobView { what: format!("view {short}"), ok: Some(true), output: name, view, ..Default::default() });
    }
    let s = saved_settings();
    let pre = kernel::module_prefix(&s.module)?;
    let stack = use_sh().map(|u| format!(". {} >/dev/null 2>&1; ", shell_quote(&u.display().to_string()))).unwrap_or_default();
    let python = "py=$(command -v python3) && export QUARTO_PYTHON=\"$py\" RETICULATE_PYTHON=\"$py\"; ";
    let quarto = "command -v quarto >/dev/null || ml Quarto >/dev/null 2>&1; \
         command -v quarto >/dev/null || { echo 'no quarto: pick an env/ module (they carry Quarto), or the stack is not mounted'; exit 127; }; ";
    let tool = |t: &str, pip: &str| format!("command -v {t} >/dev/null || {{ echo 'no {t} in this module: pip install --user {pip} (Terminal), then Render again'; exit 127; }}; ");
    let q = |p: &Path| shell_quote(&p.display().to_string());
    // (folder it runs in, the command, the folder the Viewer serves when the
    // output is not found from what the tool printed, the page it expects)
    let (run_in, cmd, root, page): (PathBuf, String, PathBuf, PathBuf) = match lower.as_str() {
        "_toc.yml" => {
            let html = dir.join("_build").join("html");
            (dir.clone(), format!("{}echo '$ jupyter-book build .'; jupyter-book build .", tool("jupyter-book", "jupyter-book")), html.clone(), html.join("index.html"))
        }
        "mkdocs.yml" | "mkdocs.yaml" => {
            let site = dir.join("site");
            (dir.clone(), format!("{}echo '$ mkdocs build'; mkdocs build -f {}", tool("mkdocs", "mkdocs"), q(&file)), site.clone(), site.join("index.html"))
        }
        _ => match quarto_project(&dir) {
            Some(proj) => {
                let whole = lower.starts_with("_quarto.");
                let target = if whole { String::new() } else { format!(" {}", q(&file)) };
                (proj.clone(), format!("{quarto}{python}echo '$ quarto render{target}'; quarto render{target}"), proj.clone(), proj.join("index.html"))
            }
            None => {
                let f = shell_quote(&short);
                (dir.clone(), format!("{quarto}{python}echo \"$ quarto render {short} --to html\"; quarto render {f} --to html -M embed-resources:true"), dir.clone(), file.with_extension("html"))
            }
        },
    };
    let cmd = format!("cd {} || exit 1; {pre}{stack}{cmd}", q(&run_in));
    stack::jobs::start(
        "render",
        format!("render {short}"),
        cmd,
        Some(Box::new(move |ok, v: &mut JobView| {
            if !ok {
                return;
            }
            let out = render_output(&v.lines, &run_in, &page).or_else(|| page.is_file().then(|| page.clone())).and_then(|p| p.canonicalize().ok());
            if let Some(out) = out {
                // A project's pages sit in its output folder (_site, _book …):
                // serve that whole folder, so links between them hold.
                let root = match out.strip_prefix(&root).ok().and_then(|r| r.components().next()) {
                    Some(std::path::Component::Normal(first)) if root.join(first).is_dir() => root.join(first),
                    _ => root.clone(),
                };
                v.view = site_address(&root, &out).unwrap_or_default();
                v.output = out.display().to_string();
                rendered().lock().unwrap().insert(out);
            }
        })),
    )
}

// ── the Viewer: pages shown in place, with what sits beside them ─────────────
// RStudio serves a widget's or a render's folder (/session/, /rmd_output/)
// and proxies local servers (/p/<port>/); Positron the same through its
// proxy. Here: /site/<id>/<page> and /port/<port>/<path> (server.rs).

/// The folders the Viewer may show, by id: each render's output and each page
/// R handed its viewer. Only these are served as /site/<id>/…, so the Viewer
/// is not a way to read any file.
fn sites() -> &'static std::sync::Mutex<std::collections::HashMap<String, PathBuf>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, PathBuf>>> = std::sync::OnceLock::new();
    S.get_or_init(Default::default)
}

/// `page` shown with the folder `root` (above it) served beside it: the
/// Viewer address /site/<id>/<page's path under root>. The id is the folder's
/// own, so a render shown again keeps its address.
pub fn site_address(root: &Path, page: &Path) -> Result<String, String> {
    use std::hash::{Hash, Hasher};
    let root = root.canonicalize().map_err(|e| format!("{}: {e}", root.display()))?;
    let page = page.canonicalize().map_err(|e| format!("{}: {e}", page.display()))?;
    let rel = page.strip_prefix(&root).map_err(|_| format!("{} is not under {}", page.display(), root.display()))?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    root.hash(&mut h);
    let id = format!("{:016x}", h.finish());
    sites().lock().unwrap().insert(id.clone(), root);
    Ok(format!("/site/{id}/{}", crate::backend::url_encode(&rel.to_string_lossy())))
}

/// What R handed its viewer, as a Viewer address (RStudio's
/// module_context::viewer): a local server goes through /port/, another web
/// address stays as it is, a file (an htmlwidget's index.html in tempdir, a
/// rendered page) is served with its folder.
pub fn view_address(target: &str) -> Result<String, String> {
    let t = target.trim();
    if t.starts_with("http://") || t.starts_with("https://") {
        let local = crate::api::local_url(t).filter(|_| ["://localhost:", "://127.0.0.1:", "://0.0.0.0:", "://[::1]:", "://[::]:"].iter().any(|h| t.contains(h)));
        return Ok(local.and_then(|l| crate::api::port_address(&l)).unwrap_or_else(|| t.to_owned()));
    }
    let p = PathBuf::from(t.strip_prefix("file://").unwrap_or(t));
    let p = if p.is_dir() { p.join("index.html") } else { p };
    let root = p.parent().ok_or_else(|| format!("{t}: no folder"))?;
    site_address(root, &p)
}

/// A file of a folder the Viewer shows: (type, bytes). A folder answers with
/// its index.html. Nothing outside the folder.
pub fn site_file(id: &str, rel: &str) -> Option<(&'static str, Vec<u8>)> {
    let p = site_path(id, rel)?;
    let bytes = std::fs::read(&p).ok()?;
    Some((web_type(&p), bytes))
}

/// The file /site/<id>/<rel> names, on this machine (the desktop app opens it directly).
pub fn site_path(id: &str, rel: &str) -> Option<PathBuf> {
    let root = sites().lock().unwrap().get(id)?.clone();
    let p = root.join(rel.trim_start_matches('/'));
    let p = if p.is_dir() { p.join("index.html") } else { p };
    p.canonicalize().ok().filter(|p| p.starts_with(&root) && p.is_file())
}

/// The Content-Type a site's file is served as.
fn web_type(p: &Path) -> &'static str {
    match p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("xml") => "application/xml",
        Some("txt" | "md" | "csv") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("pdf") => "application/pdf",
        Some("wasm") => "application/wasm",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        _ => "application/octet-stream",
    }
}

// ── files: the Files pane and the source editor ─────────────────────────────

/// "" is the project folder, "~" home; anything else must be absolute.
fn resolve(path: &str) -> Result<PathBuf, String> {
    let p = path.trim();
    if p.is_empty() {
        Ok(PathBuf::from(project_dir(&saved_settings())))
    } else if p == "~" {
        Ok(PathBuf::from(home()))
    } else if let Some(rest) = p.strip_prefix("~/") {
        Ok(PathBuf::from(home()).join(rest))
    } else if p.starts_with('/') {
        Ok(PathBuf::from(p))
    } else {
        Err(format!("{p}: not an absolute path"))
    }
}

// ── transfers: the Files pane's Download and Upload ─────────────────────────
// (operator, 2026-10-13: "need to be able to download files through the ide"
// / "upload files as well"). The machine side, shared by the desktop app on
// its own machine (backend Local) and by `serve`'s /download and /upload.

/// A previewable file's bytes and type, for the server's /preview.
pub fn preview(path: &str) -> Result<(&'static str, Vec<u8>), String> {
    let ct = crate::api::preview_type(path).ok_or_else(|| format!("{path}: not something shown in place"))?;
    let p = resolve(path)?;
    let m = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    if !m.is_file() || m.len() > TRANSFER_MAX {
        return Err(format!("{}: not a file this can show", p.display()));
    }
    std::fs::read(&p).map(|b| (ct, b)).map_err(|e| format!("{}: {e}", p.display()))
}

/// The most one transfer carries: in memory on the way through `serve`.
pub const TRANSFER_MAX: u64 = 2 << 30;

/// `path` as a download, packed as `format` asks (operator, 2026-11-18:
/// "tar.gz or zip, both should be available to select prior to download"):
/// "zip" or "tar.gz" pack a file or a folder; "" is a file's own bytes, or a
/// folder as .tar.gz (what /download?path= always gave). tar is on every
/// machine the server runs on, Windows 10+ included; zip is made here
/// (zippack.rs). Returns the file name to save it as and the bytes.
pub fn download(path: &str, format: &str) -> Result<(String, Vec<u8>), String> {
    let p = resolve(path)?.canonicalize().map_err(|e| format!("{path}: {e}"))?;
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "root".into());
    let m = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    if format == "zip" {
        return Ok((format!("{name}.zip"), crate::host::zippack::zip(&p, TRANSFER_MAX)?));
    }
    if m.is_dir() || format == "tar.gz" {
        let parent = p.parent().ok_or("not a folder that can be archived")?;
        let out = Command::new("tar")
            .arg("-czf")
            .arg("-")
            .arg("-C")
            .arg(parent)
            .arg(&name)
            .output()
            .map_err(|e| format!("tar: {e}"))?;
        if !out.status.success() {
            return Err(format!("tar {}: {}", p.display(), String::from_utf8_lossy(&out.stderr).trim()));
        }
        if out.stdout.len() as u64 > TRANSFER_MAX {
            return Err(format!("{} is {} MB packed - more than one download carries ({} MB)", p.display(), out.stdout.len() >> 20, TRANSFER_MAX >> 20));
        }
        return Ok((format!("{name}.tar.gz"), out.stdout));
    }
    if m.len() > TRANSFER_MAX {
        return Err(format!("{} is {} MB - more than one download carries ({} MB)", p.display(), m.len() >> 20, TRANSFER_MAX >> 20));
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok((name, bytes))
}

/// A file name that is only a name: no folders, no `..`, nothing hidden by
/// accident of a leading dot from another system's metadata (`._x`, `.DS_Store`).
pub fn upload_name_ok(name: &str) -> bool {
    !name.is_empty() && name.len() < 256 && !name.contains(['/', '\\', '\0']) && name != "." && name != ".." && !name.starts_with("._") && name != ".DS_Store"
}

/// Write an uploaded file into folder `dir` as `name`, never over an existing
/// file: "data.csv" already there becomes "data (1).csv". Written beside and
/// renamed, so a broken transfer leaves no half file. Returns where it went.
pub fn upload(dir: &str, name: &str, body: impl std::io::Read) -> Result<String, String> {
    if !upload_name_ok(name) {
        return Err(format!("not a file name: {name:?}"));
    }
    crate::host::storage::refuse_growth()?;
    // A domain's supporting_docs/ is made on its first upload - where the
    // Agents tab puts what a person attaches to a message (operator,
    // 2026-10-15: "upload files and supporting docs through cove agent").
    // Only that folder, and only inside a folder that is already there. It
    // used to also ask that the parent pass domain_at, on the path as sent
    // (2026-10-15, operator: "when there isnt a folder it needs one" - a
    // domain without module.r/nginx.conf/CONTEXT.md, or a ~/ path, skipped
    // the create and the upload failed on a folder that did not exist).
    let want = resolve(dir)?;
    if want.file_name().is_some_and(|n| n == "supporting_docs") && !want.exists() && want.parent().is_some_and(Path::is_dir) {
        std::fs::create_dir(&want).map_err(|e| format!("could not make {}: {e}", want.display()))?;
    }
    let d = want.canonicalize().map_err(|e| format!("{dir}: {e}"))?;
    if !d.is_dir() {
        return Err(format!("{}: not a folder", d.display()));
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_owned(), format!(".{e}")),
        _ => (name.to_owned(), String::new()),
    };
    let target = (0..1000)
        .map(|i| d.join(if i == 0 { name.to_owned() } else { format!("{stem} ({i}){ext}") }))
        .find(|p| !p.exists())
        .ok_or_else(|| format!("{name}: too many copies already"))?;
    let tmp = d.join(format!(".{name}.ndexr-upload-{}", std::process::id()));
    let written = std::fs::File::create(&tmp)
        .and_then(|mut f| std::io::copy(&mut body.take(TRANSFER_MAX + 1), &mut f))
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("{}: {e}", target.display())
        })?;
    if written > TRANSFER_MAX {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("{name} is more than one upload carries ({} MB)", TRANSFER_MAX >> 20));
    }
    std::fs::rename(&tmp, &target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", target.display())
    })?;
    Ok(target.display().to_string())
}

// ── domains: what a session starts in ───────────────────────────────────────

/// A folder name that is a hostname: `blog.ndexr.io`, `koodle.io`.
pub fn is_host_name(n: &str) -> bool {
    n.contains('.') && !n.starts_with(['.', '_', '-']) && n.len() < 254 && n.chars().all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
}

pub(crate) fn domain_at(p: &Path) -> Option<crate::api::DomainInfo> {
    let host = p.file_name()?.to_str()?.to_owned();
    let module = p.join("module.r").is_file();
    if !is_host_name(&host) || !(module || p.join("nginx.conf").is_file() || p.join("CONTEXT.md").is_file() || p.join("run.sh").is_file() || p.join(".cove/app.env").is_file()) {
        return None;
    }
    // "# Blog · blog.ndexr.io" -> "Blog"
    let title = std::fs::read_to_string(p.join("CONTEXT.md"))
        .ok()
        .and_then(|t| t.lines().next().map(str::to_owned))
        .and_then(|l| l.strip_prefix("# ").map(|t| t.split(" · ").next().unwrap_or(t).trim().to_owned()))
        .filter(|t| *t != host)
        .unwrap_or_default();
    let env = crate::host::envs::pinned(p).unwrap_or_default();
    let target = std::fs::read_to_string(p.join("nginx.conf")).map(|c| domain_target(&c)).unwrap_or_default();
    let app = app_kind(&target, p).to_owned();
    // The folder's own mtime moves only when an entry is added or removed;
    // its top-level files move on every save.
    let mtime = |q: &Path| std::fs::metadata(q).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    let modified = std::fs::read_dir(p).into_iter().flatten().flatten().filter(|e| !e.file_name().to_string_lossy().starts_with('.')).map(|e| mtime(&e.path())).max().unwrap_or(0).max(mtime(p));
    let language = crate::host::apps::language(p).0;
    let env_default = language_env(&language);
    let framework = framework_of(p, &language, &app).to_owned();
    // When the folder was made (its birth time): Connect's Created for a
    // project the registry does not list. The registry's own created_at wins
    // where there is one (app.rs connect_view) - folders were copied across
    // in the 2026-08-29 move, so their birth times say that day.
    let created = std::fs::metadata(p).and_then(|m| m.created()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
    Some(crate::api::DomainInfo { host, path: p.display().to_string(), title, module, env, target, app, owner: String::new(), modified, language, env_default, framework, created })
}

/// A domain's mark (apps.rs framework), else its app kind's: a Rust app kept
/// in app/Cargo.toml (cove.ndexr.io, scripts/new-rust-app.sh) has no
/// language at the top of its folder, and showed "Not detected" in Connect.
fn framework_of(dir: &Path, language: &str, kind: &str) -> &'static str {
    match crate::host::apps::framework(dir, language) {
        "" if kind == "rust" => "rust",
        "" if kind == "shiny" => "shiny",
        f => f,
    }
}

/// What kind of application answers a domain (api::DomainInfo::app), from
/// what serves it and what its folder holds. A Rust app is a port target
/// plus `app/Cargo.toml` (scripts/new-rust-app.sh, Cove's "as a Rust app");
/// a module or the owner's cell is the Shiny module.r.
fn app_kind(target: &str, dir: &Path) -> &'static str {
    match target {
        t if t.starts_with("port:") => {
            if dir.join("app/Cargo.toml").is_file() || dir.join("Cargo.toml").is_file() {
                "rust"
            } else {
                "process"
            }
        }
        "module" | "cell" => "shiny",
        "redirect" => "redirect",
        "" if dir.join("module.r").is_file() => "shiny",
        _ => "",
    }
}

/// What is behind a platform domain (app_kinds).
pub(crate) struct AppFacts {
    /// api::DomainInfo::app.
    pub kind: &'static str,
    /// apps.rs language: r, python, julia, rust, node, or "".
    pub language: String,
    /// apps.rs framework: its mark's key, or "".
    pub framework: &'static str,
}

/// Every platform domain's app kind, language and framework, by host:
/// src/domains/<host> and src/domains/<owner>/<host>, as the platform checkout
/// has them. A folder with none of the three is left out.
fn app_kinds() -> std::collections::HashMap<String, AppFacts> {
    let mut m = std::collections::HashMap::new();
    let mut look = |d: &Path| {
        let Some(h) = d.file_name().and_then(|n| n.to_str()).filter(|n| is_host_name(n)) else { return false };
        let target = std::fs::read_to_string(d.join("nginx.conf")).map(|c| domain_target(&c)).unwrap_or_default();
        let kind = app_kind(&target, d);
        let language = crate::host::apps::language(d).0;
        let framework = framework_of(d, &language, kind);
        if !kind.is_empty() || !language.is_empty() {
            m.insert(h.to_owned(), AppFacts { kind, language, framework });
        }
        true
    };
    for e in std::fs::read_dir(PLATFORM_DOMAINS).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() && !look(&p) {
            for f in std::fs::read_dir(&p).into_iter().flatten().flatten() {
                if f.path().is_dir() {
                    look(&f.path());
                }
            }
        }
    }
    m
}

/// What serves a domain, from its nginx.conf - the Rust twin of
/// src/r/nginx/target.r `domain_target()`, for a label only (switching stays
/// in R, behind its checks). The `# ndexr-target:` marker wins; otherwise the
/// last `location /` block (the HTTPS one, in every generated vhost) is read.
pub(crate) fn domain_target(conf: &str) -> String {
    for l in conf.lines() {
        if let Some(t) = l.trim_start().strip_prefix('#').map(str::trim_start).and_then(|t| t.strip_prefix("ndexr-target:")) {
            if let Some(t) = t.split_whitespace().next() {
                return t.to_owned();
            }
        }
    }
    let code: Vec<&str> = conf.lines().map(|l| l.split('#').next().unwrap_or("")).collect();
    if code.iter().any(|l| l.contains("include conf.d/cell.conf")) {
        return "cell".into();
    }
    let is_root = |l: &&str| l.trim().strip_prefix("location").map(str::trim_start).and_then(|r| r.strip_prefix('/')).is_some_and(|r| r.trim_start().starts_with('{'));
    let Some(start) = code.iter().rposition(|l| is_root(l)) else { return String::new() };
    let mut depth = 0i32;
    let mut body = Vec::new();
    for l in &code[start..] {
        depth += l.matches('{').count() as i32 - l.matches('}').count() as i32;
        body.push(*l);
        if depth <= 0 {
            break;
        }
    }
    let Some(pp) = body.iter().find_map(|l| l.split_once("proxy_pass").map(|(_, r)| r.trim().trim_end_matches(';').trim())) else {
        return if body.iter().any(|l| l.contains("return 301") || l.contains("return 302")) {
            "redirect".into()
        } else if body.iter().any(|l| l.contains("conf.d/code_proxy.conf")) {
            "code".into()
        } else {
            String::new()
        };
    };
    let hp = pp.trim_start_matches("http://").trim_start_matches("https://").split('/').next().unwrap_or("");
    match hp.split_once(':') {
        _ if hp == "ndexr_app" || hp == "ndexr_tenant" => "module".into(),
        Some(("127.0.0.1" | "localhost", port)) => format!("port:{port}"),
        _ if !hp.contains('.') && !hp.contains(':') => format!("upstream:{hp}"),
        _ => format!("remote:{hp}"),
    }
}

/// The domains under the project folder (or the project folder itself, when
/// it is one): `<root>/<host>/` and `<root>/<owner>/<host>/`. A session in a
/// cell starts in its domains/; on a server, ~/ndexr/src/domains is tried too.
/// The folder of one of this person's domains, by host.
/// A project of this Cove's that answers at its own name (operator,
/// 2026-11-22: "what does it take to get the subdomain to work generally" -
/// "we need cove to manage as much as possible internally"): its port app's
/// APP_PORT, and whether anyone may open it (APP_PUBLIC=1) or only who is
/// signed in to this Cove. The projects are listed at most every 5 s.
pub fn project_port(name: &str) -> Option<(u16, bool)> {
    use std::collections::HashMap;
    use std::time::{Duration, Instant};
    static SEEN: std::sync::Mutex<Option<(Instant, HashMap<String, (u16, bool)>)>> = std::sync::Mutex::new(None);
    let mut seen = SEEN.lock().unwrap();
    if seen.as_ref().is_none_or(|(at, _)| at.elapsed() > Duration::from_secs(5)) {
        let mut map = HashMap::new();
        for d in list_domains() {
            let env = std::fs::read_to_string(crate::project::dotcove::contract_dir(Path::new(&d.path)).join("app.env")).unwrap_or_default();
            let kv = crate::host::apps::read_env(&env);
            let get = |k: &str| kv.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
            if get("APP_KIND") == Some("port") {
                if let Some(port) = get("APP_PORT").and_then(|p| p.parse::<u16>().ok()) {
                    map.insert(d.host.to_ascii_lowercase(), (port, get("APP_PUBLIC") == Some("1")));
                }
            }
        }
        *seen = Some((Instant::now(), map));
    }
    seen.as_ref().and_then(|(_, m)| m.get(&name.trim().trim_end_matches('.').to_ascii_lowercase()).copied())
}

fn app_dir(host: &str) -> Result<String, String> {
    let h = host.trim().to_ascii_lowercase();
    list_domains().into_iter().find(|d| d.host == h).map(|d| d.path).ok_or_else(|| format!("{h} is not one of your domains"))
}

fn list_domains() -> Vec<crate::api::DomainInfo> {
    let s = saved_settings();
    let roots = [domains_root(Path::new(&project_dir(&s))), PathBuf::from(home()).join("domains"), PathBuf::from(home()).join("ndexr/src/domains")];
    // The projects folder always, and the older places as before: the loop
    // below stops at the first of them that holds projects, so the projects
    // folder is read on its own - otherwise a person's first project there
    // hid every project they already had elsewhere.
    // With a folder chosen, the default is read too: projects made there
    // before the choice stay listed (they are left where they are).
    let mut ours = vec![projects_root(&s)];
    let fallback = projects_root(&Settings { projects_dir: String::new(), ..s.clone() });
    if !ours.contains(&fallback) {
        ours.push(fallback);
    }
    let mut out: Vec<crate::api::DomainInfo> = ours.iter().flat_map(|r| std::fs::read_dir(r).into_iter().flatten().flatten()).filter_map(|e| domain_at(&e.path())).collect();
    let mine = out.len();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(&root) else { continue };
        let mut owners = Vec::new();
        for e in rd.flatten().filter(|e| e.path().is_dir()) {
            let name = e.file_name().to_string_lossy().into_owned();
            match domain_at(&e.path()) {
                Some(d) => out.push(d),
                None if !name.contains('.') && !name.starts_with(['.', '_']) => owners.push(e.path()),
                None => {}
            }
        }
        // Owner folders only inside a domains folder: a home directory's
        // subfolders are not owners (~/ndexr-stray's retired stubs were
        // listed as the person's domains, 2026-10-13).
        if out.len() == mine && root.file_name().is_none_or(|n| n != "domains") {
            continue;
        }
        // The person's own owner folder, when it is here: theirs, not every
        // owner's. A session running as the platform account read the whole
        // checkout, so 300-odd customers' domains were "my projects" and a
        // customer's site came up as the default (operator,
        // 2026-10-19). Other people's domains are reached by acting as them
        // (multiuser.rs act_as), not by listing them as yours.
        let me = std::env::var("COVE_OWNER").ok().filter(|o| !o.is_empty());
        if let Some(mine) = me.as_deref().and_then(|m| owners.iter().find(|o| o.file_name().is_some_and(|n| n == m)).cloned()) {
            owners = vec![mine];
        }
        for o in owners {
            for e in std::fs::read_dir(&o).into_iter().flatten().flatten() {
                if let Some(d) = domain_at(&e.path()) {
                    out.push(d);
                }
            }
        }
        if out.len() > mine {
            break;
        }
    }
    // Domains shared with this person to work on (shares.rs), at ~/shared.
    #[cfg(unix)]
    for (_, p, owner) in crate::host::shares::granted(Path::new(&home())) {
        if let Some(mut d) = domain_at(Path::new(&p)) {
            d.owner = owner;
            out.push(d);
        }
    }
    let mut out = one_per_host(out);
    out.sort_by(|a, b| a.host.cmp(&b.host));
    out.truncate(5000);
    out
}

/// One entry per host. The router (registry.r resolve_domain_segments)
/// prefers domains/<host>/ over domains/<owner>/<host>/ only when the flat one
/// has a module.r. Flat stubs holding just a CONTEXT.md or nginx.conf shadowed
/// ten owner domains here - repo.ndexr.io opened as "no module.r yet"
/// (operator, 2026-10-15). So the first of each host wins, flat ones having
/// been read first, unless a later one has the module.r it lacks.
fn one_per_host(found: Vec<crate::api::DomainInfo>) -> Vec<crate::api::DomainInfo> {
    let mut at: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut keep: Vec<crate::api::DomainInfo> = Vec::new();
    for d in found {
        match at.get(&d.host) {
            Some(&i) if !keep[i].module && d.module => keep[i] = d,
            Some(_) => {}
            None => {
                at.insert(d.host.clone(), keep.len());
                keep.push(d);
            }
        }
    }
    keep
}

/// Where to list domains from, given the project folder. Picking a domain
/// makes it the project, and the list was then read from inside it: one domain,
/// with no way back to the others (operator, 2026-10-15: "doesnt seem to have
/// all of the domains anymore ... selecting in but cant select out"). A
/// project that is a domain lists from the `domains` folder holding it - its
/// owner folder's parent - else from the folder it sits in.
fn domains_root(project: &Path) -> PathBuf {
    // The domain the project is, or is inside (a file ask's folder).
    let Some(dom) = project.ancestors().find(|a| domain_at(a).is_some()) else {
        return project.to_path_buf();
    };
    dom.ancestors()
        .skip(1)
        .find(|a| a.file_name().is_some_and(|n| n == "domains"))
        .or_else(|| dom.parent())
        .map_or_else(|| project.to_path_buf(), Path::to_path_buf)
}

/// A new domain's folder, shaped like the shared contract (AGENTS-shared §2):
/// the id is the hostname, the server is wrapped in measured().
fn create_domain(host: &str, parent: &str, starter: &str) -> Result<crate::api::DomainInfo, String> {
    use crate::project::starters::{RUST_APP, SHINY_BLANK, SHINY_CHAOS};
    let folder = starter_files(starter);
    if ![RUST_APP, SHINY_BLANK, SHINY_CHAOS].contains(&starter) && folder.is_empty() {
        return Err(format!("{starter}: not a starter Cove knows"));
    }
    let host = host.trim().to_ascii_lowercase();
    if !is_host_name(&host) || !host.contains('.') || host.ends_with('.') {
        return Err(format!("{host}: not a domain name - like mysite.ndexr.io"));
    }
    if let Some(m) = crate::api::posit_mark(&host) {
        return Err(crate::api::posit_mark_words(&host, m));
    }
    crate::host::storage::refuse_growth()?;
    let parent = resolve(parent)?;
    if !parent.is_dir() {
        return Err(format!("{}: no such folder", parent.display()));
    }
    let dir = parent.join(&host);
    let replacing = new_signup_page(&dir, &host);
    if !replacing {
        if list_domains().iter().any(|d| d.host == host) {
            return Err(format!("{host} already exists - pick it from the list"));
        }
        // Someone else's already, on this machine: refused before any file is
        // made. A taken name used to be created anyway, fail at publish ("belongs
        // to someone else"), and leave a scaffold at src/domains/<host> that
        // shadowed the owner's own src/domains/<owner>/<host> (2026-10-23,
        // meet.ndexr.io). The registry still has the last word at publish.
        if let Some(other) = held_elsewhere(&host, &parent) {
            return Err(format!("{host} is taken (it is {}) - choose another name", other));
        }
        std::fs::create_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let laid = starter_layout(&host, starter)?;
    if !laid.is_empty() {
        write_laid(&dir, laid)?;
        // Left behind if it cannot go (a folder only its owner may delete
        // from): app.env now says what the project is, and it wins.
        if replacing {
            let _ = std::fs::remove_file(dir.join("module.r"));
        }
        // Compiled already where Cove has a seed for this starter, so Up
        // starts at once; then a Build of its own, in the background, which
        // the strip shows as compiling (apps.rs `compiling`).
        seed_into(&dir, starter);
        let _ = capture_within(&format!("cd {} && git init -q && git add -A", shell_quote(&dir.display().to_string())), Duration::from_secs(20));
        let info = domain_at(&dir).ok_or_else(|| "created, but it does not read back as a domain".to_owned())?;
        let app = crate::host::apps::status(&dir);
        if app.kind == "port" && app.verbs.iter().any(|v| v == "up") {
            let _ = crate::host::apps::start_new(&host, &dir);
        }
        return Ok(info);
    }
    // Chaos Lab (templates/shiny-app/module.r, the same experiment as the Rust
    // starter's src/chaos.rs) or the blank page below.
    let module = if starter == SHINY_CHAOS { SHINY_CHAOS_MODULE.replace("__HOST__", &host) } else { format!(
        "# {host} - created in Cove.\n\
         \n\
         #' @export\n\
         ui_module <- function(id, request) {{\n  \
           box::use(\n    shiny = shiny[NS, tags],\n    r / prelude / prelude[shell, inputs]\n  )\n  \
           ns <- shiny$NS(id)\n  \
           shell$ui_shell(\n    ns        = ns,\n    request   = request,\n    \
             head_args = list(title = \"{host}\", domain = \"{host}\", stylesheet = \"css/home.css\"),\n    \
             body      = inputs$page(\n      inputs$hero(\"{host}\", lead = \"A new ndexr site.\")\n    )\n  )\n}}\n\
         \n\
         .server_inner <- function(id = \"{host}\", site_session) {{\n  \
           box::use(r / prelude / prelude[shell])\n  \
           shell$server_shell(id, site_session)\n}}\n\
         \n\
         #' @export\n\
         server_module <- local({{\n  \
           box::use(r / prelude / prelude[timing])\n  \
           wrapped <- timing$measured(\"{host}\", .server_inner)\n  \
           function(id = \"{host}\", ...) wrapped(id, ...)\n}})\n"
    ) };
    let context = format!("# {host} · {host}\nOne-paragraph purpose.\n\n## Files\n- module.r\n\n## Auth\nYou and the people you share it with (Project › Access).\n\n## Open\n- Publish it at route.ndexr.io (DNS, certificate, vhost).\n");
    let mut files: Vec<(String, String)> = vec![("module.r".into(), module), ("CONTEXT.md".into(), context), (".gitignore".into(), ".Rhistory\n.RData\n*.log\n".into())];
    app_contract(&mut files, "r", "module", "")?;
    crate::project::dotcove::write(&dir, &crate::project::dotcove::lay(&host, files, &starter_files))?;
    // Each domain is its own repo; `ndexr push` / Push all gives it a remote.
    let _ = capture_within(&format!("cd {} && git init -q && git add -A", shell_quote(&dir.display().to_string())), Duration::from_secs(20));
    domain_at(&dir).ok_or_else(|| "created, but it does not read back as a domain".into())
}

/// The app in `dir` moved, whole, to `dir/<was_rel>`: every file but its
/// history (.git), its supporting_docs and .cove, and from .cove all but the
/// earlier conversions (was/) and run state (run/). Returns `was_rel`.
fn move_aside(dir: &Path, was_rel: &str) -> Result<String, String> {
    let was = dir.join(was_rel);
    std::fs::create_dir_all(was.join(".cove")).map_err(|e| format!("{}: {e}", was.display()))?;
    let keep = |n: &str| matches!(n, ".git" | "supporting_docs" | ".cove");
    for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if !keep(&n) {
            std::fs::rename(e.path(), was.join(&n)).map_err(|err| format!("{n}: {err}"))?;
        }
    }
    let dot = dir.join(".cove");
    for e in std::fs::read_dir(&dot).map_err(|e| format!("{}: {e}", dot.display()))?.flatten() {
        let n = e.file_name().to_string_lossy().into_owned();
        if !matches!(n.as_str(), "was" | "run") {
            std::fs::rename(e.path(), was.join(".cove").join(&n)).map_err(|err| format!(".cove/{n}: {err}"))?;
        }
    }
    Ok(was_rel.to_owned())
}

/// Make `host` another framework (operator, 2026-11-22: "push a button to
/// convert the framework ... transfer to any domain across any framework with
/// agent assistance"; rust.riemann.ndexr.io began as Shiny and was meant to
/// be a Rust explorer). The app it is - every file but its history, its
/// supporting_docs and Cove's own run state - moves, whole, into
/// .cove/was/<framework>-<unix time>/, its .cove contract with it, so nothing
/// is lost and the agent can read it; `starter` is laid in its place, on the
/// same port, so an address routed to it still reaches the app. Committed.
fn convert_domain(host: &str, starter: &str) -> Result<crate::api::Converted, String> {
    let dir = PathBuf::from(app_dir(host)?);
    // Laid first, in memory: a starter that cannot be laid moves nothing.
    let laid = starter_layout(host, starter)?;
    if laid.is_empty() {
        return Err(format!("{starter}: Cove cannot convert to this starter yet - pick one with its own folder"));
    }
    crate::host::storage::refuse_growth()?;
    let cove = crate::project::dotcove::contract_dir(&dir);
    if cove.join("down.sh").is_file() {
        let _ = capture_within(&format!("cd {} && ./{}/down.sh", shell_quote(&dir.display().to_string()), cove.strip_prefix(&dir).map(|p| p.display().to_string()).unwrap_or_else(|_| ".cove".into())), Duration::from_secs(30));
    }
    let st = crate::host::apps::status(&dir);
    let fw = crate::host::apps::framework(&dir, &st.language);
    let from = if !fw.is_empty() { fw.to_owned() } else if !st.language.is_empty() { st.language.clone() } else { st.kind.clone() };
    let from = if from.is_empty() { "app".to_owned() } else { from };
    let was_rel = move_aside(&dir, &format!(".cove/was/{from}-{}", crate::ui::software::now_secs()))?;
    write_laid(&dir, laid)?;
    // The same port as before, where it had one: its address is routed there.
    if !st.port.is_empty() {
        let env = crate::project::dotcove::contract_dir(&dir).join("app.env");
        if let Ok(t) = std::fs::read_to_string(&env) {
            let t: String = t.lines().map(|l| if l.trim_start().starts_with("APP_PORT=") { format!("APP_PORT={}", st.port) } else { l.to_owned() }).collect::<Vec<_>>().join("\n") + "\n";
            let _ = std::fs::write(&env, t);
        }
    }
    seed_into(&dir, starter);
    let _ = capture_within(&format!("cd {} && git add -A && git commit -qm {}", shell_quote(&dir.display().to_string()), shell_quote(&format!("convert: {from} -> {starter} (the old app is in {was_rel})"))), Duration::from_secs(30));
    let domain = domain_at(&dir).ok_or_else(|| "converted, but it does not read back as a domain".to_owned())?;
    let app = crate::host::apps::status(&dir);
    if app.kind == "port" && app.verbs.iter().any(|v| v == "build") {
        let _ = crate::host::apps::action(host, &dir, "build");
    }
    Ok(crate::api::Converted { domain, was: was_rel, from })
}

/// A folder made in the last week that holds the page signup laid
/// (site_provision.r `.module_skeleton`), byte for byte: nobody has written
/// anything there, so a first project may take its place. Older untouched
/// pages stay: they are sites people already have (five on 2026-10-30).
fn new_signup_page(dir: &Path, host: &str) -> bool {
    const WEEK: Duration = Duration::from_secs(7 * 24 * 3600);
    let new = std::fs::metadata(dir).and_then(|m| m.created()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age < WEEK);
    new && std::fs::read_to_string(dir.join("module.r")).is_ok_and(|m| m.trim_end() == SHINY_CHAOS_MODULE.replace("__HOST__", host).trim_end())
}

/// A starter's files for `host`, the app contract's with them: the Rust app
/// and every templates/starters/<id> folder, with the host and a crate name
/// filled in (2026-10-25). Empty for the two Shiny starters create_domain
/// writes itself.
fn starter_layout(host: &str, starter: &str) -> Result<Vec<(String, String)>, String> {
    let rust = starter == crate::project::starters::RUST_APP;
    let mut laid: Vec<(String, String)> = if rust {
        rust_app_files(host).into_iter().map(|(n, b)| (n.to_owned(), b)).collect()
    } else {
        starter_files(starter).iter().map(|(name, body)| (name.to_string(), body.replace("__HOST__", host).replace("__CRATE__", &crate_name(host)))).collect()
    };
    if laid.is_empty() {
        return Ok(laid);
    }
    app_contract(&mut laid, if rust { "rust" } else { "r" }, if rust { "port" } else { "module" }, "/health")?;
    // Every project has a CONTEXT.md - it is how a folder reads back as a
    // domain (domain_at) and what its agent reads first. A starter that
    // ships none still gets one: testy.ndexr.io (julia-finance, from a
    // session on a build of the folder before its CONTEXT.md existed) was
    // created, did not read back, and the chooser closed on nothing
    // (operator, 2026-10-25).
    if !laid.iter().any(|(n, _)| n == "CONTEXT.md") {
        laid.push(("CONTEXT.md".to_owned(), format!("# {host} · {host}\nA project started in Cove ({starter}).\n\n## Files\n- .cove/ - how Cove builds, starts and stops it\n\n## Auth\nYou and the people you share it with (Project › Access).\n")));
    }
    // The contract and every type's scripts in .cove/ (dotcove.rs): the
    // person's folder starts with their app and nothing else.
    Ok(crate::project::dotcove::lay(host, laid, &starter_files))
}

fn write_laid(dir: &Path, laid: Vec<(String, String)>) -> Result<(), String> {
    for (name, body) in laid {
        let name = name.as_str();
        let p = dir.join(name);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(&p, body).map_err(|e| format!("{name}: {e}"))?;
        // Executable where there is such a bit; Windows has none (the
        // Windows build failed on this, four-box-windows, 2026-10-19).
        #[cfg(unix)]
        if name.ends_with(".sh") {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o775));
        }
    }
    Ok(())
}

// ── seeds: compiled starters, built once ────────────────────────────────────
// (operator, 2026-10-25: "if one of the apps requires compilation then we
// should make sure we have a user ready project already compiled ... so that
// it can start right up"). onprem/run.sh `publish_seeds` lays each compiled
// starter out for SEED_HOST (`cove seed lay`), runs its build.sh, and
// publishes the build outputs - target/, dist/, Cargo.lock - to
// SEEDS/<starter>/, read-only for every account. A new project copies them
// in, and its first build compiles the project's own crate alone.
//
// It works because every account builds against the same crate sources at
// the same path, the stack's vendored set (templates/starters/_rust/env.sh):
// cargo marks a dependency dirty when "the path to the source changed", and
// each person's ~/.cargo/registry is a path of their own - a seed built over
// crates.io recompiled every crate for the next person (measured 2026-10-25).
// Over the vendor, rust-api: 81 s cold, 7 s from the seed (27 crates fresh,
// 1 compiled). The Rust app cold: 14 min 30 s, 301 crates, at load 36-46.

/// Where the published seeds are.
pub const SEEDS: &str = "/usr/local/lib/cove/seeds";
const SEED_HOST: &str = "seed.ndexr.io";

/// What a seed was built from: the starter's files as laid for SEED_HOST,
/// app.env aside (its port differs per lay). A seed is used only by a Cove
/// whose templates give the same stamp - a starter edited since was never
/// built, and its seed would claim a build of other code.
fn seed_stamp(starter: &str) -> Result<String, String> {
    let mut laid = starter_layout(SEED_HOST, starter)?;
    laid.retain(|(n, _)| n != ".cove/app.env");
    laid.sort();
    // FNV-1a: the same number from every build of Cove, which a std hasher
    // does not promise.
    let mut h: u64 = 0xcbf29ce484222325;
    for (n, b) in &laid {
        for byte in n.bytes().chain([0]).chain(b.bytes()).chain([0]) {
            h = (h ^ byte as u64).wrapping_mul(0x100000001b3);
        }
    }
    Ok(format!("{h:016x}"))
}

/// The starters a seed is built for: those that compile (a Cargo.toml).
fn seeded_starters() -> Vec<&'static str> {
    std::iter::once(crate::project::starters::RUST_APP)
        .chain(crate::project::starters::FOLDER_STARTERS.iter().copied())
        .filter(|s| starter_layout(SEED_HOST, s).is_ok_and(|l| l.iter().any(|(n, _)| n == "Cargo.toml")))
        .collect()
}

/// The starter's published build outputs into a new project, when there is a
/// seed for exactly these templates. Nothing otherwise: the project builds
/// from nothing, and says so while it does.
fn seed_into(dir: &Path, starter: &str) -> bool {
    let seed = Path::new(SEEDS).join(starter);
    let Ok(stamp) = std::fs::read_to_string(seed.join(".stamp")) else { return seed_fetch(dir, starter) };
    if seed_stamp(starter).ok().as_deref() != Some(stamp.trim()) {
        return false;
    }
    let names: Vec<&str> = ["target", "dist", "Cargo.lock"].into_iter().filter(|p| seed.join(p).exists()).collect();
    let parts: Vec<String> = names.iter().map(|p| shell_quote(&seed.join(p).display().to_string())).collect();
    let laid: Vec<String> = names.iter().map(|p| shell_quote(&dir.join(p).display().to_string())).collect();
    // `cp -a` keeps the mtimes cargo's fingerprints need, and the seed's
    // mode 755 with them, which on the project's ACL'd folder sets the mask to
    // r-x: the agent (the platform account) could not write target/ or dist/
    // and built into a temp directory instead (request #170, 2026-11-01).
    // g+w lifts the mask back; the mtimes stay.
    !parts.is_empty()
        && capture_within(&format!("cp -a {} {} && chmod -R g+w {} && echo seeded", parts.join(" "), shell_quote(&dir.display().to_string()), laid.join(" ")), Duration::from_secs(180)).is_some_and(|o| o.trim() == "seeded")
}

/// Where published seeds are: beside Cove's releases on the install feed,
/// `seeds/<starter>/<stamp>/<platform>.tar.gz` and its `.sha256` (onprem/run.sh
/// publish_seeds uploads them).
fn seed_url(starter: &str, stamp: &str) -> String {
    let base = std::env::var("COVE_FEED").ok().filter(|b| !b.is_empty()).unwrap_or_else(|| "https://repo.ndexr.io/ndexr/cove".into());
    format!("{}/seeds/{starter}/{stamp}/{}.tar.gz", base.trim_end_matches('/'), crate::host::update::platform())
}

/// A seed from the install feed, where this machine has none of its own: a
/// server or desktop of someone's own (operator, 2026-11-27, a new Rust
/// project on ndexrtesting.com, 556 s and still compiling: "binaries should be
/// precompiled"). The starter's binary, its browser build and Cargo.lock,
/// built by ndexr for exactly these templates (the stamp) and checked against
/// their sha256, so Start runs at once; the project's own build then runs in
/// the background as with a local seed. Not in airgap mode, and nothing when
/// the feed has none for this stamp and platform: the project builds as before.
fn seed_fetch(dir: &Path, starter: &str) -> bool {
    if saved_settings().airgap {
        return false;
    }
    let Ok(stamp) = seed_stamp(starter) else { return false };
    let url = seed_url(starter, &stamp);
    let d = shell_quote(&dir.display().to_string());
    let u = shell_quote(&url);
    // The binary keeps the name the project's own build gives it, so the
    // project's run.sh starts it, and its own build replaces it in place.
    // A browser build's page is the project's own (laid with its name).
    let script = format!(
        "set -e; t=\"$(mktemp -d)\"; trap 'rm -rf \"$t\"' EXIT; \
         curl -fsSL -m 120 -o \"$t/s.tgz\" {u}; curl -fsSL -m 30 -o \"$t/s.sha\" {u}.sha256; \
         [ \"$(sha256sum \"$t/s.tgz\" | cut -d' ' -f1)\" = \"$(cut -d' ' -f1 \"$t/s.sha\")\" ]; \
         tar -xzf \"$t/s.tgz\" -C {d}; \
         if [ -d {d}/dist/web ] && [ -f {d}/web/index.html ]; then cp {d}/web/index.html {d}/dist/web/index.html; fi; \
         [ -x {d}/target/release/app ] && echo seeded"
    );
    capture_within(&script, Duration::from_secs(180)).is_some_and(|o| o.trim().ends_with("seeded"))
}

/// `cove seed list | stamp <starter> | lay <starter> <dir>` - for
/// onprem/run.sh `publish_seeds`.
pub fn seed_cli(args: &[String]) -> i32 {
    let a = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
    let r: Result<String, String> = match a(0) {
        "list" => Ok(seeded_starters().join("\n")),
        "stamp" => seed_stamp(a(1)),
        "lay" if !a(2).is_empty() => {
            let dir = Path::new(a(2));
            std::fs::create_dir(dir).map_err(|e| format!("{}: {e}", dir.display())).and_then(|_| starter_layout(SEED_HOST, a(1))).and_then(|l| if l.is_empty() { Err(format!("{}: not a starter Cove lays out", a(1))) } else { write_laid(dir, l) }).and_then(|_| seed_stamp(a(1)))
        }
        // A person's own site, laid into a folder that exists (scripts/reset-user-sites.py).
        "site" if !a(2).is_empty() => {
            let host = a(1).trim().to_ascii_lowercase();
            starter_layout(&host, crate::project::starters::NODE_SITE).and_then(|l| write_laid(Path::new(a(2)), l)).map(|_| format!("{host}: laid in {}", a(2)))
        }
        _ => Err("cove seed list | stamp <starter> | lay <starter> <new dir> | site <host> <dir>".into()),
    };
    match r {
        Ok(out) => {
            println!("{out}");
            0
        }
        Err(e) => {
            eprintln!("seed: {e}");
            1
        }
    }
}

/// The Shiny starter, compiled in like the Rust one so every Cove (the
/// desktop builds on neo and the laptop too) lays down the same file.
const SHINY_CHAOS_MODULE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/shiny-app/module.r"));

/// Whose `host` is on this machine, when it is not under `parent`: the
/// platform's flat src/domains/<host> or another owner's
/// src/domains/<owner>/<host>. A session that cannot read the platform
/// checkout sees nothing here, and publish decides.
fn held_elsewhere(host: &str, parent: &Path) -> Option<String> {
    let root = Path::new(PLATFORM_DOMAINS);
    let mine = parent.canonicalize().ok();
    let flat = root.join(host);
    if flat.is_dir() && mine.as_deref() != root.canonicalize().ok().as_deref() {
        return Some("a platform site".into());
    }
    for e in std::fs::read_dir(root).ok()?.flatten() {
        let p = e.path();
        if !p.join(host).is_dir() || p.canonicalize().ok() == mine {
            continue;
        }
        let owner = e.file_name().to_string_lossy().into_owned();
        if !owner.contains('.') {
            return Some(format!("{owner}'s"));
        }
    }
    None
}

/// The second way to start a domain (operator, 2026-10-18: "start with the
/// shiny module as it is, or as a Rust-based application like Cove, but
/// minimal"): templates/rust-app, compiled in so every Cove can lay it down.
/// An egui UI for the browser and the desktop, `app serve` on 127.0.0.1. It
/// opens on a small atlas of the ndexr stack (operator, 2026-10-24: "some sort
/// of interesting visual to start like a simple version of atlas ... so the
/// user has inspiration"), with the first app's media core over the stack's
/// FFmpeg (clean audio, cut, extract) as its second tab.
const RUST_APP: &[(&str, &str)] = &[
    ("Cargo.toml", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/Cargo.toml"))),
    ("CONTEXT.md", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/CONTEXT.md"))),
    (".gitignore", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/gitignore"))),
    ("run.sh", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/run.sh"))),
    ("build-web.sh", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/build-web.sh"))),
    ("web/index.html", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/web/index.html"))),
    ("src/main.rs", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/src/main.rs"))),
    ("src/api.rs", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/src/api.rs"))),
    ("src/app.rs", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/src/app.rs"))),
    ("src/atlas.rs", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/src/atlas.rs"))),
    ("src/catalog.rs", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/src/catalog.rs"))),
    ("src/chaos.rs", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/src/chaos.rs"))),
    ("src/media.rs", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/src/media.rs"))),
    ("src/server.rs", include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/templates/rust-app/src/server.rs"))),
];

/// The template's files for `host`: its name in the UI and docs, and a crate
/// name Cargo accepts (my-app.ndexr.io -> my-app-ndexr-io).
fn rust_app_files(host: &str) -> Vec<(&'static str, String)> {
    let krate = crate_name(host);
    RUST_APP.iter().map(|(name, body)| (*name, body.replace("__HOST__", host).replace("__CRATE__", &krate))).collect()
}

include!(concat!(env!("OUT_DIR"), "/starter_files.rs"));

/// The app contract (testy.ndexr.io/APP-CONTRACT.md, referral #699): every
/// project carries app.env - APP_LANGUAGE, APP_KIND (port | module),
/// APP_PORT, APP_HEALTH - and the verbs Cove's buttons and agents run:
/// build.sh, and for a port app up.sh and down.sh. A starter's own app.env
/// wins (its __PORT__ becomes a port of its own); otherwise one is written
/// from `language`, `kind` and `health`. The verbs come from
/// templates/starters/_port and _<language>, never over a starter's own file.
fn app_contract(laid: &mut Vec<(String, String)>, language: &str, kind: &str, health: &str) -> Result<(), String> {
    let field = |text: &str, key: &str| text.lines().find_map(|l| l.trim().strip_prefix(&format!("{key}=")).map(|v| v.trim().to_owned()));
    let (language, kind) = match laid.iter().find(|(n, _)| n == "app.env") {
        Some((_, text)) => (field(text, "APP_LANGUAGE").unwrap_or_else(|| language.to_owned()), field(text, "APP_KIND").unwrap_or_else(|| kind.to_owned())),
        None => {
            let mut env = format!("# How this app is built, started and stopped (APP-CONTRACT.md): read by\n# build.sh, up.sh and down.sh, by Cove's buttons and by agents.\nAPP_LANGUAGE={language}\nAPP_KIND={kind}\n");
            if kind == "port" {
                env.push_str(&format!("APP_PORT=__PORT__\nAPP_HEALTH={health}\n"));
            }
            laid.push(("app.env".to_owned(), env));
            (language.to_owned(), kind.to_owned())
        }
    };
    if kind == "port" {
        // Each `__<NAME>PORT__` gets a port of its own: APP_PORT's, and a
        // second one where the app runs two servers (Unreal's game talks to
        // its signalling server on APP_STREAMER_PORT).
        let mut given: Vec<u16> = Vec::new();
        for (n, body) in laid.iter_mut() {
            if n != "app.env" {
                continue;
            }
            let mut marks: Vec<String> = body.split("__").skip(1).step_by(2).filter(|m| m.ends_with("PORT") && m.chars().all(|c| c.is_ascii_uppercase() || c == '_')).map(|m| format!("__{m}__")).collect();
            marks.dedup();
            for mark in marks {
                let port = free_app_port_except(&given)?;
                given.push(port);
                *body = body.replace(&mark, &port.to_string());
            }
        }
    }
    let shared: Vec<String> = if kind == "port" { vec!["_port".to_owned(), format!("_{language}")] } else { vec!["_r".to_owned()] };
    for id in &shared {
        for (name, body) in starter_files(id) {
            if !laid.iter().any(|(n, _)| n == name) {
                laid.push((name.to_owned(), body.to_owned()));
            }
        }
    }
    Ok(())
}

/// A port no other project has: the lowest in 8830-8999 that is in no
/// domain's app.env, in no vhost's proxy_pass, and free to bind now
/// (APP-CONTRACT.md section 6). Every starter used to default to 8800, so a
/// second project brought up collided with the first.
fn free_app_port() -> Result<u16, String> {
    free_app_port_except(&[])
}

/// free_app_port, passing over `also` (ports just given to the same project).
fn free_app_port_except(also: &[u16]) -> Result<u16, String> {
    let mut taken = taken_ports();
    taken.extend(also);
    // What `cove multiuser` (root) saw: a person's session cannot enter
    // /home/ubuntu (750), so its own scan finds only that person's projects
    // and no vhost at all - asdfasdf.fdrennan.ndexr.io got 8848, which an
    // orphaned vhost (dddxxcgg.ndexr.io) still proxied to, and routing it
    // failed (2026-11-01).
    if let Ok(t) = std::fs::read_to_string(ports_file()) {
        taken.extend(t.split_whitespace().filter_map(|p| p.parse::<u16>().ok()));
    }
    (8830..=8999)
        .find(|p| !taken.contains(p) && std::net::TcpListener::bind(("127.0.0.1", *p)).is_ok())
        .ok_or_else(|| "no free app port in 8830-8999".to_owned())
}

/// Where `cove multiuser` writes the ports every app.env and vhost on the
/// machine holds, one per line, readable by every session.
pub fn ports_file() -> PathBuf {
    PathBuf::from(std::env::var("COVE_STATE_DIR").unwrap_or_else(|_| "/var/lib/cove".into())).join("ports.taken")
}

/// The app ports this process can see in use: every project's app.env
/// APP_PORT and every vhost's proxy_pass to 127.0.0.1 (APP-CONTRACT.md
/// section 6).
pub fn taken_ports() -> std::collections::HashSet<u16> {
    let mut taken = std::collections::HashSet::new();
    // Each project's, in .cove/ or (not adopted yet) at its root.
    let both = |p: PathBuf| [p.join(".cove/app.env"), p.join("app.env")];
    let mut envs: Vec<PathBuf> = list_domains().iter().flat_map(|d| both(PathBuf::from(&d.path))).collect();
    for top in std::fs::read_dir(PLATFORM_DOMAINS).into_iter().flatten().flatten() {
        envs.extend(both(top.path()));
        for sub in std::fs::read_dir(top.path()).into_iter().flatten().flatten() {
            envs.extend(both(sub.path()));
        }
    }
    for f in envs {
        if let Ok(t) = std::fs::read_to_string(&f) {
            // APP_PORT and any other APP_*PORT the app listens on (APP_STREAMER_PORT).
            for l in t.lines() {
                if let Some((k, v)) = l.trim().split_once('=') {
                    if k.starts_with("APP_") && k.ends_with("PORT") {
                        if let Ok(p) = v.trim().trim_matches(|c| c == '"' || c == '\'').parse::<u16>() {
                            taken.insert(p);
                        }
                    }
                }
            }
        }
    }
    for f in std::fs::read_dir(platform_root().join("nginx/sites-enabled")).into_iter().flatten().flatten() {
        if let Ok(t) = std::fs::read_to_string(f.path()) {
            for (i, _) in t.match_indices("127.0.0.1:") {
                let digits: String = t[i + 10..].chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(p) = digits.parse::<u16>() {
                    taken.insert(p);
                }
            }
        }
    }
    taken
}

/// Settings › Info's Type menu (dotcove.rs set_type): the project
/// becomes `language`, given a port of its own when it needs one.
fn app_type(dir: &Path, language: &str) -> Result<String, String> {
    crate::project::dotcove::adopt(dir, &starter_files)?;
    let running = crate::host::apps::status(dir).running;
    crate::project::dotcove::set_type(dir, language, running, &free_app_port)
}

/// `cove dotcove <dir>...`: each project folder moved into .cove/
/// (dotcove.rs adopt), saying what it did; 1 when any failed.
pub fn dotcove_cli(dirs: &[String]) -> i32 {
    let mut code = 0;
    for d in dirs {
        match crate::project::dotcove::adopt(Path::new(d), &starter_files) {
            Ok(true) => println!("moved   {d}"),
            Ok(false) => println!("skipped {d}"),
            Err(e) => {
                eprintln!("failed  {d}: {e}");
                code = 1;
            }
        }
    }
    code
}

/// A starter folder's files, (path, template), in path order; empty for an
/// id with no folder.
pub(crate) fn starter_files(id: &str) -> Vec<(&'static str, &'static str)> {
    STARTER_FILES.iter().filter(|(s, _, _)| *s == id).map(|(_, p, b)| (*p, *b)).collect()
}

/// The paths a starter lays down (tour.rs tests: the files a walkthrough
/// points at must exist in the project it creates).
#[cfg(test)]
pub fn starter_file_names(id: &str) -> Vec<String> {
    starter_files(id).into_iter().map(|(p, _)| p.to_owned()).collect()
}

/// A crate name Cargo accepts for `host` (my-app.ndexr.io ->
/// my-app-ndexr-io), as rust_app_files makes it.
fn crate_name(host: &str) -> String {
    let mut krate: String = host.chars().map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' }).collect();
    if !krate.starts_with(|c: char| c.is_ascii_alphabetic()) {
        krate.insert_str(0, "site-");
    }
    krate
}

/// Commit and push every repo under `root` (the person's domains, or one
/// owner's folder): git.ndexr.io's "every repo for one owner" sweep.
fn push_all(root: &str) -> Result<JobView, String> {
    let root = resolve(root)?.canonicalize().map_err(|e| format!("{root}: {e}"))?;
    if !root.is_dir() {
        return Err(format!("{}: not a folder", root.display()));
    }
    let r = shell_quote(&root.display().to_string());
    let cmd = format!(
        "cd {r} || exit 1; command -v ndexr >/dev/null || {{ echo 'no ndexr CLI on PATH here - Push all needs it'; exit 127; }}; \
         echo \"$ ndexr bulk-commit {r}\"; ndexr bulk-commit .; s=$?; {PUSH_VERSIONS} exit $s"
    );
    stack::jobs::start("push", format!("push all ({})", root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()), cmd, None)
}

/// Push all for `owner` from `cove multiuser`: their domains folder is the
/// platform checkout's owner folder (each home's ~/domains is a bind mount of
/// it), committed and pushed by the platform account, which has the ndexr CLI
/// and the gitlab key a person's own account does not. Unix only, as its one
/// caller (multiuser.rs) is.
#[cfg(unix)]
pub fn push_all_as(owner: &str) -> Result<JobView, String> {
    if owner.is_empty() || !owner.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-.".contains(c)) || owner.starts_with('.') {
        return Err(format!("{owner}: not an ndexr username"));
    }
    let root = Path::new(PLATFORM_DOMAINS).join(owner);
    if !root.is_dir() {
        return Err("you have no domains to save yet - make one from the Domain menu".into());
    }
    let (_, user) = platform_account().ok_or("no platform checkout on this machine")?;
    let r = shell_quote(&root.display().to_string());
    // The vault first: each project a numbered version under the account's own
    // prefix (crate git/vault.rs). It does not decide the exit status, and a CLI
    // without `vault` skips it, so git saves as before either way.
    let o = shell_quote(owner);
    let inner = format!(
        "cd {r} || exit 1; if ndexr vault --help >/dev/null 2>&1; then echo \"$ ndexr vault save --all\"; \
         ndexr vault save --all --as {o} || echo '  versions were not saved this time'; fi; \
         echo \"$ ndexr bulk-commit {r}\"; ndexr bulk-commit .; s=$?; {PUSH_VERSIONS} exit $s"
    );
    let cmd = format!("exec runuser -l {} -c {}", shell_quote(&user), shell_quote(&inner));
    stack::jobs::start(push_kind(owner), format!("push all ({owner})"), cmd, None)
}

/// After `bulk-commit`, which pushes each repo's checked-out branch only
/// (ndexr.ndexr.io handle_push.rs `finalize_push`): every branch and tag of
/// each software project, so a project restored elsewhere has every version
/// its `.cove-source` and the environments built from it name. Shell, for the
/// two commands above; a repo with no `origin` yet is left for next time.
const PUSH_VERSIONS: &str = "for f in */.cove-source; do [ -f \"$f\" ] || continue; d=\"${f%/.cove-source}\"; \
     git -C \"$d\" remote get-url origin >/dev/null 2>&1 || continue; \
     echo \"$ git push --all --tags ($d)\"; git -C \"$d\" push -q origin --all && git -C \"$d\" push -q origin --tags || { echo \"  not every version of $d was pushed\"; s=1; }; done;";

/// Register `hosts` as `owner`'s, in order (a paradigm parent before its
/// project): each one already theirs, or under a domain of theirs (never a
/// bare apex like ndexr.io), and owned by no one else - a name someone else
/// holds is refused, never taken over. Answers the hosts now theirs.
const CLAIM_R: &str = r#"
args <- commandArgs(TRUE); owner <- args[[length(args)]]; hosts <- tolower(args[-length(args)])
out <- function(x) { cat(jsonlite::toJSON(x, auto_unbox = FALSE, null = "null")); quit(status = 0) }
options(box.path = Sys.getenv("NDEXR_SRC"))
box::use(ud = r/users/user_domains, reg = r/domains/registry, pg = r/connections/postgres)
email <- tryCatch({
  con <- pg$connection_postgres_direct(); on.exit(pool::poolReturn(con), add = TRUE)
  r <- DBI::dbGetQuery(con, "SELECT email FROM ndexr_users WHERE lower(username) = lower($1) LIMIT 1", params = list(owner))
  if (nrow(r)) r$email[[1]] else ""
}, error = function(e) "")
if (!nzchar(email)) out(list(error = paste0("no ndexr account named ", owner)))
done <- character()
for (h in hosts) {
  if (!grepl("^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$", h)) out(list(error = paste0(h, ": not a domain name")))
  if (isTRUE(ud$user_domains_owns(email, h))) { done <- c(done, h); next }
  other <- tryCatch(reg$domain_owner(h), error = function(e) NULL)
  if (length(other) && nzchar(other[[1]]) && tolower(other[[1]]) != tolower(email)) out(list(error = paste0(h, " belongs to someone else")))
  labels <- strsplit(h, ".", fixed = TRUE)[[1]]
  ups <- if (length(labels) > 3) vapply(2:(length(labels) - 2), function(k) paste(labels[k:length(labels)], collapse = "."), "") else character()
  if (!any(vapply(ups, function(u) isTRUE(ud$user_domains_owns(email, u)) || u %in% done, logical(1)))) out(list(error = paste0(h, " is not under a domain of yours")))
  r <- tryCatch(ud$user_domains_add(email, h), error = function(e) e)
  if (inherits(r, "error")) out(list(error = conditionMessage(r)))
  done <- c(done, h)
}
out(list(claimed = done))
"#;

/// `Query::PublishDomain`: provision_start for one domain of `owner`'s. Their
/// own (user_domains), or one nobody holds yet - never someone else's. The
/// access level starts at `owner`: the person and the people they share it
/// with (operator, 2026-11-17: new sites default to "you and selected other
/// people and never public"; `login` before that). Project › Access widens it.
const PUBLISH_R: &str = r#"
args <- commandArgs(TRUE); owner <- args[[length(args)]]; h <- tolower(args[[1]])
# Who may open it, from the project's .cove/app.env APP_ACCESS (multiuser.rs
# PublishDomain): "owner" unless the project asks for wider.
access <- if (length(args) >= 3 && args[[2]] %in% c("public", "login", "owner")) args[[2]] else "owner"
out <- function(x) { cat(jsonlite::toJSON(x, auto_unbox = TRUE, null = "null")); quit(status = 0) }
options(box.path = Sys.getenv("NDEXR_SRC"))
box::use(ud = r/users/user_domains, reg = r/domains/registry, pg = r/connections/postgres, sp = r/users/site_provision)
if (!grepl("^[a-z0-9]([a-z0-9-]*[a-z0-9])?(\\.[a-z0-9]([a-z0-9-]*[a-z0-9])?)+$", h)) out(list(error = paste0(h, ": not a domain name")))
if (!grepl("\\.ndexr\\.io$", h)) out(list(error = paste0(h, ": only names under ndexr.io publish from Cove - a domain of your own goes through route.ndexr.io")))
email <- tryCatch({
  con <- pg$connection_postgres_direct(); on.exit(pool::poolReturn(con), add = TRUE)
  r <- DBI::dbGetQuery(con, "SELECT email FROM ndexr_users WHERE lower(username) = lower($1) LIMIT 1", params = list(owner))
  if (nrow(r)) r$email[[1]] else ""
}, error = function(e) "")
if (!nzchar(email)) out(list(error = paste0("no ndexr account named ", owner)))
other <- tryCatch(reg$domain_owner(h), error = function(e) "")
if (length(other) && nzchar(other[[1]]) && tolower(other[[1]]) != tolower(email)) out(list(error = paste0(h, " belongs to someone else")))
# A project under a domain is the domain owner's to make (operator, 2026-10-19:
# "create projects for any subdomain under their domain even if they havent
# subscribed"): every parent up to ndexr.io must be theirs or nobody's -
# x.someone.ndexr.io is not another person's to take.
parts <- strsplit(h, ".", fixed = TRUE)[[1]]
if (length(parts) > 3) for (i in 2:(length(parts) - 2)) {
  up <- paste(parts[i:length(parts)], collapse = ".")
  o <- tryCatch(reg$domain_owner(up), error = function(e) "")
  if (length(o) && nzchar(o[[1]]) && tolower(o[[1]]) != tolower(email)) out(list(error = paste0(h, " is under ", up, ", which belongs to someone else")))
}
res <- tryCatch(sp$provision_start(h, email, access = access, category = "Cove"), error = function(e) e)
if (inherits(res, "error")) out(list(error = conditionMessage(res)))
out(list(host = h, run = if (is.list(res) && !is.null(res$id)) as.character(res$id) else ""))
"#;

pub fn publish_as(owner: &str, host: &str) -> Result<String, String> {
    publish_with(owner, host, "login")
}

/// Publish `host` with `access` (public, login or owner) as the level nginx
/// gates it at from its first request: an Inlet starter runs as its owner
/// with no login of its own, so it must never be open to anyone signed in.
pub fn publish_with(owner: &str, host: &str, access: &str) -> Result<String, String> {
    if !["public", "login", "owner"].contains(&access) {
        return Err(format!("{access}: not an access level"));
    }
    let h = host.trim().to_ascii_lowercase();
    if !is_host_name(&h) || !h.contains('.') {
        return Err(format!("{h}: not a domain name"));
    }
    // On the web is where the mark would be served: never.
    if let Some(m) = crate::api::posit_mark(&h) {
        return Err(crate::api::posit_mark_words(&h, m));
    }
    let v = platform_r_as(owner, "publish.R", PUBLISH_R, &[&h, access], 120)?;
    Ok(v["host"].as_str().unwrap_or(&h).to_owned())
}

pub fn claim_as(owner: &str, hosts: &[String]) -> Result<Vec<String>, String> {
    if hosts.is_empty() || hosts.len() > 8 || hosts.iter().any(|h| !is_host_name(&h.trim().to_ascii_lowercase())) {
        return Err("name one to eight domains to register".into());
    }
    let args: Vec<&str> = hosts.iter().map(|h| h.trim()).collect();
    let v = platform_r_as(owner, "claim.R", CLAIM_R, &args, 45)?;
    Ok(v["claimed"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).collect()).unwrap_or_default())
}

/// Where a saved domain's repository is: `ndexr push`'s own rule
/// (handle_push.rs `add_default_origin`), base + `<owner>/<host>`.
fn remote_for(owner: &str, host: &str) -> String {
    let base = std::env::var("NDEXR_GIT_REMOTE_BASE").ok().filter(|b| !b.is_empty() && b != "off").unwrap_or_else(|| "git@gitlab.com:ndexr/domains".into());
    format!("{}/{owner}/{host}.git", base.trim_end_matches('/'))
}

/// A project of `owner`'s that this machine does not have: their saved
/// repository cloned into their domains folder - every branch, the newest
/// `work/<version>` checked out - then answered as a domain to open. Only
/// ever their own namespace on the remote, so only ever their own projects.
pub fn restore_as(owner: &str, host: &str) -> Result<crate::api::DomainInfo, String> {
    let host = host.trim().to_ascii_lowercase();
    if !is_host_name(&host) {
        return Err(format!("{host}: not a domain name"));
    }
    if owner.is_empty() || !owner.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-.".contains(c)) || owner.starts_with('.') {
        return Err(format!("{owner}: not an ndexr username"));
    }
    let root = Path::new(PLATFORM_DOMAINS).join(owner);
    let root = if root.is_dir() { root } else { std::env::var("HOME").map(|h| Path::new(&h).join("domains")).map_err(|_| "no domains folder".to_owned())? };
    let dir = root.join(&host);
    if dir.exists() {
        return domain_at(&dir).ok_or_else(|| format!("{} is here but is not a domain", dir.display()));
    }
    let (d, u) = (shell_quote(&dir.display().to_string()), shell_quote(&remote_for(owner, &host)));
    // Not on the git remote (or no remote at all): the private S3 git archive
    // the autosave service mirrors every project to (operator, 2026-10-25:
    // "they dont even need to have the git repos if we are already storing
    // versioned snapshots in s3"). Its names: <owner>__<host> for a person's
    // project, <host> or src__domains__<host> for older ones.
    let bucket = std::env::var("NDEXR_GIT_S3_BUCKET").ok().filter(|b| !b.is_empty()).unwrap_or_else(|| "ndexr-git-dr".into());
    let prefix = std::env::var("NDEXR_GIT_S3_PREFIX").ok().filter(|p| !p.is_empty()).unwrap_or_else(|| "git".into());
    let base = shell_quote(&format!("s3://{bucket}/{prefix}/repos"));
    let mbase = shell_quote(&format!("s3://{bucket}/{prefix}/manifests"));
    let all = shell_quote(PLATFORM_DOMAINS);
    // Which copy (measured 2026-11-14 against all 439 people's projects: this
    // restore had never worked from S3). `aws s3 sync -q` is not an aws option
    // (--quiet), so every pull failed before git ran. 662 of 1333 mirrors have
    // no refs/ (every ref packed, and S3 keeps no empty folder), which git
    // will not open. The first key found was often <owner>__<host>, the
    // oldest family (135 projects a month stale), and a mirror's HEAD can
    // name a branch the project left. So: the key with the newest manifest,
    // refs/ made, every branch kept, the manifest's branch checked out, and
    // the next key when one fails. <host> and src__domains__<host> are shared
    // by every folder with that name (autosave runs inside the owner's
    // folder), so when another folder has this host they could be someone
    // else's project: only <owner>__<host> is trusted then.
    let from_s3 = format!(
        "{{ aws=\"$HOME/.local/bin/aws\"; [ -x \"$aws\" ] || aws=aws; t=\"$(mktemp -d)\"; ok=1; \
         keys=\"{owner}__{host}\"; \
         [ -n \"$(ls -d {all}/*/{host} {all}/{host} 2>/dev/null | grep -vxF {all}/{owner}/{host})\" ] || keys=\"$keys {host} src__domains__{host}\"; \
         order=\"$(for s in $keys; do m=\"$(\"$aws\" s3 cp {mbase}/$s.json - 2>/dev/null | tr -d '\\n')\"; [ -n \"$m\" ] || continue; \
           at=\"$(printf '%s' \"$m\" | grep -o '\"archived_at\": *\"[^\"]*\"' | cut -d'\"' -f4)\"; \
           br=\"$(printf '%s' \"$m\" | grep -o '\"branch\": *\"[^\"]*\"' | cut -d'\"' -f4)\"; \
           printf '%s %s:%s\\n' \"$at\" \"$s\" \"$br\"; done | sort -r | cut -d' ' -f2)\"; \
         for sb in $order; do s=\"${{sb%%:*}}\"; br=\"${{sb#*:}}\"; \"$aws\" s3 ls {base}/$s.git/HEAD >/dev/null 2>&1 || continue; \
           echo \"pulling $s from the S3 archive\"; rm -rf \"$t/r.git\"; \
           \"$aws\" s3 sync --quiet {base}/$s.git \"$t/r.git\" && mkdir -p \"$t/r.git/refs\" && git clone -q \"$t/r.git\" {d} || {{ rm -rf {d}; continue; }}; \
           for b in $(git -C {d} for-each-ref --format='%(refname:strip=3)' refs/remotes/origin); do [ \"$b\" = HEAD ] || git -C {d} branch -q --track \"$b\" \"origin/$b\" 2>/dev/null; done; \
           git -C {d} checkout -q \"$br\" 2>/dev/null || git -C {d} checkout -q \"$(git -C {d} for-each-ref --sort=-committerdate --count=1 --format='%(refname:short)' refs/heads)\"; \
           git -C {d} remote remove origin; ok=0; break; done; \
         rm -rf \"$t\"; [ $ok -eq 0 ]; }}"
    );
    let inner = format!(
        "{{ git clone -q {u} {d} 2>/dev/null || {from_s3}; }} || exit 1; cd {d} || exit 1; \
         for b in $(git for-each-ref --format='%(refname:strip=3)' refs/remotes/origin); do [ \"$b\" = HEAD ] || git branch -q --track \"$b\" \"origin/$b\" 2>/dev/null; done; \
         w=\"$(git for-each-ref --sort=-committerdate --format='%(refname:short)' 'refs/heads/work/*' | head -n1)\"; [ -z \"$w\" ] || git checkout -q \"$w\"; echo restored"
    );
    #[cfg(unix)]
    let cmd = if is_root() {
        let (_, user) = platform_account().ok_or("no platform checkout on this machine")?;
        format!("exec runuser -l {} -c {}", shell_quote(&user), shell_quote(&inner))
    } else {
        inner
    };
    #[cfg(not(unix))]
    let cmd = inner;
    let out = capture_within(&format!("{cmd} 2>&1"), Duration::from_secs(300)).ok_or("restoring took too long")?;
    if !out.contains("restored") {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!("{host} has no saved repository to restore ({})", out.lines().last().unwrap_or("").trim()));
    }
    domain_at(&dir).ok_or_else(|| format!("{host} was restored but does not read as a domain"))
}

/// git in a project's folder as the platform account when this server runs
/// as root (cove multiuser): the account that owns the repositories the
/// autosave service keeps, so a person needs no git or credentials of their
/// own. Any owner's folder is safe to read here - the caller has checked the
/// project is theirs. `GIT_*` identity comes from `env`.
pub(crate) fn platform_git(dir: &Path, script: &str, env: &[(&str, &str)]) -> Result<String, String> {
    let d = shell_quote(&dir.display().to_string());
    let inner = format!("cd {d} && export GIT_PAGER=cat && {script}");
    let mut c;
    #[cfg(unix)]
    {
        if is_root() {
            let (_, user) = platform_account().ok_or("no platform checkout on this machine")?;
            c = Command::new("runuser");
            c.args(["-u", &user, "--", "bash", "-c", &inner]);
        } else {
            c = Command::new("bash");
            c.args(["-c", &inner]);
        }
    }
    #[cfg(not(unix))]
    {
        c = Command::new("bash");
        c.args(["-c", &inner]);
    }
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.output().map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    if out.status.success() {
        Ok(text)
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(err.lines().last().unwrap_or("git failed").trim().to_owned())
    }
}

/// A project's saved versions, newest first (the last 200).
pub fn versions_at(dir: &Path) -> Result<Vec<crate::api::Version>, String> {
    if !dir.join(".git").exists() {
        return Ok(Vec::new());
    }
    let out = platform_git(dir, "git -c safe.directory='*' log -n 200 --format='%H%x1f%ct%x1f%an%x1f%s'", &[])?;
    Ok(out
        .lines()
        .filter_map(|l| {
            let mut f = l.split('\u{1f}');
            Some(crate::api::Version { sha: f.next()?.to_owned(), when: f.next()?.parse().ok()?, author: f.next()?.to_owned(), message: f.next().unwrap_or("").to_owned() })
        })
        .collect())
}

/// The editor's completion (Query::CodeComplete): in the session of the
/// code's language - what it has loaded, argument names inside a call
/// included - or, for Rust, from rust-analyzer (lsp.rs).
pub fn code_complete(path: &str, lang: &str, line: &str, col: usize, text: &str, row: usize) -> Result<crate::api::Completion, String> {
    // `alias$` of a box::use: the module's own exports, R running or not.
    if lang == "r" {
        if let Some(c) = crate::host::boxcomplete::complete(path, text, line, col, &saved_settings().module) {
            return Ok(c);
        }
    }
    let session = match lang {
        "r" => Lang::R,
        "python" => Lang::Python,
        "julia" => Lang::Julia,
        "shell" => Lang::Shell,
        "rust" => return crate::host::lsp::complete_rust(path, text, row, col),
        _ => return Err("this kind of file completes from its own words only".into()),
    };
    kernel::complete(session, line, col)
}

/// A project's container image through the ndexr CLI (`ndexr image`,
/// docs/container-images.md), run as the platform account: its ndexr, and
/// its AWS profile, which people's sessions do not have. Cove builds and
/// pushes nothing itself. A CLI without `image` says so, and nothing is
/// pretended.
pub fn image_at(owner: &str, dir: &Path, host: &str, verb: &str) -> Result<Value, String> {
    if owner.is_empty() || !owner.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "_-.".contains(c)) || owner.starts_with('.') {
        return Err(format!("{owner}: not an ndexr username"));
    }
    let kind = image_kind(host);
    if verb == "job" {
        return serde_json::to_value(stack::jobs::view(kind)).map_err(|e| e.to_string());
    }
    let (d, o) = (shell_quote(&dir.display().to_string()), shell_quote(owner));
    let has = "ndexr image --help >/dev/null 2>&1 || { echo 'ndexr has no image command yet - asked of ndexr.ndexr.io (referral #727)' >&2; exit 3; }; ";
    match verb {
        "build" => {
            let job = stack::jobs::start(kind, format!("image build ({host})"), as_platform(&format!("{has}ndexr image build {d} --owner {o}"))?, None)?;
            serde_json::to_value(job).map_err(|e| e.to_string())
        }
        "status" | "pull" => {
            let sub = if verb == "status" { "status" } else { "pull-login" };
            let out = Command::new("bash").args(["-c", &as_platform(&format!("{has}ndexr image {sub} {d} --owner {o} --json"))?]).output().map_err(|e| e.to_string())?;
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                return Err(err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("ndexr image failed").trim().to_owned());
            }
            serde_json::from_slice(&out.stdout).map_err(|e| format!("ndexr image {sub} answered oddly: {e}"))
        }
        other => Err(format!("{other}: not something an image does")),
    }
}

/// One image build at a time per project. Leaked once per project, as
/// push_kind does per person.
fn image_kind(host: &str) -> &'static str {
    static KINDS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, &'static str>>> = std::sync::OnceLock::new();
    let mut k = KINDS.get_or_init(Default::default).lock().unwrap();
    k.entry(host.to_owned()).or_insert_with(|| Box::leak(format!("image:{host}").into_boxed_str()))
}

/// Put version `sha` back in `dir`, losing nothing: what is there now is
/// saved as a version first, then the files become that version's and that is
/// saved as the newest - history only ever grows. `owner` authors both.
pub fn version_restore_at(dir: &Path, sha: &str, owner: &str) -> Result<String, String> {
    if !(7..=40).contains(&sha.len()) || !sha.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("not a version".into());
    }
    if !dir.join(".git").exists() {
        return Err("this project has no saved versions yet".into());
    }
    let short = &sha[..7];
    let script = format!(
        "g='git -c safe.directory=*'; $g cat-file -e {sha}^{{commit}} || {{ echo 'no such version' >&2; exit 1; }};          when=\"$($g log -1 --format=%cd --date=format:'%Y-%m-%d %H:%M' {sha})\";          $g add -A && {{ $g diff --cached --quiet || $g commit -q -m \"Saved before restoring the version of $when\"; }} &&          $g read-tree -u --reset {sha} && $g commit -q --allow-empty -m \"Restored the version of $when ({short})\" && echo \"restored the version of $when\""
    );
    let email = format!("{owner}@users.ndexr.io");
    let env = [("GIT_AUTHOR_NAME", owner), ("GIT_AUTHOR_EMAIL", email.as_str()), ("GIT_COMMITTER_NAME", "ndexr saves"), ("GIT_COMMITTER_EMAIL", "saves@ndexr.io")];
    Ok(platform_git(dir, &script, &env)?.trim().to_owned())
}

/// `owner`'s Push all, as `push_all_as` started it.
pub fn push_job_as(owner: &str) -> JobView {
    stack::jobs::view(push_kind(owner))
}

/// One job slot per person, so two people's Saves neither collide nor show
/// each other's logs. Leaked once per owner: a few dozen short strings.
fn push_kind(owner: &str) -> &'static str {
    static KINDS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, &'static str>>> = std::sync::OnceLock::new();
    let mut k = KINDS.get_or_init(Default::default).lock().unwrap();
    k.entry(owner.to_owned()).or_insert_with(|| Box::leak(format!("push:{owner}").into_boxed_str()))
}

/// Ask https://<host>/ as an anonymous visitor what is in front of it. The
/// App panel frames the site; a page-password form refuses to be framed
/// (X-Frame-Options: SAMEORIGIN, on purpose), so the panel has to know.
fn site_gate(host: &str) -> Result<String, String> {
    let host = host.trim().to_ascii_lowercase();
    if !is_host_name(&host) {
        return Err(format!("{host}: not a domain name"));
    }
    let url = shell_quote(&format!("https://{host}/"));
    // The exit code rides at the front of the last line, so an empty
    // redirect_url cannot shift it.
    let Some(out) = capture_within(&format!("curl -s -m 20 -w '\\n%{{exitcode}} %{{http_code}} %{{redirect_url}}' {url} | tail -c 20000"), Duration::from_secs(25)) else {
        return Ok("open".into());
    };
    let last = out.lines().last().unwrap_or("");
    let mut f = last.split_whitespace();
    let exit: u16 = f.next().and_then(|c| c.parse().ok()).unwrap_or(0);
    let code: u16 = f.next().and_then(|c| c.parse().ok()).unwrap_or(0);
    let to = f.next().unwrap_or("");
    // Slow is not absent: svbt.ndexr.io, a live app, took 7-8 s on a cold
    // request and the old 8 s limit called it "Nothing answers ... publish it
    // at route.ndexr.io" (operator, 2026-10-16). A timeout (curl 28) frames the
    // site and lets the browser wait; "down" is kept for nothing being there -
    // no DNS (6), no connection (7), TLS refused (35, 60) - or nginx saying the
    // app behind it is gone (502-504).
    if code == 0 && exit == 28 {
        return Ok("open".into());
    }
    Ok(match code {
        0 | 502..=504 => "down".into(),
        401 if out.contains("__site_login") => "password".into(),
        301..=303 | 307 | 308 if to.contains("console.ndexr.io") || to.contains("/login") => "login".into(),
        200..=399 => "open".into(),
        c => format!("http {c}"),
    })
}

// ── the domain's agent: exec.ndexr.io's workflow, natively ──────────────────

/// Where ndexr-claude-run keeps every run's sidecars (.stream, .done, .pid).
const HOST_RUNS: &str = "/srv/ndexr/host_runs";

/// The agent bridge: the platform's own R, its own host_run_claude - the
/// function every in-app and exec.ndexr.io turn goes through (prompt tier,
/// cell / restricted / admin path, metering) - so the IDE's agent is that
/// agent, not a second one. It runs on the host the IDE session runs on.
const AGENT_R: &str = r#"
args <- commandArgs(TRUE)
domain <- args[[1]]; model <- args[[2]]; prompt_file <- args[[3]]; owner <- args[[length(args)]]
# A folder inside the domain to run at (a file ask), or "" for the domain's own.
cwd <- if (length(args) >= 5 && nzchar(args[[4]])) args[[4]] else "~/ndexr"
# How the turn may act: build (Auto), plan or ask - exec.ndexr.io's modes; the
# wrapper enforces plan and ask read-only (--permission-mode plan).
# Supervised (setup, 2026-11-27) is not a mode the wrapper enforces yet: it
# runs as plan, never broader.
mode <- if (length(args) >= 6 && args[[5]] %in% c("build", "plan", "ask")) args[[5]] else if (length(args) >= 6 && identical(args[[5]], "supervised")) "plan" else "build"
out <- function(x) { cat(jsonlite::toJSON(x, auto_unbox = TRUE, null = "null")); quit(status = 0) }
options(box.path = Sys.getenv("NDEXR_SRC"))
box::use(host = r/host, ud = r/users/user_domains, pg = r/connections/postgres)
# host.r names the runs folder by the console container's view (/var/host_runs);
# on this host it is /srv/ndexr/host_runs, which launched boxes symlink and this
# one does not - so this process uses the host path, as those boxes do.
e <- environment(host$host_run_claude)
unlockBinding("HOST_RUNS_DIR", e); assign("HOST_RUNS_DIR", Sys.getenv("NDEXR_HOST_RUNS"), envir = e); lockBinding("HOST_RUNS_DIR", e)
email <- tryCatch({
  con <- pg$connection_postgres_direct(); on.exit(pool::poolReturn(con), add = TRUE)
  r <- DBI::dbGetQuery(con, "SELECT email FROM ndexr_users WHERE lower(username) = lower($1) LIMIT 1", params = list(owner))
  if (nrow(r)) r$email[[1]] else ""
}, error = function(e) "")
if (!nzchar(email)) out(list(error = paste0("no ndexr account named ", owner, " - the agent runs as you")))
admin <- isTRUE(ud$user_domains_is_admin(email))
# The Exec tab asks for "exec.ndexr.io" and this is the one place that decides
# whose thread that is (operator, 2026-10-15: an executive agent for each
# person over all of their domains). The operator gets exec.ndexr.io itself -
# its thread, its tier, unrestricted. Anyone else gets their own account:
# host_run_claude's owner:<username> scope (cwd = their whole domains folder,
# their cell when they have one, the customer fence otherwise) in a thread of
# their own under _account/ - never exec's.
scope <- domain
if (identical(domain, "exec.ndexr.io") && !admin) {
  box::use(reg = r/domains/registry)
  own <- tryCatch(reg$owner_relpath_for_email(email), error = function(e) NULL)
  if (is.null(own) || !grepl("^[a-z0-9][a-z0-9-]*$", own))
    out(list(error = "your executive agent works across your domains folder, and you have none yet - create your first domain, then come back"))
  scope <- paste0("owner:", own)
  td <- host$host_thread_dir("_account", own)
  dir.create(td, recursive = TRUE, showWarnings = FALSE, mode = "0775")
} else {
  # A subdomain of a domain the person owns is theirs too (operator,
  # 2026-10-16: "all users should be able to create any subdomain of
  # theirs") - suno.jamie.ndexr.io under jamie.ndexr.io -
  # recorded on first use. A bare apex (ndexr.io) never counts.
  # Shared with them to work on (people_sharing.sql share_editors): the
  # domain's own thread and agent, beside its owner's.
  shared <- !admin && tryCatch({
    con <- pg$connection_postgres_direct(); on.exit(pool::poolReturn(con), add = TRUE)
    nrow(DBI::dbGetQuery(con, "SELECT 1 FROM share_editors WHERE email = lower($1) AND domain = lower($2) LIMIT 1", params = list(email, domain))) > 0
  }, error = function(e) FALSE)
  if (!admin && !shared && !isTRUE(ud$user_domains_owns(email, domain))) {
    labels <- strsplit(domain, ".", fixed = TRUE)[[1]]
    ups <- if (length(labels) > 3) vapply(2:(length(labels) - 2), function(k) paste(labels[k:length(labels)], collapse = "."), "") else character()
    mine <- any(vapply(ups, function(u) isTRUE(ud$user_domains_owns(email, u)), logical(1)))
    if (!mine) out(list(error = paste0(domain, " is not one of your domains")))
    try(ud$user_domains_add(email, domain), silent = TRUE)
  }
  host$host_ensure_domain_thread("_domain", domain)
  td <- host$host_thread_dir("_domain", domain)
}
# Asked only where the thread is (the Exec tab's history): nothing runs.
if (identical(model, "@thread")) out(list(dir = td))
# Resume the thread's newest session, as the in-app chat does.
resume <- NULL
# A session belongs to the folder it ran in: a run at another folder starts fresh.
if (identical(cwd, "~/ndexr")) for (id in sort(sub("\\.prompt$", "", list.files(td, pattern = "\\.prompt$")), decreasing = TRUE)) {
  if (grepl("-opencode_", id, fixed = TRUE)) next
  s <- file.path(Sys.getenv("NDEXR_HOST_RUNS"), paste0(id, ".session"))
  if (file.exists(s)) { v <- trimws(readLines(s, warn = FALSE)[1]); if (!is.na(v) && nzchar(v)) { resume <- v; break } }
}
prompt <- paste(readLines(prompt_file, warn = FALSE), collapse = "
")
run <- host$host_run_claude(prompt, model = model, resume = resume, cwd = cwd, user = email,
                            thread_dir = td, domain = scope, is_admin = admin, mode = mode)
out(list(id = run$id, spawn_status = run$spawn_status, spawn_output = paste(run$spawn_output, collapse = "\n"), resumed = !is.null(resume)))
"#;

/// A run id as ndexr-claude-run writes them: 20261014-160358-claude_sub-opus.
fn valid_run_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 100 && !id.starts_with(['.', '-']) && id.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
}

fn agent_run(domain: &str, prompt: &str, model: &str, cwd: Option<&str>, mode: Option<&str>) -> Result<String, String> {
    agent_run_as(&session_owner(), domain, prompt, model, cwd, mode)
}

/// The signed-in ndexr username this server answers for.
/// The person's own projects folder: the platform's src/domains/<owner>
/// where this account can see it (the operator's), else ~/domains (the bind
/// of that same folder in everyone else's home). New projects go here and
/// Save sweeps it, whatever project is open: deriving it from the open one put
/// fefefef.ndexr.io beside domains.ndexr.io at the flat src/domains root,
/// where the provisioner then made a second, Shiny copy under the owner's
/// folder (2026-10-26). None on a machine with neither (the desktop app).
pub(crate) fn own_domains() -> Option<PathBuf> {
    let owner = session_owner();
    let platform = Path::new(PLATFORM_DOMAINS).join(&owner);
    if !owner.is_empty() && platform.is_dir() {
        return Some(platform);
    }
    let home = Path::new(&home()).join("domains");
    home.is_dir().then_some(home)
}

pub(crate) fn session_owner() -> String {
    std::env::var("COVE_OWNER").ok().filter(|o| !o.is_empty()).or_else(|| std::env::var("USER").ok()).unwrap_or_default()
}

/// `agent_run` for a named owner - also what `cove multiuser` answers for a
/// session whose account cannot read the platform (every account but the
/// operator's: /home/ubuntu is 0750, so their Agent tab only ever said "this
/// needs the platform checked out", 2026-10-15).
/// `/home/<owner>/domains/...` in `text` as the platform's own path to the
/// same files, `<PLATFORM_DOMAINS>/<owner>/...`, when that folder is there.
pub(crate) fn platform_paths(owner: &str, text: &str) -> String {
    let text = shared_platform_paths(owner, text);
    let text = text.as_str();
    let ours = Path::new(PLATFORM_DOMAINS).join(owner);
    if owner.is_empty() || owner.contains('/') || !ours.is_dir() {
        return text.to_owned();
    }
    // Both ways a home names that folder: ~/domains (the bind) and
    // ~/ndexr/src/domains/<owner> (the account's own ndexr, a link to it).
    // The platform account cannot enter /home/<owner> (0750): Versions, run
    // as that account, failed with "cd: /home/fdrennan/ndexr/src/domains/
    // fdrennan/fdrennan.ndexr.io: Permission denied" (2026-10-26).
    // And the cell's: it mounts that same folder at /root/src/domains/<owner>
    // (user-provision.sh), and a desktop Cove on a cell sends its agent runs
    // here (backend.rs platform_query) with the cell's paths in them -
    // "/root/src/domains/drennanfreddy/drennanfreddy.ndexr.io/static: not a
    // folder of drennanfreddy.ndexr.io" (request #185, 2026-11-03).
    let to = format!("{}/", ours.display());
    text.replace(&format!("/home/{owner}/ndexr/src/domains/{owner}/"), &to)
        .replace(&format!("/home/{owner}/domains/"), &to)
        .replace(&format!("/root/src/domains/{owner}/"), &to)
}

/// `/home/<owner>/shared/<host>/...` (a domain shared with them to work on,
/// shares.rs) as the platform's own path to it.
#[cfg(unix)]
fn shared_platform_paths(owner: &str, text: &str) -> String {
    let home = Path::new("/home").join(owner);
    let lead = format!("{}/", home.join("shared").display());
    if owner.is_empty() || owner.contains('/') || !text.contains(&lead) {
        return text.to_owned();
    }
    let mut out = text.to_owned();
    for (host, _, _) in crate::host::shares::granted(&home) {
        if let Some(to) = crate::host::shares::platform_path_of(&home, &host) {
            let at = format!("{lead}{host}");
            out = if out == at { to.display().to_string() } else { out.replace(&format!("{at}/"), &format!("{}/", to.display())) };
        }
    }
    out
}

#[cfg(not(unix))]
fn shared_platform_paths(_owner: &str, text: &str) -> String {
    text.to_owned()
}

/// What every agent turn in a Cove project is told first: the project's app
/// is the one in its folder, and the work changes it in place. None for a
/// folder with no `.cove/app.env` (Exec, a platform domain).
fn project_brief(owner: &str, domain: &str) -> Option<String> {
    let dir = [Path::new(PLATFORM_DOMAINS).join(owner).join(domain), Path::new(PLATFORM_DOMAINS).join(domain)]
        .into_iter()
        .find(|d| d.join(crate::project::dotcove::DIR).join("app.env").is_file())?;
    let env = std::fs::read_to_string(dir.join(crate::project::dotcove::DIR).join("app.env")).ok()?;
    Some(brief_text(domain, &env))
}

fn brief_text(domain: &str, env: &str) -> String {
    let field = |k: &str| env.lines().find_map(|l| l.trim().strip_prefix(&format!("{k}=")).map(|v| v.trim().trim_matches('"').to_owned())).unwrap_or_default();
    let live = if field("APP_KIND") == "port" {
        format!(
            "It is served at https://{domain}/ from 127.0.0.1:{port} (.cove/app.env). Making a change live is yours: \
             .cove/build.sh, then .cove/down.sh and .cove/up.sh, from the project's root - Cove's Build, Down and Up run the same files. \
             No referral, deploy script, service or new port is needed, ever.",
            port = field("APP_PORT")
        )
    } else {
        format!("It is a module the platform serves at https://{domain}/: a saved change is live.")
    };
    format!(
        "[This project - from Cove, not typed by the person]\n\
         {domain}'s app is the one in this folder. Whatever the person asks you to build here, build it by changing THAT app in place: \
         the new app replaces what is there. Never keep the old app beside it (no /old, /net or /v1 sub-path for it), never start a second app, \
         and never send going live to another agent. {live} \
         If CONTEXT.md or CLAUDE.md says otherwise (scripts/new-rust-app.sh, an app-<host>.service, a deploy through exec), \
         that note is out of date: do this, and correct the note. \
         For icons or pictures run ndexr-media (icon \"<what>\", generate \"<prompt>\" --name <file>, fetch <url>): \
         files land in images/ with their source and licence in images/SOURCES.md. \
         Large files (renders, video, audio, datasets, anything over about 5 MB) go in supporting_docs/, never git: it is \
         gitignored and saved to S3 on every save; link the path a page needs to it (ln -s ../supporting_docs/media/images/x.mp4 images/x.mp4) \
         and never git add -f one (operator, 2026-11-14; AGENTS-shared.md 7b).\n---\n"
    )
}

pub fn agent_run_as(owner: &str, domain: &str, prompt: &str, model: &str, cwd: Option<&str>, mode: Option<&str>) -> Result<String, String> {
    // Only the three the platform knows; anything else is Auto.
    let mode = match mode {
        Some(m @ ("build" | "plan" | "ask")) => m,
        _ => "build",
    };
    let domain = domain.trim().to_ascii_lowercase();
    if !is_host_name(&domain) {
        return Err(format!("{domain}: not a domain name"));
    }
    if prompt.trim().is_empty() {
        return Err("say what you want done".into());
    }
    // `[1m]`: the CLI's 1M-context variants (opus[1m]); agent.rs MODELS.
    let model = if model.chars().all(|c| c.is_ascii_alphanumeric() || "-._:[]".contains(c)) && !model.is_empty() { model } else { "sub-opus" };
    // A file ask's folder: a real folder of this domain's, nothing else (the
    // platform narrows to it only when it lies inside the domain, too).
    let cwd = match cwd.map(str::trim).filter(|c| !c.is_empty()) {
        None => String::new(),
        Some(c) => {
            // As this machine names it first: the folder came from a home or
            // a cell (platform_paths), and is checked where it will be used.
            let here = platform_paths(owner, c);
            let p = Path::new(&here);
            let ok = p.is_absolute() && p.is_dir() && !here.split('/').any(|s| s == "..") && p.components().any(|s| s.as_os_str() == domain.as_str());
            if !ok {
                return Err(format!("{c}: not a folder of {domain}"));
            }
            here
        }
    };
    // The person's ~/domains is a bind of the platform's owner folder
    // (multiuser.rs), and the agent runs as the platform account, which
    // cannot enter /home/<owner> (mode 750). Attachment paths written as the
    // person sees them came back EACCES (2026-10-23, a screenshot pasted into
    // Exec): name the platform's copy of the same file instead.
    let prompt = platform_paths(owner, prompt);
    // Every turn in a project is about that project's one app, changed in
    // place (operator, 2026-11-07, after du.ndexr.io's agent built a Disk
    // Observatory beside the Network Observatory it started as, moved the old
    // one to /net/ and referred "going live" to exec through an old deploy
    // script: "agents when working on their projects must ALWAYS work on the
    // application in place of the one there").
    let prompt = match project_brief(owner, &domain) {
        Some(brief) => format!("{brief}{prompt}"),
        None => prompt,
    };
    // Over the free storage cap, every turn is about getting back under it.
    let prompt = match crate::host::storage::agent_brief_for(owner) {
        Some(brief) => format!("{brief}{prompt}"),
        None => prompt.to_owned(),
    };
    let pf = platform_file(&format!("prompt-{}-{}.txt", std::process::id(), unique()), prompt.as_bytes())?;
    let v = platform_r_as(owner, "agent.R", AGENT_R, &[&domain, model, &pf.display().to_string(), &cwd, mode], 90);
    let _ = std::fs::remove_file(&pf);
    let v = v?;
    if v["spawn_status"].as_i64().unwrap_or(0) != 0 {
        // The platform says why in words meant for the person ("You're on
        // ndexr's Claude, which is for subscribers ..."): show those, not a
        // status number (agent.rs offers Connect your Claude beside them).
        let why = v["spawn_output"].as_str().map(str::trim).filter(|w| !w.is_empty() && w.len() < 600);
        return Err(why.map(str::to_owned).unwrap_or_else(|| format!("ndexr-claude-run did not start (status {})", v["spawn_status"])));
    }
    let id = v["id"].as_str().map(str::to_owned).ok_or_else(|| "no run id".to_owned())?;
    // Who sent it, beside its prompt, so the thread can say so to anyone
    // else who sees it (agent_history_as). Best effort: a turn without it is
    // shown as before.
    if let Ok(td) = thread_dir_as(owner, &domain) {
        let _ = std::fs::write(td.join(format!("{id}.by")), owner);
    }
    Ok(id)
}

/// The env the platform's own R runs in - the one Cove gives every person and
/// the console's eb-R-env.sh loads (operator, 2026-10-18: "cove.ndexr.io gets
/// everyone on the same env, including the ndexr app"). Its site library is
/// the shared one; this was `ml R` plus a private R_LIBS=/opt/ndexr/eblib.
const PLATFORM_ENV: &str = "env/r4.6_py3.13-release";

/// The platform's own R, as this person: the checkout's .env, the env every
/// person gets (PLATFORM_ENV, as the console's eb-R-env.sh), postgres on this host, `NDEXR_HOST_LOCAL=1` - so a script
/// can box::use src/r modules exactly as the console does. `script` gets
/// `args` then the owner (the signed-in ndexr username) and answers one JSON
/// line; `{"error": …}` comes back as Err. Used by the Agent tab and Help ›
/// Send a request.
fn platform_r(name: &str, script: &str, args: &[&str], secs: u64) -> Result<Value, String> {
    platform_r_as(&session_owner(), name, script, args, secs)
}

fn unique() -> u64 {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

#[cfg(unix)]
fn is_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}
#[cfg(not(unix))]
fn is_root() -> bool {
    false
}

/// `inner` as a login shell of the platform account (its ndexr CLI, AWS
/// credentials and ssh keys) when this is `cove multiuser` (root); as-is
/// otherwise.
pub(crate) fn as_platform(inner: &str) -> Result<String, String> {
    #[cfg(unix)]
    if is_root() {
        let (_, user) = platform_account().ok_or("no platform checkout on this machine")?;
        return Ok(format!("exec runuser -l {} -c {}", shell_quote(&user), shell_quote(inner)));
    }
    Ok(inner.to_owned())
}

/// Whether `owner` is an ndexr admin (user_domains_is_admin on their email):
/// the platform's own record, not this machine's groups.
const ADMIN_R: &str = r#"
args <- commandArgs(TRUE); owner <- args[[length(args)]]
out <- function(x) { cat(jsonlite::toJSON(x, auto_unbox = TRUE, null = "null")); quit(status = 0) }
options(box.path = Sys.getenv("NDEXR_SRC"))
box::use(ud = r/users/user_domains, pg = r/connections/postgres)
email <- tryCatch({
  con <- pg$connection_postgres_direct(); on.exit(pool::poolReturn(con), add = TRUE)
  r <- DBI::dbGetQuery(con, "SELECT email FROM ndexr_users WHERE lower(username) = lower($1) LIMIT 1", params = list(owner))
  if (nrow(r)) r$email[[1]] else ""
}, error = function(e) "")
out(list(admin = nzchar(email) && isTRUE(ud$user_domains_is_admin(email))))
"#;

/// This session's owner is an ndexr admin, asked once (ndexr_admin runs R).
fn admin_cached() -> bool {
    static ADMIN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ADMIN.get_or_init(|| ndexr_admin(&session_owner()))
}

/// The path from a browser to the applications, read off this machine now
/// (operator, 2026-10-21: "visualize where the running kernel copy (on prem)
/// is and then ... the domains being passed in through route53 ... matched
/// with the external router server which passes domains into the on prem ...
/// and the resources required"). DNS by `dig`, the tunnel by systemd, the
/// rest from /proc and a connect to each listener. Kept a minute.
pub fn serving(apexes: &[String], targets: &[String]) -> crate::api::Serving {
    use std::time::{Duration, Instant};
    static LAST: std::sync::Mutex<Option<(Instant, Vec<String>, crate::api::Serving)>> = std::sync::Mutex::new(None);
    let key: Vec<String> = apexes.iter().chain(targets).cloned().collect();
    if let Some((at, k, s)) = LAST.lock().unwrap().as_ref() {
        if *k == key && at.elapsed() < Duration::from_secs(60) {
            return s.clone();
        }
    }
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_default();
    let mem = read("/proc/meminfo");
    let kb = |name: &str| mem.lines().find_map(|l| l.strip_prefix(name)).and_then(|v| v.split_whitespace().next()?.parse().ok()).unwrap_or(0);
    let answers = |port: u16| std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(250)).is_ok();
    let systemd = |args: &[&str]| std::process::Command::new("systemctl").args(args).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned()).unwrap_or_default();
    let desc = systemd(&["show", "-p", "Description", "--value", "ndexr-router-tunnel"]);
    // "… the ndexr edge router (3.146.148.247)": the address in its name.
    let router = desc.rsplit_once('(').and_then(|(_, r)| r.split(')').next()).filter(|r| r.parse::<std::net::IpAddr>().is_ok()).unwrap_or("").to_owned();
    let dig = |kind: &str, name: &str| -> Vec<String> {
        std::process::Command::new("dig")
            .args(["+short", "+time=1", "+tries=1", kind, name])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).lines().map(|l| l.trim().trim_end_matches('.').to_owned()).filter(|l| !l.is_empty()).collect())
            .unwrap_or_default()
    };
    let zones: Vec<crate::api::Zone> = std::thread::scope(|s| {
        let hs: Vec<_> = apexes
            .iter()
            .take(40)
            .filter(|a| a.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'))
            .map(|a| {
                s.spawn(move || {
                    let ns = dig("NS", a);
                    let dns = match ns.first() {
                        Some(n) if n.contains("awsdns") => "Route 53".to_owned(),
                        Some(n) => n.rsplitn(3, '.').take(2).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("."),
                        None => String::new(),
                    };
                    let address = dig("A", a).into_iter().filter(|x| x.parse::<std::net::IpAddr>().is_ok()).last().unwrap_or_default();
                    crate::api::Zone { apex: a.clone(), dns, address }
                })
            })
            .collect();
        hs.into_iter().filter_map(|h| h.join().ok()).collect()
    });
    let mut seen = std::collections::HashSet::new();
    let upstreams = targets
        .iter()
        .filter(|t| seen.insert(t.as_str()))
        .map(|t| {
            // The platform's Shiny router: five console replicas (nginx
            // ip_hash over 9000-9004).
            let ports: Vec<u16> = if t == "module" { (9000..=9004).collect() } else { t.strip_prefix("port:").and_then(|p| p.parse().ok()).into_iter().collect() };
            crate::api::Upstream { target: t.clone(), up: ports.iter().filter(|&&p| answers(p)).count() as u32, of: ports.len() as u32 }
        })
        .collect();
    let vhosts = std::fs::read_dir(Path::new(PLATFORM_DOMAINS).join("../../nginx/sites-enabled")).map(|d| d.count() as u32).unwrap_or(0);
    let s = crate::api::Serving {
        host: read("/proc/sys/kernel/hostname").trim().to_owned(),
        kernel: read("/proc/sys/kernel/osrelease").trim().to_owned(),
        arch: std::env::consts::ARCH.to_owned(),
        cpus: read("/proc/cpuinfo").lines().filter(|l| l.starts_with("processor")).count() as u32,
        mem_total_kb: kb("MemTotal:"),
        mem_avail_kb: kb("MemAvailable:"),
        load1: read("/proc/loadavg").split_whitespace().next().and_then(|v| v.parse().ok()).unwrap_or(0.0),
        vhosts,
        nginx_up: answers(443),
        router,
        tunnel: systemd(&["is-active", "ndexr-router-tunnel"]),
        zones,
        upstreams,
    };
    *LAST.lock().unwrap() = Some((Instant::now(), key, s.clone()));
    s
}

pub fn ndexr_admin(owner: &str) -> bool {
    platform_r_as(owner, "admin.R", ADMIN_R, &[], 30).ok().and_then(|v| v["admin"].as_bool()).unwrap_or(false)
}

/// The account that owns the platform checkout - the one its R runs as.
#[cfg(unix)]
fn platform_account() -> Option<(u32, String)> {
    let uid = std::os::unix::fs::MetadataExt::uid(&std::fs::metadata(platform_root()).ok()?);
    let pw = unsafe { libc::getpwuid(uid) };
    if pw.is_null() {
        return None;
    }
    let name = unsafe { std::ffi::CStr::from_ptr((*pw).pw_name) }.to_str().ok()?.to_owned();
    Some((uid, name))
}

/// Where platform R's script and inputs go: this account's cache - or, in
/// `cove multiuser` (root), /run/cove/platform, owned by the platform's
/// account, which the script then runs as. Platform R had run as root there
/// (Claude lookups, plans, speech), and a turn run that way would have started
/// claude as root.
fn platform_scratch() -> Result<PathBuf, String> {
    #[cfg(unix)]
    if is_root() {
        use std::os::unix::fs::PermissionsExt;
        let (uid, _) = platform_account().ok_or("no platform checkout on this machine")?;
        let dir = PathBuf::from("/run/cove/platform");
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::os::unix::fs::chown(&dir, Some(uid), None).map_err(|e| e.to_string())?;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        return Ok(dir);
    }
    let dir = kernel::cache_dir().join("agent");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// A file platform R reads (a prompt, a token, the script): 0600, and the
/// platform account's when written as root.
fn platform_file(name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    let f = platform_scratch()?.join(name);
    std::fs::write(&f, bytes).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).map_err(|e| e.to_string())?;
        if is_root() {
            let (uid, _) = platform_account().ok_or("no platform checkout on this machine")?;
            std::os::unix::fs::chown(&f, Some(uid), None).map_err(|e| e.to_string())?;
        }
    }
    Ok(f)
}

/// A platform R script for `owner` whose args are `a`, `b`, a file holding
/// `text` (free text never rides a command line), and `c` (messages.rs).
pub(crate) fn platform_r_with_file(owner: &str, name: &str, script: &str, a: &str, b: &str, text: &str, c: &str) -> Result<Value, String> {
    let f = platform_file(&format!("text-{}-{}.txt", std::process::id(), unique()), text.as_bytes())?;
    let v = platform_r_as(owner, name, script, &[a, b, &f.display().to_string(), c], 60);
    let _ = std::fs::remove_file(&f);
    v
}

/// `platform_r` for a named owner - for `cove multiuser`, which asks on a
/// person's behalf before their session exists.
pub(crate) fn platform_r_as(owner: &str, name: &str, script: &str, args: &[&str], secs: u64) -> Result<Value, String> {
    let root = Path::new(PLATFORM_DOMAINS).ancestors().nth(2).ok_or("no platform checkout")?.to_path_buf();
    if !root.join("src/r/host.r").is_file() {
        // Said in the words a person on a server of their own can act on: the
        // old "needs the platform checked out (src/r/host.r)" read as a broken
        // install on a new server's first page (operator, 2026-11-27).
        return Err(localagent::ON_ITS_OWN.replace("messages, requests and spoken replies go", "this goes"));
    }
    // Its own file each call: two people asking at once must not share one
    // being rewritten under the other's Rscript.
    let rf = platform_file(&format!("{}-{}-{name}", std::process::id(), unique()), script.as_bytes())?;
    let env = PLATFORM_ENV;
    let quoted: Vec<String> = args.iter().chain([&owner]).map(|a| shell_quote(a)).collect();
    let cmd = format!(
        "cd {root} || exit 1; set -a; [ -r .env ] && . ./.env; set +a; \
         case \"$POSTGRES_HOST\" in postgres | pgbouncer) export POSTGRES_HOST=127.0.0.1 ;; esac; \
         [ \"$POSTGRES_DIRECT_HOST\" = postgres ] && export POSTGRES_DIRECT_HOST=127.0.0.1; \
         export NDEXR_HOST_LOCAL=1 NDEXR_SRC={src} NDEXR_HOST_RUNS={runs} COVE_OWNER={owner} LANG=C.UTF-8; \
         set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1; ml {env} >/dev/null 2>&1; \
         ef=$(mktemp); Rscript --no-save --no-restore-data {rf} {args} 2>\"$ef\" || printf 'R-ERROR: %s\\n' \"$(grep -v '^<pool>' \"$ef\" | grep -v '^Calls:\\|^Execution halted' | tail -n 2 | tr '\\n' ' ')\"; rm -f \"$ef\"",
        root = shell_quote(&root.display().to_string()),
        src = shell_quote(&root.join("src").display().to_string()),
        runs = shell_quote(HOST_RUNS),
        owner = shell_quote(owner),
        rf = shell_quote(&rf.display().to_string()),
        args = quoted.join(" "),
    );
    // As root (`cove multiuser`), the platform's own account runs it.
    #[cfg(unix)]
    let cmd = if is_root() {
        let (_, user) = platform_account().ok_or("no platform checkout on this machine")?;
        format!("exec runuser -u {} -- bash -c {}", shell_quote(&user), shell_quote(&cmd))
    } else {
        cmd
    };
    let out = capture_within(&cmd, Duration::from_secs(secs));
    let _ = std::fs::remove_file(&rf);
    let out = out.ok_or("the platform R gave no answer in time")?;
    // R's own words when it stopped (an import that moved, a database
    // down): "gave no answer" hid that credentials.ndexr.io's module had
    // moved, and every Claude check failed for a day (2026-11-26).
    if let Some(e) = out.lines().last().and_then(|l| l.strip_prefix("R-ERROR: ")) {
        return Err(format!("the platform R stopped: {}", e.trim()));
    }
    let v: Value = serde_json::from_str(out.lines().last().unwrap_or("")).map_err(|_| format!("unexpected answer: {}", out.chars().take(300).collect::<String>()))?;
    match v["error"].as_str() {
        Some(e) => Err(e.to_owned()),
        None => Ok(v),
    }
}

/// Help › Send a request: file_complaint (src/r/complaints/complaints.r), the
/// function the site's REQUEST dialog calls - a row on complaints.ndexr.io,
/// the operator's text, and a turn in the domain's thread - as this person.
const REQUEST_R: &str = r#"
args <- commandArgs(TRUE)
domain <- args[[1]]; urgency <- args[[4]]; owner <- args[[length(args)]]
# What was pasted, dropped or picked into it: a folder, as file_complaint
# takes a Shiny upload (name + datapath), copied under supporting_docs/.
fdir <- if (length(args) >= 6L) args[[5]] else ""
files <- if (nzchar(fdir) && dir.exists(fdir)) {
  p <- list.files(fdir, full.names = TRUE)
  if (length(p)) data.frame(name = sub("^[0-9]+-", "", basename(p)), datapath = p, stringsAsFactors = FALSE)
}
what <- paste(readLines(args[[2]], warn = FALSE), collapse = "\n")
trying <- paste(readLines(args[[3]], warn = FALSE), collapse = "\n")
out <- function(x) { cat(jsonlite::toJSON(x, auto_unbox = TRUE, null = "null")); quit(status = 0) }
options(box.path = Sys.getenv("NDEXR_SRC"))
box::use(host = r/host, pg = r/connections/postgres, cp = r/complaints/complaints)
e <- environment(host$host_run_claude)
unlockBinding("HOST_RUNS_DIR", e); assign("HOST_RUNS_DIR", Sys.getenv("NDEXR_HOST_RUNS"), envir = e); lockBinding("HOST_RUNS_DIR", e)
# "@anonymous" (send_request_anon) is no account: nobody signed in filed it.
anon <- identical(owner, "@anonymous")
email <- if (anon) "anonymous" else tryCatch({
  con <- pg$connection_postgres_direct(); on.exit(pool::poolReturn(con), add = TRUE)
  r <- DBI::dbGetQuery(con, "SELECT email FROM ndexr_users WHERE lower(username) = lower($1) LIMIT 1", params = list(owner))
  if (nrow(r)) r$email[[1]] else ""
}, error = function(e) "")
if (!nzchar(email)) out(list(error = paste0("no ndexr account named ", owner)))
page <- if (anon) "https://cove.ndexr.io/view/" else paste0("https://cove.ndexr.io/?domain=", domain)
r <- tryCatch(cp$file_complaint(email, domain, page_url = page,
                                what_sucks = what, trying_to = trying, urgency = urgency, files = files),
              error = function(e) out(list(error = conditionMessage(e))))
out(list(id = r$id, sms_ok = isTRUE(r$sms_ok), n_uploads = r$n_uploads))
"#;

fn send_request(domain: &str, what: &str, trying: &str, blocked: bool, files: &[crate::api::Attachment]) -> Result<i64, String> {
    send_request_as(&session_owner(), domain, what, trying, blocked, files)
}


/// A request from someone not signed in (operator, 2026-11-14: "users should
/// be able to file anonymous requests through the messaging system without
/// being logged in"), from the public view's POST /view/request. Filed as
/// "anonymous" on cove.ndexr.io only - a stranger's words never land in a
/// customer's own thread - with no files, and with the contact they chose to
/// leave (unverified) in the trying-to text. complaints.email is NOT NULL and
/// file_complaint stores whatever it is given, so "anonymous" is that row's
/// email; it is no address, and nothing mails it.
pub fn send_request_anon(what: &str, contact: &str, page: &str) -> Result<i64, String> {
    let what: String = what.trim().chars().take(4000).collect();
    let contact: String = contact.trim().chars().filter(|c| !c.is_control()).take(200).collect();
    let page: String = page.trim().chars().filter(|c| !c.is_control()).take(300).collect();
    let mut trying = String::from("Filed without signing in, from the public view.");
    if !page.is_empty() {
        trying.push_str(&format!("\nPage: {page}"));
    }
    if !contact.is_empty() {
        trying.push_str(&format!("\nContact they left (unverified): {contact}"));
    }
    send_request_as("@anonymous", "cove.ndexr.io", &what, &trying, false, &[])
}

/// `send_request` for a named owner (`cove multiuser`, for a session whose
/// account cannot read the platform).
pub fn send_request_as(owner: &str, domain: &str, what: &str, trying: &str, blocked: bool, files: &[crate::api::Attachment]) -> Result<i64, String> {
    let domain = domain.trim().to_ascii_lowercase();
    let domain = if is_host_name(&domain) { domain } else { "cove.ndexr.io".to_owned() };
    if what.trim().is_empty() {
        return Err("say what you need first".into());
    }
    let n = unique();
    let wf = platform_file(&format!("what-{}-{n}.txt", std::process::id()), what.as_bytes())?;
    let tf = platform_file(&format!("trying-{}-{n}.txt", std::process::id()), trying.as_bytes())?;
    if files.iter().map(|f| f.size()).sum::<usize>() > crate::api::REQUEST_FILES_MAX {
        return Err(format!("the files come to more than {} MB - send fewer, or smaller", crate::api::REQUEST_FILES_MAX >> 20));
    }
    // The files into a folder of their own the platform account reads; R
    // copies them beside the request and this one goes.
    let fdir = platform_scratch()?.join(format!("files-{}-{n}", std::process::id()));
    if !files.is_empty() {
        std::fs::create_dir_all(&fdir).map_err(|e| e.to_string())?;
        for (i, f) in files.iter().enumerate() {
            let name: String = f.name.chars().map(|c| if c.is_ascii_alphanumeric() || "._-".contains(c) { c } else { '_' }).take(120).collect();
            let bytes = f.bytes().ok_or_else(|| format!("{}: not readable", f.name))?;
            let p = platform_file(&format!("files-{}-{n}/{i}-{name}", std::process::id()), &bytes)?;
            let _ = p;
        }
        #[cfg(unix)]
        if is_root() {
            let (uid, _) = platform_account().ok_or("no platform checkout on this machine")?;
            std::os::unix::fs::chown(&fdir, Some(uid), None).map_err(|e| e.to_string())?;
        }
    }
    let fd = if files.is_empty() { String::new() } else { fdir.display().to_string() };
    let v = platform_r_as(owner, "request.R", REQUEST_R, &[&domain, &wf.display().to_string(), &tf.display().to_string(), if blocked { "blocked" } else { "normal" }, &fd], 60);
    let _ = (std::fs::remove_file(&wf), std::fs::remove_file(&tf), std::fs::remove_dir_all(&fdir));
    v?["id"].as_i64().ok_or_else(|| "no request id came back".into())
}

/// Which Claude `claude` in the Terminal runs on - the same answer
/// host_run_claude gives an agent turn: the operator (sub_share owner) on this
/// machine's own `claude` login; anyone else on their own Claude, the
/// "<email>-claude" secret console.ndexr.io's Claude tab keeps (an OAuth token
/// from `claude setup-token`, or an API key). Never the house key: that is
/// metered per turn, and a terminal is not a turn.
const CLAUDE_R: &str = r#"
args <- commandArgs(TRUE); owner <- args[[length(args)]]
out <- function(x) { cat(jsonlite::toJSON(x, auto_unbox = TRUE, null = "null")); quit(status = 0) }
options(box.path = Sys.getenv("NDEXR_SRC"))
box::use(pg = r/connections/postgres, ss = r/users/sub_share, cc = r/users/claude_credential)
email <- tryCatch({
  con <- pg$connection_postgres_direct(); on.exit(pool::poolReturn(con), add = TRUE)
  r <- DBI::dbGetQuery(con, "SELECT email FROM ndexr_users WHERE lower(username) = lower($1) LIMIT 1", params = list(owner))
  if (nrow(r)) r$email[[1]] else ""
}, error = function(e) "")
if (!nzchar(email)) out(list(error = paste0("no ndexr account named ", owner)))
box::use(sec = r/aws/secrets/secrets)
raw <- tryCatch(suppressMessages(sec$secret_get(cc$secret_name(email), json = FALSE)), error = function(e) NULL)
tok <- if (is.list(raw)) raw$CLAUDE_CODE_OAUTH_TOKEN else NULL
key <- if (is.list(raw)) raw$ANTHROPIC_API_KEY else NULL
if (length(tok) && nzchar(tok)) out(list(kind = "oauth_token", value = tok, label = "your Claude (subscription)"))
if (length(key) && nzchar(key)) out(list(kind = "api_key", value = key, label = "your Claude (API key)"))
if (isTRUE(ss$is_owner(email))) out(list(kind = "own_login", label = "this machine's claude login"))
out(list(kind = "none", label = "nothing yet - Help > Connect Claude in Cove, or /login in claude"))
"#;

/// The environment that puts `claude` on the person's own Claude, and a few
/// words saying which. Asked once per server (a terminal opens in a moment
/// after the first), again after ten minutes, so a Claude connected meanwhile
/// is picked up by the next terminal.
pub fn claude_env() -> (Vec<(String, String)>, String) {
    // A session `cove multiuser` started: it keeps the person's Claude in
    // CLAUDE_DIR, rewritten when they connect one in setup.
    if let Some(v) = std::env::var("COVE_OWNER").ok().filter(|o| !o.is_empty() && !o.contains(['/', '.'])).and_then(|o| std::fs::read(Path::new(CLAUDE_DIR).join(format!("{o}.json"))).ok()).and_then(|b| serde_json::from_slice::<Value>(&b).ok()) {
        let value = v["value"].as_str().unwrap_or("").to_owned();
        let env = match v["kind"].as_str() {
            Some("oauth_token") => vec![("CLAUDE_CODE_OAUTH_TOKEN".to_owned(), value)],
            Some("api_key") => vec![("ANTHROPIC_API_KEY".to_owned(), value)],
            _ => Vec::new(),
        };
        return (env, v["label"].as_str().unwrap_or("").to_owned());
    }
    // ... or, from before that file, it looked for the person already,
    // as root, since the person's own account cannot read the platform.
    if let Ok(label) = std::env::var("COVE_CLAUDE_LABEL") {
        let value = std::env::var("COVE_CLAUDE_VALUE").unwrap_or_default();
        let env = match std::env::var("COVE_CLAUDE_KIND").as_deref() {
            Ok("oauth_token") => vec![("CLAUDE_CODE_OAUTH_TOKEN".to_owned(), value)],
            Ok("api_key") => vec![("ANTHROPIC_API_KEY".to_owned(), value)],
            _ => Vec::new(),
        };
        return (env, label);
    }
    static CACHE: std::sync::OnceLock<std::sync::Mutex<Option<(std::time::Instant, Vec<(String, String)>, String)>>> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some((at, env, label)) = cache.lock().unwrap().as_ref() {
        if at.elapsed() < Duration::from_secs(600) {
            return (env.clone(), label.clone());
        }
    }
    let (env, label) = match platform_r("claude.R", CLAUDE_R, &[], 30) {
        Ok(v) => {
            let value = v["value"].as_str().unwrap_or("").to_owned();
            let env = match v["kind"].as_str() {
                Some("oauth_token") => vec![("CLAUDE_CODE_OAUTH_TOKEN".to_owned(), value)],
                Some("api_key") => vec![("ANTHROPIC_API_KEY".to_owned(), value)],
                _ => Vec::new(),
            };
            (env, v["label"].as_str().unwrap_or("").to_owned())
        }
        // No platform here (a desktop install, a cell): claude's own /login.
        Err(_) => (Vec::new(), String::new()),
    };
    *cache.lock().unwrap() = Some((std::time::Instant::now(), env.clone(), label.clone()));
    (env, label)
}

/// Cove's setup (setup.rs): where the person stands with Claude, and
/// connecting or removing their own - src/r/credentials/connect.r (moved from
/// credentials.ndexr.io when that site was retired, 2026-11-25), the
/// same store, check and wording as console.ndexr.io's Claude tab. The token
/// comes in a 0600 file, never in argv.
const CLAUDE_SET_R: &str = r#"
args <- commandArgs(TRUE); op <- args[[1]]; owner <- args[[length(args)]]
out <- function(x) { cat(jsonlite::toJSON(x, auto_unbox = TRUE, null = "null")); quit(status = 0) }
options(box.path = Sys.getenv("NDEXR_SRC"))
box::use(pg = r/connections/postgres, cn = r/credentials/connect)
email <- tryCatch({
  con <- pg$connection_postgres_direct(); on.exit(pool::poolReturn(con), add = TRUE)
  r <- DBI::dbGetQuery(con, "SELECT email FROM ndexr_users WHERE lower(username) = lower($1) LIMIT 1", params = list(owner))
  if (nrow(r)) r$email[[1]] else ""
}, error = function(e) "")
if (!nzchar(email)) out(list(error = paste0("no ndexr account named ", owner)))
if (identical(op, "connect")) {
  token <- trimws(paste(readLines(args[[2]], warn = FALSE), collapse = ""))
  r <- cn$save(email, token)
  if (!isTRUE(r$ok)) out(list(error = r$message))
} else if (identical(op, "forget")) {
  cn$forget(email)
}
out(cn$summary(email))
"#;

/// What `op` ("status", "connect" with `token`, "forget") answers for `owner`.
pub fn claude_as(owner: &str, op: &str, token: &str) -> Result<crate::api::ClaudeView, String> {
    let tf = platform_file(&format!("claude-{}-{}.txt", std::process::id(), unique()), token.trim().as_bytes())?;
    let v = platform_r_as(owner, "claude-set.R", CLAUDE_SET_R, &[op, &tf.display().to_string()], 45);
    let _ = std::fs::remove_file(&tf);
    serde_json::from_value(v?).map_err(|e| e.to_string())
}

/// The Account tab: api.ndexr.io/account.r for the owner's email - the same
/// function api.ndexr.io answers the desktop app with. A key id revokes that
/// key of theirs first (account.r scopes it to them).
const ACCOUNT_R: &str = r#"
args <- commandArgs(TRUE); revoke <- args[[1]]; owner <- args[[length(args)]]
out <- function(x) { cat(jsonlite::toJSON(x, auto_unbox = TRUE, null = "null", na = "null")); quit(status = 0) }
options(box.path = Sys.getenv("NDEXR_SRC"))
box::use(pg = r/connections/postgres, acct = domains/drennanfreddy/`api.ndexr.io`/account)
email <- tryCatch({
  con <- pg$connection_postgres_direct(); on.exit(pool::poolReturn(con), add = TRUE)
  r <- DBI::dbGetQuery(con, "SELECT email FROM ndexr_users WHERE lower(username) = lower($1) LIMIT 1", params = list(owner))
  if (nrow(r)) r$email[[1]] else ""
}, error = function(e) "")
if (!nzchar(email)) out(list(error = paste0("no ndexr account named ", owner)))
if (nzchar(revoke)) {
  r <- tryCatch(acct$revoke(email, revoke), error = function(e) list(error = conditionMessage(e)))
  if (!is.null(r$error)) out(r)
}
out(tryCatch(acct$overview(email), error = function(e) list(error = conditionMessage(e))))
"#;

/// `owner`'s account for the Account tab; `revoke`, when not empty, a key id
/// of theirs to revoke first.
pub fn account_as(owner: &str, revoke: &str) -> Result<crate::api::AccountView, String> {
    // A revoke changes it: always asked, and what it answers is kept.
    if !revoke.trim().is_empty() {
        let v = account_fetch(owner, revoke)?;
        account_cache().lock().unwrap().insert(owner.to_owned(), (std::time::Instant::now(), v.clone()));
        return Ok(v);
    }
    // Operator, 2026-10-18: "Account data shouldnt take so long to get for
    // users on load". Asking is a platform R start (seconds), so the last
    // answer is given at once and asked again behind it when it is over a
    // minute old; the first is warmed when the session starts (account_warm).
    let cached = account_cache().lock().unwrap().get(owner).cloned();
    match cached {
        Some((at, v)) => {
            if at.elapsed() > Duration::from_secs(60) {
                account_warm(owner);
            }
            Ok(v)
        }
        None => {
            let v = account_fetch(owner, "")?;
            account_cache().lock().unwrap().insert(owner.to_owned(), (std::time::Instant::now(), v.clone()));
            Ok(v)
        }
    }
}

fn account_fetch(owner: &str, revoke: &str) -> Result<crate::api::AccountView, String> {
    let v = platform_r_as(owner, "account.R", ACCOUNT_R, &[revoke.trim()], 45)?;
    serde_json::from_value(v).map_err(|e| e.to_string())
}

type AccountCache = std::sync::Mutex<std::collections::HashMap<String, (std::time::Instant, crate::api::AccountView)>>;

fn account_cache() -> &'static AccountCache {
    static C: std::sync::OnceLock<AccountCache> = std::sync::OnceLock::new();
    C.get_or_init(Default::default)
}

/// `account_warm`, only when nothing is held for `owner` yet.
pub fn account_warm_if_cold(owner: &str) {
    if !account_cache().lock().unwrap().contains_key(owner) {
        account_warm(owner);
    }
}

/// Ask for `owner`'s account in the background, so the Account tab has it
/// before it is opened (a session starting, or a stale answer given). One at
/// a time per person.
pub fn account_warm(owner: &str) {
    static BUSY: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
    let busy = BUSY.get_or_init(Default::default);
    if owner.is_empty() || !busy.lock().unwrap().insert(owner.to_owned()) {
        return;
    }
    let owner = owner.to_owned();
    std::thread::spawn(move || {
        if let Ok(v) = account_fetch(&owner, "") {
            account_cache().lock().unwrap().insert(owner.clone(), (std::time::Instant::now(), v));
        }
        busy.lock().unwrap().remove(&owner);
    });
}

/// Where `cove multiuser` keeps each person's Claude for their sessions'
/// terminals, root-owned and readable by their group only - rewritten when
/// they connect or remove one in setup, so the next terminal has it without
/// a new session.
pub const CLAUDE_DIR: &str = "/run/cove/claude";

/// `owner`'s Claude for `cove multiuser` to hand a new session (as
/// COVE_CLAUDE_KIND / _VALUE / _LABEL): (kind, value, label), or None when it
/// could not be asked here.
pub fn claude_lookup(owner: &str) -> Option<(String, String, String)> {
    let v = platform_r_as(owner, "claude.R", CLAUDE_R, &[], 30).ok()?;
    let s = |k: &str| v[k].as_str().unwrap_or("").to_owned();
    Some((s("kind"), s("value"), s("label")))
}

/// The person's plan and storage cap, both billing.ndexr.io's: the tier from
/// r/billing/tier.r (live Stripe subscription, comped, or admin) and the cap
/// for it from billing's pricing.r. Cove states neither number itself.
const PLAN_R: &str = r#"
args <- commandArgs(TRUE); owner <- args[[length(args)]]
out <- function(x) { cat(jsonlite::toJSON(x, auto_unbox = TRUE, null = "null")); quit(status = 0) }
options(box.path = Sys.getenv("NDEXR_SRC"))
box::use(pg = r/connections/postgres, tr = r/billing/tier, pricing = domains/fdrennan/`billing.ndexr.io`/pricing)
email <- tryCatch({
  con <- pg$connection_postgres_direct(); on.exit(pool::poolReturn(con), add = TRUE)
  r <- DBI::dbGetQuery(con, "SELECT email FROM ndexr_users WHERE lower(username) = lower($1) LIMIT 1", params = list(owner))
  if (nrow(r)) r$email[[1]] else ""
}, error = function(e) "")
if (!nzchar(email)) out(list(error = paste0("no ndexr account named ", owner)))
plan <- tryCatch(tr$tier(email), error = function(e) out(list(error = conditionMessage(e))))
cap <- pricing$storage_cap_for(email, plan)
out(list(plan = plan, cap = if (is.na(cap)) 0 else cap))
"#;

/// `owner`'s (plan, storage cap in bytes; 0 = none) for `cove multiuser`'s
/// storage keeper, or None when it could not be asked.
#[cfg(unix)]
pub fn plan_lookup(owner: &str) -> Option<(String, u64)> {
    let v = platform_r_as(owner, "plan.R", PLAN_R, &[], 30).ok()?;
    Some((v["plan"].as_str()?.to_owned(), v["cap"].as_f64().unwrap_or(0.0) as u64))
}

/// Speech to text for every agent box (operator, 2026-10-15: "All agents need
/// to allow for audio submission and translation like we do with
/// exec.ndexr.io"): exec's own path - r/transcribe.r (Whisper, compressed and
/// chunked) biased by and corrected with the person's dictation vocabulary
/// (r/vocab.r), so "aws.ndexr.io" comes back as that, not "AWS index R dot io".
const TRANSCRIBE_R: &str = r#"
args <- commandArgs(TRUE); path <- args[[1]]; owner <- args[[length(args)]]
out <- function(x) { cat(jsonlite::toJSON(x, auto_unbox = TRUE, null = "null")); quit(status = 0) }
options(box.path = Sys.getenv("NDEXR_SRC"))
box::use(pg = r/connections/postgres, tr = r/transcribe, vocab = r/vocab)
email <- tryCatch({
  con <- pg$connection_postgres_direct(); on.exit(pool::poolReturn(con), add = TRUE)
  r <- DBI::dbGetQuery(con, "SELECT email FROM ndexr_users WHERE lower(username) = lower($1) LIMIT 1", params = list(owner))
  if (nrow(r)) r$email[[1]] else ""
}, error = function(e) "")
hint <- tryCatch(vocab$vocab_hint(email), error = function(e) "")
r <- tr$transcribe_audio_status(path, hint = hint)
# Nothing said (silence, or only what transcribe.r drops as made up): no text,
# not an error - a Listen piece with no speech in it is skipped quietly.
if (!isTRUE(r$ok) && identical(r$text, "empty transcript")) out(list(text = ""))
if (!isTRUE(r$ok)) out(list(error = paste("could not transcribe:", r$text)))
out(list(text = tryCatch(vocab$vocab_apply(r$text, email), error = function(e) r$text)))
"#;

/// A recording's container from its first bytes: WebM, MP4 (an iPhone's
/// AAC), Ogg, WAV, FLAC or MP3 - the file name Whisper reads the format by.
fn audio_kind(b: &[u8]) -> Option<&'static str> {
    let at = |i: usize, m: &[u8]| b.get(i..i + m.len()) == Some(m);
    Some(if at(0, &[0x1A, 0x45, 0xDF, 0xA3]) {
        "webm"
    } else if at(4, b"ftyp") {
        "mp4"
    } else if at(0, b"OggS") {
        "ogg"
    } else if at(0, b"RIFF") && at(8, b"WAVE") {
        "wav"
    } else if at(0, b"fLaC") {
        "flac"
    } else if at(0, b"ID3") || (b.len() > 1 && b[0] == 0xFF && b[1] & 0xE0 == 0xE0) {
        "mp3"
    } else {
        return None;
    })
}

/// A recording (a browser's data URL: `data:audio/webm;codecs=opus;base64,…`)
/// as text, for `owner`.
pub fn transcribe_as(owner: &str, data_url: &str) -> Result<String, String> {
    let (head, b64) = data_url.trim().split_once(',').ok_or("not a recording")?;
    let mime = head.trim_start_matches("data:").split(';').next().unwrap_or("");
    let ext = match mime {
        "audio/mp4" | "video/mp4" => "mp4",
        "audio/x-m4a" => "m4a",
        "audio/ogg" => "ogg",
        "audio/wav" | "audio/x-wav" => "wav",
        _ => "webm",
    };
    let bytes = crate::api::b64_decode(b64).ok_or("the recording did not decode")?;
    if bytes.len() < 512 {
        return Err("nothing was recorded".into());
    }
    // What the bytes are, over what the browser said: an iPhone's Safari
    // labelled its recording one thing and Whisper refused it as "Invalid
    // file format" (operator's screenshot, 2026-11-26).
    let ext = audio_kind(&bytes).unwrap_or(ext);
    let f = platform_file(&format!("voice-{}-{}.{ext}", std::process::id(), unique()), &bytes)?;
    // Then 16 kHz mono FLAC through the stack's FFmpeg, a format Whisper
    // always takes - Safari's fragmented MP4 among those it does not. Whisper
    // hears 16 kHz mono anyway, so nothing is lost; if FFmpeg is not there,
    // the recording goes as it came.
    let flac = f.with_extension("flac");
    let convert = format!(
        "set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1; module load FFmpeg >/dev/null 2>&1; ffmpeg -nostdin -hide_banner -loglevel error -y -i {} -vn -ac 1 -ar 16000 -c:a flac {} && echo converted",
        shell_quote(&f.display().to_string()),
        shell_quote(&flac.display().to_string())
    );
    let send = if capture_within(&convert, Duration::from_secs(60)).is_some_and(|o| o.trim().ends_with("converted")) && flac.is_file() { flac.clone() } else { f.clone() };
    let v = platform_r_as(owner, "transcribe.R", TRANSCRIBE_R, &[&send.display().to_string()], 180);
    let _ = std::fs::remove_file(&f);
    let _ = std::fs::remove_file(&flac);
    v?["text"].as_str().map(|t| t.trim().to_owned()).ok_or_else(|| "no text came back".into())
}

/// The OpenAI key: the server's own environment, else the platform's .env
/// (readable by `cove multiuser` as root and by the platform account - never
/// by a person's session, which is why multiuser answers `Assist` itself).
pub(crate) fn openai_key() -> Option<String> {
    if let Some(k) = std::env::var("OPENAI_API_KEY").ok().filter(|k| !k.trim().is_empty()) {
        return Some(k.trim().to_owned());
    }
    let env = std::fs::read_to_string(platform_root().join(".env")).ok()?;
    env.lines()
        .filter_map(|l| l.trim().strip_prefix("export ").unwrap_or(l.trim()).split_once('='))
        .filter(|(n, _)| n.trim() == "OPENAI_API_KEY")
        .map(|(_, v)| v.trim().trim_matches(['"', '\'']).to_owned())
        .rfind(|v| !v.is_empty())
}

/// An ask in a console's input, Agent ticked (`Query::Assist`): one short call to
/// OpenAI's chat completions for code in `lang`, given the console's recent
/// lines and its environment. Fast and small on purpose (operator,
/// 2026-10-18: "much shorter or faster updates within the R or Python or $
/// interface ... claude not needed for this, chatgpt api is good enough").
/// COVE_ASSIST_MODEL picks the model (default gpt-4o-mini, the platform's own
/// everyday model).
pub fn assist(lang: Lang, ask: &str, context: &str) -> Result<crate::api::AssistReply, String> {
    let ask = ask.trim();
    if ask.is_empty() {
        return Err("say what you want - e.g. ggplot of mtcars mpg against wt".into());
    }
    // The server's key, else the person's own (Help › Connect OpenAI…, the
    // key Codex uses): a self-install has only the second.
    let key = openai_key()
        .or_else(localagent::codex::saved_key)
        .or_else(|| std::env::var("CODEX_API_KEY").ok().filter(|k| !k.trim().is_empty()))
        .ok_or(crate::api::NO_OPENAI)?;
    let model = std::env::var("COVE_ASSIST_MODEL").ok().filter(|m| !m.is_empty()).unwrap_or_else(|| "gpt-4o-mini".into());
    let (name, how) = match lang {
        Lang::R => ("R", "R code for the R console. Use the packages the session has loaded where they fit."),
        Lang::Python => ("Python", "Python code. It runs in Python inside R through reticulate, so plain Python statements; use the modules the session has imported where they fit."),
        Lang::Shell => ("bash", "bash for the shell. One line where one line does it; the ndexr stack's programs come from `ml <module>` (Lmod)."),
        Lang::Julia => ("Julia", "Julia code for the Julia REPL. Load packages with `using`; add one with `import Pkg; Pkg.add(\"Name\")` only when asked."),
    };
    let system = format!(
        "You write {name} for a person typing into a live {name} console in Cove. {how} \
         Reply with only the code to put in the input - no prose, no markdown fences, no explanation. \
         Include the library()/import lines the code needs unless the console shows them already loaded. \
         Comments only where they help. Prefer short, runnable code that fits what the console shows."
    );
    let user = format!("Recent console and environment:\n{}\n\nWrite {name} for: {ask}", context.chars().rev().take(6000).collect::<String>().chars().rev().collect::<String>());
    let body = serde_json::json!({
        "model": model,
        "temperature": 0.2,
        "max_tokens": 700,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
    });
    let mut req = ehttp::Request::post("https://api.openai.com/v1/chat/completions", serde_json::to_vec(&body).map_err(|e| e.to_string())?);
    // Set, not inserted: post() already carries text/plain, and insert adds
    // a second Content-Type that OpenAI reads first ("you must provide a
    // model parameter" - the JSON was never parsed).
    req.headers = ehttp::Headers::new(&[("Accept", "application/json"), ("Content-Type", "application/json")]);
    req.headers.insert("Authorization", format!("Bearer {key}"));
    req.timeout = Some(Duration::from_secs(45));
    let r = ehttp::fetch_blocking(&req).map_err(|e| format!("OpenAI: {e}"))?;
    let v: Value = serde_json::from_slice(&r.bytes).map_err(|_| format!("OpenAI answered {}", r.status))?;
    if !r.ok {
        return Err(format!("OpenAI: {}", v["error"]["message"].as_str().unwrap_or(&r.status_text)));
    }
    let text = v["choices"][0]["message"]["content"].as_str().unwrap_or("").trim();
    Ok(crate::api::AssistReply { code: strip_fences(text), model })
}

/// A reply that came fenced anyway: only what is inside the first fence.
fn strip_fences(t: &str) -> String {
    let t = t.trim();
    match t.strip_prefix("```") {
        Some(rest) => {
            let rest = rest.split_once('\n').map_or("", |(_, r)| r);
            rest.rsplit_once("```").map_or(rest, |(code, _)| code).trim_end().to_owned()
        }
        None => t.to_owned(),
    }
}

/// `transcribe_as` the person this server runs for.
pub fn transcribe(data_url: &str) -> Result<String, String> {
    let owner = std::env::var("COVE_OWNER").ok().filter(|o| !o.is_empty()).or_else(|| std::env::var("USER").ok()).unwrap_or_default();
    transcribe_as(&owner, data_url)
}

/// One line of claude's stream-json, as the events the panel shows, and the
/// run's cost and how it is paid for.
pub(crate) fn agent_events(line: &str, chunk: &mut crate::api::AgentChunk) {
    let (out, cost) = (&mut chunk.events, &mut chunk.cost);
    let ev = |kind: &str, text: String| crate::api::AgentEvent { kind: kind.into(), text };
    let Ok(v) = serde_json::from_str::<Value>(line) else { return };
    // A turn on OpenAI's Codex (localagent/codex.rs): its own event names.
    if localagent::codex::events(&v, out) {
        return;
    }
    match v["type"].as_str() {
        // claude names its credential on the init line: "none" is the
        // person's own signed-in plan (OAuth), a key name is the API. Measured
        // 2026-11-18 over 40 runs in host_runs: 39 "none", 1 ANTHROPIC_API_KEY.
        Some("system") if v["subtype"] == "init" => {
            chunk.subscription = v["apiKeySource"].as_str() == Some("none");
        }
        Some("assistant") => {
            for c in v["message"]["content"].as_array().into_iter().flatten() {
                match c["type"].as_str() {
                    Some("text") => {
                        let t = c["text"].as_str().unwrap_or("").trim();
                        if !t.is_empty() {
                            out.push(ev("text", t.to_owned()));
                        }
                    }
                    Some("tool_use") => {
                        let name = c["name"].as_str().unwrap_or("tool");
                        let i = &c["input"];
                        let what = ["description", "command", "file_path", "pattern", "path", "url", "query", "prompt"]
                            .iter()
                            .find_map(|k| i[*k].as_str())
                            .unwrap_or("");
                        let what: String = what.lines().next().unwrap_or("").chars().take(160).collect();
                        out.push(ev("tool", if what.trim().is_empty() { name.to_owned() } else { format!("{name} · {what}") }));
                    }
                    _ => {}
                }
            }
        }
        Some("result") => {
            *cost = v["total_cost_usd"].as_f64().or(*cost);
            if v["is_error"].as_bool() == Some(true) {
                out.push(ev("error", v["result"].as_str().unwrap_or("the run failed").to_owned()));
            }
        }
        _ => {}
    }
}

pub fn agent_poll(id: &str, offset: u64) -> Result<crate::api::AgentChunk, String> {
    use std::io::{Read, Seek, SeekFrom};
    if !valid_run_id(id) {
        return Err("bad run id".into());
    }
    let runs = Path::new(HOST_RUNS);
    let mut chunk = crate::api::AgentChunk { offset, ..Default::default() };
    if let Ok(mut f) = std::fs::File::open(runs.join(format!("{id}.stream"))) {
        let _ = f.seek(SeekFrom::Start(offset));
        let mut buf = Vec::new();
        let _ = f.take(4 << 20).read_to_end(&mut buf);
        // Only whole lines: a line still being written waits for the next poll.
        let upto = buf.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        for line in String::from_utf8_lossy(&buf[..upto]).lines() {
            agent_events(line, &mut chunk);
        }
        chunk.offset = offset + upto as u64;
    }
    chunk.done = runs.join(format!("{id}.done")).exists();
    // A run killed before it could finish (2026-10-17: every Cove restart
    // killed the agents in its cgroup - ndexr-claude-run now detaches into a
    // scope of its own) never writes .done, and its turn spun forever. Its
    // process gone, it is over: say so once, so it can be sent again.
    if !chunk.done && run_gone(runs, id) {
        chunk.done = true;
        chunk.events.push(crate::api::AgentEvent {
            kind: "error".into(),
            text: "this run stopped without finishing - its process is gone (Cove restarting used to kill agents mid-turn). Resend it to carry on; the thread keeps everything it did.".into(),
        });
    }
    Ok(chunk)
}

/// A run with no .done whose process is no longer running: its .pid names a
/// process that is gone, or is now something other than the run (a reused
/// pid). A run is only judged after 15 s, so one still starting is never
/// called gone.
fn run_gone(runs: &Path, id: &str) -> bool {
    let pid_file = runs.join(format!("{id}.pid"));
    let Ok(m) = std::fs::metadata(&pid_file) else { return false };
    if m.modified().ok().and_then(|t| t.elapsed().ok()).is_none_or(|e| e < Duration::from_secs(15)) {
        return false;
    }
    let Some(pid) = std::fs::read_to_string(&pid_file).ok().and_then(|t| t.trim().parse::<u32>().ok()) else { return false };
    match std::fs::read(format!("/proc/{pid}/cmdline")) {
        Ok(cmd) => {
            let cmd = String::from_utf8_lossy(&cmd);
            !(cmd.contains("ndexr-claude-run") || cmd.contains("claude"))
        }
        Err(_) => true,
    }
}

fn agent_stop(id: &str) -> Result<bool, String> {
    agent_stop_run(id)
}

/// Stop a run by id - `cove multiuser` checks first that it is the asker's.
pub fn agent_stop_run(id: &str) -> Result<bool, String> {
    if !valid_run_id(id) {
        return Err("bad run id".into());
    }
    let pid: u32 = std::fs::read_to_string(Path::new(HOST_RUNS).join(format!("{id}.pid")))
        .ok()
        .and_then(|p| p.trim().parse().ok())
        .ok_or("that run is not running")?;
    let _ = Command::new("kill").args(["-TERM", &format!("-{pid}")]).status();
    Ok(Command::new("kill").args(["-TERM", &pid.to_string()]).status().is_ok_and(|s| s.success()))
}

/// Where this session's person's Exec thread is. The platform decides
/// (AGENT_R: the operator's exec.ndexr.io thread, anyone else's
/// _account/<username>), asked once and kept - a session is one person's.
/// `owner`'s Exec thread folder, asked of the platform once per owner - per
/// owner because `cove multiuser` answers for everyone (it was one folder per
/// process, right only while a process served one person).
fn exec_thread_dir(owner: &str) -> Result<PathBuf, String> {
    static DIRS: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<String, PathBuf>>> = std::sync::OnceLock::new();
    let dirs = DIRS.get_or_init(Default::default);
    if let Some(d) = dirs.lock().unwrap().get(owner) {
        return Ok(d.clone());
    }
    let v = platform_r_as(owner, "agent.R", AGENT_R, &["exec.ndexr.io", "@thread", ""], 60)?;
    let d = PathBuf::from(v["dir"].as_str().ok_or("no thread folder came back")?);
    if !d.starts_with(HOST_RUNS) || d.components().any(|c| c.as_os_str() == "..") {
        return Err(format!("{}: not a thread folder", d.display()));
    }
    dirs.lock().unwrap().insert(owner.to_owned(), d.clone());
    Ok(d)
}

/// The thread's last turns: each .prompt in _domain/<host>/ (the Exec scope:
/// `exec_thread_dir`), newest last.
fn agent_history(domain: &str) -> Result<Vec<crate::api::AgentTurn>, String> {
    agent_history_as(&session_owner(), domain)
}

/// `owner`'s thread folder for `domain`: their Exec thread, or the domain's.
fn thread_dir_as(owner: &str, domain: &str) -> Result<PathBuf, String> {
    Ok(if domain == "exec.ndexr.io" { exec_thread_dir(owner)? } else { Path::new(HOST_RUNS).join("_domain").join(domain) })
}

/// How many runs are going in each domain's thread (Query::AgentBusy, the
/// tabs' activity; operator, 2026-11-14: "for each tab, see if it has some
/// activity going with an agent or is idle"). Only a thread's newest runs are
/// read - a run is going while it has no .done and its process is there
/// (agent_poll's own test) - so a long thread costs no more than a short one.
pub fn agent_busy(owner: &str, domains: &[String]) -> std::collections::BTreeMap<String, u32> {
    let runs = Path::new(HOST_RUNS);
    domains
        .iter()
        .filter(|d| is_host_name(d))
        .filter_map(|d| {
            let td = thread_dir_as(owner, d).ok()?;
            let n = busy_in(runs, &td);
            (n > 0).then(|| (d.clone(), n))
        })
        .collect()
}

/// The runs going among a thread's newest few.
fn busy_in(runs: &Path, thread: &Path) -> u32 {
    const NEWEST: usize = 6;
    let mut ids: Vec<String> = std::fs::read_dir(thread)
        .map(|r| r.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".prompt")).map(str::to_owned)).collect())
        .unwrap_or_default();
    ids.sort();
    ids.iter().rev().take(NEWEST).filter(|id| valid_run_id(id) && !runs.join(format!("{id}.done")).exists() && runs.join(format!("{id}.stream")).exists() && !run_gone(runs, id)).count() as u32
}

/// Whether run `id` is a turn of `owner`'s thread for `domain` - its .prompt
/// is there. How `cove multiuser` knows a run is theirs after a restart has
/// emptied its memory of who started what.
pub fn run_in_thread_as(owner: &str, domain: &str, id: &str) -> bool {
    valid_run_id(id) && is_host_name(domain) && thread_dir_as(owner, domain).is_ok_and(|td| td.join(format!("{id}.prompt")).is_file())
}

/// `agent_history` for a named owner (`cove multiuser`, which checks first
/// that a domain thread is theirs; the Exec thread is theirs by construction).
pub fn agent_history_as(owner: &str, domain: &str) -> Result<Vec<crate::api::AgentTurn>, String> {
    let domain = domain.trim().to_ascii_lowercase();
    if !is_host_name(&domain) {
        return Err(format!("{domain}: not a domain name"));
    }
    let td = thread_dir_as(owner, &domain)?;
    let mut ids: Vec<String> = std::fs::read_dir(&td)
        .map(|r| r.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".prompt")).map(str::to_owned)).collect())
        .unwrap_or_default();
    ids.sort();
    let from = ids.len().saturating_sub(12);
    Ok(ids[from..]
        .iter()
        .filter(|id| valid_run_id(id))
        .map(|id| {
            let prompt = std::fs::read_to_string(td.join(format!("{id}.prompt"))).unwrap_or_default();
            let chunk = agent_poll(id, 0).unwrap_or_default();
            // Named only for someone else's turn: the viewer's own stay unmarked.
            let by = std::fs::read_to_string(td.join(format!("{id}.by"))).map(|b| b.trim().to_owned()).unwrap_or_default();
            let by = if by.is_empty() || by == owner { String::new() } else { by };
            let at = std::fs::metadata(td.join(format!("{id}.prompt")))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs() as i64);
            crate::api::AgentTurn { id: id.clone(), prompt, events: chunk.events, done: chunk.done, by, at }
        })
        .collect())
}

/// Where the platform keeps every domain's working tree - the only place
/// git.ndexr.io's Archive acts (retire.r's .DOMAINS).
pub const PLATFORM_DOMAINS: &str = "/home/ubuntu/ndexr/src/domains";

/// The platform checkout (`PLATFORM_DOMAINS`'s repository root).
#[cfg_attr(not(unix), allow(dead_code))]
pub fn platform_root() -> PathBuf {
    Path::new(PLATFORM_DOMAINS).ancestors().nth(2).map(Path::to_path_buf).unwrap_or_default()
}

/// git.ndexr.io's Archive, run as git.ndexr.io runs it: its own retire.r
/// writes the script (verify the history is in S3 - clean tree, manifest,
/// every branch at the same sha - and only then remove the tree), so there is
/// one implementation of the one action that deletes source, not two.
fn archive(host: &str) -> Result<JobView, String> {
    let host = host.trim().to_ascii_lowercase();
    let cmd = archive_cmd(&host)?;
    stack::jobs::start("archive", format!("archive {host}"), cmd, None)
}

/// The Archive command for `host`, to run as the platform account: in a
/// session that is one (the operator's, `archive`), or from `cove multiuser`
/// for everyone else, whose own account cannot read the platform's domains
/// (2026-10-26: fdrennan's Delete stopped at "Archive runs where the
/// platform's domains live ... not on this machine").
pub fn archive_cmd(host: &str) -> Result<String, String> {
    let host = host.trim().to_ascii_lowercase();
    if !is_host_name(&host) {
        return Err(format!("{host}: not a domain name"));
    }
    let root = Path::new(PLATFORM_DOMAINS);
    if !root.is_dir() {
        return Err("Archive runs where the platform's domains live (git.ndexr.io's host) - not on this machine".into());
    }
    let src = root.parent().ok_or("no src/")?.to_path_buf();
    let retire = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .flat_map(|p| [p.join("retire.r"), p.join("git.ndexr.io/retire.r")])
        .find(|p| p.is_file() && p.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "git.ndexr.io"))
        .ok_or("git.ndexr.io's retire.r is not on this machine")?;
    // box path segments, relative to src/: domains/<owner>/`git.ndexr.io`/retire
    let rel = retire.strip_prefix(&src).map_err(|e| e.to_string())?.with_extension("");
    let segs: Vec<String> = rel.iter().map(|s| {
        let s = s.to_string_lossy();
        if s.contains(['.', '-']) { format!("`{s}`") } else { s.into_owned() }
    }).collect();
    let env = PLATFORM_ENV;
    let script = format!(
        "options(box.path = getwd()); box::use(retire = {}); s <- retire$retire_cmd(commandArgs(TRUE)[1]); if (is.null(s)) quit(status = 2); cat(s)",
        segs.join(" / ")
    );
    // Saved first - committed and mirrored to the S3 git archive, as Save
    // does - so a project never committed (or with edits since) can still be
    // archived: retire's gate proves S3 holds it all, then removes the folder.
    // A new project deleted before its first Save was refused (2026-10-26).
    let cmd = format!(
        "set +u; cd {src} || exit 1; . /opt/ndexr/stack/use.sh >/dev/null 2>&1; ml {env} >/dev/null 2>&1; F=$(mktemp) || exit 1; \
         echo git.ndexr.io archive {h}; \
         d=$(ls -d domains/{h} domains/*/{h} 2>/dev/null | head -n1); \
         if [ -n \"$d\" ]; then echo saving {h} first: commit, then the S3 git archive; /home/ubuntu/.local/bin/ndexr bulk-commit \"$d\" --no-bump --no-push || echo the save did not finish - the archive below proves what S3 has; fi; \
         Rscript -e {rs} {h} > $F || {{ echo retire.r would not write the script; rm -f $F; exit 1; }}; \
         [ -s $F ] || {{ echo empty archive script; rm -f $F; exit 1; }}; \
         bash $F; rc=$?; rm -f $F; exit $rc",
        src = shell_quote(&src.display().to_string()),
        rs = shell_quote(&script),
        h = shell_quote(&host),
    );
    Ok(cmd)
}

fn list_dir(path: &str) -> Result<Listing, String> {
    let p = resolve(path)?.canonicalize().map_err(|e| format!("{path}: {e}"))?;
    let rd = std::fs::read_dir(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let ignored = ignored_names(&p);
    let mut entries: Vec<DirEntry> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || ignored.contains(&name) {
                return None;
            }
            let m = e.metadata().ok()?;
            let modified = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
            Some(DirEntry { name, dir: m.is_dir(), bytes: if m.is_dir() { 0 } else { m.len() }, modified })
        })
        .collect();
    entries.sort_by(|a, b| b.dir.cmp(&a.dir).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    Ok(Listing { path: p.display().to_string(), entries })
}

/// `ndexr glimpse <path>`: the folder's tree with each file's text, the block
/// a person pastes into another chat to give it the code (request #164,
/// 2026-11-01: "push button to immediately copy ndexr glimpse output from
/// Files section for the directory you are in"). Run as the person, so it
/// reads what they may read. A folder whose glimpse passes 4 MB is refused:
/// no chat takes that, and the clipboard holds it badly.
fn glimpse(path: &str) -> Result<String, String> {
    let p = resolve(path)?.canonicalize().map_err(|e| format!("{path}: {e}"))?;
    if !p.is_dir() {
        return Err(format!("{} is not a folder", p.display()));
    }
    let out = capture_within(&format!("ndexr glimpse {}", shell_quote(&p.display().to_string())), Duration::from_secs(30))
        .ok_or_else(|| format!("ndexr glimpse gave nothing for {} (is the ndexr CLI on this machine?)", p.display()))?;
    if out.len() > 4 << 20 {
        return Err(format!("the glimpse of {} is {} MB - open a smaller folder", p.display(), out.len() >> 20));
    }
    Ok(out)
}

/// The names the folder's own .gitignore lists outright - `target/`,
/// `dist/`, `node_modules` - which the Files pane leaves out: build output
/// and caches, no one's source (operator, 2026-11-01: "all projects need to
/// have a clean top level directory"). A pattern (`*.log`, `a/b`) hides
/// nothing; only a plain name does.
fn ignored_names(dir: &Path) -> std::collections::HashSet<String> {
    let Ok(text) = std::fs::read_to_string(dir.join(".gitignore")) else { return Default::default() };
    text.lines()
        .map(|l| l.trim().trim_start_matches('/').trim_end_matches('/'))
        .filter(|l| !l.is_empty() && !l.starts_with(['#', '!']) && !l.contains(['/', '*', '?', '[']))
        .map(str::to_owned)
        .collect()
}

/// A file's or folder's fingerprint: modified time (ns) and size; "" when it
/// cannot be read. A folder's time moves when an entry is added, removed or
/// renamed in it.
fn stamp(path: &str) -> String {
    let Ok(m) = resolve(path).and_then(|p| std::fs::metadata(&p).map_err(|e| e.to_string())) else { return String::new() };
    let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
    format!("{t}:{}", m.len())
}

/// A domain's site sources as one fingerprint - newest modified time, how
/// many, how big - "" when the folder is not there. Only what the page is made
/// of (.r, styles, scripts, markup, config); never .git, build output, uploads
/// or dependency trees, and at most 20,000 entries, so a big domain costs the
/// same as a small one.
/// When App reloads (operator, 2026-11-06: an agent building a site while a
/// podcast plays "needs a place that it can build without triggering the cove
/// refresh ... so we can only update when it actually matters"). Two rules,
/// for every project:
///   - `.cove/` never counts. Build in `.cove/stage/`, move the finished files
///     into place, and App reloads once; Cove's own run state and logs in
///     `.cove/` stop reloading it too.
///   - A project with a `.cove/reload` file reloads only when that file is
///     touched: the agent says when the content is ready, every save is quiet.
fn site_stamp(path: &str) -> Result<String, String> {
    const SKIP: [&str; 15] = [".git", ".cove", "node_modules", "renv", "dist", "logs", "supporting_docs", "__pycache__", ".venv", "venv", "packrat", "library", "site-library", ".cache", ".Rproj.user"];
    const EXT: [&str; 12] = ["r", "R", "css", "scss", "js", "html", "htm", "json", "yml", "yaml", "md", "qmd"];
    let Ok(root) = resolve(path) else { return Ok(String::new()) };
    if !root.is_dir() {
        return Ok(String::new());
    }
    if let Ok(m) = std::fs::metadata(root.join(".cove").join("reload")) {
        let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
        return Ok(format!("reload:{t}:{}", m.len()));
    }
    // What serves the site counts as the site: its live vhost
    // (nginx/sites-enabled/<host>). A route switched under a kept App frame
    // left ndexr.io showing the old redirect's page - Cove's viewer - after
    // `/` had become the React landing (operator, 2026-11-14: "make sure all
    // projects are aligned with what they are displayed as and open as").
    // Unreadable for the person: no part in the stamp, as before.
    let served = root
        .file_name()
        .and_then(|h| std::fs::metadata(Path::new(PLATFORM_DOMAINS).join("../../nginx/sites-enabled").join(h)).ok())
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    let (mut newest, mut count, mut size, mut seen) = (0u128, 0u64, 0u64, 0usize);
    let mut stack = vec![root];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > 20_000 {
                break;
            }
            let name = e.file_name();
            let name = name.to_string_lossy();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if !SKIP.contains(&name.as_ref()) && !name.starts_with("target") {
                    stack.push(e.path());
                }
                continue;
            }
            if !ft.is_file() || !name.rsplit_once('.').is_some_and(|(_, x)| EXT.contains(&x)) {
                continue;
            }
            let Ok(m) = e.metadata() else { continue };
            let t = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_nanos());
            newest = newest.max(t);
            count += 1;
            size += m.len();
        }
    }
    Ok(format!("{newest}:{count}:{size}:{served}"))
}

/// The last 64 KB of a project's app log: `.cove/run/app.log`, where up.sh
/// sends run.sh's stdout and stderr, or `.run/app.log` in a project from
/// before .cove/ (operator, 2026-11-08: "the stdout of the app needs to be
/// seen in the cove project under logs after app"). An empty path: no log.
fn app_log(dir: &str, build: bool) -> Result<FileText, String> {
    use std::io::{Read, Seek, SeekFrom};
    const KEEP: u64 = 64 * 1024;
    let root = resolve(dir)?;
    // Build's output (.cove/build.sh tees it there, 2026-11-08) or the app's.
    let names: &[&str] = if build { &[".cove/run/build.log"] } else { &[".cove/run/app.log", ".run/app.log"] };
    let Some(p) = names.iter().map(|f| root.join(f)).find(|p| p.is_file()) else {
        return Ok(FileText { path: String::new(), text: String::new() });
    };
    let mut f = std::fs::File::open(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(KEEP);
    f.seek(SeekFrom::Start(from)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let mut text = String::from_utf8_lossy(&bytes).into_owned();
    // Cut mid-file: start at a whole line.
    if from > 0 {
        if let Some(i) = text.find('\n') {
            text.drain(..=i);
        }
    }
    Ok(FileText { path: p.display().to_string(), text: keep_colour(&text) })
}

/// The shared web server's access log: every site's requests, in nginx.conf's
/// `detailed` format - `IP - HOST "METHOD PATH PROTO" STATUS BYTES
/// upstream=… ustatus=… urespt=… rt=… ua="…"`. Readable by root and `adm`.
const ACCESS_LOG: &str = "/var/log/nginx/access.log";

/// One site's recent requests, newest last (operator, 2026-11-17: "logs
/// arent helpful if they never say anything ... across all sites, react,
/// rust, etc"). Every app's traffic passes the web server, so this one
/// reader gives every project a log that says something, whatever its
/// language, without a line of the person's code changing. Visitors' IP
/// addresses and browser strings are left out: a site's owner needs what
/// was asked and how it went, not who asked. Cove's own reload polling
/// (`/__live`) is left out too, or it would be most of the log. The format
/// has no time field, so the lines carry none (order is the log's).
pub fn site_requests(host: &str) -> Result<FileText, String> {
    use std::io::{Read, Seek, SeekFrom};
    const SCAN: u64 = 8 * 1024 * 1024;
    const KEEP: usize = 300;
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() || host.contains(char::is_whitespace) {
        return Err("no site".into());
    }
    let mut f = std::fs::File::open(ACCESS_LOG).map_err(|e| format!("the web server's log cannot be read here ({e})"))?;
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    let from = len.saturating_sub(SCAN);
    f.seek(SeekFrom::Start(from)).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    f.read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&bytes);
    let www = format!("www.{host}");
    let mut lines: Vec<String> = text.lines().skip(usize::from(from > 0)).filter_map(|l| request_line(l, &host, &www)).collect();
    if lines.len() > KEEP {
        lines.drain(..lines.len() - KEEP);
    }
    if lines.is_empty() {
        return Ok(FileText { path: String::new(), text: String::new() });
    }
    Ok(FileText { path: format!("web server · {host}"), text: lines.join("\n") })
}

/// One access-log line as a person reads it - `GET /path  200  1.2 KB
/// 12 ms` - when it is for `host` (or `www.host`); None otherwise.
fn request_line(l: &str, host: &str, www: &str) -> Option<String> {
    let (_, rest) = l.split_once(" - ")?;
    let (h, rest) = rest.split_once(' ')?;
    if h != host && h != www {
        return None;
    }
    let rest = rest.strip_prefix('"')?;
    let (req, rest) = rest.split_once("\" ")?;
    let mut parts = req.split(' ');
    let (method, path) = (parts.next()?, parts.next().unwrap_or(""));
    if path.starts_with("/__live") {
        return None;
    }
    let mut it = rest.split(' ');
    let status = it.next()?;
    let bytes: u64 = it.next().and_then(|b| b.parse().ok()).unwrap_or(0);
    let field = |k: &str| rest.split(' ').find_map(|w| w.strip_prefix(k)).unwrap_or("-");
    let secs: f64 = field("urespt=").parse().or_else(|_| field("rt=").parse()).unwrap_or(0.0);
    let size = if bytes >= 1_000_000 { format!("{:.1} MB", bytes as f64 / 1e6) } else if bytes >= 1000 { format!("{:.1} KB", bytes as f64 / 1e3) } else { format!("{bytes} B") };
    let mut shown: String = path.chars().take(120).collect();
    if path.chars().count() > 120 {
        shown.push('…');
    }
    let flag = if status.starts_with('5') { "!" } else if status.starts_with('4') { "?" } else { " " };
    Some(format!("{flag} {method:<6} {shown}  {status}  {size}  {:.0} ms", secs * 1000.0))
}

/// A log as the Logs tab draws it: colour codes (ESC [ ... m) kept, for
/// ansi.rs to draw (operator, 2026-11-17: "I'm not seeing color in the text
/// output of App logs"); cursor and erase codes (ESC [ ... other letter) and
/// carriage returns dropped, since a scrolled log cannot replay them.
fn keep_colour(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '\u{1b}' {
            if it.peek() == Some(&'[') {
                it.next();
                let mut code = String::from("\u{1b}[");
                while let Some(&n) = it.peek() {
                    it.next();
                    code.push(n);
                    if n.is_ascii_alphabetic() {
                        break;
                    }
                }
                if code.ends_with('m') {
                    out.push_str(&code);
                }
            }
            continue;
        }
        if c != '\r' {
            out.push(c);
        }
    }
    out
}

fn read_file(path: &str) -> Result<FileText, String> {
    let p = resolve(path)?;
    let m = std::fs::metadata(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    if m.is_dir() {
        return Err(format!("{} is a folder", p.display()));
    }
    if m.len() > MAX_FILE {
        return Err(format!("{} is {} MB — too large to edit here", p.display(), m.len() >> 20));
    }
    let bytes = std::fs::read(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let text = String::from_utf8(bytes).map_err(|_| format!("{} is not text", p.display()))?;
    Ok(FileText { path: p.display().to_string(), text })
}

/// Written beside itself and renamed over, so a crash never leaves half a file.
fn write_file(path: &str, text: &str) -> Result<FileText, String> {
    let p = resolve(path)?;
    // A folder not made yet is made: a save keeping the agent's thread
    // writes .cove/agent/ the first time (agent_keep.rs). The session runs
    // as the person, so it makes only folders they may make.
    if let Some(d) = p.parent().filter(|d| !d.exists()) {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let parent = p.parent().filter(|d| d.is_dir()).ok_or_else(|| format!("{}: no such folder", p.display()))?;
    let name = p.file_name().ok_or("no file name")?.to_string_lossy().into_owned();
    let tmp = parent.join(format!(".{name}.cove.tmp"));
    std::fs::write(&tmp, text).map_err(|e| format!("{}: {e}", tmp.display()))?;
    if let Ok(m) = std::fs::metadata(&p) {
        let _ = std::fs::set_permissions(&tmp, m.permissions());
    }
    std::fs::rename(&tmp, &p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(FileText { path: p.display().to_string(), text: String::new() })
}

#[cfg(test)]
mod audio_kind_tests {
    #[test]
    fn a_recording_is_named_by_its_bytes() {
        let mut mp4 = vec![0, 0, 0, 0x1c];
        mp4.extend_from_slice(b"ftypiso5");
        assert_eq!(super::audio_kind(&mp4), Some("mp4"));
        assert_eq!(super::audio_kind(&[0x1A, 0x45, 0xDF, 0xA3, 1]), Some("webm"));
        assert_eq!(super::audio_kind(b"OggS\0\0"), Some("ogg"));
        assert_eq!(super::audio_kind(b"RIFF\0\0\0\0WAVEfmt "), Some("wav"));
        assert_eq!(super::audio_kind(b"hello"), None);
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn the_stack_is_found_wherever_it_was_set_up() {
        let t = std::env::temp_dir().join(format!("cove-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let (mount, home) = (t.join("opt"), t.join("home"));
        let find = |picked: &str, env: Option<std::path::PathBuf>| super::find_stack_root(picked, &mount, &home, env, "x86_64-pc-linux-gnu");
        let put = |d: &std::path::Path| {
            std::fs::create_dir_all(d).unwrap();
            std::fs::write(d.join("use.sh"), "").unwrap();
        };
        assert_eq!(find("", None), mount, "nothing here: the mount, for the messages");
        let picked = t.join("airgap");
        assert_eq!(find(picked.to_str().unwrap(), None), picked, "nothing here: the picked folder");
        put(&home.join(".svbt").join("x86_64-pc-linux-gnu"));
        assert_eq!(find(picked.to_str().unwrap(), None), home.join(".svbt"), "the CLI's default, under its triple");
        put(&home.join(".ndexr/stack"));
        assert_eq!(find("", None), home.join(".ndexr/stack"), "Cove's own build before the CLI's");
        put(&mount);
        assert_eq!(find("", None), mount, "the mount first");
        put(&picked);
        assert_eq!(find(picked.to_str().unwrap(), None), picked, "a picked stack wins");
        let _ = std::fs::remove_dir_all(&t);
    }
    /// The init line says how a run is paid for: "none" is the person's own
    /// plan, a key name the API (#736); the result line carries the cost.
    #[test]
    fn agent_events_reads_cost_and_who_pays() {
        let mut c = crate::api::AgentChunk::default();
        super::agent_events(r#"{"type":"system","subtype":"init","apiKeySource":"none","model":"claude-opus-5-5"}"#, &mut c);
        super::agent_events(r#"{"type":"result","total_cost_usd":1.25,"is_error":false}"#, &mut c);
        assert!(c.subscription);
        assert_eq!(c.cost, Some(1.25));
        let mut k = crate::api::AgentChunk::default();
        super::agent_events(r#"{"type":"system","subtype":"init","apiKeySource":"ANTHROPIC_API_KEY"}"#, &mut k);
        assert!(!k.subscription);
    }

    /// Live Development from the real tables: run on the host with
    /// `cargo test development_status_live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn development_status_live() {
        let v: serde_json::Value = serde_json::from_slice(&super::development_status().unwrap()).unwrap();
        println!("{}", serde_json::to_string_pretty(&v).unwrap());
        assert!(v["changes"].is_array() && v["pipeline"]["release"].is_string());
        // Nothing private leaves: no requester, no paths, no diff, no agent text.
        let s = v.to_string();
        for k in ["requester", "\"diff\"", "agent_summary", "\"path\"", "@gmail.com"] {
            assert!(!s.contains(k), "{k} leaked");
        }
    }

    /// The Logs tab reads .cove/run/app.log (else .run/app.log), from a whole
    /// line, its colour codes kept for ansi.rs and its cursor codes and
    /// carriage returns dropped; no log is an empty path.
    #[test]
    fn app_log_tails_the_run_log_keeping_colour() {
        let d = std::env::temp_dir().join(format!("cove-applog-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".cove/run")).unwrap();
        let p = d.to_str().unwrap();
        assert_eq!(super::app_log(p, false).unwrap().path, "");
        std::fs::write(d.join(".cove/run/build.log"), "build: ok\n").unwrap();
        assert_eq!(super::app_log(p, true).unwrap().text, "build: ok\n");
        std::fs::write(d.join(".cove/run/app.log"), "\u{1b}[32mlistening\u{1b}[0m on 8080\u{1b}[2K\r\nGET / 200\n").unwrap();
        let t = super::app_log(p, false).unwrap();
        assert!(t.path.ends_with(".cove/run/app.log"));
        assert_eq!(t.text, "\u{1b}[32mlistening\u{1b}[0m on 8080\nGET / 200\n");
        let big = "x".repeat(100) + "\n";
        std::fs::write(d.join(".cove/run/app.log"), big.repeat(1000) + "last line\n").unwrap();
        let t = super::app_log(p, false).unwrap();
        assert!(t.text.len() <= 64 * 1024 && t.text.ends_with("last line\n") && t.text.starts_with('x'));
        let _ = std::fs::remove_dir_all(&d);
    }

    // The web server's log line for a site, as a person reads it: no IP, no
    // browser string; other sites and Cove's /__live polling left out.
    #[test]
    fn site_request_lines() {
        let l = |h: &str, req: &str, st: &str| format!(r#"97.1.2.3 - {h} "{req} HTTP/2.0" {st} 2048 upstream=127.0.0.1:8916 ustatus={st} urespt=0.012 rt=0.013 ua="Mozilla/5.0""#);
        let got = super::request_line(&l("ndexr.io", "GET /static/app.js", "200"), "ndexr.io", "www.ndexr.io").unwrap();
        assert!(got.contains("GET") && got.contains("/static/app.js") && got.contains("200") && got.contains("2.0 KB") && got.contains("12 ms"), "{got}");
        assert!(!got.contains("97.1.2.3") && !got.contains("Mozilla"), "visitor details leaked: {got}");
        assert!(super::request_line(&l("www.ndexr.io", "GET /", "502"), "ndexr.io", "www.ndexr.io").unwrap().starts_with('!'));
        assert!(super::request_line(&l("other.ndexr.io", "GET /", "200"), "ndexr.io", "www.ndexr.io").is_none());
        assert!(super::request_line(&l("ndexr.io", "GET /__live", "200"), "ndexr.io", "www.ndexr.io").is_none());
    }

    // Live: the real web server log on this host (needs root or `adm`).
    #[test]
    #[ignore]
    fn site_requests_live() {
        let f = super::site_requests("ndexr.io").expect("read the access log");
        eprintln!("{}\n{}", f.path, f.text.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n"));
        assert!(!f.text.is_empty(), "ndexr.io has had requests");
    }

    /// App reloads for the project's own files, never for .cove/ (stage
    /// there), and only for .cove/reload once a project has one.
    #[test]
    fn site_stamp_ignores_cove_and_honours_the_reload_file() {
        let d = std::env::temp_dir().join(format!("cove-stamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".cove/stage")).unwrap();
        let p = d.to_str().unwrap();
        std::fs::write(d.join("index.html"), "one").unwrap();
        let a = super::site_stamp(p).unwrap();
        std::fs::write(d.join(".cove/stage/index.html"), "draft").unwrap();
        std::fs::write(d.join(".cove/run.log"), "noise").unwrap();
        assert_eq!(super::site_stamp(p).unwrap(), a, "a write under .cove/ reloaded App");
        std::fs::write(d.join("index.html"), "two!").unwrap();
        let b = super::site_stamp(p).unwrap();
        assert_ne!(b, a, "a change to the site did not reload App");
        std::fs::write(d.join(".cove/reload"), "").unwrap();
        let c = super::site_stamp(p).unwrap();
        std::fs::write(d.join("index.html"), "three").unwrap();
        assert_eq!(super::site_stamp(p).unwrap(), c, "with .cove/reload, a save still reloaded App");
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(d.join(".cove/reload"), "go").unwrap();
        assert_ne!(super::site_stamp(p).unwrap(), c, "touching .cove/reload did not reload App");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A cell's path to the owner's folder is the platform's folder (#185).
    /// Needs the platform checkout, so it says nothing on a machine without one.
    #[test]
    fn a_cells_paths_are_the_platforms() {
        let owner = "drennanfreddy";
        let ours = std::path::Path::new(super::PLATFORM_DOMAINS).join(owner);
        if !ours.is_dir() {
            return;
        }
        let got = super::platform_paths(owner, "/root/src/domains/drennanfreddy/drennanfreddy.ndexr.io/static");
        assert_eq!(got, format!("{}/drennanfreddy.ndexr.io/static", ours.display()));
        assert_eq!(super::platform_paths(owner, "/home/drennanfreddy/domains/x.ndexr.io/a"), format!("{}/x.ndexr.io/a", ours.display()));
        // Someone else's cell path is not this owner's folder.
        assert_eq!(super::platform_paths(owner, "/root/src/domains/adam/a.ndexr.io"), "/root/src/domains/adam/a.ndexr.io");
    }

    #[test]
    fn a_runs_progress_names_its_set_and_masks_its_tail() {
        let log = "[ndexr up] step 9/9 packages — 2026-10-24T21:53:56Z\n  · packages: 9 sets, one at a time\n  · packages R/4.6.1-bioc3.23: pull, build what is new, push (12 CPUs / 20G)\n  · packages R/4.6.1-bioc3.23: rc 0\n  · packages R/4.6.1-bioc3.24: pull, build what is new, push (12 CPUs / 20G)\n* now installing pak (R) - 3 of 268\nexport AWS_SECRET_ACCESS_KEY=abcdEFGHijklMNOPqrstUVWXyz0123456789abcd\n";
        let p = super::run_progress(log);
        assert_eq!((p.set.as_str(), p.set_n, p.sets), ("R/4.6.1-bioc3.24", 2, 9));
        assert_eq!(p.tail.len(), 6);
        assert!(!p.tail.join("\n").contains("abcdEFGHijklMNOP"), "{:?}", p.tail);
        // A new run starts the count again.
        let p = super::run_progress(&format!("{log}[ndexr up] step 1/9 kit — t\n"));
        assert_eq!((p.set.as_str(), p.set_n, p.sets), ("", 0, 0));
    }
    #[test]
    fn a_rust_app_is_named_for_its_domain_with_nothing_left_to_fill() {
        let files = super::rust_app_files("my-app.ndexr.io");
        assert!(files.iter().all(|(_, b)| !b.contains("__HOST__") && !b.contains("__CRATE__")));
        let cargo = &files.iter().find(|(n, _)| *n == "Cargo.toml").unwrap().1;
        assert!(cargo.contains("name = \"my-app-ndexr-io\""), "{cargo}");
        let ctx = &files.iter().find(|(n, _)| *n == "CONTEXT.md").unwrap().1;
        assert!(ctx.starts_with("# my-app.ndexr.io · "));
        let n = super::rust_app_files("3d.ndexr.io");
        assert!(n.iter().any(|(_, b)| b.contains("name = \"site-3d-ndexr-io\"")));
    }

    #[test]
    fn a_box_is_on_or_off_by_one_answer() {
        let d = std::env::temp_dir().join(format!("cove-power-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".watch")).unwrap();
        // The run file says running; AWS says stopped: off.
        std::fs::write(d.join("intel.json"), r#"{"state":"running","step":"packages"}"#).unwrap();
        std::fs::write(d.join("intel.log"), "* now installing x (R) - 1 of 2\n").unwrap();
        std::fs::write(d.join(".watch/four-box-intel.server.json"), r#"{"state":"stopped","reachable":false}"#).unwrap();
        assert_eq!(super::box_power(&d, "four-box", "intel"), ("off".into(), "off".into()));
        // AWS says running: on, and the run's own state.
        std::fs::write(d.join(".watch/four-box-intel.server.json"), r#"{"state":"running","reachable":true}"#).unwrap();
        assert_eq!(super::box_power(&d, "four-box", "intel"), ("on".into(), "running".into()));
        // No watcher: a fresh log is on but unconfirmed; the mirror's "is stopped" note is off.
        std::fs::write(d.join("gpu.json"), r#"{"state":"starting"}"#).unwrap();
        std::fs::write(d.join("gpu.log"), "==> installed ndexr\n").unwrap();
        assert_eq!(super::box_power(&d, "four-box", "gpu"), ("unconfirmed".into(), "starting".into()));
        std::fs::write(d.join("gpu.log"), "Error: 'four-box-gpu' is stopped with no public IP - nothing to read\n").unwrap();
        assert_eq!(super::box_power(&d, "four-box", "gpu").0, "off");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_site_stamp_follows_edits_and_ignores_git() {
        let dir = std::env::temp_dir().join(format!("cove-sitestamp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("module.r"), "a").unwrap();
        let p = dir.display().to_string();
        let first = site_stamp(&p).unwrap();
        // newest:count:size:served - no live vhost for a temp folder, so 0.
        assert!(first.ends_with(":1:1:0"), "{first}");
        std::fs::write(dir.join(".git").join("index.r"), "ignored").unwrap();
        assert_eq!(site_stamp(&p).unwrap(), first);
        std::fs::write(dir.join("module.r"), "ab").unwrap();
        assert_ne!(site_stamp(&p).unwrap(), first);
        assert_eq!(site_stamp(&dir.join("gone").display().to_string()).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::absent;

    #[test]
    fn only_a_new_untouched_signup_page_may_be_replaced() {
        let dir = std::env::temp_dir().join(format!("cove-signup-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let host = "ada.ndexr.io";
        assert!(!super::new_signup_page(&dir, host), "no module.r: nothing to replace");
        // writeLines adds a newline to a text that already ends in one.
        let laid = format!("{}\n", super::SHINY_CHAOS_MODULE.replace("__HOST__", host));
        std::fs::write(dir.join("module.r"), &laid).unwrap();
        assert!(super::new_signup_page(&dir, host), "as signup laid it");
        assert!(!super::new_signup_page(&dir, "other.ndexr.io"), "another site's page");
        std::fs::write(dir.join("module.r"), format!("{laid}# mine\n")).unwrap();
        assert!(!super::new_signup_page(&dir, host), "edited: it is theirs");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_port_starter_gets_a_port_of_its_own_and_the_verbs() {
        let mut laid: Vec<(String, String)> = super::starter_files("python-ml").into_iter().map(|(n, b)| (n.to_owned(), b.to_owned())).collect();
        super::app_contract(&mut laid, "r", "module", "/health").unwrap();
        let get = |n: &str| laid.iter().find(|(p, _)| p == n).map(|(_, b)| b.clone());
        let env = get("app.env").expect("app.env");
        assert!(env.contains("APP_LANGUAGE=python") && env.contains("APP_KIND=port") && env.contains("APP_HEALTH=/health"), "{env}");
        let port: u16 = env.lines().find_map(|l| l.strip_prefix("APP_PORT=")).unwrap().parse().expect("a numeric port");
        assert!((8830..=8999).contains(&port));
        for v in ["build.sh", "up.sh", "down.sh", "env.sh", "run.sh"] {
            assert!(get(v).is_some(), "python-ml lacks {v}");
        }
        // Unreal's signalling server listens on three: each its own, all in the range.
        let mut ue: Vec<(String, String)> = super::starter_files("unreal-pixelstream").into_iter().map(|(n, b)| (n.to_owned(), b.to_owned())).collect();
        super::app_contract(&mut ue, "r", "module", "/health").unwrap();
        let env = ue.iter().find(|(p, _)| p == "app.env").unwrap().1.clone();
        let num = |k: &str| env.lines().find_map(|l| l.strip_prefix(k)).and_then(|v| v.parse::<u16>().ok()).unwrap_or_else(|| panic!("{k} {env}"));
        let ports = [num("APP_PORT="), num("APP_STREAMER_PORT="), num("APP_SFU_PORT=")];
        assert!(ports.iter().all(|p| (8830..=8999).contains(p)), "{env}");
        assert!(ports[0] != ports[1] && ports[1] != ports[2] && ports[0] != ports[2], "{env}");
        assert!(env.contains("APP_LANGUAGE=unreal") && !env.contains("__"), "{env}");
        // A Shiny module: app.env says module, no port; build.sh only.
        let mut shiny: Vec<(String, String)> = super::starter_files("shiny-trial").into_iter().map(|(n, b)| (n.to_owned(), b.to_owned())).collect();
        super::app_contract(&mut shiny, "r", "module", "").unwrap();
        let env = shiny.iter().find(|(p, _)| p == "app.env").unwrap().1.clone();
        assert!(env.contains("APP_KIND=module") && !env.contains("APP_PORT"), "{env}");
        assert!(shiny.iter().any(|(p, _)| p == "build.sh") && !shiny.iter().any(|(p, _)| p == "up.sh"));
    }

    #[test]
    fn files_leaves_out_what_gitignore_names() {
        let d = std::env::temp_dir().join(format!("cove-ign-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for sub in ["target", "dist", "src", "logs"] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
        }
        std::fs::write(d.join(".gitignore"), "target/\n/dist\n*.log\nsrc/gen/\n# logs\n").unwrap();
        let names: Vec<String> = super::list_dir(&d.display().to_string()).unwrap().entries.into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["logs", "src"]);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_thread_is_busy_while_a_new_run_has_no_done() {
        let d = std::env::temp_dir().join(format!("cove-busy-{}", std::process::id()));
        let (runs, thread) = (d.join("runs"), d.join("thread"));
        std::fs::create_dir_all(&runs).unwrap();
        std::fs::create_dir_all(&thread).unwrap();
        let id = |n: u32| format!("20261114T0{n}0000Z-aaaaaa{n}");
        for n in 1..=3 {
            std::fs::write(thread.join(format!("{}.prompt", id(n))), "x").unwrap();
            std::fs::write(runs.join(format!("{}.stream", id(n))), "").unwrap();
        }
        // 1 finished; 2 and 3 going (no .pid yet: still starting, never "gone").
        std::fs::write(runs.join(format!("{}.done", id(1))), "").unwrap();
        assert_eq!(super::busy_in(&runs, &thread), 2);
        std::fs::write(runs.join(format!("{}.done", id(3))), "").unwrap();
        assert_eq!(super::busy_in(&runs, &thread), 1);
        assert_eq!(super::busy_in(&runs, &d.join("no-thread")), 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_viewer_line_is_the_owners_choice() {
        let env = "APP_LANGUAGE=r\nAPP_KIND=port\n";
        assert!(!super::viewer_line(env), "not said: not listed");
        let on = super::set_viewer_line(env, true);
        assert!(super::viewer_line(&on) && on.starts_with(env) && on.contains("# Listed on the public viewer"));
        let off = super::set_viewer_line(&on, false);
        assert!(!super::viewer_line(&off) && off.matches("APP_VIEWER=").count() == 1, "rewritten in place, not added twice: {off}");
        assert!(super::viewer_line("APP_VIEWER=\"yes\"\n"));
        assert!(super::project_folder("../etc").is_none());
    }

    #[test]
    fn every_project_turn_says_change_the_app_in_place() {
        let port = super::brief_text("du.ndexr.io", "APP_LANGUAGE=rust\nAPP_KIND=port\nAPP_PORT=8941\nAPP_HEALTH=/health\n");
        for want in ["du.ndexr.io's app is the one in this folder", "in place", "replaces what is there", "127.0.0.1:8941", ".cove/build.sh, then .cove/down.sh and .cove/up.sh", "never send going live to another agent", "go in supporting_docs/, never git"] {
            assert!(port.contains(want), "missing {want:?} in {port}");
        }
        let module = super::brief_text("x.ndexr.io", "APP_LANGUAGE=r\nAPP_KIND=module\n");
        assert!(module.contains("a saved change is live") && !module.contains("down.sh"), "{module}");
        assert!(super::project_brief("nobody-here", "no-such-project.ndexr.io").is_none());
    }

    #[test]
    fn a_new_project_is_laid_out_with_cove() {
        for id in ["python-ml", "shiny-trial", "node-site", crate::project::starters::RUST_APP] {
            let laid = super::starter_layout("x.ndexr.io", id).unwrap();
            let has = |n: &str| laid.iter().any(|(p, _)| p == n);
            assert!(has(".cove/app.env") && has(".cove/build.sh") && has(".cove/up.sh") && has(".cove/r/build.sh") && has(".cove/rust/run.sh"), "{id}");
            for root in ["app.env", "build.sh", "up.sh", "down.sh", "run.sh", "env.sh", "build-web.sh"] {
                assert!(!has(root), "{id}: {root} still at the root");
            }
        }
    }

    #[test]
    fn a_version_comes_back_and_history_only_grows() {
        let d = std::env::temp_dir().join(format!("cove-versions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let git = |args: &str| assert!(std::process::Command::new("bash").args(["-c", &format!("cd {} && git -c user.name=t -c user.email=t@t {args}", d.display())]).status().unwrap().success(), "{args}");
        std::fs::write(d.join("a.txt"), "one\n").unwrap();
        git("init -q && git add -A && git -c user.name=t -c user.email=t@t commit -q -m first");
        std::fs::write(d.join("a.txt"), "two\n").unwrap();
        std::fs::write(d.join("b.txt"), "new\n").unwrap();
        git("add -A && git -c user.name=t -c user.email=t@t commit -q -m second");
        // Unsaved work, kept as a version before the restore.
        std::fs::write(d.join("a.txt"), "three\n").unwrap();
        let v = super::versions_at(&d).unwrap();
        assert_eq!(v.iter().map(|x| x.message.as_str()).collect::<Vec<_>>(), ["second", "first"]);
        let first = v[1].sha.clone();
        let said = super::version_restore_at(&d, &first, "tester").unwrap();
        assert!(said.starts_with("restored the version of"), "{said}");
        assert_eq!(std::fs::read_to_string(d.join("a.txt")).unwrap(), "one\n");
        assert!(!d.join("b.txt").exists(), "a file the version did not have is gone");
        let after = super::versions_at(&d).unwrap();
        assert_eq!(after.len(), 4, "saved-before + restored on top of the two");
        assert!(after[0].message.starts_with("Restored the version of") && after[1].message.starts_with("Saved before restoring"));
        assert_eq!(after[0].author, "tester");
        assert!(super::version_restore_at(&d, "zzz", "t").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_home_path_is_the_platform_path_for_the_platform_account() {
        // Only meaningful where the platform folder exists (this host).
        let ours = std::path::Path::new(super::PLATFORM_DOMAINS).join("fdrennan");
        if !ours.is_dir() {
            return;
        }
        let want = format!("{}/fdrennan.ndexr.io", ours.display());
        assert_eq!(super::platform_paths("fdrennan", "/home/fdrennan/ndexr/src/domains/fdrennan/fdrennan.ndexr.io"), want);
        assert_eq!(super::platform_paths("fdrennan", "/home/fdrennan/domains/fdrennan.ndexr.io"), want);
        assert_eq!(super::platform_paths("fdrennan", "/elsewhere/x"), "/elsewhere/x");
        assert_eq!(super::platform_paths("../x", "/home/../x/domains/a"), "/home/../x/domains/a");
    }

    #[test]
        fn every_folder_starter_has_its_files() {
        // A starter in the chooser with no folder would fail at create time
        // ("not a starter Cove knows"); one with no way to run, at first use.
        for id in crate::project::starters::FOLDER_STARTERS {
            let files = super::starter_files(id);
            let has = |n: &str| files.iter().any(|(p, _)| *p == n);
            assert!(has("CONTEXT.md"), "{id}: no CONTEXT.md - it would not read back as a domain");
            assert!(has("module.r") || has("run.sh"), "{id}: neither module.r nor run.sh");
            // The file it actually runs: julia-fractals shipped with run.sh and
            // no server.jl, ready in the chooser, and passed the line above
            // (2026-10-25, referral from tester.ndexr.io).
            let runs = match id.split('-').next().unwrap_or("") {
                "julia" => has("server.jl") && has("static/index.html"),
                "python" => has("app.py") || has("index.qmd"),
                "r" => has("run.sh") && has("env.sh") && ["app.R", "plumber.R", "model.R", "report.Rmd", "index.Rmd", "index.qmd", "recipe.sh"].iter().any(|f| has(f)),
                "ide" => has("run.sh") && has("env.sh"),
                "rust" => has("Cargo.toml") && has("src/main.rs"),
                "node" => has("server.js") && has("static/index.html"),
                "unreal" => has("CoveGame.uproject") && has("Source/CoveGame/CoveGameMode.cpp") && has("Source/CoveGame/CoveGame.Build.cs"),
                _ => has("module.r"),
            };
            // An IDE starter is rebuilt from its own recipe: both scripts, at
            // the project's root where a person runs them.
            if has("recipe.sh") || id.starts_with("ide-") {
                assert!(has("recipe.sh") && has("rebuild.sh"), "{id}: an IDE starter needs recipe.sh and rebuild.sh");
            }
            assert!(runs, "{id}: the program run.sh starts is missing");
            if id.starts_with("shiny-") {
                let m = files.iter().find(|(p, _)| *p == "module.r").unwrap().1;
                assert!(m.contains("ui_module") && m.contains("server_module") && m.contains("__HOST__"), "{id}: module contract");
            }
        }
        assert_eq!(super::crate_name("2fast.ndexr.io"), "site-2fast-ndexr-io");
    }

    #[test]
    fn converting_moves_the_old_app_aside_whole() {
        let d = std::env::temp_dir().join(format!("cove-convert-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        for p in ["module.r", "r/app.r", ".git/HEAD", "supporting_docs/a.png", ".cove/app.env", ".cove/r/run.sh", ".cove/run/app.log", ".cove/was/old-1/x"] {
            std::fs::create_dir_all(d.join(p).parent().unwrap()).unwrap();
            std::fs::write(d.join(p), p).unwrap();
        }
        let was = super::move_aside(&d, ".cove/was/shiny-2").unwrap();
        let w = d.join(&was);
        for moved in ["module.r", "r/app.r", ".cove/app.env", ".cove/r/run.sh"] {
            assert!(w.join(moved).is_file(), "{moved} not kept in {was}");
            assert!(!d.join(moved).exists(), "{moved} still at the root");
        }
        for stays in [".git/HEAD", "supporting_docs/a.png", ".cove/run/app.log", ".cove/was/old-1/x"] {
            assert!(d.join(stays).is_file(), "{stays} moved");
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Every new project logs from its first run (operator, 2026-11-18: "each
    /// needs to, at a minimum, a console print or sorts at app start"): its
    /// language's helper is laid, and the starter either uses it or, where a
    /// third-party program is the app (code-server, rserver, quarto),
    /// run.sh prints the start line in the same format.
    
    #[test]
    fn every_starter_logs_from_its_first_run() {
        for (shared, helper) in [("_r", "r/log.r"), ("_python", "log.py"), ("_node", "log.js"), ("_julia", "log.jl"), ("_rust", "src/log.rs")] {
            assert!(super::starter_files(shared).iter().any(|(p, _)| *p == helper), "{shared} lays no {helper}");
        }
        for id in crate::project::starters::FOLDER_STARTERS {
            let files = super::starter_files(id);
            let logs = files.iter().any(|(p, b)| {
                let ext = p.rsplit('.').next().unwrap_or("");
                match ext {
                    "r" | "R" | "Rmd" | "qmd" => b.contains("r/log") || b.contains("r / log"),
                    "py" => b.contains("from log import"),
                    "js" => b.contains("require('./log')") || b.contains("require(\"./log\")"),
                    "jl" => b.contains("log.jl"),
                    "rs" => b.contains("mod log;"),
                    "sh" => b.contains(" INFO    ["),
                    _ => false,
                }
            });
            assert!(logs, "{id}: nothing in it logs a start line (templates/starters/_<language>/log.*)");
        }
    }

    /// The language sets a starter's env.sh loads: env/r4.6_py3.13-release
    /// joins these two (manifests/combos.txt), env/Julia/1.11.7 the third.
    const NEEDS_SETS: &[(&str, &str)] = &[("python", "Python/3.13.15"), ("r", "R/4.6.1-bioc3.23"), ("julia", "Julia/1.11.7")];

    /// Needs that come into the sets only as another record's dependency, not
    /// as records of their own: present today, but nothing keeps them there.
    /// Each waits on the referral named; when it lands, this test says to
    /// drop the line.
    // box: in env/r4.6_py3.13-release (1.2.3) but not a record of the R set
    // yet - the Shiny-with-box starter needs it (referral #796).
    const NEEDS_NOT_YET_RECORDS: &[(&str, &str, &str)] = &[("r", "box", "referral #796")];

    fn manifest_records(set: &str) -> Option<Vec<(String, bool)>> {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../ndexr.ndexr.io/crate/assets/stack-kit/manifest/manifests");
        let text = std::fs::read_to_string(dir.join(set).join("manifest.dcf")).ok()?;
        let mut out: Vec<(String, bool)> = Vec::new();
        for l in text.lines() {
            if let Some(p) = l.strip_prefix("Package: ") {
                out.push((p.trim().to_owned(), false));
            } else if l.trim() == "SiteLibrary: yes" {
                if let Some(last) = out.last_mut() {
                    last.1 = true;
                }
            }
        }
        Some(out)
    }

    /// PyPI names compare without case, `_`, `-` or `.` (PEP 503).
    fn same_package(kind: &str, a: &str, b: &str) -> bool {
        let norm = |s: &str| if kind == "python" { s.to_lowercase().replace(['_', '.'], "-") } else { s.to_owned() };
        norm(a) == norm(b)
    }

    fn starter_needs() -> Vec<(&'static str, &'static str, &'static str)> {
        let mut out = Vec::new();
        for id in crate::project::starters::FOLDER_STARTERS {
            if let Some((_, env)) = super::starter_files(id).into_iter().find(|(p, _)| *p == "app.env") {
                for (k, n) in crate::project::starters::needs(env) {
                    out.push((*id, k, n));
                }
            }
        }
        out
    }

    #[test]
    fn every_starter_need_is_built_by_the_stack() {
        // A ready starter names what it needs, and each is a record of the
        // language set its env.sh loads (built on the n-boxes, published by
        // ndexr.ndexr.io) or a module in the CVMFS stack (EasyBuild). Skipped
        // where ndexr.ndexr.io's manifests or the stack are not on disk (a
        // desktop build).
        for id in crate::project::starters::FOLDER_STARTERS {
            let files = super::starter_files(id);
            if let Some((_, env)) = files.iter().find(|(p, _)| *p == "app.env") {
                assert!(env.contains("APP_NEEDS="), "{id}: app.env has no APP_NEEDS");
            }
        }
        let modules = std::path::Path::new("/opt/ndexr/stack/x86_64-pc-linux-gnu/eb/modules/all");
        let mut wrong = Vec::new();
        for (id, kind, name) in starter_needs() {
            if kind == "module" {
                if modules.is_dir() && !modules.join(name).is_dir() {
                    wrong.push(format!("{id}: module {name} is not in the CVMFS stack"));
                }
                continue;
            }
            let Some(set) = NEEDS_SETS.iter().find(|(k, _)| *k == kind).map(|x| x.1) else {
                wrong.push(format!("{id}: {kind}:{name} - no language set for {kind}"));
                continue;
            };
            let Some(records) = manifest_records(set) else { continue };
            let known = records.iter().any(|(p, _)| same_package(kind, p, name));
            let pending = NEEDS_NOT_YET_RECORDS.iter().find(|(k, n, _)| *k == kind && *n == name);
            match (known, pending) {
                (false, None) => wrong.push(format!("{id}: {kind}:{name} is not a record of {set} - ask ndexr.ndexr.io to add it (ndexr refer)")),
                (true, Some((_, _, r))) => wrong.push(format!("{kind}:{name} is a record of {set} now ({r} landed): drop it from NEEDS_NOT_YET_RECORDS")),
                _ => {}
            }
        }
        assert!(wrong.is_empty(), "{}", wrong.join("\n"));
    }

    /// What a box without this host's full site library is missing: needs
    /// whose records are not marked `SiteLibrary: yes` are built and kept on
    /// S3 but not synced to every box (ndexr pkgs, 2026-10-31). The evidence
    /// for ndexr.ndexr.io, #756 (`cargo test needs_on_every_box -- --ignored`).
    #[test]
    #[ignore]
    fn needs_on_every_box() {
        let mut off = std::collections::BTreeMap::<String, Vec<&str>>::new();
        for (id, kind, name) in starter_needs() {
            let Some(set) = NEEDS_SETS.iter().find(|(k, _)| *k == kind).map(|x| x.1) else { continue };
            let Some(records) = manifest_records(set) else { continue };
            if !records.iter().any(|(p, synced)| *synced && same_package(kind, p, name)) {
                off.entry(format!("{set} {name}")).or_default().push(id);
            }
        }
        let report: Vec<String> = off.iter().map(|(k, ids)| format!("{k}: {}", ids.join(", "))).collect();
        assert!(off.is_empty(), "not on every box:\n{}", report.join("\n"));
    }

    #[test]
    fn the_shiny_starter_is_chaos_lab_named_for_its_host() {
        let m = super::SHINY_CHAOS_MODULE.replace("__HOST__", "trial.ndexr.io");
        assert!(!m.contains("__HOST__") && !m.contains("__DOMAIN__"), "a placeholder survived");
        for want in ["ui_module <- function(id, request)", "server_module <- local(", "measured(\"trial.ndexr.io\"", "rk4 <- function", "Chaos Lab"] {
            assert!(m.contains(want), "missing {want}");
        }
    }

    /// As root - what `cove multiuser` is - platform R runs as the platform's
    /// own account, never root, and answers for the person named: their Claude
    /// in the site's words, and the agent's own refusal for a domain that is
    /// not theirs (no run is started). Run on the platform host with
    /// `sudo <test binary> --ignored --exact host::tests::platform_r_as_root`.
    #[test]
    fn strip_fences_keeps_the_code() {
        assert_eq!(super::strip_fences("```r\nplot(1)\n```"), "plot(1)");
        assert_eq!(super::strip_fences("plot(1)"), "plot(1)");
    }

    /// What is behind every platform domain, tallied
    /// (`cargo test app_kinds_live -- --ignored`).
    #[test]
    #[ignore]
    fn app_kinds_live() {
        let k = super::app_kinds();
        let mut t = std::collections::BTreeMap::new();
        let mut fw = std::collections::BTreeMap::new();
        for v in k.values() {
            *t.entry(v.kind).or_insert(0) += 1;
            *fw.entry(v.framework).or_insert(0) += 1;
        }
        eprintln!("{t:?}  frameworks: {fw:?}  rust: {:?}", k.iter().filter(|x| x.1.kind == "rust" || x.1.kind == "process").map(|x| x.0).collect::<Vec<_>>());
        assert!(t.get("shiny").copied().unwrap_or(0) > 50, "{t:?}");
    }

    /// Against the live registry (`cargo test visible_domains_live -- --ignored`):
    /// what each kind of viewer is shown, by level.
    #[test]
    #[ignore]
    fn visible_domains_live() {
        let count = |v: Option<&str>| -> std::collections::BTreeMap<String, usize> {
            let b = super::visible_domains(v).expect("visible");
            let rows: Vec<crate::api::DomainInfo> = serde_json::from_slice(&b).expect("json");
            let mut m = std::collections::BTreeMap::new();
            for r in rows {
                *m.entry(r.target).or_insert(0) += 1;
            }
            m
        };
        for v in [None, Some("drennanfreddy"), Some("someone"), Some("no-such-user")] {
            eprintln!("{v:?}: {:?}", count(v));
        }
        let owners = |v: Option<&str>| -> std::collections::BTreeMap<String, usize> {
            let rows: Vec<crate::api::DomainInfo> = serde_json::from_slice(&super::visible_domains(v).unwrap()).unwrap();
            let mut m = std::collections::BTreeMap::new();
            for r in rows {
                *m.entry(r.owner).or_insert(0) += 1;
            }
            m
        };
        eprintln!("owners seen by drennanfreddy: {:?}", owners(Some("drennanfreddy")));
        assert!(owners(None).keys().all(|k| k.is_empty()), "no names for anyone not signed in");
        assert!(owners(Some("drennanfreddy")).keys().any(|k| !k.is_empty()), "names for a signed-in viewer");
        let anon = count(None);
        assert!(anon.keys().all(|k| k == "public"), "anonymous sees only public: {anon:?}");
        assert_eq!(count(Some("no-such-user")), anon, "an unknown name is not signed in");
        assert!(!count(Some("someone")).contains_key("admin"), "a non-admin sees no admin sites");
    }

    /// Publish one real domain (`COVE_TEST_PUBLISH=<host> COVE_TEST_OWNER=<user>
    /// cargo test publish_live -- --ignored`): provisions it for real.
    #[test]
    #[ignore]
    fn publish_live() {
        let host = std::env::var("COVE_TEST_PUBLISH").expect("COVE_TEST_PUBLISH");
        let owner = std::env::var("COVE_TEST_OWNER").expect("COVE_TEST_OWNER");
        eprintln!("{:?}", super::publish_as(&owner, &host));
        assert!(super::publish_as(&owner, "x.example.com").is_err(), "only ndexr.io names");
    }

    /// The real call (`cargo test assist_live -- --ignored`): needs the
    /// platform's OPENAI_API_KEY readable, as the platform account or root.
    #[test]
    #[ignore]
    fn assist_live() {
        let r = super::assist(crate::api::Lang::R, "ggplot of mtcars mpg against wt", "language: R\nobjects: none listed").expect("assist");
        eprintln!("{} wrote:\n{}", r.model, r.code);
        assert!(r.code.contains("ggplot") && !r.code.contains("```"), "{}", r.code);
        let p = super::assist(crate::api::Lang::Python, "import os and get the working directory", "").expect("assist py");
        eprintln!("{}", p.code);
        assert!(p.code.contains("import os") && p.code.contains("os.getcwd"), "{}", p.code);
    }

    #[test]
    #[ignore]
    fn platform_r_as_root() {
        use super::{agent_run_as, claude_as, is_root, platform_account, platform_r_as};
        assert!(is_root(), "run this one as root");
        let owner = std::env::var("COVE_TEST_OWNER").unwrap_or_else(|_| "123456789012abcd1".into());
        let v = claude_as(&owner, "status", "").expect("status");
        assert!(!v.kind.is_empty() && !v.title.is_empty(), "{v:?}");
        let who = platform_r_as(&owner, "whoami.R", "cat(jsonlite::toJSON(list(user = Sys.info()[['effective_user']]), auto_unbox = TRUE))", &[], 30).expect("whoami");
        assert_eq!(who["user"].as_str(), platform_account().map(|a| a.1).as_deref());
        let e = agent_run_as(&owner, "cove.ndexr.io", "hello", "sub-sonnet", None, None).expect_err("not their domain");
        assert!(e.contains("not one of your domains"), "{e}");
        let st = std::fs::metadata("/run/cove/platform").unwrap();
        assert_eq!(std::os::unix::fs::MetadataExt::mode(&st) & 0o777, 0o700);
    }

    #[test]
    fn build_logs_follow_by_offset_whole_lines_and_masked() {
        let d = std::env::temp_dir().join(format!("cove-buildlogs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("intel.log"), "one\ntwo AKIAABCDEFGHIJKLMNOP\nthr").unwrap();
        std::fs::write(d.join("intel.json"), r#"{"state":"running","step":"packages"}"#).unwrap();
        std::env::set_var("COVE_BUILD_LOGS_DIR", &d);
        let read = |key: Option<&str>, from: Option<u64>| -> serde_json::Value { serde_json::from_slice(&super::build_log(key, from).unwrap()).unwrap() };
        let list = read(None, None);
        assert_eq!(list["logs"][0]["key"], "intel");
        assert_eq!(list["logs"][0]["state"], "running");
        assert!(list.get("text").is_none(), "no key, no text");
        // The tail: whole lines only, the half-written one waits.
        let a = read(Some("intel"), None);
        assert_eq!(a["text"], "one\ntwo AKIA****************\n");
        let next = a["next"].as_u64().unwrap();
        std::fs::write(d.join("intel.log"), "one\ntwo AKIAABCDEFGHIJKLMNOP\nthree\n").unwrap();
        let b = read(Some("intel"), Some(next));
        assert_eq!(b["text"], "three\n");
        assert_eq!(b["reset"], false);
        // Started again (shorter than where we were): the tail, flagged.
        std::fs::write(d.join("intel.log"), "new\n").unwrap();
        assert_eq!(read(Some("intel"), Some(next))["reset"], true);
        // Only a listed log is opened.
        assert!(read(Some("../intel"), None).get("text").is_none());
        assert_eq!(super::mask_secrets("-DHAVE_CURLOPT_PASSWORD=1 GITLAB_TOKEN=glpat-abcdefghijklmnop x\n"), "-DHAVE_CURLOPT_PASSWORD=1 GITLAB_TOKEN=**** x\n");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn no_modules_says_why_not_mid_publish_by_default() {
        let d = std::env::temp_dir().join(format!("cove-stack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mine = "x86_64-pc-linux-gnu";
        assert!(absent(&d, mine, false).contains("your cell"), "a Mac goes to the cell");
        assert!(absent(&d, mine, true).contains("no ndexr stack at"), "nothing there: no stack");
        std::fs::create_dir_all(d.join(mine)).unwrap();
        assert!(absent(&d, mine, true).contains("a publish is replacing them"), "platform folder, no modules: mid-publish");
        assert!(absent(&d, mine, false).contains("your cell"), "a Mac is never mid-publish");
        std::fs::remove_dir_all(&d).unwrap();
    }


    #[test]
    fn a_log_says_which_package_and_how_far() {
        assert_eq!(parse_installing("* now installing duckdb (R) - 182 of 321"), Some(("duckdb".into(), "R".into(), 182, 321)));
        assert_eq!(parse_installing("* now installing grpcio-status (Python) - 8 of 320"), Some(("grpcio-status".into(), "Python".into(), 8, 320)));
        assert_eq!(parse_installing("  [ok] R        dashboardthemes"), None);
    }

    #[test]
    fn supporting_docs_is_made_inside_any_existing_folder() {
        let root = std::env::temp_dir().join(format!("cove-sd-{}", std::process::id()));
        let dom = root.join("site.ndexr.io");
        std::fs::create_dir_all(&dom).unwrap();
        std::fs::write(dom.join("module.r"), "").unwrap();
        let a = upload(&dom.join("supporting_docs").display().to_string(), "brief.pdf", &b"%PDF"[..]).unwrap();
        assert!(a.ends_with("/site.ndexr.io/supporting_docs/brief.pdf"), "{a}");
        // A domain with none of module.r/nginx.conf/CONTEXT.md still gets one
        // (operator, 2026-10-15: "when there isnt a folder it needs one").
        let bare = root.join("new.ndexr.io");
        std::fs::create_dir_all(&bare).unwrap();
        let b = upload(&bare.join("supporting_docs").display().to_string(), "notes.md", &b"#"[..]).unwrap();
        assert!(b.ends_with("/new.ndexr.io/supporting_docs/notes.md"), "{b}");
        // A parent that is not there: nothing is made.
        assert!(upload(&root.join("plain/supporting_docs").display().to_string(), "x.txt", &b"x"[..]).is_err());
        assert!(upload(&dom.join("other").display().to_string(), "x.txt", &b"x"[..]).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn uploads_never_clobber_and_take_only_names() {
        let d = std::env::temp_dir().join(format!("cove-up-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let dir = d.display().to_string();
        let a = upload(&dir, "data.csv", &b"1,2"[..]).unwrap();
        let b = upload(&dir, "data.csv", &b"3,4"[..]).unwrap();
        assert!(a.ends_with("/data.csv") && b.ends_with("/data (1).csv"), "{a} {b}");
        assert_eq!(std::fs::read(&a).unwrap(), b"1,2", "the first stays as it was");
        for bad in ["../x", "a/b", "", "..", "._junk", ".DS_Store"] {
            assert!(upload(&dir, bad, &b""[..]).is_err(), "{bad:?}");
        }
        let (name, bytes) = download(&a, "").unwrap();
        assert_eq!((name.as_str(), bytes.as_slice()), ("data.csv", &b"1,2"[..]));
        let (name, gz) = download(&dir, "").unwrap();
        assert!(name.ends_with(".tar.gz") && gz.starts_with(&[0x1f, 0x8b]), "{name}");
        // The choice before a download: either packing, of a file or a folder.
        let (name, gz) = download(&a, "tar.gz").unwrap();
        assert!(name == "data.csv.tar.gz" && gz.starts_with(&[0x1f, 0x8b]), "{name}");
        let (name, z) = download(&dir, "zip").unwrap();
        assert!(name.ends_with(".zip") && z.starts_with(b"PK\x03\x04"), "{name}");
        let (name, z) = download(&a, "zip").unwrap();
        assert!(name == "data.csv.zip" && z.starts_with(b"PK\x03\x04"), "{name}");
        std::fs::remove_dir_all(&d).unwrap();
    }

    use super::*;

    #[test]
    fn a_domain_project_lists_from_its_domains_folder() {
        let root = std::env::temp_dir().join(format!("cove-dr-{}", std::process::id()));
        let d = root.join("domains/owner/site.ndexr.io");
        std::fs::create_dir_all(d.join("sub")).unwrap();
        std::fs::write(d.join("module.r"), "").unwrap();
        assert_eq!(domains_root(&d), root.join("domains"));
        assert_eq!(domains_root(&d.join("sub")), root.join("domains"));
        assert_eq!(domains_root(&root), root);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn only_env_sets_with_a_library_are_offered_and_the_rest_layer() {
        let lib = std::env::temp_dir().join(format!("cove-lib-{}", std::process::id()));
        for d in ["r4.6_py3.13-release/R", "R/4.6.1-bioc3.23/R", "Python/3.13.15/python", "Rust/1.96.0/rust/vendor", "stray"] {
            std::fs::create_dir_all(lib.join(d)).unwrap();
        }
        let mut got = built_sets(&lib).unwrap();
        got.sort();
        assert_eq!(got, ["Python/3.13.15", "R/4.6.1-bioc3.23", "Rust/1.96.0", "r4.6_py3.13-release"]);
        let _ = std::fs::remove_dir_all(&lib);
        assert!(built_sets(&lib).is_none());
        let listing = "/x/modules/all:\nGDAL/3.9.1\nApptainer/1.5.4\nGDAL/3.10.0\nenv/R/4.6.1-bioc3.23\nR/4.6.1-foss-2024a\nPython/3.13.15\nGDAL/\n";
        assert_eq!(parse_other(listing), ["Apptainer/1.5.4", "GDAL/3.10.0", "GDAL/3.9.1"]);
    }

    #[test]
    fn modules_sort_newest_first_and_skip_headers() {
        let listing = "/opt/ndexr/stack/x/eb/modules/all:\nApptainer/1.5.4\nenv/r4.6_py3.13-release\nenv/r4.10_py3.9-release\nR/4.6.1-foss-2024a\nR/4.10.0-foss-2024a\nPython/3.9.25\nPython/3.13.15\nR/4.6.1-foss-2024a\n";
        let (env, r, py) = parse_modules(listing);
        assert_eq!(env, ["env/r4.10_py3.9-release", "env/r4.6_py3.13-release"]);
        assert_eq!(r, ["R/4.10.0-foss-2024a", "R/4.6.1-foss-2024a"]);
        assert_eq!(py, ["Python/3.13.15", "Python/3.9.25"]);
    }

    #[test]
    fn a_render_names_its_output_from_quarto() {
        let d = std::env::temp_dir().join(format!("cove-render-{}", std::process::id()));
        std::fs::create_dir_all(d.join("_site")).unwrap();
        let f = d.join("report.qmd");
        std::fs::write(d.join("_site/report.html"), "x").unwrap();
        let said = vec!["Output created: _site/report.html".to_owned()];
        assert_eq!(render_output(&said, &d, &f), Some(d.join("_site/report.html")));
        // Nothing said, nothing beside it: no page to show.
        assert_eq!(render_output(&[], &d, &f), None);
        assert!(rendered_page(&d.join("_site/report.html").display().to_string()).is_none(), "only pages a render registered");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_stamp_moves_when_the_file_does() {
        let d = std::env::temp_dir().join(format!("cove-stamp-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.R");
        std::fs::write(&f, "x <- 1\n").unwrap();
        let p = f.display().to_string();
        let first = stamp(&p);
        assert!(!first.is_empty());
        std::fs::write(&f, "x <- 22\n").unwrap();
        assert_ne!(stamp(&p), first, "size and time both moved");
        assert_eq!(stamp(&d.join("gone.R").display().to_string()), "");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_run_whose_process_is_gone_is_over() {
        let d = std::env::temp_dir().join(format!("cove-rungone-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let old = |f: &std::path::Path| {
            let t = std::time::SystemTime::now() - Duration::from_secs(60);
            std::fs::File::options().write(true).open(f).unwrap().set_modified(t).unwrap();
        };
        // No pid file: not judged.
        assert!(!run_gone(&d, "a"));
        // A pid no process has.
        std::fs::write(d.join("b.pid"), "4194300\n").unwrap();
        assert!(!run_gone(&d, "b"), "too new to judge");
        old(&d.join("b.pid"));
        assert!(run_gone(&d, "b"));
        // A live process that is not a run (this test): a reused pid.
        std::fs::write(d.join("c.pid"), format!("{}\n", std::process::id())).unwrap();
        old(&d.join("c.pid"));
        assert!(run_gone(&d, "c"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn domains_are_folders_named_for_their_host() {
        let d = std::env::temp_dir().join(format!("cove-domains-{}", std::process::id()));
        let site = d.join("someone/blog.ndexr.io");
        std::fs::create_dir_all(&site).unwrap();
        std::fs::write(site.join("module.r"), "").unwrap();
        std::fs::write(site.join("CONTEXT.md"), "# Blog · blog.ndexr.io\n").unwrap();
        std::fs::create_dir_all(d.join("someone/notes")).unwrap();
        let got = domain_at(&site).unwrap();
        assert_eq!((got.host.as_str(), got.title.as_str(), got.module), ("blog.ndexr.io", "Blog", true));
        assert!(domain_at(&d.join("someone/notes")).is_none());
        assert!(!is_host_name("_customer_template") && !is_host_name("x y.io"));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn domain_target_reads_marker_then_root_location() {
        let gen = "server {\n  listen 80;\n  return 301 https://$host$request_uri;\n}\nserver {\n  listen 443 ssl;\n  include conf.d/admin_app.conf;\n  location / {\n    auth_request /__site_password_check;\n    proxy_pass http://ndexr_app/;\n  }\n}\n";
        assert_eq!(domain_target(gen), "module");
        assert_eq!(domain_target(&gen.replace("http://ndexr_app/", "http://127.0.0.1:7720")), "port:7720");
        assert_eq!(domain_target(&format!("# ndexr-target: port:8001   (rewritten)\n{gen}")), "port:8001");
        assert_eq!(domain_target("server {\n  include conf.d/cell.conf;\n}\n"), "cell");
        assert_eq!(domain_target("server {\n  location / { return 301 https://cove.ndexr.io$request_uri; }\n}\n"), "redirect");
        assert_eq!(domain_target(&gen.replace("http://ndexr_app/", "http://3.15.187.107:80/")), "remote:3.15.187.107:80");
        assert_eq!(domain_target(""), "");
    }

    #[test]
    fn a_flat_stub_never_hides_the_owner_domain() {
        let info = |path: &str, module| crate::api::DomainInfo { host: "repo.ndexr.io".into(), path: path.into(), title: String::new(), module, env: String::new(), target: String::new(), app: String::new(), owner: String::new(), ..Default::default() };
        let got = one_per_host(vec![info("domains/repo.ndexr.io", false), info("domains/me/repo.ndexr.io", true)]);
        assert_eq!((got.len(), got[0].path.as_str()), (1, "domains/me/repo.ndexr.io"));
        let got = one_per_host(vec![info("domains/repo.ndexr.io", true), info("domains/me/repo.ndexr.io", true)]);
        assert_eq!(got[0].path, "domains/repo.ndexr.io", "a flat module.r wins, as in the router");
    }

    #[test]
    fn paths_must_be_absolute() {
        assert!(resolve("relative/file.R").is_err());
        assert_eq!(resolve("~/x.R").unwrap(), PathBuf::from(home()).join("x.R"));
        assert_eq!(resolve("/tmp").unwrap(), PathBuf::from("/tmp"));
    }

    #[test]
    fn files_round_trip() {
        let d = std::env::temp_dir().join(format!("cove-test-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let f = d.join("a.R").display().to_string();
        write_file(&f, "x <- 1\n").unwrap();
        assert_eq!(read_file(&f).unwrap().text, "x <- 1\n");
        let l = list_dir(&d.display().to_string()).unwrap();
        assert_eq!(l.entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["a.R"]);
        std::fs::remove_dir_all(&d).unwrap();
    }
}

#[cfg(test)]
mod mask_secrets_multibyte {

    #[test]
    fn the_binary_plan_reads_and_covers_every_accepted_wheel() {
        let kit = std::path::Path::new(super::PLATFORM_DOMAINS).join("ndexr.ndexr.io/crate/assets/stack-kit/manifest");
        let Ok(text) = std::fs::read_to_string(kit.join("binary-plan.tsv")) else { return };
        let plan = super::binary_plan(&text);
        let fate = |l: &str, n: &str| plan.iter().find(|p| p.0 == l && p.1.is_match(n)).map(|p| p.2.as_str());
        assert_eq!(fate("Python", "soundfile"), Some("source"));
        assert_eq!(fate("Python", "nvidia-cublas"), Some("remove"));
        assert_eq!(fate("Julia", "libpng-jll"), Some("source"));
        assert_eq!(fate("Prefix", "dev-lang/go-bootstrap"), Some("remove"));
        assert_eq!(fate("Python", "numpy"), None);
        // Every binary wheel the kit accepts has a fate: the two lists stay in
        // step. A pure-Python wheel (databricks-connect) is not machine code.
        let ex = std::fs::read_to_string(kit.join("wheel-exceptions.tsv")).unwrap_or_default();
        for pat in ex.lines().filter(|l| !l.starts_with('#') && !l.starts_with("pattern\t") && !l.contains("pure-Python")).filter_map(|l| l.split('\t').next()) {
            assert!(plan.iter().any(|p| p.0 == "Python" && p.1.as_str() == pat.trim()), "{pat} is accepted but has no line in binary-plan.tsv");
        }
        assert!(super::binary_plan("Python\t^x$\tlater\twhy").is_empty(), "an unknown fate is refused");
        assert!(super::binary_plan("Python\t^x$\tkeep\twhy").is_empty(), "a binaryless deployment keeps nothing");
    }

    #[test]
    fn a_curly_quote_before_a_secret_name_does_not_panic() {
        let out = super::mask_secrets("set ‘token=abcdefghijklmnopqrst’ now\n");
        assert!(out.contains("token=****"), "{out}");
        assert_eq!(super::mask_secrets("é=x ünïcode password=short\n"), "é=x ünïcode password=short\n");
    }

    /// Packages' Easyconfig section (operator, 2026-10-19): a language set's
    /// module name holds a slash (env/R/<v> is EasyBuild's env-R), and its
    /// recipe may be only in the ndexr CLI's kit; a name cannot climb out.
    #[test]
    fn easyconfigs_by_module_name() {
        if !std::path::Path::new(super::PLATFORM_DOMAINS).join("ndexr.ndexr.io/crate/assets/stack-kit").is_dir() {
            return;
        }
        let r = super::easyconfig("env/R", "4.6.1-bioc3.24").expect("env/R from the kit");
        // The installed recipe once the stack has built it, else the kit's.
        assert!(r.path.ends_with("/env-R-4.6.1-bioc3.24.eb") && r.text.contains("modaltsoftname = 'env/R'"), "{}", r.path);
        assert!(super::easyconfig("env/Python", "3.13.15").is_ok());
        assert!(super::easyconfig("../etc", "passwd").is_err());
        assert!(super::easyconfig("env", "../../x").is_err());
    }

    /// The site libraries read as where each package's machine code came
    /// from; on this machine torch's wheel is a manylinux one, from PyPI.
    #[test]
    fn provenance_catches_downloaded_binaries() {
        assert_eq!(crate::api::pkg_key("polars_runtime_32"), "polars-runtime-32");
        let p = super::provenance();
        if p.is_empty() {
            return;
        }
        let torch: Vec<_> = p.iter().filter(|r| r.language == "Python" && r.name == "torch").collect();
        assert!(torch.iter().any(|r| r.how == "external" && r.evidence.contains("manylinux")), "{torch:?}");
        assert!(p.iter().any(|r| r.how == "built") && p.iter().any(|r| r.how == "pure"));
        let ext = p.iter().filter(|r| r.how == "external").count();
        // torch is on the kit's list, with its reason; an unlisted download
        // keeps an empty one.
        assert!(torch.iter().any(|r| r.reason.contains("source")), "{torch:?}");
        println!("unaccounted: {:?}", p.iter().filter(|r| r.how == "external" && r.reason.is_empty()).map(|r| r.name.as_str()).collect::<Vec<_>>());
        println!("{} rows, {ext} external: {:?}", p.len(), p.iter().filter(|r| r.how == "external").map(|r| r.name.as_str()).collect::<Vec<_>>());
    }

    /// The serving path on this machine (`cargo test serving_live -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn serving_live() {
        let s = super::serving(&["ndexr.io".into(), "koodle.io".into()], &["module".into(), "port:8796".into(), "cell".into()]);
        println!("{}", serde_json::to_string_pretty(&s).unwrap());
        assert!(!s.kernel.is_empty() && s.cpus > 0 && s.mem_total_kb > 0);
    }

    /// An env set's easyconfig read as the modules it loads (atlas,
    /// 2026-10-21: EasyBuild → environments → packages).
    #[test]
    fn env_uses_reads_the_dependencies() {
        let ec = "name = 'env-R'\ndependencies = [\n    ('R', '4.6.1', '', ('foss', '2024a')),\n    ('Quarto', '1.9.38', '', SYSTEM),\n    ('foss', '2024a', '', SYSTEM),\n    ('x', '1', '-cuda', ('GCCcore', '13.3.0')),\n]\nmodextravars = {('a', 'b')}\n";
        let d: Vec<(String, String)> = super::easyconfig_deps(ec).into_iter().map(|u| (u.name, u.version)).collect();
        assert_eq!(d, [("R".into(), "4.6.1-foss-2024a".into()), ("Quarto".into(), "1.9.38".into()), ("foss".into(), "2024a".into()), ("x".into(), "1-cuda-GCCcore-13.3.0".into())]);
        assert!(super::easyconfig_deps("name = 'x'\n").is_empty());
        if std::path::Path::new(super::PLATFORM_DOMAINS).join("ndexr.ndexr.io/crate/assets/stack-kit").is_dir() {
            let u = super::env_uses(&["env/R/4.6.1-bioc3.24", "env/Nope/1"]);
            assert!(u[0].uses.iter().any(|x| x.name == "R" && x.version == "4.6.1-foss-2024a"), "{:?}", u[0]);
            assert!(u[1].uses.is_empty() && !u[1].error.is_empty());
        }
    }

    /// The kit's sets and combinations, and a manifest read as packages.
    #[test]
    fn env_sets_read_the_kit() {
        let p = super::manifest_packages("# head\n\nPackage: DataFrames\nLanguage: Julia\nVersion: 1.8.2\nGroup: Data frames and IO\n\nPackage: CSV\nVersion: 0.10.15\n");
        assert_eq!(p.iter().map(|p| (p.name.as_str(), p.version.as_str(), p.group.as_str())).collect::<Vec<_>>(), [("DataFrames", "1.8.2", "Data frames and IO"), ("CSV", "0.10.15", "")]);
        if std::path::Path::new(super::PLATFORM_DOMAINS).join("ndexr.ndexr.io/crate/assets/stack-kit/manifest/manifests/combos.txt").is_file() {
            let k = super::env_sets(&["Julia/1.11.7"]);
            let julia = k.sets.iter().find(|s| s.set == "Julia/1.11.7").expect("the Julia set");
            assert_eq!(julia.language, "Julia");
            assert_eq!(julia.packages.len() as u32, julia.records);
            assert!(k.sets.iter().filter(|s| s.set != "Julia/1.11.7").all(|s| s.packages.is_empty()));
            assert!(k.combos.iter().any(|c| c.name == "r4.6_py3.13-release" && c.sets == ["R/4.6.1-bioc3.23", "Python/3.13.15"]));
        }
    }

    /// Find in files: matching lines by path and line; git's store and
    /// binary files are not searched (operator, 2026-10-20).
    #[test]
    fn find_in_files_reads_the_work_not_the_store() {
        let d = crate::host::kernel::cache_dir().join("grep-test");
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".git")).unwrap();
        std::fs::create_dir_all(d.join("R")).unwrap();
        std::fs::write(d.join("module.r"), "x <- 1\nserver_app <- function() {}\n").unwrap();
        std::fs::write(d.join("R/core.r"), "# SERVER_APP twice\nserver_app()\n").unwrap();
        std::fs::write(d.join(".git/HEAD"), "server_app").unwrap();
        std::fs::write(d.join("blob.bin"), b"server_app\0\0").unwrap();
        let root = d.display().to_string();
        let v = super::grep(&root, "server_app", false, false).unwrap();
        let got: Vec<(String, usize)> = v.hits.iter().map(|h| (h.path.trim_start_matches(&format!("{root}/")).to_owned(), h.line)).collect();
        assert_eq!(got, vec![("R/core.r".to_owned(), 1), ("R/core.r".to_owned(), 2), ("module.r".to_owned(), 2)]);
        assert_eq!(super::grep(&root, "server_app", true, false).unwrap().hits.len(), 2, "case");
        assert_eq!(super::grep(&root, r"^server_\w+\(\)$", false, true).unwrap().hits.len(), 1, "regex");
        assert!(super::grep("relative", "x", false, false).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}
