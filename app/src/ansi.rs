//! A log's terminal colours, drawn (operator, 2026-11-17: "I'm not seeing
//! color in the text output of App logs in cove"). The Logs tab used to strip
//! every escape code (host.rs app_log); it now keeps the colour ones (SGR,
//! `ESC [ … m`) and this turns them into an egui LayoutJob: the 8 + 8 named
//! colours, 256-colour and true-colour, bold, reset. A line with no colour of
//! its own is coloured by what it says - R's "Error"/"Warning", a log level -
//! since base R, Shiny and most servers print plain text to a file.

use eframe::egui::{text::LayoutJob, Color32, FontId, TextFormat};

const RED: Color32 = Color32::from_rgb(0xef, 0x6b, 0x6b);
const AMBER: Color32 = Color32::from_rgb(0xe8, 0xb3, 0x4c);

/// The 16 named colours (30-37, 90-97), tuned for a dark background.
fn named(i: u8) -> Color32 {
    const C: [Color32; 16] = [
        Color32::from_rgb(0x6b, 0x72, 0x80), // black: grey, or it vanishes
        Color32::from_rgb(0xef, 0x6b, 0x6b),
        Color32::from_rgb(0x5c, 0xc9, 0x7b),
        Color32::from_rgb(0xe8, 0xb3, 0x4c),
        Color32::from_rgb(0x6c, 0x9e, 0xf8),
        Color32::from_rgb(0xc0, 0x84, 0xfc),
        Color32::from_rgb(0x4f, 0xc8, 0xd6),
        Color32::from_rgb(0xd5, 0xda, 0xe3),
        Color32::from_rgb(0x8b, 0x93, 0xa2),
        Color32::from_rgb(0xff, 0x8a, 0x8a),
        Color32::from_rgb(0x7d, 0xe0, 0x99),
        Color32::from_rgb(0xf5, 0xd0, 0x7a),
        Color32::from_rgb(0x93, 0xb8, 0xff),
        Color32::from_rgb(0xd6, 0xa8, 0xff),
        Color32::from_rgb(0x7e, 0xe0, 0xea),
        Color32::from_rgb(0xff, 0xff, 0xff),
    ];
    C[(i & 15) as usize]
}

/// xterm's 256 colours: 16 named, a 6x6x6 cube, 24 greys.
fn indexed(n: u8) -> Color32 {
    match n {
        0..=15 => named(n),
        16..=231 => {
            let v = n - 16;
            let s = |c: u8| if c == 0 { 0 } else { 55 + c * 40 };
            Color32::from_rgb(s(v / 36), s(v / 6 % 6), s(v % 6))
        }
        _ => {
            let g = 8 + (n - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
    }
}

/// The colour a plain line earns from its words, if any.
fn by_words(line: &str) -> Option<Color32> {
    let t = line.trim_start();
    let has = |w: &str| t.contains(w);
    if t.starts_with("Error") || has("ERROR") || has("[error]") || has("FATAL") || has("panicked at") || has("Traceback (most recent") {
        Some(RED)
    } else if t.starts_with("Warning") || has("WARN") || has("[warn]") || has("Deprecated") {
        Some(AMBER)
    } else {
        None
    }
}

/// Append `s` to `job` in `color` (bold drawn as the strong text colour).
fn push(job: &mut LayoutJob, s: &str, font: &FontId, color: Color32) {
    if !s.is_empty() {
        job.append(s, 0.0, TextFormat { font_id: font.clone(), color, ..Default::default() });
    }
}

/// `text` (with its SGR codes) as a LayoutJob in `font`, `base` where nothing
/// says otherwise.
pub fn job(text: &str, font: FontId, base: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    for line in text.split_inclusive('\n') {
        let mut color = if line.contains('\u{1b}') { base } else { by_words(line).unwrap_or(base) };
        let mut rest = line;
        while let Some(i) = rest.find("\u{1b}[") {
            push(&mut job, &rest[..i], &font, color);
            let after = &rest[i + 2..];
            let end = after.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(after.len());
            let (params, letter) = (&after[..end], after[end..].chars().next());
            rest = after.get(end + letter.map_or(0, char::len_utf8)..).unwrap_or("");
            if letter != Some('m') {
                continue;
            }
            let p: Vec<u16> = params.split(';').map(|x| x.parse().unwrap_or(0)).collect();
            let mut k = 0;
            while k < p.len() {
                match p[k] {
                    0 | 39 => color = base,
                    30..=37 => color = named((p[k] - 30) as u8),
                    90..=97 => color = named((p[k] - 90 + 8) as u8),
                    38 if p.get(k + 1) == Some(&5) => {
                        color = indexed(p.get(k + 2).copied().unwrap_or(7) as u8);
                        k += 2;
                    }
                    38 if p.get(k + 1) == Some(&2) => {
                        let c = |j: usize| p.get(k + j).copied().unwrap_or(0).min(255) as u8;
                        color = Color32::from_rgb(c(2), c(3), c(4));
                        k += 4;
                    }
                    _ => {}
                }
                k += 1;
            }
        }
        push(&mut job, rest, &font, color);
    }
    job
}

/// The environment that asks a program for colour though it writes to a
/// file: Node/chalk and most CLIs (FORCE_COLOR, CLICOLOR_FORCE), R's cli and
/// crayon (R_CLI_NUM_COLORS), Python's colour libraries (PY_COLORS). Set where
/// Cove runs a project's Build and Up (apps.rs).
pub const COLOR_ENV: &str = "FORCE_COLOR=1 CLICOLOR_FORCE=1 R_CLI_NUM_COLORS=256 PY_COLORS=1";

#[cfg(test)]
mod tests {
    use super::*;

    fn runs(t: &str) -> Vec<(String, Color32)> {
        let j = job(t, FontId::monospace(12.0), Color32::WHITE);
        j.sections.iter().map(|s| (j.text[s.byte_range.start.0..s.byte_range.end.0].to_owned(), s.format.color)).collect()
    }

    #[test]
    fn colours_codes_and_words() {
        let r = runs("\u{1b}[31mred\u{1b}[0m plain\n");
        assert_eq!(r[0], ("red".into(), named(1)));
        assert_eq!(r[1], (" plain\n".into(), Color32::WHITE));
        assert_eq!(runs("Error in f(): boom\n")[0].1, RED);
        assert_eq!(runs("Warning message:\n")[0].1, AMBER);
        assert_eq!(runs("Listening on http://127.0.0.1:8968\n")[0].1, Color32::WHITE);
        assert_eq!(runs("\u{1b}[38;5;196mx")[0].1, indexed(196));
        assert_eq!(runs("\u{1b}[38;2;1;2;3mx")[0].1, Color32::from_rgb(1, 2, 3));
        // A cursor code is dropped, not shown.
        assert_eq!(runs("a\u{1b}[2Kb").iter().map(|x| x.0.clone()).collect::<String>(), "ab");
    }
}
