//! The agent on this computer's own Claude (operator, 2026-11-15: "if a user
//! has claude locally and we can see it then use it, doesnt need to be forced
//! into using our services"). Signed out of ndexr, with `claude` installed and
//! signed in here, Cove's agent queries are answered on this computer:
//! `claude -p` in the project folder, its stream-json read by the same parser
//! the platform's runs use (host.rs agent_events), so the Agents tab does not
//! know the difference. Signed in, the platform answers as before.
//!
//! The files are the platform's layout, under ~/.local/state/cove/agent:
//!   runs/<id>.prompt .stream .err .done .pid   one turn
//!   threads/<domain>/<id>.prompt                which thread it belongs to
//! A turn resumes the claude session of the thread's last finished turn, so a
//! thread is one conversation, as on the platform.

/// The agent on OpenAI's Codex, for the gpt-* models (codex.rs).
pub mod codex;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::api::{AgentChunk, AgentEvent, AgentTurn, ClaudeView, Query};

fn root() -> PathBuf {
    PathBuf::from(crate::host::home()).join(".local/state/cove/agent")
}

fn runs() -> PathBuf {
    root().join("runs")
}

fn thread(domain: &str) -> PathBuf {
    root().join("threads").join(domain)
}

/// Where `claude` is: on the login shell's PATH, else where its installers
/// put it - the native installer's ~/.local/bin, the old local install, npm,
/// bun, volta, nvm, Homebrew. A person account Cove's server made often has
/// no ~/.profile, so ~/.local/bin was never on PATH: `claude` ran in the
/// Terminal (which adds it) while the agent said none was here (operator,
/// 2026-11-25, ndexrtesting.com: "should always work with whatever the
/// default claude installer is setup as").
pub fn claude_bin() -> Option<String> {
    if let Some(p) = crate::host::runner::capture_within("command -v claude", Duration::from_secs(5)).map(|p| p.trim().to_owned()).filter(|p| p.starts_with('/')) {
        return Some(p);
    }
    let home = PathBuf::from(crate::host::home());
    let mut places: Vec<PathBuf> = [".local/bin/claude", ".claude/local/claude", ".npm-global/bin/claude", ".bun/bin/claude", ".volta/bin/claude", ".local/share/pnpm/claude", "bin/claude"].iter().map(|r| home.join(r)).collect();
    if let Ok(dirs) = std::fs::read_dir(home.join(".nvm/versions/node")) {
        places.extend(dirs.flatten().map(|d| d.path().join("bin/claude")));
    }
    places.extend(["/usr/local/bin/claude", "/opt/homebrew/bin/claude", "/usr/bin/claude"].iter().map(PathBuf::from));
    places.into_iter().find(|p| is_program(p)).map(|p| p.display().to_string())
}

fn is_program(p: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        p.is_file()
    }
}

/// `claude` found (claude_bin) and signed in here: its own credentials file,
/// an account in ~/.claude.json (a Mac keeps the token in the keychain), or
/// a token or key in the environment. A yes is kept 30 s (the Agents tab
/// polls); a no is asked again after 3 s, so installing Claude or signing in
/// shows at once.
static SEEN: Mutex<Option<(Instant, bool)>> = Mutex::new(None);

/// Ask again on the next look, after anything that changes the answer: a
/// connect, a forget, Claude Code installed. A connect that had just saved a
/// good token answered "not ready" from a 3 s old "no" (operator, 2026-11-27,
/// ndexrtesting.com: "we successfully connected claude but it says continue
/// without?").
fn forget_seen() {
    *SEEN.lock().unwrap() = None;
}

pub fn usable() -> bool {
    let mut seen = SEEN.lock().unwrap();
    if let Some((at, ok)) = *seen {
        if at.elapsed() < Duration::from_secs(if ok { 30 } else { 3 }) {
            return ok;
        }
    }
    let home = PathBuf::from(crate::host::home());
    let signed_in = home.join(".claude/.credentials.json").is_file()
        || std::fs::read_to_string(home.join(".claude.json")).is_ok_and(|t| t.contains("\"oauthAccount\""))
        || !crate::host::account::find_local_claude().is_empty()
        || saved_token().is_some();
    let ok = signed_in && claude_bin().is_some();
    *seen = Some((Instant::now(), ok));
    ok
}

