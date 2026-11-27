//! Git for the Git window (ui/ide/gitview.rs), on the machine: what changed
//! where (a version's files, or the index's and the files' halves of the
//! changes not saved yet), one file's diff, and patches applied to the index
//! or the files - stage, unstage, revert, apply. Every command runs through
//! `platform_git`, as the account that owns the autosaved repositories when
//! this is `cove multiuser`. Parsing a patch and laying a diff out are not
//! here: they are project/diff, shared with the UI.

use std::path::Path;

use crate::api::{shell_quote, GitFile};
use crate::host::platform_git;

/// The tree every repository starts from: what a first version is compared with.
const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// A version id as git takes it, or "" for the changes not saved yet.
fn git_rev_ok(sha: &str) -> bool {
    sha.is_empty() || ((7..=40).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit()))
}

/// A path inside the project: relative, no way up or out, never an option.
fn git_path_ok(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.starts_with('-') && !p.contains('\0') && !p.split('/').any(|c| c == "..")
}

/// Shell that sets `$a` and `$b` for `git diff $a $b`: a version against its
/// first parent (the empty tree for a first version), or for "" the last
/// saved version against the files as they are.
fn git_pair(sha: &str) -> String {
    if sha.is_empty() {
        format!("a=$($g rev-parse -q --verify HEAD || echo {EMPTY_TREE}); b=")
    } else {
        format!("$g cat-file -e {sha}^{{commit}} || {{ echo 'no such version' >&2; exit 1; }}; a=$($g rev-parse -q --verify {sha}^1 || echo {EMPTY_TREE}); b={sha}")
    }
}

const GIT: &str = "g='git -c safe.directory=* -c core.quotepath=off'; ";

/// The files version `sha` changed (Query::GitChanges), as the Git window
/// lists them; "" the changes not saved yet, files git does not track
/// included. A project without a repository has none.
/// `path` as it was at version `sha`.
pub fn file_at(dir: &Path, sha: &str, path: &str) -> Result<String, String> {
    if sha.is_empty() || !git_rev_ok(sha) {
        return Err("not a version".into());
    }
    if !git_path_ok(path) {
        return Err("not a path in the project".into());
    }
    platform_git(dir, &format!("{GIT}$g show {}:{}", crate::api::shell_quote(sha), crate::api::shell_quote(path)), &[]).map_err(|_| format!("{path} is not in that version"))
}

pub fn changes_at(dir: &Path, sha: &str) -> Result<Vec<crate::api::GitFile>, String> {
    if !git_rev_ok(sha) {
        return Err("not a version".into());
    }
    if !dir.join(".git").exists() {
        return Ok(Vec::new());
    }
    let pair = git_pair(sha);
    let names = platform_git(dir, &format!("{GIT}{pair}; $g diff --no-color --no-ext-diff -M --name-status -z $a $b"), &[])?;
    let counts = git_numstat(&platform_git(dir, &format!("{GIT}{pair}; $g diff --no-color --no-ext-diff -M --numstat -z $a $b"), &[])?);
    let mut files = git_name_status(&names);
    for f in &mut files {
        if let Some(&(a, r)) = counts.get(&f.path) {
            (f.added, f.removed) = (a, r);
        }
    }
    if sha.is_empty() {
        let new = platform_git(dir, &format!("{GIT}$g ls-files --others --exclude-standard -z"), &[])?;
        for p in new.split('\0').filter(|p| !p.is_empty()) {
            // All its lines are new; a binary (or a huge) file has no count.
            let lines = std::fs::read(dir.join(p))
                .ok()
                .filter(|b| b.len() < 4 << 20 && !b.contains(&0))
                .map_or(-1, |b| b.iter().filter(|c| **c == b'\n').count() as i64 + i64::from(b.last().is_some_and(|c| *c != b'\n')));
            files.push(crate::api::GitFile { path: p.to_owned(), status: "?".into(), added: lines, removed: if lines < 0 { -1 } else { 0 }, ..Default::default() });
        }
    }
    files.truncate(5000);
    Ok(files)
}

/// `git diff --name-status -z`: a letter, then one path - two for a rename
/// or copy (old, new).
fn git_name_status(z: &str) -> Vec<crate::api::GitFile> {
    let mut out = Vec::new();
    let mut t = z.split('\0').filter(|s| !s.is_empty());
    while let Some(st) = t.next() {
        let status = st.chars().next().unwrap_or('M').to_string();
        let f = if matches!(status.as_str(), "R" | "C") {
            let (Some(old), Some(new)) = (t.next(), t.next()) else { break };
            crate::api::GitFile { path: new.to_owned(), old: old.to_owned(), status, ..Default::default() }
        } else {
            let Some(p) = t.next() else { break };
            crate::api::GitFile { path: p.to_owned(), status, ..Default::default() }
        };
        out.push(f);
    }
    out
}

