//! Go's Briefing: the recap as a short spoken episode - what changed across
//! the organisation and why - written by a model from the records and read by
//! a narrator voice (operator, 2026-10-23: "a far better voice model which
//! gave podcast like summaries which is really what i wanted for the recap in the go
//! section, something far more enlightening about the what and why things
//! have changed over the organization").
//!
//! recap.rs counts what happened; it cannot say why. The why is written down
//! already, in three places: what each agent was asked (the run's .prompt),
//! what it answered when it finished (the result in its .stream), and each
//! commit's message. Those, with the counts, are the material. The script is
//! held to that material: nothing in it that the records do not say.
//!
//! Writing and recording take a minute or two, longer than one request may
//! wait, so `brief()` starts the work on a thread and answers with where it
//! is; Go asks again until the episode is ready. One episode per window is
//! kept for EPISODE_TTL, so a second listen costs nothing.
//!
//! OpenAI for both steps, the server's own key (host.rs openai_key):
//! COVE_BRIEF_MODEL writes (default gpt-4.1), gpt-4o-mini-tts reads, voice
//! onyx.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime};

use serde_json::Value;

use crate::api::RecapBrief;

/// A finished episode is replayed, not remade, for this long.
const EPISODE_TTL: Duration = Duration::from_secs(15 * 60);
/// About a paragraph per voice call. gpt-4o-mini-tts takes 4096 characters,
/// but at that size it sometimes stops early and still answers 200: the same
/// 2,983 characters came back as 186 s and, asked again, as 16 s (measured
/// 2026-10-23; the first day's episode had 98 s of a 4.5-minute script). Short
/// pieces fail less, and each is checked (`whole`) and asked again.
const CHUNK_MAX_CHARS: usize = 1500;
/// Tries per piece before the longest answer is kept.
const VOICE_TRIES: usize = 3;
/// What the writer is given, at most: the newest material first.
const MATERIAL_MAX_CHARS: usize = 90_000;
const TTS_MODEL: &str = "gpt-4o-mini-tts";
const TTS_VOICE: &str = "onyx";
// tts.r's narrator, turned from a fixed walkthrough to a briefing.
const TTS_INSTRUCTIONS: &str = "You are the host of a short briefing on what changed across a software organisation. \
Voice: calm, warm, and clear - an experienced documentary narrator, not a robot. \
Pace: measured and unhurried, with natural pauses at sentence and paragraph breaks. \
Emphasis: land lightly on the first mention of each named site or system and on any figure or count. \
Stay matter-of-fact throughout - the work argues for itself, so do not oversell it, and let the problems \
land as plainly as the fixes. Never spell out punctuation or markup.";

struct Slot {
    view: RecapBrief,
    at: Instant,
}

fn slots() -> &'static Mutex<HashMap<u32, Slot>> {
    static S: OnceLock<Mutex<HashMap<u32, Slot>>> = OnceLock::new();
    S.get_or_init(Default::default)
}

/// Where the episode for the last `minutes` is; starts it when there is
/// none, it failed, it is older than EPISODE_TTL, or `fresh` asks again.
/// The audio rides only on a ready answer.
pub fn brief(minutes: u32, fresh: bool) -> RecapBrief {
    let minutes = minutes.clamp(5, 7 * 24 * 60);
    let mut map = slots().lock().unwrap();
    if let Some(s) = map.get(&minutes) {
        let working = matches!(s.view.state.as_str(), "writing" | "recording");
        if working || (s.view.state == "ready" && !fresh && s.at.elapsed() < EPISODE_TTL) {
            return s.view.clone();
        }
    }
    let view = RecapBrief { minutes, state: "writing".into(), ..Default::default() };
    map.insert(minutes, Slot { view: view.clone(), at: Instant::now() });
    drop(map);
    std::thread::spawn(move || {
        let set = |v: RecapBrief| {
            slots().lock().unwrap().insert(minutes, Slot { view: v, at: Instant::now() });
        };
        let made = now();
        let fail = |e: String| RecapBrief { minutes, state: "failed".into(), error: e, made_at: made, ..Default::default() };
        let script = match write(minutes) {
            Ok(s) => s,
            Err(e) => return set(fail(e)),
        };
        set(RecapBrief { minutes, state: "recording".into(), script: script.clone(), made_at: made, ..Default::default() });
        match record(&script) {
            Ok(mp3) => set(RecapBrief { minutes, state: "ready".into(), script, audio: crate::api::b64_encode(&mp3), made_at: made, ..Default::default() }),
            // The script stands without its voice.
            Err(e) => set(RecapBrief { minutes, state: "failed".into(), script, error: e, made_at: made, ..Default::default() }),
        }
    });
    view
}

