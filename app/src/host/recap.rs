//! Go's Recap: the last stretch of work everywhere, read aloud (operator,
//! 2026-10-18: "push a button and hear a voice recorded or a voice summary
//! that we can listen to, and then talk back to our executive agent ... an
//! on-the-go summary ... users, sites, changes, updates, deployments").
//!
//! Records only, no model: what changed is counted from where it is kept, so
//! the summary is always true and always fast.
//!
//!   files     src/domains: files saved in the window, by domain
//!   agents    /srv/ndexr/host_runs/<who>/<host>/*.prompt: agent turns, by domain
//!   commits   git_commits: what was committed, and whether it was pushed
//!   deploys   deploy_requests
//!   requests  complaints, and request_messages' status moves
//!   people    new accounts, direct messages, who is here now
//!   sites     domain_registry: registered and published
//!   cove      /usr/local/lib/cove/releases: Cove itself, released
//!   machines  the build boxes (boxes.rs), on or off
//!
//! Each source stands alone: one that cannot be read is left out, not fatal.
//! For an ndexr admin only - it covers every account.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime};

use serde_json::Value;

use crate::api::{Recap, RecapPart};

/// The recap of the last `minutes`.
pub fn recap(minutes: u32) -> Recap {
    let minutes = minutes.clamp(5, 7 * 24 * 60);
    let since = SystemTime::now() - Duration::from_secs(u64::from(minutes) * 60);
    let mins = minutes.to_string();
    let mut parts = Vec::new();
    let mut add = |title: &str, lines: Vec<String>| {
        if !lines.is_empty() {
            parts.push(RecapPart { title: title.to_owned(), lines });
        }
    };
    add("Files", files(since));
    add("Agents", agents(since));
    add("Commits", rows(COMMITS, &mins).map(commits).unwrap_or_default());
    add("Deploys", rows(DEPLOYS, &mins).map(deploys).unwrap_or_default());
    add("Requests", rows(REQUESTS, &mins).map(requests).unwrap_or_default());
    add("People", rows(PEOPLE, &mins).map(people).unwrap_or_default());
    add("Sites", rows(SITES, &mins).map(sites).unwrap_or_default());
    add("Cove", releases(since));
    add("Build machines", machines());
    let span = span_words(minutes);
    let spoken = if parts.iter().all(|p| p.title == "Build machines") {
        format!("Nothing changed in the last {span}. {}", parts.iter().flat_map(|p| p.lines.clone()).collect::<Vec<_>>().join(" "))
    } else {
        let mut s = format!("Here is the last {span}. ");
        for p in &parts {
            s.push_str(&p.lines.join(" "));
            s.push(' ');
        }
        s.trim_end().to_owned()
    };
    Recap { minutes, parts, spoken }
}

pub(crate) fn span_words(m: u32) -> String {
    match m {
        60 => "hour".into(),
        m if m % 1440 == 0 => format!("{} days", m / 1440).replace("1 days", "day"),
        m if m % 60 == 0 => format!("{} hours", m / 60),
        m => format!("{m} minutes"),
    }
}

/// "a, b and c".
fn and_list(xs: &[String]) -> String {
    match xs {
        [] => String::new(),
        [a] => a.clone(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// The busiest few of a count by name, as words: "blog.ndexr.io (8)".
fn top(counts: &BTreeMap<String, usize>, n: usize) -> String {
    let mut v: Vec<(&String, &usize)> = counts.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let mut words: Vec<String> = v.iter().take(n).map(|(k, c)| format!("{k} ({c})")).collect();
    if v.len() > n {
        words.push(plural(v.len() - n, "other", "others"));
    }
    and_list(&words)
}

fn newer(p: &Path, since: SystemTime) -> bool {
    std::fs::metadata(p).and_then(|m| m.modified()).is_ok_and(|t| t >= since)
}

/// Files saved under src/domains in the window, by domain - a domain is the
/// first folder named like a host, flat (<host>/) or under its owner
/// (<owner>/<host>/). Build output, git and dependencies are not work.
fn files(since: SystemTime) -> Vec<String> {
    const SKIP: [&str; 6] = [".git", "target", "node_modules", "renv", ".rstudio-src", "dist"];
    let root = Path::new(crate::host::PLATFORM_DOMAINS);
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    let mut stack = vec![(root.to_path_buf(), String::new(), 0usize)];
    while let Some((dir, host, depth)) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                if depth < 5 && !SKIP.contains(&name.as_str()) && !name.starts_with("target") {
                    let h = if host.is_empty() && name.contains('.') && !name.starts_with('.') { name.clone() } else { host.clone() };
                    stack.push((e.path(), h, depth + 1));
                }
            } else if ft.is_file() && !host.is_empty() && !name.ends_with(".css") && newer(&e.path(), since) {
                *by.entry(host.clone()).or_default() += 1;
            }
        }
    }
    if by.is_empty() {
        return Vec::new();
    }
    let n: usize = by.values().sum();
    vec![format!("{} saved in {}: {}.", plural(n, "file", "files"), plural(by.len(), "domain", "domains"), top(&by, 4))]
}

