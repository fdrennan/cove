//! The agent on OpenAI, for the GPT models in the picker (operator,
//! 2026-11-27: "open AI is used and users need to be able to do that as
//! well"). On ndexr's platform a gpt-* turn goes through the platform's
//! LiteLLM proxy on ndexr's key (src/litellm/config.yaml); on a computer or
//! server of the person's own there is no proxy, so the turn runs OpenAI's
//! own coding agent, the Codex CLI (`codex exec --json`), on the person's own
//! OpenAI account - an API key they paste, or Codex's own ChatGPT sign-in.
//!
//! A Codex turn lives in the same files as a Claude one (localagent.rs:
//! runs/<id>.prompt .stream .err .done .pid, threads/<domain>/<id>.prompt):
//! its JSON Lines go into .stream, and host.rs agent_events reads them beside
//! Claude's - the two never share an event name - so the Agents tab, Stop and
//! the thread do not know which agent ran. A thread carries on in Codex's own
//! session (`codex exec resume <thread_id>`), as a Claude thread does in its.
//! Event shapes: codex-rs/exec/src/exec_events.rs.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// The models that run on Codex here.
pub fn is_codex(model: &str) -> bool {
    model.starts_with("gpt-")
}

/// Where `codex` is: on the login shell's PATH, else where its installers put
/// it (the standalone installer, npm's global prefixes).
pub fn bin() -> Option<String> {
    let home = PathBuf::from(crate::host::home());
    #[cfg(unix)]
    if let Some(p) = crate::host::runner::capture_within("command -v codex", Duration::from_secs(5)) {
        let p = p.trim();
        if !p.is_empty() && Path::new(p).is_file() {
            return Some(p.to_owned());
        }
    }
    [".local/bin/codex", ".codex/bin/codex", ".npm-global/bin/codex", ".bun/bin/codex"]
        .iter()
        .map(|r| home.join(r))
        .chain([PathBuf::from("/usr/local/bin/codex")])
        .find(|p| p.is_file())
        .map(|p| p.display().to_string())
}

/// Where an OpenAI API key pasted into Cove is kept for this account (0600),
/// handed to `codex` as CODEX_API_KEY on every turn.
fn key_file() -> PathBuf {
    PathBuf::from(crate::host::home()).join(".config/ndexr/openai-key")
}

pub fn saved_key() -> Option<String> {
    let k = std::fs::read_to_string(key_file()).ok()?.trim().to_owned();
    k.starts_with("sk-").then_some(k)
}

/// Codex can answer: installed, and signed in - a key kept here, a key in
/// the environment, or Codex's own sign-in (ChatGPT plan or key, in
/// ~/.codex/auth.json).
pub fn usable() -> bool {
    let home = PathBuf::from(crate::host::home());
    let signed_in = saved_key().is_some()
        || std::env::var("CODEX_API_KEY").is_ok_and(|k| !k.is_empty())
        || std::env::var("OPENAI_API_KEY").is_ok_and(|k| !k.is_empty())
        || home.join(".codex/auth.json").is_file();
    signed_in && bin().is_some()
}

/// Where this account's OpenAI stands (Query::OpenAiStatus).
pub fn view() -> crate::api::OpenAiView {
    let home = PathBuf::from(crate::host::home());
    let how = if let Some(k) = saved_key() {
        format!("API key …{}", &k[k.len().saturating_sub(4)..])
    } else if std::env::var("CODEX_API_KEY").is_ok_and(|k| !k.is_empty()) || std::env::var("OPENAI_API_KEY").is_ok_and(|k| !k.is_empty()) {
        "an API key in Cove's environment".into()
    } else if home.join(".codex/auth.json").is_file() {
        "Codex sign-in".into()
    } else {
        String::new()
    };
    let installed = bin().is_some();
    let ready = installed && !how.is_empty();
    crate::api::OpenAiView { installed, ready, how, detail: if ready { String::new() } else { why_not() } }
}

