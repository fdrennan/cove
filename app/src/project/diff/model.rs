//! One file's diff laid out once, for every way of showing it. The rows are
//! the diff's lines in order, numbered on both sides; a change is a run of
//! removed and added rows; the hunks are the smallest patches the changes
//! make (git's three lines of context, near changes joined), so stage,
//! unstage and revert act on exactly what a person points at. Unchanged
//! runs fold, and open by request. Side by side and inline are pairings of
//! the same rows: a view never re-reads the patch.

use std::collections::HashMap;
use std::ops::Range;

use super::patch::{FilePatch, Hunk, Kind, Line};
use super::words::{self, Piece};

/// Lines of context around a change, in a hunk and left open by a fold.
pub const CONTEXT: usize = 3;
/// A run of unchanged rows shorter than this stays open.
const MIN_FOLD: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RowKind {
    Same,
    Add,
    Del,
    /// Lines the diff does not carry (between git's hunks): only a count.
    Gap(u32),
}

#[derive(Clone, Debug)]
pub struct Row {
    pub kind: RowKind,
    pub old: Option<u32>,
    pub new: Option<u32>,
    /// As the patch has it, tabs and all: what a hunk written back carries.
    pub text: String,
    pub no_eol: bool,
    /// Bytes of `text` that changed against its pair, word by word.
    pub marks: Vec<Range<usize>>,
    /// The row it replaced, or that replaced it.
    pub pair: Option<usize>,
    /// The change it is part of.
    pub change: Option<usize>,
}

/// A run of removed and added rows, and the hunk that carries it.
#[derive(Clone, Debug)]
pub struct Change {
    pub rows: Range<usize>,
    pub hunk: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub rows: Vec<Row>,
    pub changes: Vec<Change>,
    /// The smallest patches the changes make, each with its rows.
    pub hunks: Vec<Hunk>,
    pub hunk_rows: Vec<Range<usize>>,
    /// Word-level pieces of each paired change, by the removed row.
    pub pieces: HashMap<usize, Vec<Piece>>,
    pub added: usize,
    pub removed: usize,
    /// The widest row, in characters with tabs as four: the views' extent.
    pub widest: usize,
}

