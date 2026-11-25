//! Unified diffs - `git diff`'s output, or a patch a person brings - as
//! files, hunks and lines, and back into text `git apply` takes.

/// What a line of a hunk is.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Same,
    Add,
    Del,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
    /// Followed by "\ No newline at end of file".
    pub no_eol: bool,
}

/// One `@@ -old_start,old_len +new_start,new_len @@ section` and its lines.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Hunk {
    pub old_start: u32,
    pub old_len: u32,
    pub new_start: u32,
    pub new_len: u32,
    /// What git names after the second @@: the enclosing function, often.
    pub section: String,
    pub lines: Vec<Line>,
}

impl Hunk {
    /// A hunk of `lines` starting at old line `old_from` and new line
    /// `new_from` (1-based, the first line's numbers), its counts taken from
    /// the lines - what a hunk cut out of a bigger one needs.
    pub fn of_lines(old_from: u32, new_from: u32, section: &str, lines: Vec<Line>) -> Hunk {
        let old_len = lines.iter().filter(|l| l.kind != Kind::Add).count() as u32;
        let new_len = lines.iter().filter(|l| l.kind != Kind::Del).count() as u32;
        // git's convention: an empty side starts at the line before it.
        let old_start = if old_len == 0 { old_from.saturating_sub(1) } else { old_from };
        let new_start = if new_len == 0 { new_from.saturating_sub(1) } else { new_from };
        Hunk { old_start, old_len, new_start, new_len, section: section.to_owned(), lines }
    }
}

/// One file's part of a diff.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct FilePatch {
    /// Every line before the first hunk (diff --git, index, modes, ---, +++),
    /// as given: written back as it is.
    pub header: Vec<String>,
    /// Its name before and after ("" for /dev/null: new, or deleted).
    pub old: String,
    pub new: String,
    /// What the header says beyond names: new file, deleted, renamed, mode.
    pub notes: Vec<String>,
    pub binary: bool,
    pub hunks: Vec<Hunk>,
}

impl FilePatch {
    /// The name a person knows it by: the new one, or the old when deleted.
    pub fn name(&self) -> &str {
        if self.new.is_empty() {
            &self.old
        } else {
            &self.new
        }
    }
}

/// `a/x` or `b/x` as `x`; `/dev/null` as "". A quoted name (git quotes
/// unusual ones) loses its quotes.
fn side_name(s: &str) -> String {
    let s = s.split('\t').next().unwrap_or("").trim();
    let s = s.strip_prefix('"').and_then(|x| x.strip_suffix('"')).unwrap_or(s);
    if s == "/dev/null" {
        return String::new();
    }
    s.strip_prefix("a/").or_else(|| s.strip_prefix("b/")).unwrap_or(s).to_owned()
}

/// `@@ -3,4 +3,5 @@ fn f` → (3, 4, 3, 5, "fn f"); a missing count is 1.
fn hunk_head(l: &str) -> Option<(u32, u32, u32, u32, String)> {
    let rest = l.strip_prefix("@@ ")?;
    let (ranges, section) = rest.split_once(" @@").unwrap_or((rest, ""));
    let mut it = ranges.split_whitespace();
    let range = |w: &str| -> Option<(u32, u32)> {
        let (a, b) = w.split_once(',').unwrap_or((w, "1"));
        Some((a.parse().ok()?, b.parse().ok()?))
    };
    let (os, ol) = range(it.next()?.strip_prefix('-')?)?;
    let (ns, nl) = range(it.next()?.strip_prefix('+')?)?;
    Some((os, ol, ns, nl, section.trim().to_owned()))
}

