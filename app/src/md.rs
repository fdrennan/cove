//! Markdown, drawn: what the agent writes, formatted as it means it
//! (operator, 2026-10-15: "text being returned by the agent in cove.ndexr.io
//! should be formatted well and properly, commons.ndexr.io does this well").
//! commons.ndexr.io is shinychat, which renders the model's markdown; this is
//! the same for egui - headings, paragraphs that wrap as prose, lists (nested,
//! numbered), quotes, rules, pipe tables, fenced code with a ▶ to run it in its
//! console, and inline bold, italics, `code`, ~~strike~~ and [links](…).
//!
//! It never fails: whatever it does not recognise is drawn as text, and an
//! unclosed fence or `**` (a reply still streaming in) draws as far as it goes.

use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, RichText, Sense, Stroke, TextFormat};

use crate::api::Lang;
use crate::theme;

/// One block of a reply.
#[derive(Debug, PartialEq)]
enum Block {
    Heading(u8, String),
    Para(String),
    /// (depth, marker: None for a bullet, Some(n) numbered, text)
    Item(usize, Option<String>, String),
    Quote(String),
    Code(String, String),
    Table(Vec<Vec<String>>),
    Rule,
}

fn list_marker(line: &str) -> Option<(usize, Option<String>, &str)> {
    let indent = line.len() - line.trim_start().len();
    let t = line.trim_start();
    for b in ["- ", "* ", "+ "] {
        if let Some(rest) = t.strip_prefix(b) {
            return Some((indent / 2, None, rest));
        }
    }
    let digits = t.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=3).contains(&digits) {
        let rest = &t[digits..];
        if let Some(r) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((indent / 2, Some(t[..digits].to_owned()), r));
        }
    }
    None
}

fn is_rule(t: &str) -> bool {
    let t: String = t.chars().filter(|c| !c.is_whitespace()).collect();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*') || t.chars().all(|c| c == '_'))
}

fn cells(line: &str) -> Vec<String> {
    let t = line.trim().trim_start_matches('|').trim_end_matches('|');
    t.split('|').map(|c| c.trim().to_owned()).collect()
}

fn is_table_sep(line: &str) -> bool {
    let t = line.trim();
    t.contains('-') && t.contains('|') && t.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

fn blocks(text: &str) -> Vec<Block> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut para = String::new();
    let flush = |para: &mut String, out: &mut Vec<Block>| {
        if !para.trim().is_empty() {
            out.push(Block::Para(para.trim().to_owned()));
        }
        para.clear();
    };
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            flush(&mut para, &mut out);
            let fence = &t[..3];
            let tag = t.trim_start_matches(['`', '~']).trim().to_owned();
            let mut code = String::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with(fence) {
                code.push_str(lines[i]);
                code.push('\n');
                i += 1;
            }
            out.push(Block::Code(tag, code.trim_end_matches('\n').to_owned()));
            i += 1;
            continue;
        }
        if t.is_empty() {
            flush(&mut para, &mut out);
        } else if let Some(h) = t.strip_prefix('#').map(|_| t.trim_start_matches('#')).filter(|r| r.starts_with(' ') && t.len() - r.len() <= 6) {
            flush(&mut para, &mut out);
            out.push(Block::Heading((t.len() - h.len()) as u8, h.trim().trim_end_matches('#').trim().to_owned()));
        } else if is_rule(t) && para.is_empty() {
            out.push(Block::Rule);
        } else if t.starts_with('>') {
            flush(&mut para, &mut out);
            let mut q = String::new();
            while i < lines.len() && lines[i].trim_start().starts_with('>') {
                let l = lines[i].trim_start().trim_start_matches('>').trim();
                if !q.is_empty() {
                    q.push(if l.is_empty() { '\n' } else { ' ' });
                }
                q.push_str(l);
                i += 1;
            }
            out.push(Block::Quote(q));
            continue;
        } else if t.contains('|') && lines.get(i + 1).is_some_and(|n| is_table_sep(n)) {
            flush(&mut para, &mut out);
            let mut rows = vec![cells(line)];
            i += 2;
            while i < lines.len() && lines[i].contains('|') && !lines[i].trim().is_empty() {
                rows.push(cells(lines[i]));
                i += 1;
            }
            out.push(Block::Table(rows));
            continue;
        } else if let Some((depth, marker, rest)) = list_marker(line) {
            flush(&mut para, &mut out);
            let mut item = rest.trim().to_owned();
            // Lines that continue the item: indented, not a new item or block.
            while let Some(n) = lines.get(i + 1) {
                let nt = n.trim_start();
                if nt.is_empty() || list_marker(n).is_some() || nt.starts_with("```") || nt.starts_with('#') || n.len() == nt.len() {
                    break;
                }
                item.push(' ');
                item.push_str(nt.trim());
                i += 1;
            }
            out.push(Block::Item(depth, marker, item));
        } else {
            if !para.is_empty() {
                // Markdown's soft break: a line break inside a paragraph is a
                // space, so prose flows to the width it is given. Two trailing
                // spaces or a backslash keep the break.
                para.push(if para.ends_with("  ") || para.ends_with('\\') { '\n' } else { ' ' });
                if para.ends_with("\\\n") {
                    para.truncate(para.len() - 2);
                    para.push('\n');
                }
            }
            para.push_str(line.trim());
        }
        i += 1;
    }
    flush(&mut para, &mut out);
    out
}

