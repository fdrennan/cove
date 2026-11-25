//! Where a build machine is, read off the end of its log (operator,
//! 2026-10-18: "a build machine right now should be more clear, we know all
//! of the different stages of the builds and so can be more specific on the
//! stage and what is happening based on what we can derive from the text,
//! high level").
//!
//! A run is `ndexr stack up`'s nine steps, each announced on a line of its
//! own - `[ndexr up] step 6/9 suite — …` - and inside a step the tools say
//! what they are on:
//!
//!   suite      `* [build] GDAL-3.10.0-foss-2024a.eb` (also [rebuild], [retry]),
//!              cmake's `[ 42%] Building CXX …`, R's `* installing *source* package 'x'`
//!   packages   `pkgs build Python/3.12.14 [all] → …`, then
//!              `* now installing torch (Python) - 2 of 7`, `* done: …`
//!   publish    `publishing /opt/ndexr/stack/…`, `published s3://…`
//!
//! `read` takes the tail and answers in words: the step, what it is doing,
//! how far through. Nothing here guesses beyond those lines.

/// The nine steps, in order: (log name, what it means).
pub const STEPS: [(&str, &str); 9] = [
    ("kit", "Fetching the build kit"),
    ("cache", "Restoring the build cache"),
    ("prefix", "Building Gentoo Prefix - the base system"),
    ("publish-base", "Publishing the base system"),
    ("ndexr-cli", "Building the ndexr CLI"),
    ("suite", "Compiling the EasyBuild suite"),
    ("cove", "Building Cove"),
    ("publish", "Publishing the stack"),
    ("packages", "Installing R and Python packages"),
];

/// What a machine is doing, as its log says it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stage {
    /// Step n of m (1-based) and its name, when the log has reached one.
    pub step: Option<(usize, usize, String)>,
    /// The step in words: "Installing R and Python packages".
    pub headline: String,
    /// What it is on right now: "torch (Python) for Python/3.12.14".
    pub doing: String,
    /// How far through what it is doing: (n, of).
    pub progress: Option<(u64, u64)>,
    /// The log ends in an error (a stopped machine, a failed step).
    pub error: Option<String>,
}

impl Stage {
    /// The step's place in STEPS (0-based), for the track of nine.
    pub fn index(&self) -> Option<usize> {
        let (_, _, name) = self.step.as_ref()?;
        STEPS.iter().position(|(n, _)| n == name || (*n == "cove" && name == "ndexr-desktop"))
    }
}

fn step_words(name: &str) -> String {
    let name = if name == "ndexr-desktop" { "cove" } else { name };
    STEPS.iter().find(|(n, _)| *n == name).map_or_else(|| format!("Step: {name}"), |(_, w)| (*w).to_owned())
}

/// `[ndexr up] step 6/9 suite — …` → (6, 9, "suite").
fn parse_step(line: &str) -> Option<(usize, usize, String)> {
    let rest = line.trim().strip_prefix("[ndexr up] step ")?;
    let (frac, rest) = rest.split_once(' ')?;
    let (n, m) = frac.split_once('/')?;
    let name = rest.split_whitespace().next()?.to_owned();
    Some((n.parse().ok()?, m.parse().ok()?, name))
}

/// `* now installing torch (Python) - 2 of 7` → ("torch", "Python", 2, 7).
fn parse_installing(line: &str) -> Option<(String, String, u64, u64)> {
    let rest = line.trim().strip_prefix("* now installing ")?;
    let (name, rest) = rest.split_once(" (")?;
    let (lang, rest) = rest.split_once(')')?;
    let rest = rest.trim().trim_start_matches('-').trim();
    let (n, m) = rest.split_once(" of ")?;
    Some((name.trim().to_owned(), lang.to_owned(), n.trim().parse().ok()?, m.trim().parse().ok()?))
}

/// `* [build] GDAL-3.10.0-foss-2024a.eb` → "GDAL-3.10.0-foss-2024a".
fn parse_eb(line: &str) -> Option<String> {
    let t = line.trim();
    let rest = ["* [build] ", "* [rebuild] ", "* [retry] "].iter().find_map(|p| t.strip_prefix(p))?;
    Some(rest.split_whitespace().next()?.trim_end_matches(".eb").to_owned())
}