/// Agent turns in the window: each run leaves a .prompt under
/// host_runs/<who>/<host>/ (`_domain` for a site's own agent).
fn agents(since: SystemTime) -> Vec<String> {
    let root = Path::new("/srv/ndexr/host_runs");
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    for who in std::fs::read_dir(root).into_iter().flatten().flatten() {
        if !who.path().is_dir() {
            continue;
        }
        for site in std::fs::read_dir(who.path()).into_iter().flatten().flatten() {
            let host = site.file_name().to_string_lossy().into_owned();
            if !host.contains('.') || !site.path().is_dir() {
                continue;
            }
            let n = std::fs::read_dir(site.path())
                .into_iter()
                .flatten()
                .flatten()
                .filter(|f| f.file_name().to_string_lossy().ends_with(".prompt") && newer(&f.path(), since))
                .count();
            if n > 0 {
                *by.entry(host).or_default() += n;
            }
        }
    }
    if by.is_empty() {
        return Vec::new();
    }
    let n: usize = by.values().sum();
    vec![format!("Agents ran {} on {}: {}.", plural(n, "turn", "turns"), plural(by.len(), "domain", "domains"), top(&by, 4))]
}

/// One query's rows (a JSON array), the window a psql variable.
fn rows(sql: &str, minutes: &str) -> Option<Vec<Value>> {
    let out = crate::host::platform_psql_vars(sql, &[("mins", minutes)]).ok()?;
    let v: Value = serde_json::from_slice(&out).ok()?;
    v.as_array().cloned()
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap_or("")
}

const COMMITS: &str = "select coalesce(json_agg(x order by x.at desc), '[]') from (select regexp_replace(repo_path, '^.*/', '') as repo, \
    split_part(message, E'\\n', 1) as message, pushed, committed_at as at from git_commits \
    where committed_at > now() - make_interval(mins => :'mins'::int) order by committed_at desc limit 40) x";

fn commits(v: Vec<Value>) -> Vec<String> {
    if v.is_empty() {
        return Vec::new();
    }
    let pushed = v.iter().filter(|r| r["pushed"].as_bool() == Some(true)).count();
    let mut by: BTreeMap<String, usize> = BTreeMap::new();
    for r in &v {
        *by.entry(s(r, "repo").to_owned()).or_default() += 1;
    }
    let mut out = vec![format!("{}, {} pushed, in {}.", plural(v.len(), "commit", "commits"), pushed, top(&by, 3))];
    if let Some(m) = v.first().map(|r| s(r, "message")).filter(|m| !m.is_empty()) {
        out.push(format!("The latest: {m}."));
    }
    out
}

const DEPLOYS: &str = "select coalesce(json_agg(x order by x.at desc), '[]') from (select status, coalesce(error, '') as error, \
    regexp_replace(coalesce(command, ''), '^.*ndexr ', '') as command, requested_at as at from deploy_requests \
    where requested_at > now() - make_interval(mins => :'mins'::int) limit 20) x";

fn deploys(v: Vec<Value>) -> Vec<String> {
    if v.is_empty() {
        return Vec::new();
    }
    let ok = v.iter().filter(|r| s(r, "status") == "ok").count();
    let failed = v.iter().filter(|r| s(r, "status") == "error").count();
    let mut line = format!("{}: {ok} finished", plural(v.len(), "deploy", "deploys"));
    if failed > 0 {
        line.push_str(&format!(", {failed} failed"));
    }
    let running = v.len() - ok - failed;
    if running > 0 {
        line.push_str(&format!(", {running} still going"));
    }
    line.push('.');
    vec![line]
}

const REQUESTS: &str = "select coalesce(json_agg(x order by x.at desc nulls last), '[]') from ( \
    select 'new' as what, c.domain, coalesce(nullif(u.username, ''), 'someone') as who, split_part(c.what_sucks, E'\\n', 1) as body, c.status, c.created_at as at \
      from complaints c left join ndexr_users u on lower(u.email) = lower(c.email) \
      where c.created_at > now() - make_interval(mins => :'mins'::int) \
    union all \
    select 'moved', c.domain, '', m.body, c.status, m.created_at from request_messages m join complaints c on c.id = m.complaint_id \
      where m.kind = 'status' and m.created_at > now() - make_interval(mins => :'mins'::int) \
    union all \
    select 'open', '', '', '', '', null::timestamptz from complaints where status in ('open', 'working')) x";

