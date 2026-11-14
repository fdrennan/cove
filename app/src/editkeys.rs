//! The editor's keys that depend on what is being written (operator,
//! 2026-11-01: "tab should be intelligent (know when auto completing or
//! looking for help vs actual tab)"). Pure functions over the text and the
//! selection, in characters, so each rule is tested here; ide.rs only feeds
//! them the keys and puts the result back.
//!
//! Tab, in order (the completion list, when open, has already taken it -
//! edcomplete.rs `keys`):
//!   1. a snippet is active: to its next placeholder;
//!   2. in markup, an Emmet abbreviation before the cursor (`ul>li*3`,
//!      `div.card`, `!`): expanded, with a placeholder in each element;
//!   3. a selection over several lines: those lines indented;
//!   4. otherwise: one indent (the file's own - tabs, or 2 or 4 spaces).
//! Shift+Tab: back a placeholder, else the line(s) outdented.
//! Enter: the line's indent carried on, one more after an opener, and `{|}`
//! or `<div>|</div>` opened onto their own lines.
//! Typing `( [ { " ' \``: the pair, the cursor between - or round the
//! selection. Typing a closer that is already next: stepped over. In markup,
//! `>` ending an open tag adds its closing tag; `</` finishes the innermost
//! open one. Backspace in an empty pair takes both.

use crate::syntax::{self, Kind};

/// Text and selection after a key: (text, anchor, cursor) in chars.
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub text: String,
    pub anchor: usize,
    pub cursor: usize,
    /// Placeholders a snippet left (char offsets, in order): Tab walks them.
    pub marks: Vec<usize>,
}

fn chars(s: &str) -> Vec<char> {
    s.chars().collect()
}

fn line_start(c: &[char], at: usize) -> usize {
    c[..at.min(c.len())].iter().rposition(|&x| x == '\n').map_or(0, |i| i + 1)
}

fn line_end(c: &[char], at: usize) -> usize {
    c[at.min(c.len())..].iter().position(|&x| x == '\n').map_or(c.len(), |i| at + i)
}

/// The file's indent: a tab where most indented lines start with one, else
/// the smallest step its space-indented lines use (2 or 4), else the
/// language's custom.
pub fn indent_unit(text: &str, kind: Option<Kind>) -> String {
    let (mut tabs, mut spaced) = (0, Vec::new());
    for l in text.lines().take(2000) {
        if l.starts_with('\t') {
            tabs += 1;
        } else if l.starts_with(' ') && !l.trim().is_empty() {
            let n = l.len() - l.trim_start_matches(' ').len();
            spaced.push(n);
        }
    }
    if tabs > spaced.len() && tabs > 0 {
        return "\t".into();
    }
    let step = spaced.iter().filter(|&&n| n > 0).min().copied();
    let width = match step {
        Some(n) if n == 2 || n == 4 => n,
        Some(n) if n % 4 == 0 => 4,
        Some(n) if n % 2 == 0 => 2,
        _ => kind.map_or(4, Kind::indent_width),
    };
    " ".repeat(width)
}

fn edit(c: Vec<char>, anchor: usize, cursor: usize) -> Edit {
    Edit { text: c.into_iter().collect(), anchor, cursor, marks: Vec::new() }
}

/// Tab with nothing special at the cursor (rules 3 and 4).
pub fn tab(text: &str, anchor: usize, cursor: usize, unit: &str) -> Edit {
    let mut c = chars(text);
    let (a, b) = (anchor.min(cursor), anchor.max(cursor));
    if c[a..b].contains(&'\n') {
        return shift_lines(text, anchor, cursor, unit, true);
    }
    let u = chars(unit);
    c.splice(a..b, u.iter().copied());
    let at = a + u.len();
    edit(c, at, at)
}