/// Most of a reply that is read for its spoken line: the end is where an
/// agent says what it did and what it needs.
const SPOKEN_REPLY_MAX: usize = 6000;
const SPOKEN_INSTRUCTIONS: &str = "You are a colleague answering across the desk. Voice: warm, natural and conversational, \
at an easy pace. Say it once, plainly; never spell out punctuation or markup.";

/// An agent's finished reply, as one or two sentences said aloud - or
/// nothing. Speak is for a conversation, not a reading of the chat: it talks
/// only when the person asked a question it can answer, or when the agent
/// needs something from them (operator, 2026-11-05: "it shouldn't turn into a
/// meeting where everyone's interrupting each other ... it only speaks when you
/// ask or if it really needs something from you"). The full reply stays in
/// the chat.
pub fn spoken(asked: &str, reply: &str) -> Result<crate::api::Spoken, String> {
    let key = crate::host::openai_key().ok_or("no OpenAI key on this server (OPENAI_API_KEY)")?;
    let reply = prose(reply);
    if reply.trim().is_empty() {
        return Ok(Default::default());
    }
    let tail: String = {
        let n = reply.chars().count();
        reply.chars().skip(n.saturating_sub(SPOKEN_REPLY_MAX)).collect()
    };
    let system = "A person is working with a coding agent by voice. You decide whether the agent's finished reply \
        should be said aloud, and if so you write what is said. Speak ONLY when (a) the person's message asked a \
        question the reply answers, or (b) the reply needs something from the person: a question, a choice, a \
        confirmation, or it is blocked. Otherwise stay silent - finished work, progress, summaries and lists stay in \
        the chat. When you speak: one or two short sentences, at most 40 words, said to the person as you. Give the \
        answer, or the one thing needed from them. No code, file names, paths, commands, URLs, markdown or numbers \
        nobody needs; say what a thing is, not what it is called on disk. No filler, no greeting, no sign-off. \
        Answer as JSON: {\"speak\": true|false, \"line\": \"...\"}.";
    let user = format!("The person said:\n{}\n\nThe agent replied (its end, if long):\n{tail}", clip(asked, 2000));
    let body = serde_json::json!({
        "model": "gpt-4.1-mini",
        "temperature": 0.3,
        "max_tokens": 200,
        "response_format": {"type": "json_object"},
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
    });
    let v = openai_json("chat/completions", &key, &body, 60)?;
    let out: Value = serde_json::from_str(v["choices"][0]["message"]["content"].as_str().unwrap_or("{}")).unwrap_or_default();
    let line = out["line"].as_str().unwrap_or("").trim().to_owned();
    if !out["speak"].as_bool().unwrap_or(false) || line.is_empty() {
        return Ok(Default::default());
    }
    let tts = serde_json::json!({
        "model": TTS_MODEL, "voice": "alloy", "input": line, "instructions": SPOKEN_INSTRUCTIONS, "response_format": "mp3",
    });
    let r = openai("audio/speech", &key, &tts, 60)?;
    if !r.ok {
        return Err(format!("OpenAI voice: {}", r.status_text));
    }
    Ok(crate::api::Spoken { line, audio: crate::api::b64_encode(&r.bytes) })
}