pub fn build(f: &FilePatch) -> Model {
    let mut m = Model::default();
    // Each row's place in both files before it: where a hunk cut there starts.
    let mut at: Vec<(u32, u32)> = Vec::new();
    // (hunk of the patch, its rows, its section) - hunks are cut inside these.
    let mut spans: Vec<(Range<usize>, String)> = Vec::new();
    let mut last_old = 0u32;
    for h in &f.hunks {
        let (mut o, mut n) = (if h.old_len == 0 { h.old_start + 1 } else { h.old_start }, if h.new_len == 0 { h.new_start + 1 } else { h.new_start });
        if o > last_old + 1 {
            m.rows.push(Row { kind: RowKind::Gap(o - last_old - 1), old: None, new: None, text: String::new(), no_eol: false, marks: Vec::new(), pair: None, change: None });
            at.push((o, n));
        }
        let start = m.rows.len();
        for l in &h.lines {
            at.push((o, n));
            let (kind, old, new) = match l.kind {
                Kind::Same => (RowKind::Same, Some(o), Some(n)),
                Kind::Del => (RowKind::Del, Some(o), None),
                Kind::Add => (RowKind::Add, None, Some(n)),
            };
            if l.kind != Kind::Add {
                o += 1;
            }
            if l.kind != Kind::Del {
                n += 1;
            }
            m.rows.push(Row { kind, old, new, text: l.text.clone(), no_eol: l.no_eol, marks: Vec::new(), pair: None, change: None });
        }
        last_old = o.saturating_sub(1);
        spans.push((start..m.rows.len(), h.section.clone()));
    }
    m.added = m.rows.iter().filter(|r| r.kind == RowKind::Add).count();
    m.removed = m.rows.iter().filter(|r| r.kind == RowKind::Del).count();
    m.widest = m.rows.iter().map(|r| r.text.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum::<usize>()).max().unwrap_or(0);

    // Changes: runs of removed and added rows, never across a hunk of the patch.
    for (span, _) in &spans {
        let mut i = span.start;
        while i < span.end {
            if matches!(m.rows[i].kind, RowKind::Add | RowKind::Del) {
                let s = i;
                while i < span.end && matches!(m.rows[i].kind, RowKind::Add | RowKind::Del) {
                    i += 1;
                }
                m.changes.push(Change { rows: s..i, hunk: 0 });
            } else {
                i += 1;
            }
        }
    }
    // Pair each change's removed rows with its added ones, in order, and
    // mark what changed inside the pairs that read as edits of one line.
    for (ci, c) in m.changes.iter().enumerate() {
        let dels: Vec<usize> = c.rows.clone().filter(|&i| m.rows[i].kind == RowKind::Del).collect();
        let adds: Vec<usize> = c.rows.clone().filter(|&i| m.rows[i].kind == RowKind::Add).collect();
        for i in c.rows.clone() {
            m.rows[i].change = Some(ci);
        }
        for (&d, &a) in dels.iter().zip(&adds) {
            m.rows[d].pair = Some(a);
            m.rows[a].pair = Some(d);
            if let Some(p) = words::compare(&m.rows[d].text, &m.rows[a].text) {
                let (mo, mn) = words::marks(&p);
                m.rows[d].marks = mo;
                m.rows[a].marks = mn;
                m.pieces.insert(d, p);
            }
        }
    }
    // Hunks: changes within 2 × CONTEXT unchanged rows of each other join,
    // as git's do; each takes CONTEXT rows either side, inside its span.
    let mut ci = 0;
    for (span, section) in &spans {
        let mine: Vec<usize> = (ci..m.changes.len()).take_while(|&k| m.changes[k].rows.start < span.end).collect();
        ci += mine.len();
        let mut k = 0;
        while k < mine.len() {
            let mut last = k;
            while last + 1 < mine.len() && m.changes[mine[last + 1]].rows.start - m.changes[mine[last]].rows.end <= 2 * CONTEXT {
                last += 1;
            }
            let rows = m.changes[mine[k]].rows.start.saturating_sub(CONTEXT).max(span.start)..(m.changes[mine[last]].rows.end + CONTEXT).min(span.end);
            let lines: Vec<Line> = rows
                .clone()
                .map(|i| {
                    let r = &m.rows[i];
                    let kind = match r.kind {
                        RowKind::Add => Kind::Add,
                        RowKind::Del => Kind::Del,
                        _ => Kind::Same,
                    };
                    Line { kind, text: r.text.clone(), no_eol: r.no_eol }
                })
                .collect();
            let (o, n) = at[rows.start];
            let hi = m.hunks.len();
            m.hunks.push(Hunk::of_lines(o, n, section, lines));
            m.hunk_rows.push(rows);
            for &c in &mine[k..=last] {
                m.changes[c].hunk = hi;
            }
            k = last + 1;
        }
    }
    m
}

/// What a fold has been asked to show: per run of unchanged rows (by its
/// first row), extra rows open above and below; or everything.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Folds {
    pub open: HashMap<usize, (usize, usize)>,
    pub all: bool,
}

/// The rows as a viewer walks them: shown runs, and folds between.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    Rows(Range<usize>),
    /// Hidden unchanged rows; `run` names the fold for `Folds::open`.
    Fold { hidden: Range<usize>, run: usize },
}

