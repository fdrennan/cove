//! What changed inside a line: a removed line and the line that took its
//! place, compared word by word, so a viewer can mark the few characters that
//! moved rather than the whole line.

use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Op {
    Same,
    Del,
    Add,
}

/// A run of one kind, with the bytes it covers in the old line (Same, Del)
/// and in the new one (Same, Add).
#[derive(Clone, PartialEq, Debug)]
pub struct Piece {
    pub op: Op,
    pub old: Range<usize>,
    pub new: Range<usize>,
}

/// Words, runs of spaces, and each other character on its own: the units a
/// person reads a change in.
fn tokens(s: &str) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut it = s.char_indices().peekable();
    while let Some((i, c)) = it.next() {
        let class = |c: char| if c.is_alphanumeric() || c == '_' { 1 } else if c.is_whitespace() { 2 } else { 0 };
        let k = class(c);
        let mut end = i + c.len_utf8();
        if k != 0 {
            while let Some(&(j, d)) = it.peek() {
                if class(d) != k {
                    break;
                }
                end = j + d.len_utf8();
                it.next();
            }
        }
        out.push(i..end);
    }
    out
}

/// The two lines as pieces, or None when they share too little for a
/// word-level mark to help (the whole line is the change) or are too long to
/// compare at once.
pub fn compare(old: &str, new: &str) -> Option<Vec<Piece>> {
    let (a, b) = (tokens(old), tokens(new));
    if a.len() * b.len() > 160_000 {
        return None;
    }
    // Longest common subsequence of tokens, filled from the end.
    let (n, m) = (a.len(), b.len());
    let mut t = vec![0u32; (n + 1) * (m + 1)];
    let at = |i: usize, j: usize| i * (m + 1) + j;
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            t[at(i, j)] = if old[a[i].clone()] == new[b[j].clone()] { t[at(i + 1, j + 1)] + 1 } else { t[at(i + 1, j)].max(t[at(i, j + 1)]) };
        }
    }
    let mut pieces: Vec<Piece> = Vec::new();
    let mut push = |op: Op, o: Range<usize>, nw: Range<usize>| match pieces.last_mut() {
        Some(p) if p.op == op && p.old.end == o.start && p.new.end == nw.start => {
            p.old.end = o.end;
            p.new.end = nw.end;
        }
        _ => pieces.push(Piece { op, old: o, new: nw }),
    };
    let (mut i, mut j) = (0, 0);
    let (mut oe, mut ne) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && old[a[i].clone()] == new[b[j].clone()] {
            push(Op::Same, a[i].clone(), b[j].clone());
            (oe, ne) = (a[i].end, b[j].end);
            i += 1;
            j += 1;
        } else if j < m && (i == n || t[at(i, j + 1)] >= t[at(i + 1, j)]) {
            push(Op::Add, oe..oe, b[j].clone());
            ne = b[j].end;
            j += 1;
        } else {
            push(Op::Del, a[i].clone(), ne..ne);
            oe = a[i].end;
            i += 1;
        }
    }
    // Shared enough to read as an edit of one line: a third of it, not
    // counting spaces.
    let solid = |s: &str, r: &Range<usize>| s[r.clone()].chars().filter(|c| !c.is_whitespace()).count();
    let same: usize = pieces.iter().filter(|p| p.op == Op::Same).map(|p| solid(old, &p.old)).sum();
    let longest = solid(old, &(0..old.len())).max(solid(new, &(0..new.len())));
    if longest == 0 || same * 3 < longest {
        return None;
    }
    Some(pieces)
}

/// The byte ranges to mark in the old line and the new one.
pub fn marks(pieces: &[Piece]) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let old = pieces.iter().filter(|p| p.op == Op::Del && !p.old.is_empty()).map(|p| p.old.clone()).collect();
    let new = pieces.iter().filter(|p| p.op == Op::Add && !p.new.is_empty()).map(|p| p.new.clone()).collect();
    (old, new)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_word_changed_is_one_mark_each_side() {
        let (o, n) = ("x <- mean(values)", "x <- median(values)");
        let p = compare(o, n).unwrap();
        let (mo, mn) = marks(&p);
        assert_eq!(mo.iter().map(|r| &o[r.clone()]).collect::<Vec<_>>(), vec!["mean"]);
        assert_eq!(mn.iter().map(|r| &n[r.clone()]).collect::<Vec<_>>(), vec!["median"]);
    }

    #[test]
    fn pieces_cover_both_lines_in_order() {
        let (o, n) = ("let a = 1;", "let b = 1 + 2;");
        let p = compare(o, n).unwrap();
        let old: String = p.iter().filter(|p| p.op != Op::Add).map(|p| &o[p.old.clone()]).collect();
        let new: String = p.iter().filter(|p| p.op != Op::Del).map(|p| &n[p.new.clone()]).collect();
        assert_eq!((old.as_str(), new.as_str()), (o, n));
    }

    #[test]
    fn unrelated_lines_are_not_marked_word_by_word() {
        assert!(compare("library(shiny)", "return(total / count)").is_none());
        assert!(compare("", "").is_none());
    }

    #[test]
    fn multibyte_text_stays_on_char_boundaries() {
        let p = compare("naïve café", "naïve cafés").unwrap();
        let (_, mn) = marks(&p);
        assert!(mn.iter().all(|r| "naïve cafés".is_char_boundary(r.start) && "naïve cafés".is_char_boundary(r.end)));
    }
}
