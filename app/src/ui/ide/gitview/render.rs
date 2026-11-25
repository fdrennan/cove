//! The diff drawn: the model's rows as lines on the page, in one of three
//! modes, set in the editor's own type and colours. Quiet by design (the
//! brief: "an annotated technical manuscript"): line numbers subdued, added
//! and removed lines on a faint wash of green or red, the words that changed
//! a shade stronger, each hunk headed by a margin note with its actions, and
//! unchanged code folded to a single line that opens on request. Drawing
//! only: what a click asks for goes back to gitview.rs as an `Intent`.

use eframe::egui::{self, text::LayoutJob, Color32, FontId, Sense, Stroke, TextFormat};

use crate::project::diff::model::{self, Folds, InlineRow, Model, RowKind, Seg};
use crate::project::diff::words::Op;
use crate::ui::kit::{bar, icons::Glyph, theme};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    Split,
    #[default]
    Unified,
    Inline,
}

/// One line of the page.
#[derive(Clone, Debug, PartialEq)]
pub enum Line {
    /// A hunk's margin note: where it is, and its actions.
    Head(usize),
    /// Unchanged rows folded away (`run` names the fold).
    Fold { hidden: std::ops::Range<usize>, run: usize },
    /// One row, unified.
    Uni(usize),
    /// Left and right, side by side.
    Pair(Option<usize>, Option<usize>),
    /// Inline: a row, or an edited line as one.
    In(InlineRow),
}

/// What a hunk can have done to it here, as gitview.rs decides by where the
/// diff came from (staged, unstaged, a version).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HunkAct {
    Stage,
    Unstage,
    Revert,
    Copy,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FoldAct {
    Up,
    Down,
    All,
}

/// What a click on the page asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Intent {
    Hunk(usize, HunkAct),
    Fold(usize, FoldAct),
}

/// How far a fold opens per click.
pub const FOLD_STEP: usize = 20;

/// The page for `mode`: the model's segments, each hunk headed by its note.
pub fn lines(m: &Model, folds: &Folds, mode: Mode) -> Vec<Line> {
    let mut out = Vec::new();
    for seg in model::segments(m, folds) {
        match seg {
            Seg::Fold { hidden, run } => out.push(Line::Fold { hidden, run }),
            Seg::Rows(r) => {
                // Cut at hunk starts, so each note sits above its hunk.
                let mut cuts: Vec<usize> = m.hunk_rows.iter().map(|h| h.start).filter(|s| r.contains(s)).collect();
                cuts.push(r.end);
                let mut from = r.start;
                for cut in cuts {
                    if cut > from {
                        add_rows(m, from..cut, mode, &mut out);
                    }
                    if let Some(h) = m.hunk_rows.iter().position(|h| h.start == cut) {
                        out.push(Line::Head(h));
                    }
                    from = cut;
                }
            }
        }
    }
    out
}

fn add_rows(m: &Model, r: std::ops::Range<usize>, mode: Mode, out: &mut Vec<Line>) {
    match mode {
        Mode::Unified => out.extend(r.map(Line::Uni)),
        Mode::Split => out.extend(model::side_by_side(m, r).into_iter().map(|(a, b)| Line::Pair(a, b))),
        Mode::Inline => out.extend(model::inline(m, r).into_iter().map(Line::In)),
    }
}

/// The line a row is drawn on.
pub fn line_of_row(lines: &[Line], row: usize) -> Option<usize> {
    lines.iter().position(|l| match l {
        Line::Uni(i) => *i == row,
        Line::Pair(a, b) => *a == Some(row) || *b == Some(row),
        Line::In(InlineRow::One(i)) => *i == row,
        Line::In(InlineRow::Edit { del, add }) => *del == row || *add == row,
        _ => false,
    })
}

/// How a file's text is coloured: by its language, as the editor does.
pub struct Look {
    lang: Option<crate::api::Lang>,
    kind: Option<crate::ui::editor::syntax::Kind>,
}

impl Look {
    pub fn of(path: &str) -> Look {
        let lang = crate::api::Lang::of_path(path);
        Look { lang, kind: if lang.is_none() { crate::ui::editor::syntax::of_path(path) } else { None } }
    }

    fn job(&self, text: &str) -> LayoutJob {
        match (self.lang, self.kind) {
            (Some(l), _) => crate::ui::ide::highlight(text, Some(l)),
            (None, Some(k)) => crate::ui::ide::highlight_kind(text, k),
            _ => crate::ui::ide::highlight(text, None),
        }
    }
}