pub fn segments(m: &Model, folds: &Folds) -> Vec<Seg> {
    let mut out = Vec::new();
    let mut shown_from = 0;
    let n = m.rows.len();
    let mut i = 0;
    while i < n {
        if m.rows[i].kind != RowKind::Same {
            i += 1;
            continue;
        }
        let a = i;
        while i < n && m.rows[i].kind == RowKind::Same {
            i += 1;
        }
        let b = i;
        if folds.all {
            continue;
        }
        // Context stays open next to a change; at the file's ends, or next
        // to lines the diff does not carry, there is nothing to keep open.
        let edge = |k: Option<usize>| k.is_none_or(|k| matches!(m.rows[k].kind, RowKind::Gap(_)));
        let (more_top, more_bottom) = folds.open.get(&a).copied().unwrap_or((0, 0));
        let top = if edge(a.checked_sub(1)) { 0 } else { CONTEXT } + more_top;
        let bottom = if edge((b < n).then_some(b)) { 0 } else { CONTEXT } + more_bottom;
        if b - a >= top + bottom + MIN_FOLD.max(1) {
            let hidden = a + top..b - bottom;
            if hidden.start > shown_from {
                out.push(Seg::Rows(shown_from..hidden.start));
            }
            shown_from = hidden.end;
            out.push(Seg::Fold { hidden, run: a });
        }
    }
    if shown_from < n {
        out.push(Seg::Rows(shown_from..n));
    }
    out
}

/// Rows side by side: (left, right). Unchanged rows face themselves; a
/// change's removed rows face its added ones, in order, a blank where one
/// side has more.
pub fn side_by_side(m: &Model, r: Range<usize>) -> Vec<(Option<usize>, Option<usize>)> {
    let mut out = Vec::new();
    let mut i = r.start;
    while i < r.end {
        match m.rows[i].kind {
            RowKind::Same | RowKind::Gap(_) => {
                out.push((Some(i), Some(i)));
                i += 1;
            }
            RowKind::Add | RowKind::Del => {
                let s = i;
                while i < r.end && matches!(m.rows[i].kind, RowKind::Add | RowKind::Del) {
                    i += 1;
                }
                let dels: Vec<usize> = (s..i).filter(|&k| m.rows[k].kind == RowKind::Del).collect();
                let adds: Vec<usize> = (s..i).filter(|&k| m.rows[k].kind == RowKind::Add).collect();
                for k in 0..dels.len().max(adds.len()) {
                    out.push((dels.get(k).copied(), adds.get(k).copied()));
                }
            }
        }
    }
    out
}

/// Rows inline: an edit of one line is one row, its removed words struck
/// through beside the added ones; everything else as unified has it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum InlineRow {
    One(usize),
    Edit { del: usize, add: usize },
}

pub fn inline(m: &Model, r: Range<usize>) -> Vec<InlineRow> {
    let mut out = Vec::new();
    let mut i = r.start;
    while i < r.end {
        if !matches!(m.rows[i].kind, RowKind::Add | RowKind::Del) {
            out.push(InlineRow::One(i));
            i += 1;
            continue;
        }
        let s = i;
        while i < r.end && matches!(m.rows[i].kind, RowKind::Add | RowKind::Del) {
            i += 1;
        }
        let dels: Vec<usize> = (s..i).filter(|&k| m.rows[k].kind == RowKind::Del).collect();
        let adds: Vec<usize> = (s..i).filter(|&k| m.rows[k].kind == RowKind::Add).collect();
        let mut rest_del = Vec::new();
        let mut rest_add = Vec::new();
        for k in 0..dels.len().max(adds.len()) {
            match (dels.get(k), adds.get(k)) {
                (Some(&d), Some(&a)) if m.pieces.contains_key(&d) => out.push(InlineRow::Edit { del: d, add: a }),
                (d, a) => {
                    rest_del.extend(d.copied());
                    rest_add.extend(a.copied());
                }
            }
        }
        out.extend(rest_del.into_iter().map(InlineRow::One));
        out.extend(rest_add.into_iter().map(InlineRow::One));
    }
    out
}

/// The change at or after row `from` (forward), or before it (back),
/// round the file.
pub fn next_change(m: &Model, from: usize, forward: bool) -> Option<usize> {
    if m.changes.is_empty() {
        return None;
    }
    let i = if forward { m.changes.iter().position(|c| c.rows.start > from).unwrap_or(0) } else { m.changes.iter().rposition(|c| c.rows.start < from).unwrap_or(m.changes.len() - 1) };
    Some(i)
}

#[cfg(test)]
mod tests {
    use super::super::patch;
    use super::*;

