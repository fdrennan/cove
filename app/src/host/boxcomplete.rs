//! Completion that knows `box::use` (operator, 2026-11-18: "file completion
//! in cove is not box:: aware and should be"). After `alias$` in an R file,
//! where the file says `box::use(alias = ...)`, `box::use(shiny)` or
//! `box::use(r / app)`, the list is what that module exports:
//!
//! - a local module (`r / app`, `./x`, `../y`): the names its file marks
//!   `#' @export`, found the way box finds it - beside the file, then up its
//!   folders (box.path is the project's root in every starter);
//! - a package (`shiny`, `dplyr`): the `export(...)` lines of its NAMESPACE,
//!   in the R libraries of the project's environment.
//!
//! Neither needs R running: R's own completion cannot see into an alias it
//! never attached, and a busy R answers nothing at all (the operator's
//! screenshot: `shiny$shinyA` offered only the file's own words).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use crate::api::Completion;

/// What `alias` names in `box::use(...)`.
#[derive(Debug, Clone, PartialEq)]
enum Target {
    Package(String),
    /// Path segments, as written: ["r", "app"], [".", "x"], ["..", "y"].
    Module(Vec<String>),
}

/// The completion for `alias$partial` at the cursor, when alias comes from
/// a box::use in `text` and its names can be read.
pub fn complete(path: &str, text: &str, line: &str, col: usize, env: &str) -> Option<Completion> {
    let before: String = line.chars().take(col).collect();
    let (alias, partial) = dollar_word(&before)?;
    let target = uses(text).into_iter().find(|(a, _)| *a == alias)?.1;
    let names = match &target {
        Target::Module(segs) => module_exports(&find_module(Path::new(path), segs)?),
        Target::Package(p) => package_exports(p, env)?,
    };
    let mut items: Vec<String> = names.into_iter().filter(|n| n.starts_with(&partial)).collect();
    items.sort();
    items.dedup();
    let what = match &target {
        Target::Package(p) => format!("{p} (box)"),
        Target::Module(s) => format!("{} (box module)", s.join("/")),
    };
    let detail = vec![what; items.len()];
    Some(Completion { start: col - partial.chars().count(), items, detail })
}

/// `alias$part` ending the text before the cursor.
fn dollar_word(before: &str) -> Option<(String, String)> {
    let part: String = before.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.').collect::<Vec<_>>().into_iter().rev().collect();
    let rest = &before[..before.len() - part.len()];
    let rest = rest.strip_suffix('$')?;
    let alias: String = rest.chars().rev().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.').collect::<Vec<_>>().into_iter().rev().collect();
    (!alias.is_empty() && !alias.starts_with(|c: char| c.is_ascii_digit())).then_some((alias, part))
}

/// Every `box::use(...)` in the file, as (alias, what it names).
fn uses(text: &str) -> Vec<(String, Target)> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find("box::use(") {
        let start = from + i + "box::use(".len();
        // To the matching ")", across lines.
        let (mut depth, mut end) = (1i32, start);
        for (k, c) in text[start..].char_indices() {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = start + k;
                        break;
                    }
                }
                _ => {}
            }
        }
        if end <= start {
            break;
        }
        for entry in split_top(&text[start..end]) {
            if let Some(u) = entry_target(&entry) {
                out.push(u);
            }
        }
        from = end;
    }
    out
}

/// The entries of a box::use, split at its own commas (not those inside an
/// attach list `[a, b]` or a call).
fn split_top(s: &str) -> Vec<String> {
    let (mut out, mut cur, mut depth) = (Vec::new(), String::new(), 0i32);
    for c in s.chars() {
        match c {
            '(' | '[' => depth += 1,
            ')' | ']' => depth -= 1,
            ',' if depth == 0 => {
                out.push(std::mem::take(&mut cur));
                continue;
            }
            _ => {}
        }
        cur.push(c);
    }
    out.push(cur);
    out.into_iter().map(|e| e.lines().map(|l| l.split('#').next().unwrap_or("")).collect::<Vec<_>>().join(" ").trim().to_owned()).filter(|e| !e.is_empty()).collect()
}

fn entry_target(entry: &str) -> Option<(String, Target)> {
    // The attach list is not the module's name.
    let entry = entry.split('[').next().unwrap_or(entry).trim();
    let (alias, path) = match entry.split_once('=') {
        Some((a, p)) => (Some(clean(a)), p.trim()),
        None => (None, entry),
    };
    let segs: Vec<String> = path.split('/').map(clean).filter(|s| !s.is_empty()).collect();
    let last = segs.last()?.clone();
    let target = if segs.len() == 1 && !last.starts_with('.') { Target::Package(last.clone()) } else { Target::Module(segs) };
    Some((alias.unwrap_or(last), target))
}

fn clean(s: &str) -> String {
    s.trim().trim_matches('`').trim().to_owned()
}

/// The module's file, as box looks for it: `<segs>.R` (or `.r`, or the
/// folder's `__init__.R`), relative to the file for `.`/`..`, else from the
/// file's folder up.
fn find_module(file: &Path, segs: &[String]) -> Option<PathBuf> {
    let here = file.parent()?;
    let tries = |base: &Path| -> Option<PathBuf> {
        let mut p = base.to_path_buf();
        for s in segs {
            p.push(s);
        }
        [p.with_extension("R"), p.with_extension("r"), p.join("__init__.R"), p.join("__init__.r")].into_iter().find(|c| c.is_file())
    };
    if segs.first().is_some_and(|s| s == "." || s == "..") {
        return tries(here);
    }
    here.ancestors().take(8).find_map(tries)
}