/// What stands between this server and an agent, in a sentence the person
/// can act on: no Claude here, or one that is not signed in.
pub fn why_not() -> String {
    let user = std::env::var("USER").unwrap_or_default();
    let who = if user.is_empty() { String::new() } else { format!(" for {user}") };
    match claude_bin() {
        None => format!("Claude Code is not installed{who} on this server - run its installer (curl -fsSL https://claude.ai/install.sh | bash) in the Terminal, then claude login. Or sign in to ndexr."),
        Some(p) => format!("{p} is here but not signed in{who} - run claude login (or claude setup-token) in the Terminal. Or sign in to ndexr."),
    }
}

/// The agent's queries, answered here when nobody is signed in to ndexr and
/// this computer's Claude is ready; None leaves them to the usual way.
pub fn answer(q: &Query) -> Option<Result<Value, String>> {
    if crate::host::account::find_key().is_some() {
        return None;
    }
    answer_here(q)
}

/// The same for `cove serve`, asked by a browser with no ndexr sign-in: an
/// ndexr key saved on the server does not matter there - without the
/// person's own sign-in the platform cannot be asked as them anyway, and a
/// key left by the installer sent every agent turn to a 401.
pub fn answer_here(q: &Query) -> Option<Result<Value, String>> {
    let mine = matches!(q, Query::AgentRun { .. } | Query::AgentPoll { .. } | Query::AgentStop { .. } | Query::AgentHistory { .. } | Query::AgentBusy { .. } | Query::ClaudeStatus);
    // Claude or Codex: either one ready here is enough for the agent.
    if !mine || !(usable() || codex::usable()) {
        return None;
    }
    let v = |r: Result<Value, String>| Some(r);
    match q {
        Query::AgentRun { model, .. } if codex::is_codex(model) && !codex::usable() => v(Err(codex::why_not())),
        Query::AgentRun { model, .. } if !codex::is_codex(model) && !usable() => v(Err(why_not())),
        Query::AgentRun { domain, prompt, model, cwd, mode, fresh } => v(run(domain, prompt, model, cwd.as_deref(), mode.as_deref(), *fresh).map(Value::from)),
        Query::AgentPoll { id, offset } => v(poll(id, *offset).and_then(|c| serde_json::to_value(c).map_err(|e| e.to_string()))),
        Query::AgentStop { id } => v(stop(id).map(Value::from)),
        Query::AgentHistory { domain } => v(history(domain).and_then(|h| serde_json::to_value(h).map_err(|e| e.to_string()))),
        Query::AgentBusy { domains } => v(serde_json::to_value(busy(domains)).map_err(|e| e.to_string())),
        Query::ClaudeStatus => v(serde_json::to_value(status()).map_err(|e| e.to_string())),
        _ => None,
    }
}