/// Listen under an agent's reply: all of it, read aloud as written (operator,
/// 2026-11-13: "for the responses that we get, we need to be able to push a
/// button within that response ... convert into audio so that we can listen to
/// it"). The words only - no code blocks, markdown marks or addresses - in
/// pieces under the speech model's 4,096-character input, one mp3. Spoken
/// replies (Speak) stay `spoken`'s: a line, and only when it is wanted.
pub fn read_aloud(text: &str) -> Result<crate::api::Spoken, String> {
    let key = crate::host::openai_key().ok_or("no OpenAI key on this server (OPENAI_API_KEY)")?;
    let words = aloud(text);
    if words.trim().is_empty() {
        return Err("this reply has nothing to read - it is all code".into());
    }
    let mut mp3 = Vec::new();
    for piece in pieces(&words, READ_PIECE).into_iter().take(READ_PIECES) {
        let tts = serde_json::json!({
            "model": TTS_MODEL, "voice": "alloy", "input": piece, "instructions": SPOKEN_INSTRUCTIONS, "response_format": "mp3",
        });
        let r = openai("audio/speech", &key, &tts, 120)?;
        if !r.ok {
            let v: Value = serde_json::from_slice(&r.bytes).unwrap_or_default();
            return Err(format!("OpenAI voice: {}", v["error"]["message"].as_str().unwrap_or(&r.status_text)));
        }
        // mp3 frames stand alone: pieces played end to end are one reading.
        mp3.extend_from_slice(&r.bytes);
    }
    Ok(crate::api::Spoken { line: words, audio: crate::api::b64_encode(&mp3) })
}

/// Characters per spoken piece (the model takes 4,096), and how many pieces
/// at most: about 28,000 characters, a long reply read whole.
const READ_PIECE: usize = 3500;
const READ_PIECES: usize = 8;

/// A reply as words to say: code blocks out, inline code and emphasis marks
/// off, a link's words kept and its address dropped, headings and bullets as
/// plain lines.
fn aloud(t: &str) -> String {
    let mut out = String::new();
    for l in prose(t).lines() {
        let l = l.trim();
        if l.is_empty() || l.chars().all(|c| matches!(c, '-' | '|' | ':' | ' ' | '*' | '_' | '=')) {
            out.push('\n');
            continue;
        }
        // A table row: its cells, one after another.
        if l.starts_with('|') && l.ends_with('|') {
            let cells: Vec<&str> = l.trim_matches('|').split('|').map(str::trim).filter(|c| !c.is_empty()).collect();
            out.push_str(&cells.join(", ").replace(['`', '*'], ""));
            out.push('\n');
            continue;
        }
        let l = l.trim_start_matches(['#', '>']).trim_start();
        let l = l.strip_prefix("- ").or_else(|| l.strip_prefix("* ")).unwrap_or(l);
        // [words](address) -> words; a bare address -> "a link".
        let mut s = String::new();
        let mut rest = l;
        while let Some(i) = rest.find('[') {
            let (before, after) = rest.split_at(i);
            match after.find("](").and_then(|j| after[j..].find(')').map(|k| (j, j + k))) {
                Some((j, k)) => {
                    s.push_str(before);
                    s.push_str(&after[1..j]);
                    rest = &after[k + 1..];
                }
                None => {
                    s.push_str(before);
                    s.push('[');
                    rest = &after[1..];
                }
            }
        }
        s.push_str(rest);
        let s: Vec<String> = s.split(' ').map(|w| if w.starts_with("http://") || w.starts_with("https://") { "a link".to_owned() } else { w.to_owned() }).collect();
        let s = s.join(" ").replace(['`', '*'], "").replace("__", "");
        out.push_str(s.trim());
        out.push('\n');
    }
    // One pause between paragraphs, however many blank lines made them.
    let mut tidy = out.trim().to_owned();
    while tidy.contains("\n\n\n") {
        tidy = tidy.replace("\n\n\n", "\n\n");
    }
    tidy
}