/// The washes: faint for a line, stronger for the words that changed.
fn wash(add: bool, strong: bool) -> Color32 {
    let c = if add { theme::ok() } else { theme::alt() };
    let light = theme::is_light();
    c.gamma_multiply(match (strong, light) {
        (false, false) => 0.09,
        (false, true) => 0.12,
        (true, false) => 0.26,
        (true, true) => 0.30,
    })
}

/// Tabs as four spaces, and the byte ranges moved with them.
fn expand(text: &str, marks: &[std::ops::Range<usize>]) -> (String, Vec<std::ops::Range<usize>>) {
    if !text.contains('\t') {
        return (text.to_owned(), marks.to_vec());
    }
    let shift = |b: usize| b + 3 * text[..b].matches('\t').count();
    (text.replace('\t', "    "), marks.iter().map(|r| shift(r.start)..shift(r.end)).collect())
}

/// `job` with `marks` (byte ranges) on `bg`: its sections cut at the marks.
fn mark(job: LayoutJob, marks: &[std::ops::Range<usize>], bg: Color32) -> LayoutJob {
    if marks.is_empty() {
        return job;
    }
    let mut out = LayoutJob { text: job.text.clone(), ..Default::default() };
    for s in &job.sections {
        let (start, end): (usize, usize) = (s.byte_range.start.into(), s.byte_range.end.into());
        let mut cuts: Vec<usize> = vec![start, end];
        for m in marks {
            for b in [m.start, m.end] {
                if b > start && b < end {
                    cuts.push(b);
                }
            }
        }
        cuts.sort_unstable();
        cuts.dedup();
        for w in cuts.windows(2) {
            let mut f = s.format.clone();
            if marks.iter().any(|m| m.start <= w[0] && w[1] <= m.end) {
                f.background = bg;
            }
            out.sections.push(egui::text::LayoutSection { leading_space: 0.0, byte_range: egui::text::ByteIndex(w[0])..egui::text::ByteIndex(w[1]), format: f });
        }
    }
    out
}

/// Measures of the page, from the editor's font.
pub struct Metrics {
    pub font: FontId,
    pub row_h: f32,
    pub cw: f32,
    pub digits: usize,
}

impl Metrics {
    pub fn of(ui: &egui::Ui, m: &Model) -> Metrics {
        let font = FontId::monospace(13.0);
        let (h, cw) = ui.fonts_mut(|f| (f.row_height(&font), f.glyph_width(&font, '0')));
        let biggest = m.rows.iter().filter_map(|r| r.old.max(r.new)).max().unwrap_or(1);
        // A finger or a ray needs taller lines for the margin's actions.
        let row_h = (h + 5.0).max(if crate::ui::kit::device::coarse() { 28.0 } else { 0.0 });
        Metrics { font, row_h, cw, digits: biggest.to_string().len().max(2) }
    }

    fn num_w(&self) -> f32 {
        self.cw * 0.88 * self.digits as f32 + 10.0
    }
}