/// The names a module file marks `#' @export`: the assignment after the tag.
fn module_exports(file: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(file) else { return Vec::new() };
    let mut out = Vec::new();
    let mut tagged = false;
    for l in text.lines() {
        let t = l.trim();
        if t.starts_with("#'") {
            tagged |= t.contains("@export");
            continue;
        }
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        if tagged {
            let name: String = t.chars().take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.').collect();
            let rest = t[name.len()..].trim_start();
            if !name.is_empty() && (rest.starts_with("<-") || rest.starts_with('=')) {
                out.push(name);
            }
        }
        tagged = false;
    }
    out
}

/// A package's exports from its NAMESPACE, in the environment's libraries.
fn package_exports(pkg: &str, env: &str) -> Option<Vec<String>> {
    let libs = lib_paths(env)?;
    let ns = libs.iter().map(|l| l.join(pkg).join("NAMESPACE")).find(|p| p.is_file())?;
    let text = std::fs::read_to_string(ns).ok()?;
    Some(namespace_exports(&text))
}

fn namespace_exports(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for l in text.lines() {
        if let Some(inner) = l.trim().strip_prefix("export(").and_then(|r| r.strip_suffix(')')) {
            out.extend(inner.split(',').map(|n| n.trim().trim_matches('"').trim_matches('`').to_owned()).filter(|n| !n.is_empty()));
        }
    }
    out
}

/// The R libraries of `env` (its .libPaths()), asked of R once per
/// environment on a thread of its own (about 7 s), then kept. None until
/// then: the list falls back to R's own completion and the file's words.
fn lib_paths(env: &str) -> Option<Vec<PathBuf>> {
    static KNOWN: Mutex<Option<HashMap<String, Option<Vec<PathBuf>>>>> = Mutex::new(None);
    let mut known = KNOWN.lock().unwrap();
    let map = known.get_or_insert_with(HashMap::new);
    if let Some(v) = map.get(env) {
        return v.clone();
    }
    map.insert(env.to_owned(), None);
    let env = env.to_owned();
    std::thread::spawn(move || {
        let prefix = crate::host::kernel::module_prefix(&env).unwrap_or_default();
        let out = crate::host::runner::capture_within(&format!("{prefix}Rscript -e 'cat(.libPaths(), sep=\"\\n\")'"), Duration::from_secs(60)).unwrap_or_default();
        let libs: Vec<PathBuf> = out.lines().map(str::trim).filter(|l| l.starts_with('/')).map(PathBuf::from).collect();
        let mut known = KNOWN.lock().unwrap();
        known.get_or_insert_with(HashMap::new).insert(env, (!libs.is_empty()).then_some(libs));
    });
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn box_use_names_what_each_alias_is() {
        let text = "box::use(\n  shiny[NS, tags],\n  d = dplyr,\n  r / app,\n  r / log[log_info],\n  `my.mod` = ./helpers,\n)\nbox::use(ggplot2)";
        let u = uses(text);
        assert!(u.contains(&("shiny".into(), Target::Package("shiny".into()))), "{u:?}");
        assert!(u.contains(&("d".into(), Target::Package("dplyr".into()))));
        assert!(u.contains(&("app".into(), Target::Module(vec!["r".into(), "app".into()]))));
        assert!(u.contains(&("log".into(), Target::Module(vec!["r".into(), "log".into()]))));
        assert!(u.contains(&("my.mod".into(), Target::Module(vec![".".into(), "helpers".into()]))));
        assert!(u.contains(&("ggplot2".into(), Target::Package("ggplot2".into()))));
    }

    #[test]
    fn the_word_after_the_dollar() {
        assert_eq!(dollar_word("  shiny$shinyA"), Some(("shiny".into(), "shinyA".into())));
        assert_eq!(dollar_word("x <- app$"), Some(("app".into(), "".into())));
        assert_eq!(dollar_word("shinyA"), None);
    }

    #[test]
    fn a_local_module_completes_from_its_exports() {
        let d = std::env::temp_dir().join(format!("cove-box-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("r")).unwrap();
        std::fs::write(d.join("r/app.R"), "#' The ui\n#' @export\nui <- function(id) NULL\n\nhelper <- 1\n#' @export\nserver = function(id) NULL\n").unwrap();
        let file = d.join("run.r");
        std::fs::write(&file, "").unwrap();
        let text = "box::use(r / app)\napp$";
        let c = complete(file.to_str().unwrap(), text, "app$se", 6, "").unwrap();
        assert_eq!(c.items, vec!["server"]);
        assert_eq!(c.start, 4);
        let all = complete(file.to_str().unwrap(), text, "app$", 4, "").unwrap();
        assert_eq!(all.items, vec!["server", "ui"], "not helper: it is not exported");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_namespace_lists_its_exports() {
        let ns = "# Generated by roxygen2\nexport(\"conditionStackTrace<-\")\nexport(NS)\nexport(shinyApp)\nimport(htmltools)\n";
        assert_eq!(namespace_exports(ns), vec!["conditionStackTrace<-", "NS", "shinyApp"]);
    }
}