/// What stands between this computer and a GPT turn, in words to act on.
pub fn why_not() -> String {
    match bin() {
        None => "The GPT models run on OpenAI's Codex, which is not installed here - Help › Connect OpenAI… installs it, then takes your OpenAI API key.".into(),
        Some(_) => "Codex is installed but not signed in - Help › Connect OpenAI… takes your OpenAI API key, or sign in with your ChatGPT plan there.".into(),
    }
}

/// Keep an OpenAI API key for this account, once OpenAI has accepted it.
pub fn connect(key: &str) -> Result<(), String> {
    let k = key.trim();
    if !k.starts_with("sk-") || k.len() > 400 || !k.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)) {
        return Err("that is not an OpenAI API key (sk-…)".into());
    }
    let code = crate::host::runner::capture_within(
        &format!("curl -s -o /dev/null -w '%{{http_code}}' -m 20 -H {} https://api.openai.com/v1/models", crate::api::shell_quote(&format!("Authorization: Bearer {k}"))),
        Duration::from_secs(25),
    )
    .unwrap_or_default();
    match code.trim() {
        "200" => {}
        "401" | "403" => return Err("OpenAI did not accept that API key".into()),
        c => return Err(format!("could not reach OpenAI to check the key (HTTP {})", if c.is_empty() { "none" } else { c })),
    }
    let f = key_file();
    std::fs::create_dir_all(f.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&f, format!("{k}\n")).map_err(|e| format!("{}: {e}", f.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

pub fn forget() {
    let _ = std::fs::remove_file(key_file());
}

/// The Codex CLI from OpenAI's own installer, for this account. Five minutes
/// at most; the answer is where it landed, or the installer's last lines.
pub fn install() -> Result<String, String> {
    if let Some(p) = bin() {
        return Ok(format!("Codex is already installed: {p}"));
    }
    if crate::host::saved_settings().airgap {
        return Err("airgap mode: Cove does not download Codex".into());
    }
    let out = crate::host::runner::capture_within("curl -fsSL https://chatgpt.com/codex/install.sh | sh 2>&1; echo \"exit=$?\"", Duration::from_secs(300)).ok_or("Codex's installer took longer than five minutes")?;
    match bin() {
        Some(p) => Ok(format!("Installed: {p}")),
        None => {
            let tail: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
            Err(format!("Codex's installer ran but no codex is here yet: {}", tail[tail.len().saturating_sub(4)..].join(" / ")))
        }
    }
}

/// How a turn may act, as Codex's sandbox: Plan and Ask read only; Auto may
/// do anything this account may (Cove runs the agent as the person, as Claude
/// runs with bypassPermissions).
fn sandbox(mode: Option<&str>) -> &'static str {
    match mode {
        Some("plan" | "ask") => "read-only",
        // Writes inside the project only, no network for its commands.
        Some("supervised") => "workspace-write",
        _ => "danger-full-access",
    }
}

/// The shell line for one turn: the prompt from its file on stdin, the JSON
/// Lines into `stream`, its own words into `err`, its exit status into `done`.
/// `resume`: the Codex session of the thread's last turn.
pub struct Turn<'a> {
    pub dir: &'a str,
    pub model: &'a str,
    pub mode: Option<&'a str>,
    pub resume: Option<&'a str>,
    pub prompt: &'a Path,
    pub stream: &'a Path,
    pub err: &'a Path,
    pub done: &'a Path,
}

pub fn script(codex: &str, t: &Turn) -> String {
    let q = |s: &str| crate::api::shell_quote(s);
    let p = |p: &Path| q(&p.display().to_string());
    let resume = t.resume.map(|s| format!(" resume {}", q(s))).unwrap_or_default();
    format!(
        "cd {dir} || exit 1; {codex} exec --json --skip-git-repo-check -C {dir} -m {model} --sandbox {sandbox}{resume} - < {prompt} > {stream} 2> {err}; echo $? > {done}",
        dir = q(t.dir),
        codex = q(codex),
        model = q(t.model),
        sandbox = sandbox(t.mode),
        prompt = p(t.prompt),
        stream = p(t.stream),
        err = p(t.err),
        done = p(t.done),
    )
}