    fn file(lines: usize, edits: &[(usize, &str)]) -> FilePatch {
        // A whole-file diff, as git diff -U1000000 gives one.
        let mut body = String::new();
        let mut new_len = 0;
        for i in 1..=lines {
            match edits.iter().find(|e| e.0 == i) {
                Some((_, t)) => {
                    body.push_str(&format!("-line {i}\n+{t}\n"));
                }
                None => body.push_str(&format!(" line {i}\n")),
            }
            new_len += 1;
        }
        let text = format!("diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,{lines} +1,{new_len} @@\n{body}");
        patch::parse(&text).remove(0)
    }

    #[test]
    fn rows_are_numbered_on_both_sides_and_paired() {
        let m = build(&file(5, &[(3, "line three")]));
        let k: Vec<_> = m.rows.iter().map(|r| (r.kind, r.old, r.new)).collect();
        assert_eq!(k[2], (RowKind::Del, Some(3), None));
        assert_eq!(k[3], (RowKind::Add, None, Some(3)));
        assert_eq!(k[4], (RowKind::Same, Some(4), Some(4)));
        assert_eq!((m.rows[2].pair, m.rows[3].pair), (Some(3), Some(2)));
        assert_eq!((m.added, m.removed, m.changes.len()), (1, 1, 1));
        assert_eq!(&m.rows[3].text[m.rows[3].marks[0].clone()], "three");
    }

    #[test]
    fn far_changes_are_their_own_hunks_and_near_ones_join() {
        let far = build(&file(40, &[(2, "two"), (30, "thirty")]));
        assert_eq!(far.hunks.len(), 2);
        let near = build(&file(40, &[(10, "ten"), (15, "fifteen")]));
        assert_eq!(near.hunks.len(), 1, "4 rows apart, within 2 x CONTEXT");
        let h = &far.hunks[1];
        assert_eq!((h.old_start, h.old_len, h.new_start, h.new_len), (27, 7, 27, 7));
        let f = file(40, &[(2, "two"), (30, "thirty")]);
        let text = patch::write(&f, &far.hunks[1..]);
        assert!(text.contains("@@ -27,7 +27,7 @@\n line 27\n line 28\n line 29\n-line 30\n+thirty\n line 31\n"), "{text}");
    }

    #[test]
    fn unchanged_code_folds_and_opens() {
        let m = build(&file(40, &[(20, "twenty")]));
        let s = segments(&m, &Folds::default());
        // The top folds down to CONTEXT rows above the change, the bottom
        // likewise below it.
        assert_eq!(s.first(), Some(&Seg::Fold { hidden: 0..16, run: 0 }));
        assert!(matches!(s.last(), Some(Seg::Fold { hidden, .. }) if hidden.end == m.rows.len()));
        let mut open = Folds::default();
        open.open.insert(0, (0, 10));
        assert_eq!(segments(&m, &open).first(), Some(&Seg::Fold { hidden: 0..6, run: 0 }));
        assert_eq!(segments(&m, &Folds { all: true, ..Default::default() }), vec![Seg::Rows(0..m.rows.len())]);
    }

    #[test]
    fn side_by_side_and_inline_pair_the_same_rows() {
        let m = build(&file(5, &[(3, "line three")]));
        let sbs = side_by_side(&m, 0..m.rows.len());
        assert_eq!(sbs[2], (Some(2), Some(3)));
        assert_eq!(sbs.len(), 5);
        let il = inline(&m, 0..m.rows.len());
        assert_eq!(il[2], InlineRow::Edit { del: 2, add: 3 });
        assert_eq!(il.len(), 5);
    }

    #[test]
    fn changes_are_found_forward_and_back_round_the_file() {
        let m = build(&file(40, &[(2, "two"), (30, "thirty")]));
        assert_eq!(next_change(&m, 0, true), Some(0));
        assert_eq!(next_change(&m, m.changes[0].rows.start, true), Some(1));
        assert_eq!(next_change(&m, m.changes[1].rows.start, true), Some(0), "round to the first");
        assert_eq!(next_change(&m, 0, false), Some(1), "round to the last");
    }
}