// ── domains: a host the person can open is a link into it ──────────────────

/// The person's domains (Ide keeps it current from ListDomains), and the one a
/// click picked, for Ide to step into (operator, 2026-10-16: "when an agent
/// references another user domain then it should be able to click on its text
/// and send that user to that domain within the application"). Only these
/// become links: any other host name stays text.
static DOMAINS: std::sync::Mutex<Option<std::collections::HashSet<String>>> = std::sync::Mutex::new(None);
static PICKED: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// What a domain link's url is: `cove-domain:<host>`, never a web address.
const DOMAIN_LINK: &str = "cove-domain:";

pub fn set_domains(hosts: impl IntoIterator<Item = String>) {
    *DOMAINS.lock().unwrap() = Some(hosts.into_iter().map(|h| h.to_ascii_lowercase()).collect());
}

/// The domain a link in a reply was clicked for, once.
pub fn take_domain() -> Option<String> {
    PICKED.lock().unwrap().take()
}

/// Each run that is not already a link, split around the person's domains
/// named in it - in prose, in `code`, inside a path - each such name a link.
fn mark_domains(runs: Vec<(String, Style)>, links: &mut Vec<(std::ops::Range<usize>, String)>) -> Vec<(String, Style)> {
    let known = DOMAINS.lock().unwrap();
    let Some(known) = known.as_ref().filter(|k| !k.is_empty()) else { return runs };
    let mut out = Vec::with_capacity(runs.len());
    let mut at = 0usize;
    for (s, st) in runs {
        let n = s.chars().count();
        if st.link {
            out.push((s, st));
            at += n;
            continue;
        }
        let c: Vec<char> = s.chars().collect();
        let host_char = |ch: char| ch.is_ascii_alphanumeric() || ch == '.' || ch == '-';
        let (mut i, mut from) = (0usize, 0usize);
        while i < c.len() {
            if !host_char(c[i]) {
                i += 1;
                continue;
            }
            let start = i;
            while i < c.len() && host_char(c[i]) {
                i += 1;
            }
            // "plots.ndexr.io." at a sentence's end: the dot is not the host's.
            let mut end = i;
            while end > start && c[end - 1] == '.' {
                end -= 1;
            }
            let word: String = c[start..end].iter().collect::<String>().to_ascii_lowercase();
            if word.contains('.') && known.contains(&word) {
                if start > from {
                    out.push((c[from..start].iter().collect(), st));
                }
                links.push((at + start..at + end, format!("{DOMAIN_LINK}{word}")));
                out.push((c[start..end].iter().collect(), Style { link: true, ..st }));
                from = end;
            }
        }
        if from < c.len() {
            out.push((c[from..].iter().collect(), st));
        }
        at += n;
    }
    out
}