/// `t` in pieces of at most `n` characters, cut at a paragraph, else a
/// sentence, else a space.
fn pieces(t: &str, n: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = t.trim();
    while rest.chars().count() > n {
        let head: String = rest.chars().take(n).collect();
        let cut = head.rfind("\n\n").or_else(|| head.rfind(". ").map(|i| i + 1)).or_else(|| head.rfind(' ')).filter(|&i| i > 0).unwrap_or(head.len());
        out.push(rest[..cut].trim().to_owned());
        rest = rest[cut..].trim_start();
    }
    if !rest.is_empty() {
        out.push(rest.to_owned());
    }
    out
}

fn now() -> i64 {
    SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// The episode's script, from the window's material.
fn write(minutes: u32) -> Result<String, String> {
    let key = crate::host::openai_key().ok_or("no OpenAI key on this server (OPENAI_API_KEY)")?;
    let model = std::env::var("COVE_BRIEF_MODEL").ok().filter(|m| !m.is_empty()).unwrap_or_else(|| "gpt-4.1".into());
    let material = material(minutes);
    let span = crate::host::recap::span_words(minutes);
    let system = format!(
        "You write the script of a short spoken briefing - a podcast episode of one host - for the founder of ndexr, \
         a platform of many web sites, a scientific software stack (Gentoo Prefix, EasyBuild, R, Python, Rust and Julia \
         package sets, built on several build machines) and Cove, its IDE. The episode covers the last {span}.\n\n\
         What it is for: to understand what changed across the organisation and why - the problem or the ask behind each \
         change, what was done about it, and what it means or what is left open. Not a list of activity: the story of the work.\n\n\
         Shape: open with two or three sentences that say what the stretch was mostly about. Then three to six stories, \
         grouped by theme rather than by source, each told as what was wrong or wanted, what changed, and why it matters. \
         Close with what is still open or waiting, if the material says so. 350 to 900 words, more only when much happened.\n\n\
         Rules: use only facts in the material - if it does not say why, do not guess; say what was done. No markdown, \
         no lists, no headings, no URLs, no code, no file names, variable names or commands - say what the thing is (the deployment notes, \
         the pod setting), not what it is called on disk. Say numbers as a person would. \
         Name sites the way they would be said aloud (cove dot ndexr dot io becomes Cove; four-box-arm becomes the arm build \
         machine). Talk to the founder as you, plainly. No filler words (right now, currently, basically, just, simply, actually), \
         no hype, no sign-off, no sound cues."
    );
    let user = format!("The material for the last {span}, newest first:\n\n{material}");
    let body = serde_json::json!({
        "model": model,
        "temperature": 0.4,
        "max_tokens": 2200,
        "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
    });
    let v = openai_json("chat/completions", &key, &body, 120)?;
    let text = v["choices"][0]["message"]["content"].as_str().unwrap_or("").trim().to_owned();
    if text.is_empty() {
        return Err("the writer returned nothing".into());
    }
    Ok(text)
}

/// The script, read aloud: one mp3, chunk by chunk (mp3 frames join as bytes).
fn record(script: &str) -> Result<Vec<u8>, String> {
    let key = crate::host::openai_key().ok_or("no OpenAI key on this server (OPENAI_API_KEY)")?;
    let chunks = chunk(script, CHUNK_MAX_CHARS);
    let handles: Vec<_> = chunks
        .into_iter()
        .map(|c| {
            let key = key.clone();
            std::thread::spawn(move || speak_whole(&c, &key))
        })
        .collect();
    let mut out = Vec::new();
    for (i, h) in handles.into_iter().enumerate() {
        let bytes = h.join().map_err(|_| "the voice thread stopped".to_string())?.map_err(|e| format!("part {}: {e}", i + 1))?;
        out.extend(bytes);
    }
    Ok(out)
}

/// A piece read whole: asked again while its audio is far shorter than its
/// words take, keeping the longest answer.
fn speak_whole(text: &str, key: &str) -> Result<Vec<u8>, String> {
    let mut best: Vec<u8> = Vec::new();
    for _ in 0..VOICE_TRIES {
        let mp3 = speak(text, key)?;
        if mp3.len() > best.len() {
            best = mp3;
        }
        if whole(text, best.len()) {
            break;
        }
    }
    Ok(best)
}

/// Long enough for its words: OpenAI's mp3 is 128 kbit/s (16,000 bytes a
/// second), and a narrator reads about 2.6 words a second; under 60% of that
/// is a reading that stopped.
fn whole(text: &str, bytes: usize) -> bool {
    let secs = bytes as f64 / 16_000.0;
    let want = text.split_whitespace().count() as f64 / 2.6;
    secs >= 0.6 * want
}

fn speak(text: &str, key: &str) -> Result<Vec<u8>, String> {
    let body = serde_json::json!({
        "model": TTS_MODEL, "voice": TTS_VOICE, "input": text, "instructions": TTS_INSTRUCTIONS, "response_format": "mp3",
    });
    let r = openai("audio/speech", key, &body, 180)?;
    if !r.ok {
        let v: Value = serde_json::from_slice(&r.bytes).unwrap_or_default();
        return Err(format!("OpenAI voice: {}", v["error"]["message"].as_str().unwrap_or(&r.status_text)));
    }
    Ok(r.bytes)
}

fn openai(path: &str, key: &str, body: &Value, secs: u64) -> Result<ehttp::Response, String> {
    let mut req = ehttp::Request::post(format!("https://api.openai.com/v1/{path}"), serde_json::to_vec(body).map_err(|e| e.to_string())?);
    // Set, not inserted: post() carries text/plain already (host.rs assist).
    req.headers = ehttp::Headers::new(&[("Content-Type", "application/json")]);
    req.headers.insert("Authorization", format!("Bearer {key}"));
    req.timeout = Some(Duration::from_secs(secs));
    ehttp::fetch_blocking(&req).map_err(|e| format!("OpenAI: {e}"))
}

fn openai_json(path: &str, key: &str, body: &Value, secs: u64) -> Result<Value, String> {
    let r = openai(path, key, body, secs)?;
    let v: Value = serde_json::from_slice(&r.bytes).map_err(|_| format!("OpenAI answered {}", r.status))?;
    if !r.ok {
        return Err(format!("OpenAI: {}", v["error"]["message"].as_str().unwrap_or(&r.status_text)));
    }
    Ok(v)
}

/// Paragraphs gathered into chunks of at most `max` characters, never cut
/// mid-sentence (tts.r .chunk_text); a paragraph over `max` alone is cut at
/// its sentences.
fn chunk(text: &str, max: usize) -> Vec<String> {
    let mut pieces = Vec::new();
    for p in text.split("\n\n").map(str::trim).filter(|p| !p.is_empty()) {
        if p.chars().count() <= max {
            pieces.push(p.to_owned());
            continue;
        }
        let mut cur = String::new();
        for s in p.split_inclusive(['.', '!', '?']) {
            if !cur.is_empty() && cur.chars().count() + s.chars().count() > max {
                pieces.push(std::mem::take(&mut cur).trim().to_owned());
            }
            cur.push_str(s);
        }
        if !cur.trim().is_empty() {
            pieces.push(cur.trim().to_owned());
        }
    }
    let mut out: Vec<String> = Vec::new();
    for p in pieces {
        match out.last_mut() {
            Some(last) if last.chars().count() + 2 + p.chars().count() <= max => {
                last.push_str("\n\n");
                last.push_str(&p);
            }
            _ => out.push(p),
        }
    }
    out
}

// ── the material ───────────────────────────────────────────────────────────

/// Everything the writer may use, as plain sections: the counts (recap.rs),
/// then each agent run's ask and answer, the commits' messages, and the
/// requests. Secrets are blanked (host.rs mask_secrets) - this leaves the
/// machine for OpenAI.
pub fn material(minutes: u32) -> String {
    let since = SystemTime::now() - Duration::from_secs(u64::from(minutes) * 60);
    let mut out = String::new();
    let counts = crate::host::recap::recap(minutes);
    out.push_str("COUNTS\n");
    for p in &counts.parts {
        out.push_str(&format!("{}: {}\n", p.title, p.lines.join(" ")));
    }
    out.push_str("\nAGENT WORK (what each agent was asked, and what it reported when it finished)\n");
    for r in runs(since) {
        out.push_str(&r);
        out.push('\n');
        if out.len() > MATERIAL_MAX_CHARS * 2 / 3 {
            out.push_str("[older agent work left out]\n");
            break;
        }
    }
    if let Some(v) = rows(COMMITS, minutes) {
        if !v.is_empty() {
            out.push_str("\nCOMMITS\n");
            for c in v {
                out.push_str(&format!("- {}: {}\n", s(&c, "repo"), clip(s(&c, "message"), 600)));
            }
        }
    }
    if let Some(v) = rows(REQUESTS, minutes) {
        if !v.is_empty() {
            out.push_str("\nREQUESTS FROM PEOPLE (new ones, and the replies or status moves on them)\n");
            for r in v {
                out.push_str(&format!("- {} on {} ({}): {}\n", s(&r, "what"), s(&r, "domain"), s(&r, "status"), clip(s(&r, "body"), 500)));
            }
        }
    }
    let out = crate::host::mask_secrets(&out);
    out.chars().take(MATERIAL_MAX_CHARS).collect()
}

/// The window's agent runs, newest first: "### <host> (<who>)\nASKED: …\nREPORTED: …".
/// Each run is a .prompt under host_runs/<who>/<host>/, and its answer the last
/// result in host_runs/<stamp>.stream.
fn runs(since: SystemTime) -> Vec<String> {
    let root = Path::new("/srv/ndexr/host_runs");
    let mut found: Vec<(SystemTime, String)> = Vec::new();
    for who in std::fs::read_dir(root).into_iter().flatten().flatten() {
        if !who.path().is_dir() {
            continue;
        }
        let who_name = who.file_name().to_string_lossy().into_owned();
        for site in std::fs::read_dir(who.path()).into_iter().flatten().flatten() {
            let host = site.file_name().to_string_lossy().into_owned();
            if !host.contains('.') || !site.path().is_dir() {
                continue;
            }
            for f in std::fs::read_dir(site.path()).into_iter().flatten().flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                let Some(stamp) = name.strip_suffix(".prompt") else { continue };
                let Ok(at) = f.metadata().and_then(|m| m.modified()) else { continue };
                if at < since {
                    continue;
                }
                let ask = std::fs::read_to_string(f.path()).unwrap_or_default();
                let answer = result_of(&root.join(format!("{stamp}.stream")));
                let who_said = if who_name == "_domain" { "the site's own agent".to_owned() } else { who_name.clone() };
                let mut r = format!("### {host} ({who_said})\nASKED: {}\n", clip(&ask, 700));
                if !answer.is_empty() {
                    r.push_str(&format!("REPORTED: {}\n", clip(&prose(&answer), 1600)));
                }
                found.push((at, r));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0));
    found.into_iter().map(|(_, r)| r).collect()
}

/// The last `result` a run's stream-json holds (its final answer), or "".
fn result_of(stream: &Path) -> String {
    let Ok(text) = std::fs::read_to_string(stream) else { return String::new() };
    text.lines()
        .rev()
        .filter(|l| l.contains("\"type\":\"result\""))
        .find_map(|l| serde_json::from_str::<Value>(l).ok()?["result"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// An answer without its code blocks: the writer wants what was done, not the code.
fn prose(t: &str) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for l in t.lines() {
        if l.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if !fenced {
            out.push_str(l);
            out.push('\n');
        }
    }
    out
}

/// At most `n` characters, on one line.
fn clip(t: &str, n: usize) -> String {
    let one: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= n {
        one
    } else {
        format!("{}…", one.chars().take(n).collect::<String>())
    }
}

fn rows(sql: &str, minutes: u32) -> Option<Vec<Value>> {
    let out = crate::host::platform_psql_vars(sql, &[("mins", &minutes.to_string())]).ok()?;
    serde_json::from_slice::<Value>(&out).ok()?.as_array().cloned()
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

// Whole messages, not recap.rs's first lines: the body is where the why is.
const COMMITS: &str = "select coalesce(json_agg(x order by x.at desc), '[]') from (select regexp_replace(repo_path, '^.*/', '') as repo, \
    message, committed_at as at from git_commits \
    where committed_at > now() - make_interval(mins => :'mins'::int) order by committed_at desc limit 60) x";

const REQUESTS: &str = "select coalesce(json_agg(x order by x.at desc), '[]') from ( \
    select 'new' as what, c.domain, c.status, c.what_sucks as body, c.created_at as at from complaints c \
      where c.created_at > now() - make_interval(mins => :'mins'::int) \
    union all \
    select m.kind, c.domain, c.status, m.body, m.created_at from request_messages m join complaints c on c.id = m.complaint_id \
      where m.created_at > now() - make_interval(mins => :'mins'::int) \
    limit 60) x";

#[cfg(test)]
mod tests {
    #[test]
    fn a_reply_is_read_as_words() {
        let r = "## What changed\n\n**Listen** is under every reply - see [the release](https://cove.ndexr.io/x).\n\n```bash\nrm -rf /\n```\n\n- one `thing`\n- see https://example.com now\n\n| a | b |\n|---|---|\n| 1 | 2 |";
        let a = super::aloud(r);
        assert_eq!(a, "What changed\n\nListen is under every reply - see the release.\n\none thing\nsee a link now\n\na, b\n\n1, 2");
        assert!(!a.contains("rm -rf") && !a.contains("http"));
        let long = "Sentence one is here. ".repeat(400);
        let p = super::pieces(&long, 3500);
        assert!(p.len() >= 3 && p.iter().all(|x| x.chars().count() <= 3500 && x.ends_with('.')), "{:?}", p.iter().map(|x| x.len()).collect::<Vec<_>>());
        assert_eq!(p.join(" ").split_whitespace().count(), long.split_whitespace().count());
    }

    #[test]
    fn chunks_stay_under_the_voice_limit_and_keep_sentences_whole() {
        let para = "One sentence here. ".repeat(30);
        let text = format!("{para}\n\n{para}\n\nShort close.");
        let c = super::chunk(&text, 600);
        assert!(c.iter().all(|x| x.chars().count() <= 600), "{c:?}");
        assert!(c.iter().all(|x| x.trim_end().ends_with('.')));
        assert_eq!(c.concat().matches("One sentence here.").count(), 60);
    }

    #[test]
    fn a_reading_that_stopped_early_is_not_whole() {
        let words = "word ".repeat(260); // ~100 s to read
        assert!(super::whole(&words, 100 * 16_000));
        assert!(!super::whole(&words, 16 * 16_000), "16 s of a 100 s piece");
    }

    #[test]
    fn an_answer_loses_its_code_and_long_text_is_clipped() {
        assert_eq!(super::prose("Done.\n```bash\nrm -rf x\n```\nAll good."), "Done.\nAll good.\n");
        assert_eq!(super::clip("a  b\nc", 10), "a b c");
        assert_eq!(super::clip("abcdef", 3), "abc…");
    }
}

#[cfg(test)]
mod live {
    /// A real episode, written and recorded (spends OpenAI money):
    /// `cargo test --release brief::live -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn an_episode_is_written_and_recorded() {
        let mins: u32 = std::env::var("BRIEF_MINUTES").ok().and_then(|m| m.parse().ok()).unwrap_or(1440);
        let script = super::write(mins).expect("script");
        println!("{script}");
        let mp3 = super::record(&script).expect("audio");
        std::fs::write(std::env::var("BRIEF_OUT").unwrap_or("/tmp/brief.mp3".into()), &mp3).unwrap();
        assert!(mp3.len() > 50_000);
    }
}