/// Read the end of a log (oldest line first).
pub fn read(text: &str) -> Stage {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let mut s = Stage::default();
    // The step: the last announcement anywhere in the tail.
    let step_at = lines.iter().rposition(|l| parse_step(l).is_some());
    if let Some(k) = step_at {
        s.step = parse_step(lines[k]);
    }
    let since = step_at.map_or(0, |k| k + 1);
    let tail = &lines[since..];
    // The last thing the tools said, newest first.
    let mut set: Option<String> = None;
    for l in tail.iter().rev() {
        let t = l.trim();
        if s.doing.is_empty() {
            if let Some((name, lang, n, m)) = parse_installing(t) {
                s.doing = format!("{name} ({lang})");
                s.progress = Some((n, m));
            } else if let Some(eb) = parse_eb(t) {
                s.doing = format!("compiling {eb}");
            } else if let Some(p) = t.strip_prefix("* installing *source* package ") {
                s.doing = format!("compiling R package {}", p.trim_matches(|c: char| "'‘’ .".contains(c)));
            } else if let Some(p) = t.strip_prefix("publishing ") {
                s.doing = format!("uploading {}", p.split_whitespace().next().unwrap_or(p));
            } else if let Some(p) = t.strip_prefix("published ") {
                s.doing = format!("uploaded to {}", p.split_whitespace().next().unwrap_or(p));
            }
            // cmake inside a compile: how far it is through.
            if s.progress.is_none() && t.starts_with('[') {
                if let Some(pct) = t.trim_start_matches('[').split('%').next().and_then(|p| p.trim().parse::<u64>().ok()) {
                    s.progress = Some((pct, 100));
                }
            }
        }
        if set.is_none() {
            if let Some(p) = t.strip_prefix("pkgs build ") {
                set = p.split_whitespace().next().map(str::to_owned);
            }
        }
    }
    if let (Some(set), false) = (&set, s.doing.is_empty()) {
        if s.doing.ends_with(')') {
            s.doing = format!("{} for {set}", s.doing);
        }
    }
    // The last line an error: say so, in its own words.
    if let Some(last) = lines.last() {
        let t = last.trim();
        if t.starts_with("Error:") || t.starts_with("ERROR") || t.contains("FAILED") {
            s.error = Some(t.trim_start_matches("Error:").trim().to_owned());
        }
    }
    s.headline = match &s.step {
        Some((_, _, name)) => step_words(name),
        None if s.error.is_some() => "Not running".into(),
        None => "Starting".into(),
    };
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_package_lane_reads_as_its_step_set_and_package() {
        let log = "[ndexr up] step 8/9 publish — 2026-10-18T00:00:00Z\npublishing /opt/ndexr/stack/x86_64-pc-linux-gnu (x)\n\
                   [ndexr up] step 9/9 packages — 2026-10-18T01:00:00Z\n  pkgs build Python/3.12.14 [all] → /opt/x · 12 CPUs\n\
                   * installing 7 record(s), Ncpus=12\n* now installing delta-spark (Python) - 1 of 7\n  [ok] Python   delta-spark\n\
                   * now installing torch (Python) - 2 of 7\n";
        let s = read(log);
        assert_eq!(s.step, Some((9, 9, "packages".into())));
        assert_eq!(s.index(), Some(8));
        assert_eq!(s.headline, "Installing R and Python packages");
        assert_eq!(s.doing, "torch (Python) for Python/3.12.14");
        assert_eq!(s.progress, Some((2, 7)));
        assert!(s.error.is_none());
    }

    #[test]
    fn the_suite_says_what_it_compiles_and_how_far() {
        let log = "[ndexr up] step 6/9 suite — x\n* [skip] R-4.6.1-foss-2024a.eb\n* [build] GDAL-3.10.0-foss-2024a.eb\n[ 41%] Building CXX object gdal.cpp.o\n[ 42%] Building CXX object ogr.cpp.o\n";
        let s = read(log);
        assert_eq!(s.headline, "Compiling the EasyBuild suite");
        assert_eq!(s.doing, "compiling GDAL-3.10.0-foss-2024a");
        assert_eq!(s.progress, Some((42, 100)));
    }

    #[test]
    fn a_stopped_machine_says_so() {
        let s = read("Error: 'four-box-gpu' (i-0b4) is stopped with no public IP — nothing to read\n");
        assert_eq!(s.headline, "Not running");
        assert!(s.error.as_deref().unwrap().contains("is stopped"));
        assert_eq!(read("").headline, "Starting");
        assert_eq!(read("[ndexr up] step 7/9 ndexr-desktop — x\n").index(), Some(6));
    }
}
