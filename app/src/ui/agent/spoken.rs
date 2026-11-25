//! Where each word of a message came from (operator, 2026-11-19: "a clearer
//! color on the screen for transcribed audio or typed in ... agent should know
//! what's coming from where"). A message is a run of typed and spoken pieces:
//!
//! - in the composer, the pieces the mic dictated are remembered as strings and
//!   found again in the text, so an edit that rewrites one makes it typed -
//!   the colour follows what is on screen, with no offsets to keep right;
//! - sent, spoken pieces are wrapped in `<spoken>...</spoken>` with a one-line
//!   note first, so the agent reads dictation as dictation (a name may be
//!   misheard) and typing as exact;
//! - the bubble reads the tags back: spoken in the spoken colour and italic,
//!   typed in the strong ink, the note left out (agent.rs shown_prompt).

use eframe::egui::{self, text::LayoutJob, Color32, FontId, TextFormat};

pub const OPEN: &str = "<spoken>";
pub const CLOSE: &str = "</spoken>";

/// First line of a message that carries spoken pieces: for the agent, never
/// shown in the bubble.
pub const NOTE: &str = "(Parts of this message are inside <spoken> tags: dictated by voice and transcribed, so a word may be misheard - names of sites and agents especially. Read them for what was meant. Everything outside the tags was typed, exactly as written.)";

/// Byte ranges of `text` that are one of `pieces`, in order, not overlapping.
/// Each piece is found once, from where the last ended.
pub fn ranges(text: &str, pieces: &[String]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for p in pieces {
        let p = p.trim();
        if p.is_empty() {
            continue;
        }
        let mut from = 0;
        while let Some(i) = text[from..].find(p) {
            let (s, e) = (from + i, from + i + p.len());
            if out.iter().all(|&(a, b)| e <= a || s >= b) {
                out.push((s, e));
                break;
            }
            from = s + p.len().max(1);
            while !text.is_char_boundary(from) {
                from += 1;
            }
        }
    }
    out.sort_unstable();
    out
}

/// `text` with its spoken pieces tagged, and the note before it; unchanged
/// when none of `pieces` is in it.
pub fn mark(text: &str, pieces: &[String]) -> String {
    let rs = ranges(text, pieces);
    if rs.is_empty() {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len() + 64);
    let mut at = 0;
    for (s, e) in rs {
        out.push_str(&text[at..s]);
        out.push_str(OPEN);
        out.push_str(&text[s..e]);
        out.push_str(CLOSE);
        at = e;
    }
    out.push_str(&text[at..]);
    out
}

/// The message as it goes to the agent: note first when anything is spoken.
pub fn for_agent(marked: &str) -> String {
    // Listen's pieces open with a note of their own, which says it already.
    if marked.contains(OPEN) && !marked.starts_with('(') {
        format!("{NOTE}\n\n{marked}")
    } else {
        marked.to_owned()
    }
}

/// (spoken?, text) runs of a tagged message, empty runs dropped.
pub fn segments(marked: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    let mut rest = marked;
    while let Some(i) = rest.find(OPEN) {
        if i > 0 {
            out.push((false, &rest[..i]));
        }
        let after = &rest[i + OPEN.len()..];
        match after.find(CLOSE) {
            Some(j) => {
                out.push((true, &after[..j]));
                rest = &after[j + CLOSE.len()..];
            }
            None => {
                out.push((true, after));
                rest = "";
            }
        }
    }
    if !rest.is_empty() {
        out.push((false, rest));
    }
    out
}

/// The message without its tags (Copy, Resend).
pub fn plain(marked: &str) -> String {
    marked.replace(OPEN, "").replace(CLOSE, "")
}

/// The text as one laid-out run: spoken in `spoken` and italic, typed in
/// `typed`. `wrap` is the width in points.
pub fn job(segs: &[(bool, &str)], font: &FontId, typed: Color32, spoken: Color32, wrap: f32) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    for (is_spoken, text) in segs {
        let fmt = TextFormat { font_id: font.clone(), color: if *is_spoken { spoken } else { typed }, italics: *is_spoken, ..Default::default() };
        job.append(text, 0.0, fmt);
    }
    job
}

/// The composer's text in the same two inks, for a TextEdit layouter.
pub fn composer_job(text: &str, pieces: &[String], font: &FontId, typed: Color32, spoken: Color32, wrap: f32) -> LayoutJob {
    let mut segs: Vec<(bool, &str)> = Vec::new();
    let mut at = 0;
    for (s, e) in ranges(text, pieces) {
        if s > at {
            segs.push((false, &text[at..s]));
        }
        segs.push((true, &text[s..e]));
        at = e;
    }
    if at < text.len() || segs.is_empty() {
        segs.push((false, &text[at..]));
    }
    job(&segs, font, typed, spoken, wrap)
}

/// Make `ui`'s fonts lay `job` out (a TextEdit layouter's body).
pub fn galley(ui: &egui::Ui, job: LayoutJob) -> std::sync::Arc<egui::Galley> {
    ui.fonts_mut(|f| f.layout_job(job))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pieces(p: &[&str]) -> Vec<String> {
        p.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn dictation_is_found_where_it_sits_and_edits_make_it_typed() {
        let t = "fix the header\n\nthe cove one and I could say something";
        let p = pieces(&["the cove one and I could say something"]);
        assert_eq!(mark(t, &p), "fix the header\n\n<spoken>the cove one and I could say something</spoken>");
        // Edited: no longer the dictated words, so it is typed.
        let edited = "fix the header\n\nthe Cove one and I could say something";
        assert_eq!(mark(edited, &p), edited);
        assert!(!for_agent(edited).contains("<spoken>"));
        assert!(for_agent(&mark(t, &p)).starts_with(NOTE));
        // Listen's own note stands in for it, and stays first.
        let listen = "(Said aloud in Cove while I keep talking.)\n\n<spoken>fix it</spoken>";
        assert_eq!(for_agent(listen), listen);
    }

    #[test]
    fn a_repeated_piece_is_marked_once_each_and_never_overlaps() {
        let t = "yes\n\nyes";
        assert_eq!(mark(t, &pieces(&["yes", "yes"])), "<spoken>yes</spoken>\n\n<spoken>yes</spoken>");
        assert_eq!(mark("héllo wörld", &pieces(&["wörld"])), "héllo <spoken>wörld</spoken>");
    }

    #[test]
    fn tags_read_back_and_strip() {
        let m = "do this <spoken>and the thing I said</spoken> now";
        assert_eq!(segments(m), vec![(false, "do this "), (true, "and the thing I said"), (false, " now")]);
        assert_eq!(plain(m), "do this and the thing I said now");
        assert_eq!(segments("typed only"), vec![(false, "typed only")]);
        assert_eq!(segments("<spoken>cut off"), vec![(true, "cut off")]);
    }
}
