//! Completion in the editor, as a modern IDE has it (operator, 2026-11-01:
//! "complete text within context of file, arg completion, etc for rust, R,
//! julia, python, or shell"). Typing a word, or a `.` `$` `::` `(` `,` after
//! one, opens a list at the cursor: what the code's language knows - its
//! session (R, Python through jedi, Julia, the shell), from what is loaded,
//! argument names inside a call included, or rust-analyzer for Rust (lsp.rs)
//! - then the words of the file itself, which every file has. Ctrl+Space asks
//! for it anywhere. ↑ ↓ choose, Enter or Tab put it in, Esc closes it.
//!
//! The list is asked for once per word; typing on narrows it here. A
//! session that is busy or not started answers nothing, and the file's own
//! words stand alone.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, FontId, Key, RichText};

use crate::api::{Completion, Lang, Query};
use crate::backend::Backend;
use crate::theme;

/// One choice: what it puts in, what it is, and the character column of its
/// line it replaces from.
#[derive(Clone, Debug, PartialEq)]
struct Item {
    text: String,
    detail: String,
    start: usize,
    from_file: bool,
}

#[derive(Default)]
pub struct EdComplete {
    slot: Arc<Mutex<Option<(u64, Result<Completion, String>)>>>,
    seq: u64,
    /// The editor the list belongs to, and the char index of the start of
    /// the cursor's line when it was asked for.
    doc: Option<egui::Id>,
    line_at: usize,
    row: usize,
    /// Every item asked for, and those the word typed so far leaves.
    all: Vec<Item>,
    shown: Vec<Item>,
    sel: usize,
    /// An ask waiting for typing to pause: when, and for which cursor.
    pending: Option<(f64, usize)>,
    /// What the language said instead of completions ("busy", "not running").
    note: String,
    /// The text and cursor last seen, to tell typing from moving.
    seen: (u64, usize),
    accept: Option<usize>,
}

/// The editor's view of one frame, for `after`.
pub struct Here<'a> {
    pub id: egui::Id,
    pub path: &'a str,
    pub text: &'a mut String,
    pub document: bool,
    pub lang: Option<Lang>,
    /// Where the text's first character is drawn, and a character's size.
    pub origin: egui::Pos2,
    pub char_w: f32,
    pub row_h: f32,
}

impl EdComplete {
    pub fn open(&self) -> bool {
        !self.shown.is_empty()
    }

