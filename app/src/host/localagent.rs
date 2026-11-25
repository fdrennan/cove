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

/// `claude` on the login shell's PATH, and signed in here: its own
/// credentials file, an account in ~/.claude.json (a Mac keeps the token in
/// the keychain), or a token or key in the environment. Asked again every
/// 30 s at most: the Agents tab polls.
pub fn usable() -> bool {
    static SEEN: Mutex<Option<(Instant, bool)>> = Mutex::new(None);
    let mut seen = SEEN.lock().unwrap();
    if let Some((at, ok)) = *seen {
        if at.elapsed() < Duration::from_secs(30) {
            return ok;
        }
    }
    let home = PathBuf::from(crate::host::home());
    let signed_in = home.join(".claude/.credentials.json").is_file()
        || std::fs::read_to_string(home.join(".claude.json")).is_ok_and(|t| t.contains("\"oauthAccount\""))
        || !crate::host::account::find_local_claude().is_empty();
    let ok = signed_in && crate::host::runner::capture_within("command -v claude", Duration::from_secs(5)).is_some_and(|p| !p.trim().is_empty());
    *seen = Some((Instant::now(), ok));
    ok
}

/// The agent's queries, answered here when nobody is signed in to ndexr and
/// this computer's Claude is ready; None leaves them to the usual way.
pub fn answer(q: &Query) -> Option<Result<Value, String>> {
    let mine = matches!(q, Query::AgentRun { .. } | Query::AgentPoll { .. } | Query::AgentStop { .. } | Query::AgentHistory { .. } | Query::AgentBusy { .. } | Query::ClaudeStatus);
    if !mine || crate::host::account::find_key().is_some() || !usable() {
        return None;
    }
    let v = |r: Result<Value, String>| Some(r);
    match q {
        Query::AgentRun { domain, prompt, model, cwd, mode } => v(run(domain, prompt, model, cwd.as_deref(), mode.as_deref()).map(Value::from)),
        Query::AgentPoll { id, offset } => v(poll(id, *offset).and_then(|c| serde_json::to_value(c).map_err(|e| e.to_string()))),
        Query::AgentStop { id } => v(stop(id).map(Value::from)),
        Query::AgentHistory { domain } => v(history(domain).and_then(|h| serde_json::to_value(h).map_err(|e| e.to_string()))),
        Query::AgentBusy { domains } => v(serde_json::to_value(busy(domains)).map_err(|e| e.to_string())),
        Query::ClaudeStatus => v(serde_json::to_value(status()).map_err(|e| e.to_string())),
        _ => None,
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
        _ => "bypassPermissions",
    }
}

fn run(domain: &str, prompt: &str, model: &str, cwd: Option<&str>, mode: Option<&str>) -> Result<String, String> {
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
    let resume = last_session(&td, &runs);
    let id = new_id();
    let f = |ext: &str| runs.join(format!("{id}.{ext}"));
    std::fs::write(f("prompt"), prompt).map_err(|e| e.to_string())?;
    std::fs::write(td.join(format!("{id}.prompt")), prompt).map_err(|e| e.to_string())?;
    let q = |p: PathBuf| crate::api::shell_quote(&p.display().to_string());
    let script = format!(
        "cd {dir} || exit 1; claude -p --output-format stream-json --verbose --model {model} --permission-mode {perm}{resume} < {prompt} > {stream} 2> {err}; echo $? > {done}",
        dir = crate::api::shell_quote(&dir),
        model = crate::api::shell_quote(&model_name(model)),
        perm = permission(mode),
        resume = resume.map(|s| format!(" --resume {}", crate::api::shell_quote(&s))).unwrap_or_default(),
        prompt = q(f("prompt")),
        stream = q(f("stream")),
        err = q(f("err")),
        done = q(f("done")),
    );
    let mut cmd = Command::new(crate::host::runner::login_shell());
    crate::host::runner::forget_loaded_modules(&mut cmd);
    cmd.args(["-lc", &crate::host::runner::with_path(&script)]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own group, so Stop ends claude and everything it started.
        cmd.process_group(0);
    }
    let child = cmd.spawn().map_err(|e| format!("could not start claude: {e}"))?;
    std::fs::write(f("pid"), child.id().to_string()).map_err(|e| e.to_string())?;
    // Reaped on a thread of its own: no zombie for the poll to mistake.
    std::thread::spawn(move || {
        let mut child = child;
        let _ = child.wait();
    });
    Ok(id)
}

/// The claude session of the thread's newest finished turn, to carry on.
fn last_session(td: &Path, runs: &Path) -> Option<String> {
    let mut ids = prompts(td);
    ids.reverse();
    ids.iter().filter(|id| runs.join(format!("{id}.done")).is_file()).find_map(|id| {
        let text = std::fs::read_to_string(runs.join(format!("{id}.stream"))).ok()?;
        text.lines().rev().find_map(|l| serde_json::from_str::<Value>(l).ok()?["session_id"].as_str().map(str::to_owned))
    })
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
        let said = std::fs::read_to_string(runs.join(format!("{id}.stream"))).unwrap_or_default().contains("\"type\":\"result\"");
        if gone || (rc != "0" && !said) {
            let err = std::fs::read_to_string(runs.join(format!("{id}.err"))).unwrap_or_default();
            let tail: Vec<&str> = err.lines().filter(|l| !l.trim().is_empty()).rev().take(4).collect();
            let why = if tail.is_empty() { format!("claude ended without an answer (exit {})", if rc.is_empty() { "?" } else { rc }) } else { tail.into_iter().rev().collect::<Vec<_>>().join("\n") };
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
        assert_eq!(super::model_name("sub-opus"), "opus");
        assert_eq!(super::model_name("opus[1m]"), "opus[1m]");
        assert_eq!(super::model_name("x; rm -rf /"), "opus");
        let id = super::new_id();
        assert!(super::valid_id(&id), "{id}");
    }
}