/// Draw the page: `lines` in a scroll area, numbered, washed and noted.
/// `jump` scrolls so that line sits a quarter down. Returns what was asked,
/// and where the view is (offset, view height, content height) for the ruler.
#[allow(clippy::too_many_arguments)]
pub fn page(ui: &mut egui::Ui, m: &Model, lines: &[Line], mode: Mode, look: &Look, acts: &[HunkAct], jump: Option<f32>, current: Option<usize>) -> (Option<Intent>, (f32, f32, f32)) {
    let mt = Metrics::of(ui, m);
    let text_w = mt.cw * (m.widest as f32 + 4.0);
    let one = mt.num_w() * 2.0 + 14.0 + text_w;
    // Side by side, each half is half the view, long lines cut at its edge:
    // halves sized to the longest line pushed the new side off the screen.
    let width = match mode {
        Mode::Split => ui.available_width(),
        _ => one.max(ui.available_width()),
    };
    let mut intent = None;
    let mut area = egui::ScrollArea::both().id_salt("git-page").auto_shrink([false, false]);
    if let Some(y) = jump {
        area = area.vertical_scroll_offset(y);
    }
    let out = area.show_rows(ui, mt.row_h, lines.len(), |ui, range| {
        for li in range {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, mt.row_h), Sense::hover());
            let line = &lines[li];
            let on = current.is_some_and(|c| m.changes.get(c).is_some_and(|ch| line_rows(line).iter().any(|r| ch.rows.contains(r))));
            match line {
                Line::Head(h) => {
                    if let Some(a) = head(ui, rect, m, *h, acts, &mt) {
                        intent = Some(Intent::Hunk(*h, a));
                    }
                }
                Line::Fold { hidden, run } => {
                    if let Some(a) = fold(ui, rect, hidden.len(), *run, &mt) {
                        intent = Some(Intent::Fold(*run, a));
                    }
                }
                Line::Uni(i) => row(ui, rect, m, *i, look, &mt, Side::Both, on),
                Line::Pair(a, b) => {
                    let half = rect.width() / 2.0;
                    let (l, r) = (egui::Rect::from_min_size(rect.min, egui::vec2(half, rect.height())), egui::Rect::from_min_size(rect.min + egui::vec2(half, 0.0), egui::vec2(half, rect.height())));
                    for (side_rect, row_i, side) in [(l, a, Side::Old), (r, b, Side::New)] {
                        match row_i {
                            Some(i) => row(ui, side_rect, m, *i, look, &mt, side, on),
                            None => blank(ui, side_rect),
                        }
                    }
                    ui.painter().vline(rect.left() + half, rect.y_range(), Stroke::new(1.0, theme::moss().gamma_multiply(0.6)));
                }
                Line::In(InlineRow::One(i)) => row(ui, rect, m, *i, look, &mt, Side::Both, on),
                Line::In(InlineRow::Edit { del, add }) => edit(ui, rect, m, *del, *add, &mt, on),
            }
        }
    });
    let view = (out.state.offset.y, out.inner_rect.height(), out.content_size.y);
    (intent, view)
}