/// `git diff --numstat -z`: "added TAB removed TAB path", or for a rename
/// "added TAB removed TAB" then old and new as their own fields; "-" counts
/// are a binary file's (-1 here). Keyed by the path as it is now.
fn git_numstat(z: &str) -> std::collections::HashMap<String, (i64, i64)> {
    let mut out = std::collections::HashMap::new();
    let mut t = z.split('\0');
    while let Some(rec) = t.next() {
        let mut f = rec.splitn(3, '\t');
        let (Some(a), Some(r), Some(p)) = (f.next(), f.next(), f.next()) else { continue };
        let path = if p.is_empty() {
            let _old = t.next();
            match t.next() {
                Some(new) => new.to_owned(),
                None => break,
            }
        } else {
            p.to_owned()
        };
        let n = |x: &str| x.parse::<i64>().unwrap_or(-1);
        out.insert(path, (n(a), n(r)));
    }
    out
}

/// One file's unified diff in version `sha` (Query::GitDiff); "" against the
/// last saved version, where a file git does not track yet is all new.
/// `old` is its name before a rename; `area` "staged" or "unstaged" narrows
/// "" to the index's half or the files' half; `full` keeps the whole file as
/// context. Cut at 1 MB.
pub fn diff_at(dir: &Path, sha: &str, path: &str, old: &str, area: &str, full: bool) -> Result<String, String> {
    if !git_rev_ok(sha) {
        return Err("not a version".into());
    }
    if !git_path_ok(path) || !(old.is_empty() || git_path_ok(old)) {
        return Err("not a file of this project".into());
    }
    if !dir.join(".git").exists() {
        return Err("this project has no versions yet".into());
    }
    let (p, o) = (shell_quote(path), if old.is_empty() { String::new() } else { shell_quote(old) });
    // git diff --no-index exits 1 when the files differ, which they do.
    // The whole file as context (the viewer folds what did not change and
    // opens it on request), else git's three lines.
    let u = if full { " -U1000000" } else { "" };
    let untracked = if sha.is_empty() {
        format!("if [ -e {p} ] && ! $g ls-files --error-unmatch -- {p} >/dev/null 2>&1; then $g diff --no-color --no-ext-diff{u} --no-index -- /dev/null {p}; [ $? -le 1 ]; exit; fi; ")
    } else {
        String::new()
    };
    let script = match area {
        // What is staged: the last saved version against the index.
        "staged" => format!("{GIT}a=$($g rev-parse -q --verify HEAD || echo {EMPTY_TREE}); $g diff --no-color --no-ext-diff -M{u} --cached $a -- {o} {p}"),
        // What is not staged: the index against the files as they are.
        "unstaged" | "untracked" => format!("{GIT}{untracked}$g diff --no-color --no-ext-diff -M{u} -- {o} {p}"),
        _ => format!("{GIT}{untracked}{}; $g diff --no-color --no-ext-diff -M{u} $a $b -- {o} {p}", git_pair(sha)),
    };
    if !matches!(area, "" | "staged" | "unstaged" | "untracked") {
        return Err("not a part of the repository".into());
    }
    if !sha.is_empty() && !area.is_empty() {
        return Err("staged and unstaged are of the changes not saved yet".into());
    }
    let mut out = platform_git(dir, &script, &[])?;
    if out.len() > 1 << 20 {
        let mut cut = 1 << 20;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
        out.push_str("\n\\ cut at 1 MB\n");
    }
    Ok(out)
}


/// The changes not saved yet in their three parts (Query::GitStatus): staged
/// (the index against the last version), unstaged (the files against the
/// index) and untracked, each file with `area` saying which. A file can be in
/// two: part staged, part not.
pub fn status_at(dir: &Path) -> Result<Vec<GitFile>, String> {
    if !dir.join(".git").exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for (area, range) in [("staged", format!("--cached $($g rev-parse -q --verify HEAD || echo {EMPTY_TREE})")), ("unstaged", String::new())] {
        let names = platform_git(dir, &format!("{GIT}$g diff --no-color --no-ext-diff -M --name-status -z {range}"), &[])?;
        let counts = git_numstat(&platform_git(dir, &format!("{GIT}$g diff --no-color --no-ext-diff -M --numstat -z {range}"), &[])?);
        for mut f in git_name_status(&names) {
            if let Some(&(a, r)) = counts.get(&f.path) {
                (f.added, f.removed) = (a, r);
            }
            f.area = area.into();
            out.push(f);
        }
    }
    let new = platform_git(dir, &format!("{GIT}$g ls-files --others --exclude-standard -z"), &[])?;
    for p in new.split('\0').filter(|p| !p.is_empty()) {
        let lines = std::fs::read(dir.join(p))
            .ok()
            .filter(|b| b.len() < 4 << 20 && !b.contains(&0))
            .map_or(-1, |b| b.iter().filter(|c| **c == b'\n').count() as i64 + i64::from(b.last().is_some_and(|c| *c != b'\n')));
        out.push(GitFile { path: p.to_owned(), status: "?".into(), added: lines, removed: if lines < 0 { -1 } else { 0 }, area: "untracked".into(), ..Default::default() });
    }
    out.truncate(5000);
    Ok(out)
}