/// Shift+Tab, or Tab over several lines: each line of the selection one
/// step in or out.
pub fn shift_lines(text: &str, anchor: usize, cursor: usize, unit: &str, inward: bool) -> Edit {
    let c = chars(text);
    let (a, b) = (anchor.min(cursor), anchor.max(cursor));
    let first = line_start(&c, a);
    // A selection ending at a line's very start leaves that line alone.
    let last_end = if b > a && b > 0 && c.get(b - 1) == Some(&'\n') { b - 1 } else { b };
    let last = line_end(&c, last_end);
    let u = chars(unit);
    let mut out: Vec<char> = c[..first].to_vec();
    let (mut da, mut db) = (0isize, 0isize);
    let mut pos = first;
    for (k, line) in c[first..last].split(|&x| x == '\n').enumerate() {
        if k > 0 {
            out.push('\n');
        }
        let delta: isize = if inward {
            if line.is_empty() && first != last {
                0
            } else {
                out.extend(&u);
                u.len() as isize
            }
        } else {
            // Out: one tab, or up to one unit's spaces.
            let n = if line.first() == Some(&'\t') { 1 } else { line.iter().take(u.len().max(1)).take_while(|&&x| x == ' ').count() };
            -(n as isize)
        };
        let keep = if delta < 0 { &line[(-delta) as usize..] } else { line };
        out.extend(keep);
        if pos <= a {
            da += if delta < 0 { -((a - pos).min((-delta) as usize) as isize) } else { delta };
        }
        if pos <= b {
            db += if delta < 0 { -((b - pos).min((-delta) as usize) as isize) } else { delta };
        }
        pos += line.len() + 1;
    }
    out.extend(&c[last..]);
    let na = (a as isize + da).max(first as isize) as usize;
    let nb = (b as isize + db).max(na as isize) as usize;
    let (anc, cur) = if anchor <= cursor { (na, nb) } else { (nb, na) };
    edit(out, anc, cur)
}

/// Enter: a new line indented as this one - one deeper after an opener, and
/// a pair split round the cursor (`{|}` → three lines, the cursor between).
pub fn enter(text: &str, anchor: usize, cursor: usize, unit: &str, kind: Option<Kind>) -> Edit {
    let mut c = chars(text);
    let (a, b) = (anchor.min(cursor), anchor.max(cursor));
    c.drain(a..b);
    let at = a;
    let ls = line_start(&c, at);
    let indent: Vec<char> = c[ls..].iter().take_while(|&&x| x == ' ' || x == '\t').copied().collect();
    let before: String = c[ls..at].iter().collect();
    let prev = before.trim_end().chars().last();
    let next = c.get(at).copied();
    let opener = matches!(prev, Some('{' | '[' | '('))
        || prev == Some(':') && matches!(kind, Some(Kind::Yaml)) && !before.trim_start().starts_with('-') && before.trim_end().ends_with(':')
        || kind.is_some_and(Kind::markup) && prev == Some('>') && opens_tag(&before);
    let closes = matches!((prev, next), (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('('), Some(')')))
        || kind.is_some_and(Kind::markup) && prev == Some('>') && c[at..].iter().collect::<String>().starts_with("</");
    let u = chars(unit);
    let mut ins: Vec<char> = vec!['\n'];
    ins.extend(&indent);
    if opener {
        ins.extend(&u);
    }
    let cur = at + ins.len();
    if closes {
        ins.push('\n');
        ins.extend(&indent);
    }
    c.splice(at..at, ins);
    edit(c, cur, cur)
}

/// Does `line` end in an opening tag (not a closing, void or self-closed one)?
fn opens_tag(line: &str) -> bool {
    let Some(lt) = line.rfind('<') else { return false };
    let t = &line[lt + 1..];
    let name: String = t.chars().take_while(|c| c.is_alphanumeric() || *c == '-').collect();
    !t.starts_with('/') && !name.is_empty() && !t.trim_end().ends_with("/>") && !syntax::VOID_TAGS.contains(&name.to_ascii_lowercase().as_str())
}