/// A server of the person's own, with no ndexr platform (`cove serve` or
/// `cove multiuser` on, say, ndexrtesting.com): Claude is this server's
/// business alone (operator, 2026-11-27: "claude should be accessible
/// immediately on install, like cove should be able to set up and install
/// it"; "we show errors on the landing page for a new cove system"). Its
/// status, connect and forget are answered here; Claude Code is installed
/// for the person the first time anyone asks; and until it can run, the
/// agent's thread and busy marks are empty rather than errors - only a turn
/// says what is missing.
pub fn answer_own(q: &Query) -> Option<Result<Value, String>> {
    let j = |r: Result<ClaudeView, String>| Some(r.and_then(|v| serde_json::to_value(v).map_err(|e| e.to_string())));
    match q {
        Query::ClaudeStatus => j(Ok(own_status())),
        Query::ClaudeConnect { token } => j(connect(token)),
        Query::ClaudeForget => j(forget().map(|_| own_status())),
        Query::AgentHistory { .. } if !usable() && !codex::usable() => Some(Ok(Value::Array(Vec::new()))),
        Query::AgentBusy { .. } if !usable() && !codex::usable() => Some(Ok(Value::Object(Default::default()))),
        // A GPT model runs on OpenAI's Codex (codex.rs), anything else on Claude.
        Query::AgentRun { model, .. } if codex::is_codex(model) && !codex::usable() => Some(Err(codex::why_not())),
        Query::AgentRun { model, .. } if !codex::is_codex(model) && !usable() => Some(Err(why_not())),
        // Messages are ndexr's: a Cove on its own has none, and says nothing
        // about them - not an error on the first page a new server shows
        // (operator, 2026-11-27: "this cannot be seen on first login, this
        // means setup has failed").
        Query::Inbox { .. } => Some(serde_json::to_value(crate::api::InboxView::default()).map_err(|e| e.to_string())),
        Query::ProjectNames { .. } => Some(serde_json::to_value(crate::api::ProjectNames::default()).map_err(|e| e.to_string())),
        Query::InboxAct { .. } | Query::SendRequest { .. } | Query::Spoken { .. } | Query::ReadAloud { .. } => Some(Err(ON_ITS_OWN.into())),
        _ => answer_here(q),
    }
}

/// What a Cove with no ndexr behind it says of what only ndexr's servers do.
pub const ON_ITS_OWN: &str = "This Cove runs on its own, without ndexr's servers: messages, requests and spoken replies go through those servers, so they are not here.";

/// Where a key or token Cove was given is kept: in the person's home, 0600,
/// one `NAME=value` line, handed to every `claude` the agent starts.
fn token_file() -> PathBuf {
    PathBuf::from(crate::host::home()).join(".config/ndexr/claude.env")
}

/// The variable Claude Code reads a token or key from, by its prefix.
fn token_var(t: &str) -> Option<&'static str> {
    if t.starts_with("sk-ant-oat") {
        Some("CLAUDE_CODE_OAUTH_TOKEN")
    } else if t.starts_with("sk-ant-api") {
        Some("ANTHROPIC_API_KEY")
    } else {
        None
    }
}

fn saved_token() -> Option<(&'static str, String)> {
    let text = std::fs::read_to_string(token_file()).ok()?;
    let v = text.lines().find_map(|l| l.split_once('=').map(|(_, v)| v.trim().to_owned()))?;
    token_var(&v).map(|n| (n, v))
}