/// A patch put to work (Query::GitPatch): "stage" into the index, "unstage"
/// out of it, "revert" it out of the files, "apply" it to the files, or
/// "check" whether it would apply. git apply refuses paths that leave the
/// repository; `--recount` lets a hand-cut hunk keep imperfect counts.
pub fn patch_at(dir: &Path, action: &str, patch: &str) -> Result<String, String> {
    let (flags, done) = match action {
        "stage" => ("--cached", "staged"),
        "unstage" => ("--cached -R", "unstaged"),
        "revert" => ("-R", "reverted"),
        "apply" => ("", "applied"),
        "check" => ("--check", "applies cleanly"),
        _ => return Err(format!("{action}: not something a patch does here")),
    };
    if !dir.join(".git").exists() {
        return Err("this project has no versions yet".into());
    }
    let body = patch.trim_end_matches(['\n', '\r']);
    if body.trim().is_empty() {
        return Err("the patch is empty".into());
    }
    if body.len() > 8 << 20 {
        return Err("the patch is over 8 MB".into());
    }
    // A quoted here-document: the patch goes in as it is, and its end
    // marker is one the patch itself does not contain.
    let mut end = String::from("COVE_PATCH_END");
    while body.contains(&end) {
        end.push('_');
    }
    let script = format!("{GIT}$g apply {flags} --recount --whitespace=nowarn - <<'{end}'\n{body}\n{end}\n");
    platform_git(dir, &script, &[]).map(|_| done.to_owned())
}