const PAIRS: [(char, char); 6] = [('(', ')'), ('[', ']'), ('{', '}'), ('"', '"'), ('\'', '\''), ('`', '`')];

/// A character typed: the pair, the step over a closer, a tag closed. None:
/// type it as it is.
pub fn typed(text: &str, anchor: usize, cursor: usize, ch: char, kind: Option<Kind>) -> Option<Edit> {
    let mut c = chars(text);
    let (a, b) = (anchor.min(cursor), anchor.max(cursor));
    let next = c.get(b).copied();
    let prev = if a > 0 { c.get(a - 1).copied() } else { None };
    let quote = matches!(ch, '"' | '\'' | '`');
    // Over the closer that is already there.
    if a == b && next == Some(ch) && (matches!(ch, ')' | ']' | '}') || quote) {
        return Some(edit(c, a + 1, a + 1));
    }
    if let Some(&(open, close)) = PAIRS.iter().find(|p| p.0 == ch) {
        // A backtick pairs only where it means something.
        if ch == '`' && !matches!(kind, Some(Kind::JavaScript | Kind::TypeScript | Kind::Jsx | Kind::Tsx | Kind::Markdown | Kind::Sql) | None) {
            return None;
        }
        if a != b {
            // Round the selection.
            let inner: Vec<char> = c[a..b].to_vec();
            let mut w = vec![open];
            w.extend(&inner);
            w.push(close);
            c.splice(a..b, w);
            return Some(edit(c, a + 1, b + 1));
        }
        // Only before a space, a closer or the end - never mid-word; a quote
        // not after a letter (it's an apostrophe there: don't).
        let free = next.is_none_or(|n| n.is_whitespace() || matches!(n, ')' | ']' | '}' | ',' | ';' | '>' | ':'));
        let word_before = prev.is_some_and(|p| p.is_alphanumeric() || p == '_');
        if !free || quote && word_before || ch == '\'' && kind == Some(Kind::Markdown) {
            return None;
        }
        c.splice(a..a, [open, close]);
        return Some(edit(c, a + 1, a + 1));
    }
    if kind.is_some_and(Kind::markup) && a == b {
        let ls = line_start(&c, a);
        let before: String = c[ls..a].iter().collect();
        if ch == '>' {
            // `<div class="x"` + '>' → `<div class="x">|</div>`
            if let Some(lt) = before.rfind('<') {
                let tag = &before[lt + 1..];
                let name: String = tag.chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == ':' || *c == '.').collect();
                let open_quotes = tag.matches('"').count() % 2 == 1 || tag.matches('\'').count() % 2 == 1;
                let jsx_expr = tag.matches('{').count() > tag.matches('}').count();
                if !name.is_empty() && !tag.contains('>') && !tag.ends_with('/') && !open_quotes && !jsx_expr && !syntax::VOID_TAGS.contains(&name.to_ascii_lowercase().as_str()) {
                    let close: Vec<char> = format!("></{name}>").chars().collect();
                    c.splice(a..a, close);
                    return Some(edit(c, a + 1, a + 1));
                }
            }
        }
        if ch == '/' && prev == Some('<') {
            let upto: String = c[..a - 1].iter().collect();
            if let Some(open) = syntax::unclosed_tag(&upto) {
                let fill: Vec<char> = format!("/{open}>").chars().collect();
                let n = fill.len();
                c.splice(a..a, fill);
                return Some(edit(c, a + n, a + n));
            }
        }
    }
    None
}

/// Backspace: in an empty pair, both go.
pub fn backspace(text: &str, anchor: usize, cursor: usize) -> Option<Edit> {
    if anchor != cursor || cursor == 0 {
        return None;
    }
    let mut c = chars(text);
    let (p, n) = (c[cursor - 1], c.get(cursor).copied());
    if PAIRS.iter().any(|&(o, cl)| o == p && Some(cl) == n) {
        c.drain(cursor - 1..cursor + 1);
        return Some(edit(c, cursor - 1, cursor - 1));
    }
    None
}