/// The Codex session a finished turn's stream started (`thread.started`).
pub fn session_of(stream: &str) -> Option<String> {
    stream.lines().find_map(|l| {
        let v: serde_json::Value = serde_json::from_str(l).ok()?;
        (v["type"] == "thread.started").then(|| v["thread_id"].as_str().map(str::to_owned)).flatten()
    })
}

/// A Codex turn said something final: its turn ended, well or badly.
pub fn answered(stream: &str) -> bool {
    stream.contains("\"type\":\"turn.completed\"") || stream.contains("\"type\":\"turn.failed\"")
}

/// One line of a Codex stream as the Agents tab's marks, or false when the
/// line is not Codex's (host.rs agent_events then reads it as Claude's).
pub fn events(v: &serde_json::Value, out: &mut Vec<crate::api::AgentEvent>) -> bool {
    let ev = |kind: &str, text: String| crate::api::AgentEvent { kind: kind.into(), text };
    let line = |s: &str| -> String { s.lines().next().unwrap_or("").chars().take(160).collect() };
    match v["type"].as_str() {
        Some("thread.started" | "turn.started" | "item.started" | "item.updated" | "turn.completed") => true,
        // The turn's own failure, in its human part: Codex appends the URL,
        // cf-ray and request id after ", url:" (measured 2026-11-27, 0.172.0).
        Some("turn.failed") => {
            let m = v["error"]["message"].as_str().unwrap_or("the turn failed");
            out.push(ev("error", m.split(", url:").next().unwrap_or(m).trim().to_owned()));
            true
        }
        // Codex's `error` lines are its retries ("Reconnecting... 2/5 (...)")
        // and, last, the same words turn.failed carries: five red marks for
        // one bad key. turn.failed alone says it. Claude never writes a bare
        // {"type":"error","message":...} line.
        Some("error") if v.get("message").is_some() && v.get("subtype").is_none() => true,
        Some("item.completed") => {
            let it = &v["item"];
            match it["type"].as_str() {
                Some("agent_message") => {
                    let t = it["text"].as_str().unwrap_or("").trim();
                    if !t.is_empty() {
                        out.push(ev("text", t.to_owned()));
                    }
                }
                Some("command_execution") => out.push(ev("tool", format!("Bash · {}", line(it["command"].as_str().unwrap_or(""))))),
                Some("file_change") => {
                    let paths: Vec<&str> = it["changes"].as_array().into_iter().flatten().filter_map(|c| c["path"].as_str()).collect();
                    out.push(ev("tool", format!("Edit · {}", line(&paths.join(", ")))));
                }
                Some("mcp_tool_call") => out.push(ev("tool", format!("{}.{}", it["server"].as_str().unwrap_or("mcp"), it["tool"].as_str().unwrap_or("tool")))),
                Some("web_search") => out.push(ev("tool", format!("WebSearch · {}", line(it["query"].as_str().unwrap_or(""))))),
                Some("error") => out.push(ev("error", it["message"].as_str().unwrap_or("").to_owned())),
                _ => {}
            }
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lines as codex-rs/exec/src/exec_events.rs serialises them.
    #[test]
    fn a_codex_stream_reads_as_the_agents_marks() {
        let stream = [
            r#"{"type":"thread.started","thread_id":"0199a213-81c0-7800-8aa1-bbab2a035a53"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"item.started","item":{"id":"item_1","type":"command_execution","command":"bash -lc ls","aggregated_output":"","exit_code":null,"status":"in_progress"}}"#,
            r#"{"type":"item.completed","item":{"id":"item_1","type":"command_execution","command":"bash -lc ls","aggregated_output":"app.R\n","exit_code":0,"status":"completed"}}"#,
            r#"{"type":"item.completed","item":{"id":"item_2","type":"file_change","changes":[{"path":"app.R","kind":"update"}],"status":"completed"}}"#,
            r#"{"type":"item.completed","item":{"id":"item_3","type":"agent_message","text":"Done: the plot is blue now."}}"#,
            r#"{"type":"turn.completed","usage":{"input_tokens":24763,"cached_input_tokens":24448,"output_tokens":122,"reasoning_output_tokens":0}}"#,
        ]
        .join("\n");
        let mut out = Vec::new();
        for l in stream.lines() {
            assert!(events(&serde_json::from_str(l).unwrap(), &mut out), "{l}");
        }
        let got: Vec<(String, String)> = out.into_iter().map(|e| (e.kind, e.text)).collect();
        assert_eq!(got, [("tool".into(), "Bash · bash -lc ls".into()), ("tool".into(), "Edit · app.R".into()), ("text".into(), "Done: the plot is blue now.".into())]);
        assert_eq!(session_of(&stream).as_deref(), Some("0199a213-81c0-7800-8aa1-bbab2a035a53"));
        assert!(answered(&stream));
        // A bad key, as Codex 0.172.0 printed it (2026-11-27): four retries,
        // the same words as an error, then turn.failed - one mark.
        let bad = [
            r#"{"type":"thread.started","thread_id":"01a21aec-9713-7bd0-9fd3-45107a1dd6b9"}"#,
            r#"{"type":"turn.started"}"#,
            r#"{"type":"error","message":"Reconnecting... 2/5 (unexpected status 401 Unauthorized: Incorrect API key provided: sk-inval*******-key. You can find your API key at https://platform.openai.com/account/api-keys., url: wss://api.openai.com/v1/responses, cf-ray: a60dd89b9a4e819b-DEN, request id: req_x, auth error: 401, auth error code: invalid_api_key)"}"#,
            r#"{"type":"error","message":"unexpected status 401 Unauthorized: Incorrect API key provided: sk-inval*******-key. You can find your API key at https://platform.openai.com/account/api-keys., url: https://api.openai.com/v1/responses, cf-ray: a60dd8faf8ee859e-DEN, request id: req_y, auth error: 401, auth error code: invalid_api_key"}"#,
            r#"{"type":"turn.failed","error":{"message":"unexpected status 401 Unauthorized: Incorrect API key provided: sk-inval*******-key. You can find your API key at https://platform.openai.com/account/api-keys., url: https://api.openai.com/v1/responses, cf-ray: a60dd8faf8ee859e-DEN, request id: req_y, auth error: 401, auth error code: invalid_api_key"}}"#,
        ];
        let mut out = Vec::new();
        for l in bad {
            assert!(events(&serde_json::from_str(l).unwrap(), &mut out), "{l}");
        }
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].kind, "error");
        assert_eq!(out[0].text, "unexpected status 401 Unauthorized: Incorrect API key provided: sk-inval*******-key. You can find your API key at https://platform.openai.com/account/api-keys.");
        assert!(answered(&bad.join("\n")));
        // Claude's lines are not Codex's.
        assert!(!events(&serde_json::from_str(r#"{"type":"assistant","message":{"content":[]}}"#).unwrap(), &mut Vec::new()));
        assert!(!events(&serde_json::from_str(r#"{"type":"result","subtype":"success"}"#).unwrap(), &mut Vec::new()));
    }

    #[test]
    fn a_turn_resumes_its_thread_and_plan_reads_only() {
        let (p, s, e, d) = (Path::new("/r/1.prompt"), Path::new("/r/1.stream"), Path::new("/r/1.err"), Path::new("/r/1.done"));
        let t = Turn { dir: "/home/a/site", model: "gpt-5.5", mode: Some("plan"), resume: Some("abc"), prompt: p, stream: s, err: e, done: d };
        let line = script("/home/a/.local/bin/codex", &t);
        assert!(line.contains("exec --json --skip-git-repo-check -C '/home/a/site' -m 'gpt-5.5' --sandbox read-only resume 'abc' - < '/r/1.prompt'"), "{line}");
        let s = Turn { mode: Some("supervised"), ..t };
        assert!(script("codex", &s).contains("--sandbox workspace-write"), "Supervised writes in the project only");
        let t = Turn { mode: None, resume: None, ..t };
        assert!(script("codex", &t).contains("--sandbox danger-full-access - <"));
    }
}