#[cfg(test)]
mod tests {
    /// The Git window's server half on a real repository: a first version
    /// against the empty tree, the unsaved changes with a file git does not
    /// track yet, a rename, and what it refuses.
    #[test]
    fn git_window_reads_versions_files_and_diffs() {
        use std::process::Command;
        let d = std::env::temp_dir().join(format!("cove-git-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let git = |args: &[&str]| assert!(Command::new("git").current_dir(&d).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap().status.success(), "git {args:?}");
        git(&["init", "-q"]);
        std::fs::write(d.join("a.txt"), "one\ntwo\n").unwrap();
        std::fs::write(d.join("old name.txt"), "same\nlines\nhere\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        let sha = crate::host::versions_at(&d).unwrap()[0].sha.clone();
        let first = super::changes_at(&d, &sha).unwrap();
        assert!(first.iter().any(|f| f.path == "a.txt" && f.status == "A" && (f.added, f.removed) == (2, 0)), "{first:?}");

        std::fs::write(d.join("a.txt"), "one\nTWO\n").unwrap();
        std::fs::write(d.join("b.txt"), "hello\n").unwrap();
        git(&["mv", "old name.txt", "new name.txt"]);
        let now = super::changes_at(&d, "").unwrap();
        assert!(now.iter().any(|f| f.path == "a.txt" && f.status == "M" && (f.added, f.removed) == (1, 1)), "{now:?}");
        assert!(now.iter().any(|f| f.path == "b.txt" && f.status == "?" && f.added == 1), "{now:?}");
        assert!(now.iter().any(|f| f.path == "new name.txt" && f.old == "old name.txt" && f.status == "R"), "{now:?}");

        let a = super::diff_at(&d, "", "a.txt", "", "", false).unwrap();
        assert!(a.contains("\n-two") && a.contains("\n+TWO"), "{a}");
        let b = super::diff_at(&d, "", "b.txt", "", "", false).unwrap();
        assert!(b.contains("+hello"), "{b}");
        let r = super::diff_at(&d, "", "new name.txt", "old name.txt", "", false).unwrap();
        assert!(r.contains("rename from old name.txt"), "{r}");
        let f = super::diff_at(&d, &sha, "a.txt", "", "", false).unwrap();
        assert!(f.contains("+one") && f.contains("+two"), "{f}");

        assert!(super::diff_at(&d, "", "../etc/passwd", "", "", false).is_err());
        assert!(super::diff_at(&d, "", "-p", "", "", false).is_err());
        assert!(super::changes_at(&d, "zz; rm -rf /").is_err());
        assert!(super::changes_at(&d, "0000000").is_err(), "no such version");
        let _ = std::fs::remove_dir_all(&d);
    }



    /// Stage, unstage and revert one hunk of a real repository, as the Git
    /// window does: the index and the files move by exactly that hunk.
    #[test]
    fn a_hunk_stages_unstages_and_reverts() {
        use std::process::Command;
        let d = std::env::temp_dir().join(format!("cove-git-patch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let git = |args: &[&str]| -> String {
            let o = Command::new("git").current_dir(&d).args(["-c", "user.name=t", "-c", "user.email=t@t"]).args(args).output().unwrap();
            assert!(o.status.success(), "git {args:?}");
            String::from_utf8_lossy(&o.stdout).into_owned()
        };
        git(&["init", "-q"]);
        let lines: Vec<String> = (1..=20).map(|i| format!("line {i}")).collect();
        std::fs::write(d.join("f.txt"), lines.join("\n") + "\n").unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "first"]);
        let mut edited = lines.clone();
        edited[1] = "LINE 2".into();
        edited[17] = "LINE 18".into();
        std::fs::write(d.join("f.txt"), edited.join("\n") + "\n").unwrap();

        let st = super::status_at(&d).unwrap();
        assert!(st.iter().any(|f| f.path == "f.txt" && f.area == "unstaged" && (f.added, f.removed) == (2, 2)), "{st:?}");
        // The first change alone, cut from the whole-file diff.
        let full = super::diff_at(&d, "", "f.txt", "", "unstaged", true).unwrap();
        let file = crate::project::diff::patch::parse(&full).remove(0);
        let model = crate::project::diff::model::build(&file);
        assert_eq!(model.hunks.len(), 2, "two changes far apart are two hunks");
        let first = crate::project::diff::patch::write(&file, &model.hunks[..1]);
        super::patch_at(&d, "stage", &first).unwrap();
        let cached = git(&["diff", "--cached"]);
        assert!(cached.contains("+LINE 2") && !cached.contains("LINE 18"), "{cached}");
        let st = super::status_at(&d).unwrap();
        assert!(st.iter().any(|f| f.area == "staged") && st.iter().any(|f| f.area == "unstaged"), "part staged, part not: {st:?}");
        super::patch_at(&d, "unstage", &first).unwrap();
        assert_eq!(git(&["diff", "--cached"]), "");
        super::patch_at(&d, "revert", &first).unwrap();
        let now = std::fs::read_to_string(d.join("f.txt")).unwrap();
        assert!(now.contains("line 2\n") && now.contains("LINE 18"), "{now}");
        // Reverted, the hunk would apply to the files again.
        assert_eq!(super::patch_at(&d, "check", &first).unwrap(), "applies cleanly");
        assert!(super::patch_at(&d, "check", &crate::project::diff::patch::write(&file, &model.hunks[1..])).is_err(), "the second is in the files already");
        assert!(super::patch_at(&d, "explode", &first).is_err());
        assert!(super::patch_at(&d, "apply", "").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }
}

#[cfg(test)]
mod file_at_tests {
    use super::*;

    /// A kept thread read back from the version it was saved with; a path
    /// leaving the project, or a file not in it, is refused.
    #[test]
    fn a_file_reads_back_as_it_was_at_a_version() {
        let d = std::env::temp_dir().join(format!("cove-file-at-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join(".cove/agent")).unwrap();
        std::fs::write(d.join(".cove/agent/thread.md"), "# kept\n").unwrap();
        let q = crate::api::shell_quote(&d.display().to_string());
        let ok = std::process::Command::new("bash").arg("-c").arg(format!("cd {q} && git init -q && git -c user.email=t@t -c user.name=t add -A && git -c user.email=t@t -c user.name=t commit -qm one && git rev-parse HEAD")).output().unwrap();
        let sha = String::from_utf8_lossy(&ok.stdout).trim().to_owned();
        std::fs::write(d.join(".cove/agent/thread.md"), "# changed since\n").unwrap();
        assert_eq!(file_at(&d, &sha, ".cove/agent/thread.md").unwrap().trim(), "# kept");
        assert!(file_at(&d, &sha, "../etc/passwd").is_err());
        assert!(file_at(&d, &sha, "missing.md").is_err());
        assert!(file_at(&d, "", ".cove/agent/thread.md").is_err(), "the working tree is not a version");
        let _ = std::fs::remove_dir_all(&d);
    }
}