// ── inline: bold, italics, code, strike, links ─────────────────────────────

#[derive(Clone, Copy, Default)]
struct Style {
    bold: bool,
    italic: bool,
    code: bool,
    strike: bool,
    link: bool,
}

/// The text as runs of one style each, and the links' (char range, url).
fn inline(text: &str) -> (Vec<(String, Style)>, Vec<(std::ops::Range<usize>, String)>) {
    let c: Vec<char> = text.chars().collect();
    let mut runs: Vec<(String, Style)> = Vec::new();
    let mut links = Vec::new();
    let mut st = Style::default();
    let mut cur = String::new();
    let mut at = 0usize; // chars emitted so far
    let push = |runs: &mut Vec<(String, Style)>, cur: &mut String, st: Style| {
        if !cur.is_empty() {
            runs.push((std::mem::take(cur), st));
        }
    };
    let closes = |from: usize, pat: &[char]| -> bool {
        (from..c.len().saturating_sub(pat.len() - 1)).any(|j| c[j..j + pat.len()] == *pat)
    };
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        // `code` - nothing inside it is markdown
        if ch == '`' {
            if let Some(end) = (i + 1..c.len()).find(|&j| c[j] == '`') {
                push(&mut runs, &mut cur, st);
                let s: String = c[i + 1..end].iter().collect();
                at += s.chars().count();
                runs.push((s, Style { code: true, ..st }));
                i = end + 1;
                continue;
            }
        }
        if ch == '\\' && i + 1 < c.len() && c[i + 1].is_ascii_punctuation() {
            cur.push(c[i + 1]);
            at += 1;
            i += 2;
            continue;
        }
        // [text](url)
        if ch == '[' && !st.link {
            if let Some(mid) = (i + 1..c.len()).find(|&j| c[j] == ']') {
                if c.get(mid + 1) == Some(&'(') {
                    if let Some(end) = (mid + 2..c.len()).find(|&j| c[j] == ')') {
                        push(&mut runs, &mut cur, st);
                        let label: String = c[i + 1..mid].iter().collect();
                        let url: String = c[mid + 2..end].iter().collect();
                        let n = label.chars().count();
                        links.push((at..at + n, url.trim().to_owned()));
                        at += n;
                        runs.push((label, Style { link: true, ..st }));
                        i = end + 1;
                        continue;
                    }
                }
            }
        }
        let two = c.get(i + 1) == Some(&ch);
        if (ch == '*' || ch == '_') && two && (st.bold || closes(i + 2, &[ch, ch])) {
            push(&mut runs, &mut cur, st);
            st.bold = !st.bold;
            i += 2;
            continue;
        }
        if ch == '~' && two && (st.strike || closes(i + 2, &['~', '~'])) {
            push(&mut runs, &mut cur, st);
            st.strike = !st.strike;
            i += 2;
            continue;
        }
        // A pair that closes nothing (`2 ** 3`) is text.
        if (ch == '*' || ch == '_' || ch == '~') && two {
            cur.push(ch);
            cur.push(ch);
            at += 2;
            i += 2;
            continue;
        }
        // *italic* / _italic_: an opener is followed by a non-space, and an
        // underscore inside a word (snake_case) is just an underscore.
        if ch == '*' || ch == '_' {
            let prev_word = i > 0 && c[i - 1].is_alphanumeric();
            let next = c.get(i + 1).copied().unwrap_or(' ');
            let opens = !st.italic && !next.is_whitespace() && !(ch == '_' && prev_word) && closes(i + 1, &[ch]);
            let ends = st.italic && i > 0 && !c[i - 1].is_whitespace() && !(ch == '_' && next.is_alphanumeric());
            if opens || ends {
                push(&mut runs, &mut cur, st);
                st.italic = !st.italic;
                i += 1;
                continue;
            }
        }
        cur.push(ch);
        at += 1;
        i += 1;
    }
    push(&mut runs, &mut cur, st);
    let runs = mark_domains(runs, &mut links);
    (runs, links)
}