/// Claude Code, installed for this person by Anthropic's own installer
/// (into ~/.local/bin, where claude_bin looks), once per process and in the
/// background: the setup step shows it going and then asks only for the
/// sign-in. Not in airgap mode, which asks nothing of the network.
static INSTALL: Mutex<Option<Result<(), String>>> = Mutex::new(None);
static INSTALLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn install_in_background() {
    if crate::host::saved_settings().airgap || INSTALL.lock().unwrap().is_some() || INSTALLING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(|| {
        let out = crate::host::runner::capture_within("curl -fsSL https://claude.ai/install.sh | bash 2>&1; echo \"exit=$?\"", Duration::from_secs(600)).unwrap_or_default();
        let r = if claude_bin().is_some() {
            Ok(())
        } else {
            let tail: Vec<&str> = out.lines().rev().take(3).collect();
            Err(format!("Claude Code's installer did not finish: {}", tail.into_iter().rev().collect::<Vec<_>>().join(" / ")))
        };
        *INSTALL.lock().unwrap() = Some(r);
        forget_seen();
        INSTALLING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

fn own_status() -> ClaudeView {
    if usable() {
        let mut v = status();
        if let Some((name, value)) = saved_token() {
            v.cred_kind = Some(if name == "ANTHROPIC_API_KEY" { "api_key" } else { "oauth_token" }.into());
            v.last4 = Some(value.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect());
            v.label = format!("Your Claude · {} on this server", if name == "ANTHROPIC_API_KEY" { "API key" } else { "subscription" });
        }
        return v;
    }
    let none = |title: &str, detail: String| ClaudeView { kind: "none".into(), title: title.into(), label: "Claude · not ready".into(), detail, cred_kind: Some("local".into()), ..Default::default() };
    if claude_bin().is_some() {
        return none("Claude Code is installed here - sign in to it", "Connect your Claude Pro or Max subscription, or paste an Anthropic API key. It stays on this server and is used only by your agent.".into());
    }
    install_in_background();
    if INSTALLING.load(std::sync::atomic::Ordering::SeqCst) {
        return none("Installing Claude Code on this server…", "Anthropic's installer is running for you; it takes under a minute. Sign-in comes next.".into());
    }
    match INSTALL.lock().unwrap().clone() {
        Some(Err(e)) => none("Claude Code could not be installed here", format!("{e}. Run curl -fsSL https://claude.ai/install.sh | bash in the Terminal to see why.")),
        _ => none("Claude Code is not installed here", why_not()),
    }
}

/// Keep a token or key - once a `claude` run with it has answered.
fn connect(token: &str) -> Result<ClaudeView, String> {
    let t = token.trim();
    let name = token_var(t).ok_or("that is neither an Anthropic API key (sk-ant-api…) nor a Claude token (sk-ant-oat…)")?;
    let claude = claude_bin().ok_or("Claude Code is not installed here yet - wait for its install, then connect")?;
    // The other variable emptied: a key in the login shell must not answer
    // for the token being checked.
    let other = if name == "ANTHROPIC_API_KEY" { "CLAUDE_CODE_OAUTH_TOKEN" } else { "ANTHROPIC_API_KEY" };
    let script = format!("unset {other}; {} -p --max-turns 1 'Reply with the single word OK.'", crate::api::shell_quote(&claude));
    let out = crate::host::runner::output_within(&script, &[(name, t)], Duration::from_secs(90)).ok_or("Claude did not answer within 90 s")?;
    // The answer itself, not a word inside it: "Failed to authenticate ...
    // OAuth access token is invalid" has "ok" in "token".
    if !said_ok(&out) {
        return Err(format!("Anthropic did not accept it: {}", out.lines().last().unwrap_or("no answer").trim()));
    }
    let f = token_file();
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&f, format!("{name}={t}\n")).map_err(|e| format!("{}: {e}", f.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600));
    }
    forget_seen();
    Ok(own_status())
}

fn said_ok(out: &str) -> bool {
    out.lines().map(str::trim).filter(|l| !l.is_empty()).last().is_some_and(|l| l.trim_end_matches(['.', '!']).eq_ignore_ascii_case("ok"))
}

fn forget() -> Result<(), String> {
    forget_seen();
    match std::fs::remove_file(token_file()) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

fn status() -> ClaudeView {
    ClaudeView {
        kind: "own".into(),
        title: "Claude on this computer".into(),
        label: "Your Claude · this computer".into(),
        detail: "Cove's agent runs the `claude` installed here, signed in as you. ndexr is not asked, and does not meter or limit it.".into(),
        ..Default::default()
    }
}

/// A run id: its start, sortable, and the platform's alphabet (valid_run_id).
fn new_id() -> String {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    format!("{}-{:09}-local", t.as_secs(), t.subsec_nanos())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() < 100 && !id.starts_with(['.', '-']) && id.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c))
}

/// The claude model for one of Cove's names: "sub-opus" → "opus".
fn model_name(m: &str) -> String {
    let m = m.trim().trim_start_matches("sub-");
    if m.is_empty() || !m.chars().all(|c| c.is_ascii_alphanumeric() || "-._:[]".contains(c)) { "opus".into() } else { m.to_owned() }
}

/// What a mode lets the turn do. Build edits and runs commands without asking
/// - the person's own machine, own Claude, own project folder, as the
/// platform's Build does; Plan and Ask read and change nothing.
fn permission(mode: Option<&str>) -> &'static str {
    match mode {
        Some("plan" | "ask") => "plan",
        // Edits files; a command needs a permission no one is there to give
        // in -p, so none runs.
        Some("supervised") => "acceptEdits",
        _ => "bypassPermissions",
    }
}