    /// Before the editor draws: while the list is up, the keys that move in
    /// it and take from it are its, not the text's. Ctrl+Space asks.
    pub fn keys(&mut self, ui: &mut egui::Ui, id: egui::Id) -> bool {
        let ask = ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::Space) || i.consume_key(egui::Modifiers::CTRL, Key::Space));
        if !self.open() || self.doc != Some(id) {
            return ask;
        }
        let n = self.shown.len();
        ui.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowDown) {
                self.sel = (self.sel + 1) % n;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::ArrowUp) {
                self.sel = (self.sel + n - 1) % n;
            }
            if i.consume_key(egui::Modifiers::NONE, Key::Escape) {
                self.close();
            } else if i.consume_key(egui::Modifiers::NONE, Key::Enter) || i.consume_key(egui::Modifiers::NONE, Key::Tab) {
                self.accept = Some(self.sel);
            }
        });
        ask
    }

    fn close(&mut self) {
        self.shown.clear();
        self.all.clear();
        self.pending = None;
        self.note.clear();
    }

    /// After the editor draws: put a choice in, follow the typing, ask when
    /// it pauses, take the answer, and draw the list at the cursor.
    pub fn after(&mut self, ui: &mut egui::Ui, backend: &Backend, h: Here, ask: bool) {
        let ctx = ui.ctx().clone();
        let focused = ctx.memory(|m| m.has_focus(h.id));
        let Some(cursor) = egui::text_edit::TextEditState::load(&ctx, h.id).and_then(|s| s.cursor.char_range()).map(|r| -> usize { r.primary.index.into() }) else {
            return;
        };
        if self.doc != Some(h.id) {
            self.close();
            self.doc = Some(h.id);
        }
        if let Some(k) = self.accept.take() {
            if let Some(it) = self.shown.get(k).cloned() {
                let end = put(h.text, self.line_at + it.start, cursor, &it.text);
                let mut st = egui::text_edit::TextEditState::load(&ctx, h.id).unwrap_or_default();
                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(end))));
                st.store(&ctx, h.id);
                ctx.memory_mut(|m| m.request_focus(h.id));
                self.close();
                self.seen = (hash(h.text), end);
                // A call's name in: its arguments next.
                if it.text.ends_with('(') {
                    self.pending = Some((ctx.input(|i| i.time), end));
                }
            }
            return;
        }
        let now = ctx.input(|i| i.time);
        let seen = (hash(h.text), cursor);
        let typed = seen.0 != self.seen.0;
        let moved = seen != self.seen;
        self.seen = seen;
        let lang = lang_at(h.path, h.text, cursor, h.document, h.lang);
        let (line_at, row, line) = line_around(h.text, cursor);
        let col = cursor - line_at;
        let before: Vec<char> = line.chars().take(col).collect();
        let word = word_before(&before, lang.as_deref());
        if !focused {
            self.close();
            return;
        }
        // The list follows the word being typed, and goes when the cursor
        // leaves it.
        if self.open() && moved {
            if row != self.row || line_at != self.line_at {
                self.close();
            } else {
                self.narrow(&before);
            }
        }
        let last = before.last().copied();
        let trigger = typed && match last {
            Some(c) if c.is_alphanumeric() || c == '_' => word.chars().count() >= 2 && !self.open(),
            Some('.') | Some('$') | Some('@') => lang.is_some(),
            // HTML: a tag's name after '<'; CSS: a value after 'prop:' or a
            // pseudo-class after 'a:'.
            Some('<') => matches!(lang.as_deref(), Some("html" | "xml" | "php")) || kind_of(h.path, h.text).is_some_and(crate::syntax::Kind::markup),
            Some(':') => before.len() >= 2 && before[before.len() - 2] == ':' || matches!(lang.as_deref(), Some("css" | "scss")),
            Some(' ') => matches!(lang.as_deref(), Some("css" | "scss")) && before.iter().rev().nth(1) == Some(&':') || matches!(lang.as_deref(), Some("html" | "xml" | "php" | "javascript" | "typescript")) && in_open_tag(&before),
            Some('(') | Some(',') => matches!(lang.as_deref(), Some("r" | "python" | "julia" | "rust")),
            _ => false,
        };
        if ask || trigger {
            self.pending = Some((if ask { now - 1.0 } else { now }, cursor));
        }
        // Asked once the typing pauses, for the cursor it paused at.
        if let Some((at, c)) = self.pending {
            if c != cursor {
                self.pending = if typed { Some((now, cursor)) } else { None };
            } else if now - at >= 0.12 {
                self.pending = None;
                self.ask(backend, &ctx, &h, lang.as_deref(), line_at, row, &line, col, &before);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(130));
            }
        }
        // The language's answer, for the newest ask only.
        let answer = self.slot.lock().unwrap().take();
        if let Some((seq, r)) = answer {
            if seq == self.seq {
                match r {
                    Ok(c) => {
                        let mut got: Vec<Item> = c.items.iter().enumerate().map(|(k, t)| Item { text: t.clone(), detail: c.detail.get(k).cloned().unwrap_or_default(), start: c.start, from_file: false }).collect();
                        got.retain(|i| !i.text.is_empty());
                        // The language's first, then the file's words it lacks.
                        let file: Vec<Item> = self.all.iter().filter(|i| i.from_file && !got.iter().any(|g| g.text == i.text)).cloned().collect();
                        got.extend(file);
                        self.all = got;
                        self.note.clear();
                        self.narrow(&before);
                    }
                    Err(e) => self.note = why_not(&e, lang.as_deref()),
                }
            }
        }
        if self.open() {
            self.draw(ui, &h, col);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn ask(&mut self, backend: &Backend, ctx: &egui::Context, h: &Here, lang: Option<&str>, line_at: usize, row: usize, line: &str, col: usize, before: &[char]) {
        self.seq += 1;
        self.line_at = line_at;
        self.row = row;
        // The file's own words at once; the language's when they come.
        let word = word_before(before, lang);
        let start = col - word.chars().count();
        self.all = file_words(h.text, &word).into_iter().map(|w| Item { text: w, detail: String::new(), start, from_file: true }).collect();
        self.sel = 0;
        self.narrow(before);
        let Some(lang) = lang else { return };
        if !served(lang) {
            // Answered here: the language's own words, then the file's.
            if let Some(kind) = kind_of(h.path, h.text) {
                let byte = h.text.char_indices().nth(line_at + col).map_or(h.text.len(), |(i, _)| i);
                let line_text: String = before.iter().collect();
                if let Some((start, items)) = crate::syntax::complete(kind, h.text, &line_text, byte) {
                    let mut got: Vec<Item> = items.into_iter().map(|(t, d)| Item { text: t, detail: d.to_owned(), start, from_file: false }).collect();
                    let file: Vec<Item> = self.all.iter().filter(|i| i.from_file && !got.iter().any(|g| g.text == i.text)).cloned().collect();
                    got.extend(file);
                    self.all = got;
                    self.note.clear();
                    self.narrow(before);
                }
            }
            return;
        }
        let slot = self.slot.clone();
        let seq = self.seq;
        let text = if lang == "rust" { h.text.clone() } else { String::new() };
        let q = Query::CodeComplete { path: h.path.to_owned(), lang: lang.to_owned(), line: line.to_owned(), col, text, row };
        backend.query(q, ctx, move |r: Result<Completion, String>| {
            *slot.lock().unwrap() = Some((seq, r));
        });
    }

    /// What the word before the cursor leaves of the list: an item stays when
    /// it begins with what was typed from its start, case aside.
    fn narrow(&mut self, before: &[char]) {
        let keep: Vec<Item> = self
            .all
            .iter()
            .filter(|i| {
                let typed: String = before.get(i.start.min(before.len())..).unwrap_or_default().iter().collect::<String>().to_lowercase();
                let t = i.text.to_lowercase();
                t.starts_with(&typed) && !(i.from_file && t == typed)
            })
            .take(60)
            .cloned()
            .collect();
        if keep != self.shown {
            self.sel = 0;
        }
        self.shown = keep;
    }

    /// The list under the cursor's line: the choice, what it is beside it.
    fn draw(&mut self, ui: &mut egui::Ui, h: &Here, col: usize) {
        let pos = h.origin + egui::vec2(h.char_w * col as f32, h.row_h * (self.row + 1) as f32 + 2.0);
        let mut clicked = None;
        let sel = self.sel;
        // Tooltip order: above the editor, and too small a thing for app.rs
        // to hide the App pane's page for.
        egui::Area::new(egui::Id::new("ed-complete")).order(egui::Order::Tooltip).fixed_pos(pos).interactable(true).show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(4)).show(ui, |ui| {
                ui.set_max_width(520.0);
                let rows = self.shown.len().min(10);
                egui::ScrollArea::vertical().max_height(h.row_h.max(18.0) * 1.25 * rows as f32 + 4.0).show(ui, |ui| {
                    for (k, it) in self.shown.iter().enumerate() {
                        let on = k == sel;
                        let r = ui.horizontal(|ui| {
                            ui.set_min_width(260.0);
                            let mark = if it.from_file { "○" } else { "◆" };
                            ui.label(RichText::new(mark).size(10.5).color(if it.from_file { theme::muted() } else { theme::primary() }));
                            ui.label(RichText::new(&it.text).font(FontId::monospace(13.0)).color(if on { theme::text_strong() } else { theme::text() }));
                            if !it.detail.is_empty() {
                                ui.add(egui::Label::new(RichText::new(&it.detail).font(FontId::monospace(11.5)).color(theme::muted())).truncate());
                            }
                        });
                        let rect = r.response.rect.expand2(egui::vec2(3.0, 1.0));
                        if on {
                            ui.painter().rect_filled(rect, 3.0, theme::primary().gamma_multiply(0.22));
                            r.response.scroll_to_me(None);
                        }
                        if ui.interact(rect, ui.id().with(("ed-item", k)), egui::Sense::click()).clicked() {
                            clicked = Some(k);
                        }
                    }
                });
                if !self.note.is_empty() {
                    ui.label(RichText::new(format!("only this file's words: {}", self.note)).size(11.0).color(theme::muted()));
                }
            });
        });
        if let Some(k) = clicked {
            self.sel = k;
            self.accept = Some(k);
            ui.ctx().request_repaint();
        }
    }
}