fn job(text: &str, size: f32, color: Color32, bold_all: bool, width: f32) -> (LayoutJob, Vec<(std::ops::Range<usize>, String)>) {
    let (runs, links) = inline(text);
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    for (s, st) in runs {
        let font = if st.code {
            FontId::monospace(size * 0.92)
        } else if st.bold || bold_all {
            FontId::new(size, FontFamily::Name("display".into()))
        } else {
            FontId::proportional(size)
        };
        let fg = if st.link {
            theme::primary()
        } else if st.code {
            theme::amber()
        } else if st.bold {
            theme::text_strong()
        } else {
            color
        };
        job.append(
            &s,
            0.0,
            TextFormat {
                font_id: font,
                color: fg,
                italics: st.italic,
                background: if st.code { theme::surface_hi() } else { Color32::TRANSPARENT },
                underline: if st.link { Stroke::new(1.0, theme::primary().gamma_multiply(0.6)) } else { Stroke::NONE },
                strikethrough: if st.strike { Stroke::new(1.0, color) } else { Stroke::NONE },
                line_height: Some(size * 1.45),
                ..Default::default()
            },
        );
    }
    (job, links)
}

/// One run of inline text, wrapped to the width left, its links clickable.
fn text(ui: &mut egui::Ui, s: &str, size: f32, color: Color32, bold: bool) {
    let w = ui.available_width();
    text_at(ui, s, size, color, bold, w, false);
}

/// Plain text with only the person's domains made links (no markdown read):
/// what they typed in the Agents panel (agent.rs user_bubble), so a domain
/// named there opens in Cove just as one in a reply does (operator,
/// 2026-10-18: "when i reference a domain in cove.ndexr.io agents panel ...
/// they should be able to click over into it if they have access").
pub fn typed(ui: &mut egui::Ui, s: &str, size: f32, color: Color32, wrap: f32) {
    typed_parts(ui, &[(s, color)], size, wrap);
}

/// `typed`, in parts of their own colour: a message of typed and dictated words
/// (agent.rs heard_parts). Domains are links in every part.
pub fn typed_parts(ui: &mut egui::Ui, parts: &[(&str, Color32)], size: f32, wrap: f32) {
    let mut links = Vec::new();
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let font = egui::FontId::proportional(size);
    let mut base = 0usize;
    for (s, color) in parts {
        let mut mine = Vec::new();
        let runs = mark_domains(vec![((*s).to_owned(), Style::default())], &mut mine);
        links.extend(mine.into_iter().map(|(r, u)| (r.start + base..r.end + base, u)));
        base += s.chars().count();
        for (t, st) in &runs {
            let fmt = egui::TextFormat {
                font_id: font.clone(),
                color: if st.link { theme::primary() } else { *color },
                underline: if st.link { Stroke::new(1.0, theme::primary().gamma_multiply(0.6)) } else { Stroke::NONE },
                ..Default::default()
            };
            job.append(t, 0.0, fmt);
        }
    }
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let resp = ui.add(egui::Label::new(galley.clone()).sense(if links.is_empty() { Sense::hover() } else { Sense::click() }));
    if links.is_empty() {
        return;
    }
    let Some(p) = resp.hover_pos() else { return };
    let at = galley.cursor_from_pos(p - resp.rect.min).index.0;
    if let Some(host) = links.iter().find(|(r, _)| r.contains(&at)).and_then(|(_, u)| u.strip_prefix(DOMAIN_LINK)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if resp.clicked() {
            *PICKED.lock().unwrap() = Some(host.to_owned());
        }
        resp.on_hover_text(format!("open {host} in Cove"));
    }
}