fn run(domain: &str, prompt: &str, model: &str, cwd: Option<&str>, mode: Option<&str>, fresh: bool) -> Result<String, String> {
    let domain = domain.trim().to_ascii_lowercase();
    if !crate::host::is_host_name(&domain) {
        return Err(format!("{domain}: not a domain name"));
    }
    if prompt.trim().is_empty() {
        return Err("say what you want done".into());
    }
    let dir = cwd.map(str::trim).filter(|c| Path::new(c).is_dir()).map(str::to_owned).unwrap_or_else(|| crate::host::project_dir(&crate::host::saved_settings()));
    let (runs, td) = (runs(), thread(&domain));
    std::fs::create_dir_all(&runs).and_then(|_| std::fs::create_dir_all(&td)).map_err(|e| e.to_string())?;
    let on_codex = codex::is_codex(model);
    // `fresh`: a saved thread brought back "as it was" starts a new session.
    let resume = if fresh { None } else { last_session(&td, &runs, on_codex) };
    let id = new_id();
    let f = |ext: &str| runs.join(format!("{id}.{ext}"));
    std::fs::write(f("prompt"), prompt).map_err(|e| e.to_string())?;
    std::fs::write(td.join(format!("{id}.prompt")), prompt).map_err(|e| e.to_string())?;
    let q = |p: PathBuf| crate::api::shell_quote(&p.display().to_string());
    // Which agent ran it, so its thread carries on in that agent's session.
    if on_codex {
        std::fs::write(f("agent"), "codex").map_err(|e| e.to_string())?;
    }
    let script = if on_codex {
        let bin = codex::bin().ok_or_else(codex::why_not)?;
        let (p, st, e, d) = (f("prompt"), f("stream"), f("err"), f("done"));
        codex::script(&bin, &codex::Turn { dir: &dir, model, mode, resume: resume.as_deref(), prompt: &p, stream: &st, err: &e, done: &d })
    } else {
    let claude = claude_bin().ok_or("no claude here - install Claude Code, then claude login")?;
    format!(
        "cd {dir} || exit 1; {claude} -p --output-format stream-json --verbose --model {model} --permission-mode {perm}{resume} < {prompt} > {stream} 2> {err}; echo $? > {done}",
        dir = crate::api::shell_quote(&dir),
        claude = crate::api::shell_quote(&claude),
        model = crate::api::shell_quote(&model_name(model)),
        perm = permission(mode),
        resume = resume.map(|s| format!(" --resume {}", crate::api::shell_quote(&s))).unwrap_or_default(),
        prompt = q(f("prompt")),
        stream = q(f("stream")),
        err = q(f("err")),
        done = q(f("done")),
    )
    };
    let mut cmd = Command::new(crate::host::runner::login_shell());
    crate::host::runner::forget_loaded_modules(&mut cmd);
    cmd.args(["-lc", &crate::host::runner::with_path(&script)]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    if let Some((name, value)) = saved_token() {
        cmd.env(name, value);
    }
    if let Some(k) = codex::saved_key().filter(|_| on_codex) {
        cmd.env("CODEX_API_KEY", k);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own group, so Stop ends claude and everything it started.
        cmd.process_group(0);
    }
    let child = cmd.spawn().map_err(|e| format!("could not start {}: {e}", if on_codex { "codex" } else { "claude" }))?;
    std::fs::write(f("pid"), child.id().to_string()).map_err(|e| e.to_string())?;
    // Reaped on a thread of its own: no zombie for the poll to mistake.
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(id)
}

/// The claude session of the thread's newest finished turn, to carry on.
fn last_session(td: &Path, runs: &Path, on_codex: bool) -> Option<String> {
    let mut ids = prompts(td);
    ids.reverse();
    ids.iter().filter(|id| runs.join(format!("{id}.done")).is_file() && ran_codex(runs, id) == on_codex).find_map(|id| {
        let text = std::fs::read_to_string(runs.join(format!("{id}.stream"))).ok()?;
        if on_codex {
            return codex::session_of(&text);
        }
        text.lines().rev().find_map(|l| serde_json::from_str::<Value>(l).ok()?["session_id"].as_str().map(str::to_owned))
    })
}

/// The turn ran on Codex (run wrote `<id>.agent`), not Claude.
fn ran_codex(runs: &Path, id: &str) -> bool {
    std::fs::read_to_string(runs.join(format!("{id}.agent"))).is_ok_and(|a| a.trim() == "codex")
}

/// A thread's turns, oldest first.
fn prompts(td: &Path) -> Vec<String> {
    let mut ids: Vec<String> = std::fs::read_dir(td)
        .map(|r| r.flatten().filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".prompt")).map(str::to_owned)).collect())
        .unwrap_or_default();
    ids.retain(|id| valid_id(id));
    ids.sort();
    ids
}