fn requests(v: Vec<Value>) -> Vec<String> {
    let new: Vec<&Value> = v.iter().filter(|r| s(r, "what") == "new").collect();
    let resolved = v.iter().filter(|r| s(r, "what") == "moved" && s(r, "body") == "resolved").count();
    let open = v.iter().filter(|r| s(r, "what") == "open").count();
    let mut out = Vec::new();
    if !new.is_empty() {
        let mut by: BTreeMap<String, usize> = BTreeMap::new();
        for r in &new {
            *by.entry(s(r, "who").to_owned()).or_default() += 1;
        }
        out.push(format!("{} came in, from {}.", plural(new.len(), "new request", "new requests"), top(&by, 3)));
        if let Some(r) = new.first() {
            let body: String = s(r, "body").chars().take(140).collect();
            if !body.is_empty() {
                out.push(format!("The newest, on {}: {body}.", s(r, "domain")));
            }
        }
    }
    if resolved > 0 {
        out.push(format!("{} resolved.", plural(resolved, "request", "requests")));
    }
    if !out.is_empty() || open > 0 {
        out.push(format!("{} open now.", plural(open, "request is", "requests are")));
    }
    out
}

const PEOPLE: &str = "select json_build_array(json_build_object( \
    'joined', (select coalesce(json_agg(coalesce(nullif(username, ''), email)), '[]') from ndexr_users where created_at > now() - make_interval(mins => :'mins'::int)), \
    'dms', (select count(*) from direct_messages where created_at > now() - make_interval(mins => :'mins'::int)), \
    'here', (select coalesce(json_agg(username order by username), '[]') from user_presence where username <> '' and last_seen > now() - interval '2 minutes')))";

fn people(v: Vec<Value>) -> Vec<String> {
    let Some(p) = v.first() else { return Vec::new() };
    let names = |k: &str| -> Vec<String> { p[k].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_owned)).collect() };
    let (joined, here) = (names("joined"), names("here"));
    let dms = p["dms"].as_i64().unwrap_or(0) as usize;
    let mut out = Vec::new();
    if !joined.is_empty() {
        out.push(format!("{} joined: {}.", plural(joined.len(), "new person", "new people"), and_list(&joined)));
    }
    if dms > 0 {
        out.push(format!("{} sent between people.", plural(dms, "chat message was", "chat messages were")));
    }
    match here.len() {
        0 => {}
        n if n <= 6 => out.push(format!("In Cove now: {}.", and_list(&here))),
        n => out.push(format!("{n} people are in Cove now.")),
    }
    out
}

const SITES: &str = "select coalesce(json_agg(x), '[]') from ( \
    select 'new' as what, domain from domain_registry where created_at > now() - make_interval(mins => :'mins'::int) \
    union all select 'published', domain from domain_registry where published_at > now() - make_interval(mins => :'mins'::int)) x";

fn sites(v: Vec<Value>) -> Vec<String> {
    let of = |w: &str| -> Vec<String> { v.iter().filter(|r| s(r, "what") == w).map(|r| s(r, "domain").to_owned()).collect() };
    let (new, published) = (of("new"), of("published"));
    let mut out = Vec::new();
    if !new.is_empty() {
        out.push(format!("{} registered: {}.", plural(new.len(), "new site", "new sites"), and_list(&new)));
    }
    if !published.is_empty() {
        out.push(format!("Published: {}.", and_list(&published)));
    }
    out
}

/// Cove's own releases (onprem/run.sh: one folder per published build).
fn releases(since: SystemTime) -> Vec<String> {
    let n = std::fs::read_dir("/usr/local/lib/cove/releases").into_iter().flatten().flatten().filter(|e| newer(&e.path(), since)).count();
    if n == 0 {
        return Vec::new();
    }
    vec![format!("Cove was released {}.", plural(n, "time", "times"))]
}

fn machines() -> Vec<String> {
    let bs = crate::host::boxes::list();
    if bs.is_empty() {
        return Vec::new();
    }
    let on: Vec<String> = bs.iter().filter(|b| b.up).map(|b| format!("{} ({})", b.server.trim_start_matches("four-box-"), b.state)).collect();
    if on.is_empty() {
        vec![format!("All {} build machines are off.", bs.len())]
    } else {
        vec![format!("Build machines on: {}.", and_list(&on))]
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn words_read_aloud() {
        assert_eq!(super::span_words(60), "hour");
        assert_eq!(super::span_words(240), "4 hours");
        assert_eq!(super::span_words(1440), "day");
        assert_eq!(super::and_list(&["a".into(), "b".into(), "c".into()]), "a, b and c");
        let mut m = std::collections::BTreeMap::new();
        for (k, v) in [("x", 1), ("y", 5), ("z", 2), ("w", 1)] {
            m.insert(k.to_owned(), v);
        }
        assert_eq!(super::top(&m, 2), "y (5), z (2) and 2 others");
    }
}