/// `text`, wrapped at `wrap` rather than the room the layout offers; a table
/// cell may break inside a word, so a long path never pushes its grid past
/// the card (on a phone that widened every line after it, 2026-10-15).
fn text_at(ui: &mut egui::Ui, s: &str, size: f32, color: Color32, bold: bool, wrap: f32, anywhere: bool) {
    let (mut job, links) = job(s, size, color, bold, wrap);
    job.wrap.break_anywhere = anywhere;
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    // A label, not painted text, so a drag selects it and Ctrl/Cmd+C copies
    // (operator, 2026-10-17: "need to make it easier to copy text across the
    // app"); its links still answer a click.
    let _ = color;
    let resp = ui.add(egui::Label::new(galley.clone()).sense(if links.is_empty() { Sense::hover() } else { Sense::click() }));
    let rect = resp.rect;
    let mut hit = None;
    if !links.is_empty() {
        if let Some(p) = resp.hover_pos() {
            let at = galley.cursor_from_pos(p - rect.min).index.0;
            hit = links.iter().find(|(r, _)| r.contains(&at)).map(|(_, u)| u.clone());
        }
    }
    if let Some(url) = hit {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        match url.strip_prefix(DOMAIN_LINK) {
            // One of the person's domains: Cove steps into it.
            Some(host) => {
                if resp.clicked() {
                    *PICKED.lock().unwrap() = Some(host.to_owned());
                }
                resp.on_hover_text(format!("open {host} in Cove"));
            }
            None => {
                if resp.clicked() {
                    crate::links::new_tab(ui.ctx(), &url);
                }
                resp.on_hover_text(url);
            }
        }
    }
}

/// The language a fence names, if a console runs it: ```r and ```python to the
/// R session (Python through reticulate), ```bash / ```sh to the Terminal.
pub fn fence_lang(tag: &str) -> Option<Lang> {
    let tag = tag.trim().trim_matches(|c| c == '{' || c == '}').to_ascii_lowercase();
    match tag.split([' ', ',']).next().unwrap_or("") {
        "r" => Some(Lang::R),
        "python" | "py" => Some(Lang::Python),
        "bash" | "sh" | "shell" | "console" => Some(Lang::Shell),
        _ => None,
    }
}

/// A fenced block: its language, copy, and ▶ into the console it names.
pub fn code_block(ui: &mut egui::Ui, tag: &str, code: &str, run: &mut Vec<(Lang, String)>) {
    let lang = fence_lang(tag);
    egui::Frame::new().fill(theme::surface()).corner_radius(6).stroke(Stroke::new(1.0, theme::pane_brd())).inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let tag = tag.trim().trim_matches(|c| c == '{' || c == '}');
            ui.label(RichText::new(if tag.is_empty() { "code" } else { tag }).size(10.5).color(theme::hush()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::tool(ui, "copy", "copy it").clicked() {
                    ui.ctx().copy_text(code.to_owned());
                }
                if let Some(l) = lang.filter(|_| !code.trim().is_empty()) {
                    let to = if l == Lang::Shell { "Terminal" } else { "Console" };
                    if ui.add(theme::primary_button(&format!("▶ {to}"))).on_hover_text(format!("run it in the {to} ({})", l.word())).clicked() {
                        run.push((l, code.to_owned()));
                    }
                }
            });
        });
        egui::ScrollArea::horizontal().id_salt(("md-code", code.len(), code.get(..code.len().min(40)).unwrap_or(""))).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(code).monospace().size(12.0).color(theme::ok())).extend());
        });
    });
}