// ---- Emmet -------------------------------------------------------------------------

/// The abbreviation right before the cursor, if it is one worth expanding:
/// a known tag, or one with Emmet's operators that begins with a tag, a
/// class or an id - so "hello" + Tab stays "hello" and a tab.
fn abbreviation(c: &[char], at: usize) -> Option<(usize, String)> {
    // Back to whitespace - or to a '>' that ends a real tag, which is one with
    // a '<' before it on the line ("<p>hello"); Emmet's child '>' has none.
    let ls = line_start(c, at);
    let mut start = at;
    while start > ls {
        let x = c[start - 1];
        if x.is_whitespace() || x == '>' && c[ls..start - 1].contains(&'<') {
            break;
        }
        start -= 1;
    }
    let abbr: String = c[start..at].iter().collect();
    if abbr.is_empty() {
        return None;
    }
    if abbr == "!" {
        return Some((start, abbr));
    }
    let first: String = abbr.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '-').collect();
    let known = |t: &str| syntax::complete(Kind::Html, &format!("<{t}"), &format!("<{t}"), t.len() + 1).is_some_and(|(_, v)| v.iter().any(|(x, _)| x == t));
    let operators = abbr.contains(['>', '+', '*', '.', '#', '{']);
    let ok = abbr.chars().all(|c| c.is_ascii_alphanumeric() || "-_>+*.#{}[]=$ \"'".contains(c))
        && (first.is_empty() && abbr.starts_with(['.', '#']) || !first.is_empty() && (known(&first) || operators && first.chars().next().is_some_and(|c| c.is_ascii_lowercase())));
    ok.then_some((start, abbr))
}

struct Node {
    tag: String,
    classes: Vec<String>,
    id: String,
    text: String,
    times: usize,
    children: Vec<Node>,
}

/// `ul>li.item*3+p{hi}` → a tree. `>` goes down, `+` stays beside, `*n`
/// repeats; no groups or numbering - the common part of Emmet.
fn parse(abbr: &str) -> Vec<Node> {
    let mut levels: Vec<Vec<Node>> = vec![Vec::new()];
    let mut i = 0;
    let s: Vec<char> = abbr.chars().collect();
    loop {
        let mut n = Node { tag: String::new(), classes: Vec::new(), id: String::new(), text: String::new(), times: 1, children: Vec::new() };
        let mut cur = String::new();
        let mut mode = 't';
        while i < s.len() && !matches!(s[i], '>' | '+') {
            match s[i] {
                '.' | '#' | '*' => {
                    flush(&mut n, mode, &mut cur);
                    mode = s[i];
                }
                '{' => {
                    flush(&mut n, mode, &mut cur);
                    let e = s[i..].iter().position(|&x| x == '}').map_or(s.len(), |p| i + p);
                    n.text = s[i + 1..e].iter().collect();
                    i = e;
                    mode = 'x';
                }
                c => cur.push(c),
            }
            i += 1;
        }
        flush(&mut n, mode, &mut cur);
        if n.tag.is_empty() {
            n.tag = "div".into();
        }
        levels.last_mut().unwrap().push(n);
        if i >= s.len() {
            break;
        }
        if s[i] == '>' {
            levels.push(Vec::new());
        }
        i += 1;
    }
    // Fold each level into the last node of the one above.
    while levels.len() > 1 {
        let kids = levels.pop().unwrap();
        if let Some(p) = levels.last_mut().unwrap().last_mut() {
            p.children = kids;
        }
    }
    levels.pop().unwrap()
}

fn flush(n: &mut Node, mode: char, cur: &mut String) {
    let v = std::mem::take(cur);
    match mode {
        't' => n.tag = v,
        '.' if !v.is_empty() => n.classes.push(v),
        '#' => n.id = v,
        '*' => n.times = v.parse().unwrap_or(1).clamp(1, 50),
        _ => {}
    }
}