/// A diff of any number of files. Lines that are none of a diff's are let go;
/// a hunk ends when its counts are met.
pub fn parse(text: &str) -> Vec<FilePatch> {
    let mut files: Vec<FilePatch> = Vec::new();
    // Lines still owed by the hunk being read: (old, new).
    let mut owed: Option<(u32, u32)> = None;
    for raw in text.split('\n') {
        let l = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some((o, n)) = owed {
            let first = l.chars().next();
            if first == Some('\\') {
                mark_no_eol(&mut files);
                continue;
            }
            // An empty line is an empty context line some tools trim.
            let kind = match first {
                Some('+') => Some(Kind::Add),
                Some('-') => Some(Kind::Del),
                Some(' ') | None => Some(Kind::Same),
                _ => None,
            };
            if let (true, Some(kind)) = (o > 0 || n > 0, kind) {
                let h = files.last_mut().and_then(|f| f.hunks.last_mut()).expect("a hunk is being read");
                h.lines.push(Line { kind, text: l.get(1..).unwrap_or("").to_owned(), no_eol: false });
                owed = Some(match kind {
                    Kind::Add => (o, n.saturating_sub(1)),
                    Kind::Del => (o.saturating_sub(1), n),
                    Kind::Same => (o.saturating_sub(1), n.saturating_sub(1)),
                });
                continue;
            }
            owed = None;
        }
        if l.starts_with("diff --git ") {
            files.push(FilePatch { header: vec![l.to_owned()], ..Default::default() });
            continue;
        }
        if let Some((os, ol, ns, nl, section)) = hunk_head(l) {
            if files.is_empty() {
                files.push(FilePatch::default());
            }
            let f = files.last_mut().unwrap();
            f.hunks.push(Hunk { old_start: os, old_len: ol, new_start: ns, new_len: nl, section, lines: Vec::new() });
            owed = Some((ol, nl));
            continue;
        }
        if let Some(rest) = l.strip_prefix("--- ") {
            // A plain patch (no diff --git line), or a second file in one.
            if files.last().is_none_or(|f| !f.hunks.is_empty() || f.header.iter().any(|h| h.starts_with("--- "))) {
                files.push(FilePatch::default());
            }
            let f = files.last_mut().unwrap();
            f.old = side_name(rest);
            f.header.push(l.to_owned());
            continue;
        }
        let Some(f) = files.last_mut() else { continue };
        if !f.hunks.is_empty() {
            continue;
        }
        if let Some(rest) = l.strip_prefix("+++ ") {
            f.new = side_name(rest);
        } else if l.starts_with("Binary files") || l.starts_with("GIT binary patch") {
            f.binary = true;
            f.notes.push(l.to_owned());
        } else if ["new file mode", "deleted file mode", "rename from", "rename to", "copy from", "copy to", "old mode", "new mode", "similarity index"].iter().any(|p| l.starts_with(p)) {
            f.notes.push(l.to_owned());
            if let Some(n) = l.strip_prefix("rename from ").or_else(|| l.strip_prefix("copy from ")) {
                f.old = n.to_owned();
            }
            if let Some(n) = l.strip_prefix("rename to ").or_else(|| l.strip_prefix("copy to ")) {
                f.new = n.to_owned();
            }
        } else if l.is_empty() {
            continue;
        }
        f.header.push(l.to_owned());
    }
    // A file named only by its diff --git line (a mode change, a binary).
    for f in &mut files {
        if f.old.is_empty() && f.new.is_empty() {
            if let Some(h) = f.header.first().and_then(|h| h.strip_prefix("diff --git ")) {
                if let Some((a, b)) = h.split_once(" b/") {
                    f.old = side_name(a);
                    f.new = b.to_owned();
                }
            }
        }
        if f.notes.iter().any(|n| n.starts_with("new file mode")) {
            f.old.clear();
        }
        if f.notes.iter().any(|n| n.starts_with("deleted file mode")) {
            f.new.clear();
        }
    }
    files
}

fn mark_no_eol(files: &mut [FilePatch]) {
    if let Some(l) = files.last_mut().and_then(|f| f.hunks.last_mut()).and_then(|h| h.lines.last_mut()) {
        l.no_eol = true;
    }
}