/// Draw `md` at body size `size`; fenced code that a console runs gets a ▶,
/// whose clicks land in `run`.
pub fn show(ui: &mut egui::Ui, md: &str, size: f32, run: &mut Vec<(Lang, String)>) {
    let gap = size * 0.55;
    let mut first = true;
    let mut prev_item = false;
    for b in blocks(md) {
        let item = matches!(b, Block::Item(..));
        if !first {
            // A list's items sit close; everything else breathes.
            ui.add_space(if item && prev_item { gap * 0.2 } else { gap });
        }
        first = false;
        prev_item = item;
        match b {
            Block::Heading(level, t) => {
                let s = match level {
                    1 => size * 1.38,
                    2 => size * 1.22,
                    3 => size * 1.1,
                    _ => size,
                };
                ui.add_space(gap * 0.6);
                text(ui, &t, s, theme::text_strong(), true);
            }
            Block::Para(t) => text(ui, &t, size, theme::text(), false),
            Block::Item(depth, marker, t) => {
                ui.horizontal_top(|ui| {
                    ui.add_space(depth as f32 * size * 1.3);
                    let m = match &marker {
                        Some(n) => format!("{n}."),
                        None => ["•", "◦", "▪"][depth.min(2)].to_owned(),
                    };
                    let (r, _) = ui.allocate_exact_size(egui::vec2(size * 1.4, size * 1.45), Sense::hover());
                    ui.painter().text(r.left_top() + egui::vec2(0.0, size * 0.2), egui::Align2::LEFT_TOP, m, FontId::proportional(size), if marker.is_some() { theme::muted() } else { theme::primary() });
                    ui.vertical(|ui| text(ui, &t, size, theme::text(), false));
                });
            }
            Block::Quote(t) => {
                let r = egui::Frame::new().inner_margin(egui::Margin { left: 12, right: 4, top: 2, bottom: 2 }).show(ui, |ui| {
                    text(ui, &t, size, theme::muted(), false);
                });
                let rr = r.response.rect;
                ui.painter().vline(rr.left() + 2.0, rr.y_range(), Stroke::new(3.0, theme::primary().gamma_multiply(0.5)));
            }
            Block::Code(tag, code) => code_block(ui, &tag, &code, run),
            Block::Rule => {
                ui.separator();
            }
            Block::Table(rows) => {
                // Each column wraps at its share of the card, never narrower
                // than 90 px: inside the horizontal scroll a cell was offered
                // almost no width and wrapped a letter per line (2026-10-15).
                let cols = rows.iter().map(|r| r.len()).max().unwrap_or(1).max(1) as f32;
                let wrap = ((ui.available_width() - 18.0 - 16.0 * (cols - 1.0)) / cols).max(48.0);
                egui::Frame::new().stroke(Stroke::new(1.0, theme::pane_brd())).corner_radius(4).inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                    egui::ScrollArea::horizontal().id_salt(("md-table", rows.len(), rows.first().map(|r| r.join("|")))).show(ui, |ui| {
                        egui::Grid::new(("md-grid", rows.first().map(|r| r.join("|")))).striped(true).spacing(egui::vec2(16.0, 4.0)).show(ui, |ui| {
                            for (k, row) in rows.iter().enumerate() {
                                for cell in row {
                                    text_at(ui, cell, size * 0.95, if k == 0 { theme::text_strong() } else { theme::text() }, k == 0, wrap, true);
                                }
                                ui.end_row();
                            }
                        });
                    });
                });
            }
        }
    }
}