fn alive(pid: u32) -> bool {
    Command::new("kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

fn poll(id: &str, offset: u64) -> Result<AgentChunk, String> {
    use std::io::{Read, Seek, SeekFrom};
    if !valid_id(id) {
        return Err("bad run id".into());
    }
    let runs = runs();
    let mut chunk = AgentChunk { offset, ..Default::default() };
    if let Ok(mut f) = std::fs::File::open(runs.join(format!("{id}.stream"))) {
        let _ = f.seek(SeekFrom::Start(offset));
        let mut buf = Vec::new();
        let _ = f.take(4 << 20).read_to_end(&mut buf);
        // Only whole lines: a line still being written waits for the next poll.
        let upto = buf.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
        for line in String::from_utf8_lossy(&buf[..upto]).lines() {
            crate::host::agent_events(line, &mut chunk);
        }
        chunk.offset = offset + upto as u64;
    }
    let done = std::fs::read_to_string(runs.join(format!("{id}.done"))).ok();
    let pid = std::fs::read_to_string(runs.join(format!("{id}.pid"))).ok().and_then(|p| p.trim().parse::<u32>().ok());
    let gone = done.is_none() && pid.is_some_and(|p| !alive(p));
    chunk.done = done.is_some() || gone;
    // Said once, on the poll that finds it over: claude's own words when it
    // ended badly before writing a result (not signed in, a bad model).
    if chunk.done && offset == chunk.offset {
        let rc = done.as_deref().map(str::trim).unwrap_or("");
        let stream = std::fs::read_to_string(runs.join(format!("{id}.stream"))).unwrap_or_default();
        let on_codex = ran_codex(&runs, id);
        let said = stream.contains("\"type\":\"result\"") || (on_codex && codex::answered(&stream));
        if gone || (rc != "0" && !said) {
            let err = std::fs::read_to_string(runs.join(format!("{id}.err"))).unwrap_or_default();
            let tail: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).rev().take(4).collect();
            let why = if tail.is_empty() { format!("{} ended without an answer (exit {})", if on_codex { "codex" } else { "claude" }, if rc.is_empty() { "?" } else { rc }) } else { tail.into_iter().rev().collect::<Vec<_>>().join("\n") };
            chunk.events.push(AgentEvent { kind: "error".into(), text: why });
        }
    }
    Ok(chunk)
}

fn stop(id: &str) -> Result<bool, String> {
    if !valid_id(id) {
        return Err("bad run id".into());
    }
    let pid: u32 = std::fs::read_to_string(runs().join(format!("{id}.pid"))).ok().and_then(|p| p.trim().parse().ok()).ok_or("that run is not running")?;
    let _ = Command::new("kill").args(["-TERM", &format!("-{pid}")]).status();
    Ok(Command::new("kill").args(["-TERM", &pid.to_string()]).status().is_ok_and(|s| s.success()))
}

fn history(domain: &str) -> Result<Vec<AgentTurn>, String> {
    let domain = domain.trim().to_ascii_lowercase();
    if !crate::host::is_host_name(&domain) {
        return Err(format!("{domain}: not a domain name"));
    }
    let td = thread(&domain);
    let ids = prompts(&td);
    let from = ids.len().saturating_sub(12);
    Ok(ids[from..]
        .iter()
        .map(|id| {
            let prompt = std::fs::read_to_string(td.join(format!("{id}.prompt"))).unwrap_or_default();
            let chunk = poll(id, 0).unwrap_or_default();
            let at = id.split('-').next().and_then(|s| s.parse().ok()).unwrap_or(0);
            AgentTurn { id: id.clone(), prompt, events: chunk.events, done: chunk.done, by: String::new(), at }
        })
        .collect())
}

fn busy(domains: &[String]) -> std::collections::BTreeMap<String, u32> {
    let runs = runs();
    domains
        .iter()
        .filter(|d| crate::host::is_host_name(d))
        .filter_map(|d| {
            let n = prompts(&thread(d))
                .iter()
                .rev()
                .take(6)
                .filter(|id| !runs.join(format!("{id}.done")).exists() && std::fs::read_to_string(runs.join(format!("{id}.pid"))).ok().and_then(|p| p.trim().parse().ok()).is_some_and(alive))
                .count() as u32;
            (n > 0).then(|| (d.clone(), n))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn modes_and_models_map_to_claude() {
        assert_eq!(super::permission(Some("plan")), "plan");
        assert_eq!(super::permission(Some("ask")), "plan");
        assert_eq!(super::permission(None), "bypassPermissions");
        // Setup's Supervised: edits, no commands - never the full permission.
        assert_eq!(super::permission(Some("supervised")), "acceptEdits");
        assert_eq!(super::model_name("sub-opus"), "opus");
        assert_eq!(super::model_name("opus[1m]"), "opus[1m]");
        assert_eq!(super::model_name("x; rm -rf /"), "opus");
        let id = super::new_id();
        assert!(super::valid_id(&id), "{id}");
    }
}

#[cfg(test)]
mod own_server_tests {
    use super::*;

    /// A fresh "no" in the cache must not answer the look right after a
    /// connect: the connect clears it, so the next look reads the token.
    #[test]
    fn a_connect_is_not_answered_by_an_old_no() {
        *SEEN.lock().unwrap() = Some((Instant::now(), false));
        forget_seen();
        assert!(SEEN.lock().unwrap().is_none(), "the next usable() asks again");
    }

    /// A new server's first page asks for Messages and project names: a Cove
    /// on its own answers them empty, never with an error about a platform.
    #[test]
    fn a_cove_on_its_own_answers_the_first_page_quietly() {
        let inbox = answer_own(&Query::Inbox { all: false, chat: String::new(), find: String::new() }).expect("answered here").expect("not an error");
        assert_eq!(serde_json::from_value::<crate::api::InboxView>(inbox).unwrap(), crate::api::InboxView::default());
        let names = answer_own(&Query::ProjectNames { names: vec!["a.example.org".into()] }).expect("answered here").expect("not an error");
        assert_eq!(serde_json::from_value::<crate::api::ProjectNames>(names).unwrap(), crate::api::ProjectNames::default());
        let said = answer_own(&Query::ReadAloud { text: "hi".into() }).expect("answered here").unwrap_err();
        assert!(said.contains("without ndexr's servers") && !said.contains("src/r/host.r"), "{said}");
    }

    #[test]
    fn a_token_is_kept_in_the_variable_claude_reads() {
        assert_eq!(token_var("sk-ant-oat01-abc"), Some("CLAUDE_CODE_OAUTH_TOKEN"));
        assert_eq!(token_var("sk-ant-api03-abc"), Some("ANTHROPIC_API_KEY"));
        assert_eq!(token_var("hello"), None);
    }

    /// Measured 2026-11-27 with a made-up token: claude printed "Failed to
    /// authenticate. API Error: 401 OAuth access token is invalid." - which
    /// a "contains ok" check took as yes ("t-ok-en").
    #[test]
    fn only_an_answer_of_ok_is_a_yes() {
        assert!(said_ok("OK"));
        assert!(said_ok("ok.\n"));
        assert!(said_ok("some banner\nOK"));
        assert!(!said_ok("Failed to authenticate. API Error: 401 OAuth access token is invalid."));
        assert!(!said_ok(""));
        assert!(!said_ok("OK then, but no"));
    }
}