/// Why the language gave nothing, in words: the server's status number
/// off, the session's two usual answers said as what they mean.
fn why_not(e: &str, lang: Option<&str>) -> String {
    let e = e.trim_start_matches(|c: char| c.is_ascii_digit()).trim();
    let who = match lang {
        Some("r") => "R",
        Some("python") => "Python",
        Some("julia") => "Julia",
        Some("shell") => "the shell",
        _ => "the session",
    };
    match e {
        "not running" => format!("{who} has not started - run a line to start it"),
        "busy" => format!("{who} is busy running something"),
        other => other.to_owned(),
    }
}

fn hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

/// The language of the code at the cursor, as CodeComplete names it: a
/// document's chunk's, else the file's; None for a file that has no
/// language here (its own words only).
fn lang_at(path: &str, text: &str, cursor: usize, document: bool, lang: Option<Lang>) -> Option<String> {
    let word = |l: Lang| match l {
        Lang::R => "r",
        Lang::Python => "python",
        Lang::Julia => "julia",
        Lang::Shell => "shell",
    };
    if document {
        let row = text.chars().take(cursor).filter(|&c| c == '\n').count();
        return crate::ide::chunks(text).into_iter().find(|c| c.body <= row && row < c.end).and_then(|c| c.lang).map(|l| word(l).to_owned());
    }
    if path.to_ascii_lowercase().ends_with(".rs") {
        return Some("rust".into());
    }
    lang.or_else(|| Lang::of_path(path)).map(|l| word(l).to_owned()).or_else(|| kind_of(path, text).map(|k| k.word().to_owned()))
}

