//! A project's agent thread, kept with its saves (operator, 2026-11-27: "when
//! we push save, to choose to compact and store thread history in git and s3
//! as well, that way we can compact and build onto existing connected systems,
//! and a way to revert back to the state of an agent (with current context)
//! or with old").
//!
//! The thread goes into the project's own files, so it travels the way the
//! project does - every save is a version in git, mirrored to S3 - and any
//! agent on any system that has the project can read it:
//!   .cove/agent/thread.md    compacted: each turn's request, the agent's last
//!                            reply, the tools it used - not the stream
//!   .cove/agent/thread.json  the same turns, as data
//! Bringing a saved thread back gives it to the agent's next turn
//! (`carried`): on a fresh session, the agent as it was then; or on top of
//! the current context.

use serde::{Deserialize, Serialize};

use crate::api::AgentTurn;

pub const DIR: &str = ".cove/agent";
pub const MD: &str = ".cove/agent/thread.md";
pub const JSON: &str = ".cove/agent/thread.json";

/// The most a carried thread may weigh in a prompt; the oldest turns go first.
const CARRY_MAX: usize = 24_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct KeptTurn {
    pub at: i64,
    pub prompt: String,
    /// The agent's last words on it.
    pub reply: String,
    /// The tools it used, one line each ("Bash · cargo test").
    pub tools: Vec<String>,
    pub done: bool,
    #[serde(default)]
    pub by: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct KeptThread {
    pub domain: String,
    /// Unix seconds.
    pub saved_at: i64,
    pub turns: Vec<KeptTurn>,
}

pub fn compact(domain: &str, turns: &[AgentTurn], now: i64) -> KeptThread {
    let turns = turns
        .iter()
        .map(|t| KeptTurn {
            at: t.at,
            prompt: t.prompt.trim().to_owned(),
            reply: t.events.iter().filter(|e| e.kind == "text").map(|e| e.text.trim()).filter(|s| !s.is_empty()).last().unwrap_or("").to_owned(),
            tools: t.events.iter().filter(|e| e.kind == "tool").map(|e| e.text.lines().next().unwrap_or("").to_owned()).collect(),
            done: t.done,
            by: t.by.clone(),
        })
        .collect();
    KeptThread { domain: domain.to_owned(), saved_at: now, turns }
}

/// The thread as a page a person or an agent reads.
pub fn markdown(k: &KeptThread) -> String {
    let mut s = format!(
        "# The agent's thread for {} (compacted)\n\nKept by Cove with a save. Each turn: what was asked, the agent's last reply, and the tools it used.\nAn agent picking up this project may read it to know what was done and why.\n\n",
        k.domain
    );
    for (i, t) in k.turns.iter().enumerate() {
        let who = if t.by.is_empty() { String::new() } else { format!(" · {}", t.by) };
        s.push_str(&format!("## Turn {}{}{}\n\n", i + 1, if t.at > 0 { format!(" · {}", stamp(t.at)) } else { String::new() }, who));
        s.push_str(&format!("**Asked:** {}\n\n", t.prompt));
        if !t.reply.is_empty() {
            s.push_str(&format!("**Reply:** {}\n\n", t.reply));
        }
        if !t.tools.is_empty() {
            s.push_str(&format!("**Tools ({}):** {}\n\n", t.tools.len(), t.tools.iter().take(12).cloned().collect::<Vec<_>>().join("; ")));
        }
        if !t.done {
            s.push_str("(not finished when saved)\n\n");
        }
    }
    s
}

/// A UTC date and time, from Unix seconds, without a time library.
fn stamp(t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", secs / 3600, secs % 3600 / 60)
}

/// The next turn's prompt with a saved thread carried into it. `as_it_was`:
/// the agent starts from that thread alone (a fresh session); otherwise it
/// keeps what it knows now and is given the saved thread as well.
pub fn carried(thread_md: &str, from: &str, as_it_was: bool, prompt: &str) -> String {
    let mut t = thread_md.trim().to_owned();
    if t.len() > CARRY_MAX {
        // The newest turns matter most: cut from the front, at a turn.
        let cut = t.len() - CARRY_MAX;
        let at = t[cut..].find("\n## Turn ").map_or(cut, |i| cut + i + 1);
        t = format!("(earlier turns left out)\n\n{}", &t[at..]);
    }
    let how = if as_it_was {
        "Pick this project up from the saved thread below, as the agent was at that version. Treat it as your memory of the work so far."
    } else {
        "Below is the agent's thread as saved with an earlier version. Keep what you know now, and take this into account."
    };
    format!("{how} (Saved thread from {from}.)\n\n<saved-thread>\n{t}\n</saved-thread>\n\n{}", prompt.trim())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::AgentEvent;

    fn ev(kind: &str, text: &str) -> AgentEvent {
        AgentEvent { kind: kind.into(), text: text.into() }
    }

    #[test]
    fn a_thread_compacts_to_requests_replies_and_tools() {
        let turns = vec![AgentTurn {
            id: "1".into(),
            prompt: "  make the plot blue ".into(),
            events: vec![ev("text", "Looking."), ev("tool", "Edit · app.R"), ev("tool", "Bash · Rscript -e 1\nmore"), ev("text", "Done: it is blue.")],
            done: true,
            by: String::new(),
            at: 1_794_873_600,
        }];
        let k = compact("a.example.org", &turns, 1_795_712_400);
        assert_eq!(k.turns[0].prompt, "make the plot blue");
        assert_eq!(k.turns[0].reply, "Done: it is blue.");
        assert_eq!(k.turns[0].tools, vec!["Edit · app.R", "Bash · Rscript -e 1"]);
        let md = markdown(&k);
        assert!(md.contains("## Turn 1 · 2026-11-17 00:00 UTC"), "{md}");
        assert!(md.contains("**Asked:** make the plot blue") && md.contains("**Reply:** Done: it is blue.") && md.contains("**Tools (2):**"));
        let back: KeptThread = serde_json::from_str(&serde_json::to_string(&k).unwrap()).unwrap();
        assert_eq!(back, k);
    }

    #[test]
    fn a_carried_thread_leads_the_prompt_and_keeps_its_newest_turns() {
        let p = carried("# t\n\n## Turn 1\n\nold", "version abc1234", true, "  go on ");
        assert!(p.starts_with("Pick this project up") && p.contains("<saved-thread>\n# t") && p.ends_with("go on"));
        assert!(carried("x", "v", false, "q").starts_with("Below is the agent's thread"));
        let big = format!("# t\n\n## Turn 1\n\n{}\n## Turn 2\n\nnewest", "a".repeat(CARRY_MAX + 10));
        let p = carried(&big, "v", true, "q");
        assert!(p.contains("(earlier turns left out)") && p.contains("## Turn 2\n\nnewest") && !p.contains("## Turn 1"));
    }
}