/// The rows a line shows.
fn line_rows(l: &Line) -> Vec<usize> {
    match l {
        Line::Uni(i) | Line::In(InlineRow::One(i)) => vec![*i],
        Line::Pair(a, b) => a.iter().chain(b).copied().collect(),
        Line::In(InlineRow::Edit { del, add }) => vec![*del, *add],
        _ => Vec::new(),
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Side {
    Old,
    New,
    Both,
}

/// One row: its numbers, a hairline in the margin when it changed, its text.
#[allow(clippy::too_many_arguments)]
fn row(ui: &egui::Ui, rect: egui::Rect, m: &Model, i: usize, look: &Look, mt: &Metrics, side: Side, current: bool) {
    let r = &m.rows[i];
    let p = ui.painter_at(rect);
    let (add, del) = (r.kind == RowKind::Add, r.kind == RowKind::Del);
    if add || del {
        p.rect_filled(rect, 0.0, wash(add, false));
    }
    let nw = mt.num_w();
    let numc = theme::hush().gamma_multiply(if add || del { 0.95 } else { 0.7 });
    let small = FontId::monospace(11.5);
    let y = rect.center().y;
    let mut x = rect.left();
    let num = |n: Option<u32>| n.map(|n| n.to_string()).unwrap_or_default();
    if side != Side::New {
        p.text(egui::pos2(x + nw - 6.0, y), egui::Align2::RIGHT_CENTER, num(r.old), small.clone(), numc);
        x += nw;
    }
    if side != Side::Old {
        p.text(egui::pos2(x + nw - 6.0, y), egui::Align2::RIGHT_CENTER, num(r.new), small.clone(), numc);
        x += nw;
    }
    // The margin: a hairline in the change's colour, brighter on the one
    // the navigation is at.
    if add || del {
        let c = if add { theme::ok() } else { theme::alt() };
        p.vline(x + 3.0, rect.y_range(), Stroke::new(if current { 3.0 } else { 2.0 }, c.gamma_multiply(if current { 0.95 } else { 0.55 })));
    }
    x += 14.0;
    if let RowKind::Gap(n) = r.kind {
        p.text(egui::pos2(x, y), egui::Align2::LEFT_CENTER, format!("{n} lines not in this diff"), FontId::proportional(11.5), theme::hush());
        return;
    }
    let (text, marks) = expand(&r.text, &r.marks);
    let job = mark(look.job(&text), &marks, wash(add, true));
    let g = ui.fonts_mut(|f| f.layout_job(job));
    p.galley(egui::pos2(x, y - g.size().y / 2.0), g, theme::text());
    if r.no_eol {
        let w = mt.cw * text.chars().count() as f32;
        p.text(egui::pos2(x + w + 6.0, y), egui::Align2::LEFT_CENTER, "no newline at end of file", FontId::proportional(10.5), theme::hush());
    }
}

/// The empty side of a pair: a quieter ground, so the eye skips it.
fn blank(ui: &egui::Ui, rect: egui::Rect) {
    ui.painter().rect_filled(rect, 0.0, theme::moss().gamma_multiply(0.16));
}

/// An edited line inline: the new line, its removed words struck through
/// before the words that took their place.
fn edit(ui: &egui::Ui, rect: egui::Rect, m: &Model, del: usize, add: usize, mt: &Metrics, current: bool) {
    let p = ui.painter_at(rect);
    let (o, n) = (&m.rows[del], &m.rows[add]);
    let nw = mt.num_w();
    let small = FontId::monospace(11.5);
    let y = rect.center().y;
    let numc = theme::hush().gamma_multiply(0.95);
    p.text(egui::pos2(rect.left() + nw - 6.0, y), egui::Align2::RIGHT_CENTER, o.old.map(|v| v.to_string()).unwrap_or_default(), small.clone(), numc);
    p.text(egui::pos2(rect.left() + nw * 2.0 - 6.0, y), egui::Align2::RIGHT_CENTER, n.new.map(|v| v.to_string()).unwrap_or_default(), small, numc);
    let x = rect.left() + nw * 2.0;
    // Half red, half green: both happened on this line.
    let mid = rect.center().y;
    p.line_segment([egui::pos2(x + 3.0, rect.top()), egui::pos2(x + 3.0, mid)], Stroke::new(if current { 3.0 } else { 2.0 }, theme::alt().gamma_multiply(0.7)));
    p.line_segment([egui::pos2(x + 3.0, mid), egui::pos2(x + 3.0, rect.bottom())], Stroke::new(if current { 3.0 } else { 2.0 }, theme::ok().gamma_multiply(0.7)));
    let mut job = LayoutJob::default();
    let plain = TextFormat { font_id: mt.font.clone(), color: theme::text(), ..Default::default() };
    for piece in m.pieces.get(&del).map(Vec::as_slice).unwrap_or(&[]) {
        let (text, fmt) = match piece.op {
            Op::Same => (&n.text[piece.new.clone()], plain.clone()),
            Op::Add => (&n.text[piece.new.clone()], TextFormat { background: wash(true, true), color: theme::text_strong(), ..plain.clone() }),
            Op::Del => (&o.text[piece.old.clone()], TextFormat { background: wash(false, true), color: theme::alt(), strikethrough: Stroke::new(1.0, theme::alt().gamma_multiply(0.8)), ..plain.clone() }),
        };
        job.append(&text.replace('\t', "    "), 0.0, fmt);
    }
    let g = ui.fonts_mut(|f| f.layout_job(job));
    p.galley(egui::pos2(x + 14.0, y - g.size().y / 2.0), g, theme::text());
}

/// A hunk's margin note: its ranges and section quiet on the left, its
/// actions as small glyphs after them - always shown, for a finger or a ray.
fn head(ui: &mut egui::Ui, rect: egui::Rect, m: &Model, h: usize, acts: &[HunkAct], mt: &Metrics) -> Option<HunkAct> {
    let hunk = &m.hunks[h];
    let p = ui.painter_at(rect);
    let y = rect.center().y;
    p.hline(rect.x_range(), rect.top() + 0.5, Stroke::new(1.0, theme::moss().gamma_multiply(0.5)));
    let note = format!("−{},{}  +{},{}", hunk.old_start, hunk.old_len, hunk.new_start, hunk.new_len);
    let x0 = rect.left() + mt.num_w() * 2.0 + 14.0;
    let g = ui.fonts_mut(|f| f.layout_no_wrap(note, FontId::monospace(11.0), theme::hush()));
    let mut x = x0;
    p.galley(egui::pos2(x, y - g.size().y / 2.0), g.clone(), theme::hush());
    x += g.size().x + 10.0;
    if !hunk.section.is_empty() {
        let s = ui.fonts_mut(|f| f.layout_no_wrap(hunk.section.clone(), FontId::new(11.5, egui::FontFamily::Proportional), theme::muted()));
        p.galley(egui::pos2(x, y - s.size().y / 2.0), s.clone(), theme::muted());
        x += s.size().x + 10.0;
    }
    let side = mt.row_h;
    let mut picked = None;
    for (k, a) in acts.iter().enumerate() {
        let (g, tip) = match a {
            HunkAct::Stage => (Glyph::Plus, "Stage this hunk"),
            HunkAct::Unstage => (Glyph::Minus, "Unstage this hunk"),
            HunkAct::Revert => (Glyph::Undo, "Revert this hunk - take it out of the file"),
            HunkAct::Copy => (Glyph::Copy, "Copy this hunk as a patch"),
        };
        let r = egui::Rect::from_min_size(egui::pos2(x + k as f32 * (side + 2.0), rect.top()), egui::vec2(side, side));
        if bar::glyph_at(ui, r, ui.id().with(("hunk", h, k)), g, tip).clicked() {
            picked = Some(*a);
        }
    }
    picked
}

/// Unchanged rows folded to one quiet line: how many, and the ways to open
/// it - a step up, a step down, or all of it.
fn fold(ui: &mut egui::Ui, rect: egui::Rect, n: usize, run: usize, mt: &Metrics) -> Option<FoldAct> {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, theme::surface().gamma_multiply(0.55));
    let side = mt.row_h;
    // The three controls in the number columns, the count after them.
    let x0 = rect.left() + 4.0;
    let mut picked = None;
    let step = FOLD_STEP.min(n);
    for (k, (g, a, tip)) in [(Glyph::ChevronUp, FoldAct::Up, format!("show {step} more above")), (Glyph::ChevronDown, FoldAct::Down, format!("show {step} more below")), (Glyph::Unfold, FoldAct::All, format!("show all {n}"))].into_iter().enumerate() {
        let r = egui::Rect::from_min_size(egui::pos2(x0 + k as f32 * side, rect.top()), egui::vec2(side, side));
        if bar::glyph_at(ui, r, ui.id().with(("fold", run, k)), g, &tip).clicked() {
            picked = Some(a);
        }
    }
    let word = if n == 1 { "1 unchanged line".to_owned() } else { format!("{n} unchanged lines") };
    let after = (x0 + side * 3.0 + 10.0).max(rect.left() + mt.num_w() * 2.0 + 14.0);
    p.text(egui::pos2(after, rect.center().y), egui::Align2::LEFT_CENTER, word, FontId::proportional(11.5), theme::hush());
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::diff::{model, patch};

    fn m() -> Model {
        let mut body = String::new();
        for i in 1..=40 {
            if i == 20 {
                body.push_str("-line 20\n+line twenty\n");
            } else {
                body.push_str(&format!(" line {i}\n"));
            }
        }
        model::build(&patch::parse(&format!("--- a/f\n+++ b/f\n@@ -1,40 +1,40 @@\n{body}")).remove(0))
    }

    #[test]
    fn each_mode_heads_the_hunk_and_folds_the_rest() {
        let m = m();
        for mode in [Mode::Unified, Mode::Split, Mode::Inline] {
            let l = lines(&m, &Folds::default(), mode);
            assert!(matches!(l.first(), Some(Line::Fold { .. })), "{mode:?}: {l:?}");
            assert!(l.contains(&Line::Head(0)), "{mode:?}");
            assert!(matches!(l.last(), Some(Line::Fold { .. })), "{mode:?}");
            let change = line_of_row(&l, m.changes[0].rows.start).unwrap();
            assert_eq!(l[change - 4], Line::Head(0), "{mode:?}: the note sits above the hunk's 3 lines of context");
        }
        assert_eq!(lines(&m, &Folds::default(), Mode::Unified).len(), 1 + 1 + 3 + 2 + 3 + 1);
        assert_eq!(lines(&m, &Folds::default(), Mode::Split).len(), 1 + 1 + 3 + 1 + 3 + 1, "the pair is one line");
    }

    #[test]
    fn marks_cut_sections_and_tabs_move_them() {
        let (t, mk) = expand("\tab", &[1..2]);
        assert_eq!((t.as_str(), mk), ("    ab", vec![4..5]));
        let mut job = LayoutJob::default();
        job.append("hello world", 0.0, TextFormat::default());
        let j = mark(job, &[6..11], Color32::RED);
        let got: Vec<_> = j.sections.iter().map(|s| (usize::from(s.byte_range.start)..usize::from(s.byte_range.end), s.format.background == Color32::RED)).collect();
        assert_eq!(got, vec![(0..6, false), (6..11, true)]);
    }
}