/// A file syntax.rs knows (JS, HTML, CSS, SCSS, JSON, SQL, YAML …).
fn kind_of(path: &str, text: &str) -> Option<crate::syntax::Kind> {
    crate::syntax::of_path(path).or_else(|| crate::syntax::of_text(text))
}

/// The languages a session or language server answers for (kernel.rs, lsp.rs);
/// the rest are answered here, from syntax.rs's vocabularies.
fn served(lang: &str) -> bool {
    matches!(lang, "r" | "python" | "julia" | "shell" | "rust")
}

/// Among a tag's attributes: `<a href="x" |`.
fn in_open_tag(before: &[char]) -> bool {
    let s: String = before.iter().collect();
    s.rfind('<').is_some_and(|lt| {
        let after = &s[lt + 1..];
        !after.contains('>') && !after.starts_with('/') && after.chars().next().is_some_and(char::is_alphabetic) && after.matches('"').count() % 2 == 0
    })
}

/// The cursor's line: the char index it starts at, its index, its text.
fn line_around(text: &str, cursor: usize) -> (usize, usize, String) {
    let mut at = 0;
    for (row, line) in text.split('\n').enumerate() {
        let n = line.chars().count();
        if cursor <= at + n {
            return (at, row, line.to_owned());
        }
        at += n + 1;
    }
    (at, 0, String::new())
}