/// The first `n` characters of `s` (a reply being revealed as it streams).
pub fn prefix(s: &str, n: usize) -> &str {
    match s.char_indices().nth(n) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_are_read_as_markdown_means_them() {
        let b = blocks("# Title\n\nSome prose that\nwraps on.\n\n- one\n  more of one\n- two\n  1. nested\n\n> quoted\n\n```r\nx <- 1\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n---\nend");
        assert_eq!(b[0], Block::Heading(1, "Title".into()));
        assert_eq!(b[1], Block::Para("Some prose that wraps on.".into()));
        assert_eq!(b[2], Block::Item(0, None, "one more of one".into()));
        assert_eq!(b[3], Block::Item(0, None, "two".into()));
        assert_eq!(b[4], Block::Item(1, Some("1".into()), "nested".into()));
        assert_eq!(b[5], Block::Quote("quoted".into()));
        assert_eq!(b[6], Block::Code("r".into(), "x <- 1".into()));
        assert_eq!(b[7], Block::Table(vec![vec!["a".into(), "b".into()], vec!["1".into(), "2".into()]]));
        assert_eq!(b[8], Block::Rule);
        assert_eq!(b[9], Block::Para("end".into()));
    }

    #[test]
    fn an_unclosed_fence_still_draws() {
        assert_eq!(blocks("look:\n```bash\nls -la"), vec![Block::Para("look:".into()), Block::Code("bash".into(), "ls -la".into())]);
    }

    #[test]
    fn the_persons_domains_are_links_into_cove() {
        set_domains(["plots.ndexr.io".to_owned(), "exec.ndexr.io".to_owned()]);
        let (runs, links) = inline("Changed plots.ndexr.io. See `exec.ndexr.io` and /d/plots.ndexr.io/module.r - not other.ndexr.io");
        let shown: String = runs.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(shown, "Changed plots.ndexr.io. See exec.ndexr.io and /d/plots.ndexr.io/module.r - not other.ndexr.io");
        let urls: Vec<&str> = links.iter().map(|l| l.1.as_str()).collect();
        assert_eq!(urls, ["cove-domain:plots.ndexr.io", "cove-domain:exec.ndexr.io", "cove-domain:plots.ndexr.io"]);
        let words: Vec<String> = links.iter().map(|(r, _)| shown.chars().skip(r.start).take(r.len()).collect()).collect();
        assert_eq!(words, ["plots.ndexr.io", "exec.ndexr.io", "plots.ndexr.io"], "the sentence's full stop is not part of the link");
        // What the person typed (md::typed) marks the same way: theirs only.
        let mut typed = Vec::new();
        let runs = mark_domains(vec![("look at plots.ndexr.io and other.ndexr.io".to_owned(), Style::default())], &mut typed);
        assert_eq!(typed.iter().map(|l| l.1.as_str()).collect::<Vec<_>>(), vec![format!("{DOMAIN_LINK}plots.ndexr.io").as_str()]);
        assert_eq!(runs.iter().map(|r| r.0.as_str()).collect::<String>(), "look at plots.ndexr.io and other.ndexr.io");
        set_domains(Vec::new());
    }

    #[test]
    fn inline_styles_and_links() {
        let (runs, links) = inline("a **bold** and *it* with `x_y` and [site](https://x.io) snake_case_name");
        let shown: String = runs.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(shown, "a bold and it with x_y and site snake_case_name");
        assert!(runs.iter().any(|(s, st)| s == "bold" && st.bold));
        assert!(runs.iter().any(|(s, st)| s == "it" && st.italic));
        assert!(runs.iter().any(|(s, st)| s == "x_y" && st.code));
        assert_eq!(links.len(), 1);
        assert_eq!(&shown.chars().skip(links[0].0.start).take(4).collect::<String>(), "site");
        // an unmatched ** is text, not a style that never ends
        let (runs, _) = inline("2 ** 3 is 8");
        assert_eq!(runs.iter().map(|r| r.0.as_str()).collect::<String>(), "2 ** 3 is 8");
    }

    #[test]
    fn fences_name_their_console() {
        assert_eq!(fence_lang("{r}"), Some(Lang::R));
        assert_eq!(fence_lang("python"), Some(Lang::Python));
        assert_eq!(fence_lang("bash"), Some(Lang::Shell));
        assert_eq!(fence_lang("toml"), None);
        assert_eq!(prefix("héllo", 2), "hé");
    }
}