/// Write the tree, each element on its own line under `indent`; every
/// element's empty inside is a placeholder.
fn render(nodes: &[Node], indent: &str, unit: &str, jsx: bool, out: &mut String, marks: &mut Vec<usize>) {
    for (k, n) in nodes.iter().enumerate() {
        for t in 0..n.times {
            if k > 0 || t > 0 || !out.is_empty() {
                out.push('\n');
                out.push_str(indent);
            }
            out.push('<');
            out.push_str(&n.tag);
            if !n.id.is_empty() {
                out.push_str(&format!(" id=\"{}\"", n.id));
            }
            if !n.classes.is_empty() {
                out.push_str(&format!(" {}=\"{}\"", if jsx { "className" } else { "class" }, n.classes.join(" ")));
            }
            let void = syntax::VOID_TAGS.contains(&n.tag.as_str());
            if void {
                out.push_str(if jsx { " />" } else { ">" });
                if !jsx && n.tag == "img" {
                    // where the src goes
                }
                continue;
            }
            out.push('>');
            if n.children.is_empty() {
                out.push_str(&n.text);
                if n.text.is_empty() {
                    marks.push(out.chars().count());
                }
            } else {
                out.push_str(&n.text);
                let inner = format!("{indent}{unit}");
                out.push('\n');
                out.push_str(&inner);
                let mut sub = String::new();
                let base = out.chars().count();
                let mut sub_marks = Vec::new();
                render(&n.children, &inner, unit, jsx, &mut sub, &mut sub_marks);
                out.push_str(&sub);
                marks.extend(sub_marks.into_iter().map(|m| m + base));
                out.push('\n');
                out.push_str(indent);
            }
            out.push_str(&format!("</{}>", n.tag));
        }
    }
}

const HTML5: &str = "<!doctype html>\n<html lang=\"en\">\n<head>\n  <meta charset=\"utf-8\">\n  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n  <title>\u{1}</title>\n</head>\n<body>\n  \u{1}\n</body>\n</html>";

/// Tab on an Emmet abbreviation, in markup: the expansion, the cursor in its
/// first placeholder and the rest kept for the next Tabs.
pub fn emmet(text: &str, anchor: usize, cursor: usize, kind: Kind, unit: &str) -> Option<Edit> {
    if anchor != cursor || !kind.markup() {
        return None;
    }
    let c = chars(text);
    let (start, abbr) = abbreviation(&c, cursor)?;
    let ls = line_start(&c, start);
    let indent: String = c[ls..start].iter().take_while(|&&x| x == ' ' || x == '\t').collect();
    let (body, marks) = if abbr == "!" {
        let mut marks = Vec::new();
        let mut out = String::new();
        for ch in HTML5.chars() {
            if ch == '\u{1}' {
                marks.push(out.chars().count());
            } else {
                out.push(ch);
            }
        }
        (out, marks)
    } else {
        let mut out = String::new();
        let mut marks = Vec::new();
        render(&parse(&abbr), &indent, unit, matches!(kind, Kind::Jsx | Kind::Tsx), &mut out, &mut marks);
        (out, marks)
    };
    let mut cc = c;
    let body_c = chars(&body);
    cc.splice(start..cursor, body_c.iter().copied());
    let marks: Vec<usize> = marks.into_iter().map(|m| m + start).collect();
    let first = marks.first().copied().unwrap_or(start + body_c.len());
    Some(Edit { text: cc.into_iter().collect(), anchor: first, cursor: first, marks })
}

/// A live snippet's placeholders, kept up to date as the person types into
/// one of them: every later mark moves by what the text grew by since.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Snippet {
    pub marks: Vec<usize>,
    pub at: usize,
    pub len: usize,
}

impl Snippet {
    pub fn new(marks: Vec<usize>, text: &str) -> Option<Snippet> {
        (marks.len() > 1).then(|| Snippet { marks, at: 0, len: text.chars().count() })
    }