/// The word the cursor ends: letters, digits and _ - and R's . too, which
/// its names are made of.
fn word_before(before: &[char], lang: Option<&str>) -> String {
    let dot = lang == Some("r");
    let n = before.iter().rev().take_while(|c| c.is_alphanumeric() || **c == '_' || (dot && **c == '.')).count();
    before[before.len() - n..].iter().collect()
}

/// The file's own words that begin with `typed` (case aside), longest-used
/// first: names of three letters or more, the word being typed left out.
fn file_words(text: &str, typed: &str) -> Vec<String> {
    let low = typed.to_lowercase();
    let mut count: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for w in text.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).map(|w| w.trim_matches('.')) {
        if w.chars().count() >= 3 && !w.chars().next().is_some_and(|c| c.is_ascii_digit()) && w.to_lowercase().starts_with(&low) && w != typed {
            *count.entry(w).or_default() += 1;
        }
    }
    let mut v: Vec<(&str, usize)> = count.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.into_iter().take(40).map(|(w, _)| w.to_owned()).collect()
}

/// Replace the chars from `from` to `to` with `with`; the cursor's new place.
fn put(text: &mut String, from: usize, to: usize, with: &str) -> usize {
    let byte = |c: usize| text.char_indices().nth(c).map_or(text.len(), |(b, _)| b);
    let (a, b) = (byte(from.min(to)), byte(to));
    text.replace_range(a..b, with);
    from.min(to) + with.chars().count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_and_its_language() {
        let b: Vec<char> = "x <- my.data$col".chars().collect();
        assert_eq!(word_before(&b, Some("r")), "col");
        let b: Vec<char> = "summ.ar".chars().collect();
        assert_eq!(word_before(&b, Some("r")), "summ.ar");
        assert_eq!(word_before(&b, Some("python")), "ar");
        assert_eq!(lang_at("/p/src/main.rs", "", 0, false, None).as_deref(), Some("rust"));
        assert_eq!(lang_at("/p/a.jl", "", 0, false, None).as_deref(), Some("julia"));
        // Files the consoles do not run are known by syntax.rs now (2026-11-01).
        assert_eq!(lang_at("/p/notes.toml", "", 0, false, None).as_deref(), Some("toml"));
        assert_eq!(lang_at("/p/web/site.css", "", 0, false, None).as_deref(), Some("css"));
        assert_eq!(lang_at("/p/notes.txt", "just words", 0, false, None), None);
        let qmd = "---\ntitle: x\n---\n```{python}\nimport os\n```\n";
        let cur = qmd.find("import").unwrap() + 3;
        assert_eq!(lang_at("/p/a.qmd", qmd, cur, true, None).as_deref(), Some("python"));
        assert_eq!(lang_at("/p/a.qmd", qmd, 2, true, None), None, "front matter is no chunk");
    }

    #[test]
    fn the_files_words_most_used_first() {
        let t = "penguins <- read(); summary(penguins); pen; peng; penguins_raw";
        assert_eq!(file_words(t, "pen"), vec!["penguins", "peng", "penguins_raw"]);
        assert!(file_words(t, "penguins").contains(&"penguins_raw".to_owned()));
    }

    #[test]
    fn a_refusal_is_said_plainly() {
        assert_eq!(why_not("500 not running", Some("r")), "R has not started - run a line to start it");
        assert_eq!(why_not("busy", Some("python")), "Python is busy running something");
    }

    #[test]
    fn a_choice_replaces_the_word() {
        let mut t = "x <- summ(1)\n".to_owned();
        let end = put(&mut t, 5, 9, "summary");
        assert_eq!(t, "x <- summary(1)\n");
        assert_eq!(end, 12);
        let mut u = "é = sel".to_owned();
        assert_eq!(put(&mut u, 4, 7, "select"), 10);
        assert_eq!(u, "é = select");
        assert_eq!(line_around("ab\ncd\nef", 4), (3, 1, "cd".into()));
    }
}