/// `file`'s header and `hunks` as a patch `git apply` takes: the header as
/// it came (its ---/+++ lines named), each hunk's counts from its lines.
pub fn write(file: &FilePatch, hunks: &[Hunk]) -> String {
    let mut out = String::new();
    let mut named = false;
    for h in &file.header {
        named |= h.starts_with("--- ");
        out.push_str(h);
        out.push('\n');
    }
    if !named {
        let side = |pre: &str, n: &str| if n.is_empty() { "/dev/null".to_owned() } else { format!("{pre}{n}") };
        out.push_str(&format!("--- {}\n+++ {}\n", side("a/", &file.old), side("b/", &file.new)));
    }
    for h in hunks {
        // The counts from the lines, whatever the hunk said.
        let old_len = h.lines.iter().filter(|l| l.kind != Kind::Add).count() as u32;
        let new_len = h.lines.iter().filter(|l| l.kind != Kind::Del).count() as u32;
        let range = |s: u32, n: u32| if n == 1 { s.to_string() } else { format!("{s},{n}") };
        let section = if h.section.is_empty() { String::new() } else { format!(" {}", h.section) };
        out.push_str(&format!("@@ -{} +{} @@{section}\n", range(h.old_start, old_len), range(h.new_start, new_len)));
        for l in &h.lines {
            out.push(match l.kind {
                Kind::Same => ' ',
                Kind::Add => '+',
                Kind::Del => '-',
            });
            out.push_str(&l.text);
            out.push('\n');
            if l.no_eol {
                out.push_str("\\ No newline at end of file\n");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ONE: &str = "diff --git a/x.R b/x.R\nindex 1111111..2222222 100644\n--- a/x.R\n+++ b/x.R\n@@ -3,3 +3,4 @@ f <- function()\n a\n-b\n+B\n+C\n c\n\\ No newline at end of file\n";

    #[test]
    fn a_file_reads_as_header_hunks_and_lines() {
        let f = parse(ONE);
        assert_eq!(f.len(), 1);
        let f = &f[0];
        assert_eq!((f.old.as_str(), f.new.as_str()), ("x.R", "x.R"));
        assert_eq!(f.header.len(), 4);
        let h = &f.hunks[0];
        assert_eq!((h.old_start, h.old_len, h.new_start, h.new_len, h.section.as_str()), (3, 3, 3, 4, "f <- function()"));
        let k: Vec<_> = h.lines.iter().map(|l| (l.kind, l.text.as_str(), l.no_eol)).collect();
        assert_eq!(k, vec![(Kind::Same, "a", false), (Kind::Del, "b", false), (Kind::Add, "B", false), (Kind::Add, "C", false), (Kind::Same, "c", true)]);
    }

    #[test]
    fn written_back_it_is_the_same_patch() {
        let f = parse(ONE).remove(0);
        assert_eq!(write(&f, &f.hunks), ONE);
    }

    #[test]
    fn several_files_new_deleted_renamed_and_binary() {
        let t = "diff --git a/n b/n\nnew file mode 100644\n--- /dev/null\n+++ b/n\n@@ -0,0 +1,2 @@\n+one\n+two\ndiff --git a/gone b/gone\ndeleted file mode 100644\n--- a/gone\n+++ /dev/null\n@@ -1 +0,0 @@\n-bye\ndiff --git a/old b/new\nsimilarity index 100%\nrename from old\nrename to new\ndiff --git a/p.png b/p.png\nBinary files a/p.png and b/p.png differ\n";
        let f = parse(t);
        let names: Vec<_> = f.iter().map(|f| (f.old.as_str(), f.new.as_str(), f.binary, f.hunks.len())).collect();
        assert_eq!(names, vec![("", "n", false, 1), ("gone", "", false, 1), ("old", "new", false, 0), ("p.png", "p.png", true, 0)]);
        assert_eq!(f[1].hunks[0].lines.len(), 1);
    }

    #[test]
    fn a_plain_patch_without_git_lines() {
        let f = parse("--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n--- a/y\n+++ b/y\n@@ -1 +1 @@\n-c\n+d\n");
        assert_eq!(f.iter().map(|f| f.new.as_str()).collect::<Vec<_>>(), vec!["x", "y"]);
        assert!(write(&f[1], &f[1].hunks).starts_with("--- a/y\n+++ b/y\n@@ -1 +1 @@\n-c\n+d\n"));
    }

    #[test]
    fn a_cut_hunk_counts_itself() {
        let lines = vec![Line { kind: Kind::Same, text: "a".into(), no_eol: false }, Line { kind: Kind::Add, text: "b".into(), no_eol: false }];
        let h = Hunk::of_lines(5, 5, "", lines);
        assert_eq!((h.old_start, h.old_len, h.new_start, h.new_len), (5, 1, 5, 2));
        let only_add = Hunk::of_lines(5, 5, "", vec![Line { kind: Kind::Add, text: "x".into(), no_eol: false }]);
        assert_eq!((only_add.old_start, only_add.old_len), (4, 0), "an empty side starts at the line before");
    }
}