    /// The text changed (typed into the current placeholder): shift the
    /// marks after it.
    pub fn follow(&mut self, text: &str) {
        let n = text.chars().count();
        let d = n as isize - self.len as isize;
        if d != 0 {
            for m in self.marks.iter_mut().skip(self.at + 1) {
                *m = (*m as isize + d).max(0) as usize;
            }
            self.len = n;
        }
    }

    /// Tab (forward) or Shift+Tab: the next placeholder's offset; None when
    /// past the last (the snippet is done).
    pub fn step(&mut self, forward: bool) -> Option<usize> {
        if forward {
            if self.at + 1 >= self.marks.len() {
                return None;
            }
            self.at += 1;
        } else {
            self.at = self.at.saturating_sub(1);
        }
        self.marks.get(self.at).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Text with `|` as the cursor (or `[...]` as a selection) → (text, anchor, cursor).
    fn at(s: &str) -> (String, usize, usize) {
        if let (Some(a), Some(b)) = (s.find('['), s.find(']')) {
            let t: String = s.replacen('[', "", 1).replacen(']', "", 1);
            let a = s[..a].chars().count();
            let b = s[..b].chars().count() - 1;
            return (t, a, b);
        }
        let c = s[..s.find('|').unwrap()].chars().count();
        (s.replacen('|', "", 1), c, c)
    }

    fn show(e: &Edit) -> String {
        let mut c: Vec<char> = e.text.chars().collect();
        c.insert(e.cursor, '|');
        c.into_iter().collect()
    }

    #[test]
    fn the_indent_is_the_files_own() {
        assert_eq!(indent_unit("a {\n\tb;\n}", None), "\t");
        assert_eq!(indent_unit("a:\n  b: 1\n    c: 2", None), "  ");
        assert_eq!(indent_unit("def f():\n    return 1", None), "    ");
        assert_eq!(indent_unit("", Some(Kind::Html)), "  ");
        assert_eq!(indent_unit("", Some(Kind::Java)), "    ");
    }

    #[test]
    fn tab_without_a_list_indents_and_never_takes_a_suggestion() {
        let (t, a, c) = at("let x|");
        assert_eq!(show(&tab(&t, a, c, "  ")), "let x  |");
        let (t, a, c) = at("[a\nb\n]c");
        let e = tab(&t, a, c, "  ");
        assert_eq!(e.text, "  a\n  b\nc");
        let (t, a, c) = at("  a\n  [b\n  c]");
        assert_eq!(shift_lines(&t, a, c, "  ", false).text, "  a\nb\nc");
        let (t, a, c) = at("\t|x");
        assert_eq!(show(&shift_lines(&t, a, c, "\t", false)), "|x");
    }

    #[test]
    fn enter_keeps_the_indent_and_opens_pairs() {
        let (t, a, c) = at("  if (x) {|}");
        assert_eq!(show(&enter(&t, a, c, "  ", Some(Kind::JavaScript))), "  if (x) {\n    |\n  }");
        let (t, a, c) = at("    a = 1|");
        assert_eq!(show(&enter(&t, a, c, "    ", None)), "    a = 1\n    |");
        let (t, a, c) = at("<ul>|</ul>");
        assert_eq!(show(&enter(&t, a, c, "  ", Some(Kind::Html))), "<ul>\n  |\n</ul>");
        let (t, a, c) = at("<br>|");
        assert_eq!(show(&enter(&t, a, c, "  ", Some(Kind::Html))), "<br>\n|");
        let (t, a, c) = at("server:|");
        assert_eq!(show(&enter(&t, a, c, "  ", Some(Kind::Yaml))), "server:\n  |");
    }

    #[test]
    fn brackets_and_quotes_pair_step_over_and_wrap() {
        let (t, a, c) = at("f|");
        assert_eq!(show(&typed(&t, a, c, '(', None).unwrap()), "f(|)");
        let (t, a, c) = at("f(x|)");
        assert_eq!(show(&typed(&t, a, c, ')', None).unwrap()), "f(x)|");
        let (t, a, c) = at("say [hi]");
        assert_eq!(typed(&t, a, c, '"', None).unwrap().text, "say \"hi\"");
        let (t, a, c) = at("it|");
        assert!(typed(&t, a, c, '\'', None).is_none(), "an apostrophe, not a pair");
        let (t, a, c) = at("|word");
        assert!(typed(&t, a, c, '(', None).is_none(), "not mid-word");
        let (t, a, c) = at("(|)");
        assert_eq!(show(&backspace(&t, a, c).unwrap()), "|");
    }

    #[test]
    fn markup_closes_its_tags() {
        let (t, a, c) = at("<div class=\"card\"|");
        assert_eq!(show(&typed(&t, a, c, '>', Some(Kind::Html)).unwrap()), "<div class=\"card\">|</div>");
        let (t, a, c) = at("<img src=\"x\"|");
        assert!(typed(&t, a, c, '>', Some(Kind::Html)).is_none(), "void");
        let (t, a, c) = at("<a href=\"x>y|");
        assert!(typed(&t, a, c, '>', Some(Kind::Html)).is_none(), "inside a quote");
        let (t, a, c) = at("<ul><li>a</li><|");
        assert_eq!(show(&typed(&t, a, c, '/', Some(Kind::Html)).unwrap()), "<ul><li>a</li></ul>|");
        let (t, a, c) = at("const x = <Card title={go(1)}|");
        assert_eq!(show(&typed(&t, a, c, '>', Some(Kind::Jsx)).unwrap()), "const x = <Card title={go(1)}>|</Card>");
    }

    #[test]
    fn emmet_expands_with_placeholders_and_tab_walks_them() {
        let (t, a, c) = at("  ul>li.item*3|");
        let e = emmet(&t, a, c, Kind::Html, "  ").unwrap();
        assert_eq!(e.text, "  <ul>\n    <li class=\"item\"></li>\n    <li class=\"item\"></li>\n    <li class=\"item\"></li>\n  </ul>");
        assert_eq!(e.marks.len(), 3);
        assert_eq!(e.cursor, e.marks[0]);
        let (t, a, c) = at("div.card#main|");
        assert_eq!(emmet(&t, a, c, Kind::Html, "  ").unwrap().text, "<div id=\"main\" class=\"card\"></div>");
        let (t, a, c) = at("p{hello}+img|");
        assert_eq!(emmet(&t, a, c, Kind::Html, "  ").unwrap().text, "<p>hello</p>\n<img>");
        let (t, a, c) = at("!|");
        let h = emmet(&t, a, c, Kind::Html, "  ").unwrap();
        assert!(h.text.starts_with("<!doctype html>") && h.marks.len() == 2);
        let (t, a, c) = at(".row>.col|");
        assert_eq!(emmet(&t, a, c, Kind::Jsx, "  ").unwrap().text, "<div className=\"row\">\n  <div className=\"col\"></div>\n</div>");
        // Plain words are not abbreviations.
        let (t, a, c) = at("<p>hello|");
        assert!(emmet(&t, a, c, Kind::Html, "  ").is_none());
        let (t, a, c) = at("div|");
        assert!(emmet(&t, a, c, Kind::Css, "  ").is_none(), "not in CSS");
        // Typing in the first placeholder, then Tab to the second.
        let mut s = Snippet::new(e.marks.clone(), &e.text).unwrap();
        let mut text: Vec<char> = e.text.chars().collect();
        text.splice(e.marks[0]..e.marks[0], "one".chars());
        let text: String = text.into_iter().collect();
        s.follow(&text);
        let second = s.step(true).unwrap();
        assert_eq!(&text[..], &text[..]);
        let before: String = text.chars().take(second).collect();
        assert!(before.ends_with("one</li>\n    <li class=\"item\">"), "{before}");
        assert_eq!(s.step(false), Some(e.marks[0]));
    }
}
