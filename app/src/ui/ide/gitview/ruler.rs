//! The overview ruler beside the diff: the whole page in a narrow strip,
//! a tick for each change in its colour, the part on screen as a frame, and
//! a click anywhere to go there.

use eframe::egui::{self, Sense, Stroke};

use super::render::Line;
use crate::project::diff::model::{Model, RowKind};
use crate::ui::kit::theme;

pub const WIDTH: f32 = 10.0;

/// Where on the page each change's ticks go: (first line, last line, kind)
/// with kind 1 added, 2 removed, 3 both.
fn ticks(m: &Model, lines: &[Line]) -> Vec<(usize, usize, u8)> {
    let mut out: Vec<(usize, usize, u8)> = Vec::new();
    for (li, l) in lines.iter().enumerate() {
        let rows: Vec<usize> = match l {
            Line::Uni(i) => vec![*i],
            Line::Pair(a, b) => a.iter().chain(b).copied().collect(),
            Line::In(crate::project::diff::model::InlineRow::One(i)) => vec![*i],
            Line::In(crate::project::diff::model::InlineRow::Edit { del, add }) => vec![*del, *add],
            _ => continue,
        };
        let kind = rows.iter().fold(0u8, |k, &r| {
            k | match m.rows[r].kind {
                RowKind::Add => 1,
                RowKind::Del => 2,
                _ => 0,
            }
        });
        if kind == 0 {
            continue;
        }
        // Runs of one kind: removed red, added green, a side-by-side pair
        // that is both split down the middle.
        match out.last_mut() {
            Some(t) if t.1 + 1 == li && t.2 == kind => t.1 = li,
            _ => out.push((li, li, kind)),
        }
    }
    out
}

/// Draw it at `rect`. `view` is (offset, view height, content height) from
/// the page; returns the page offset a click asks for.
pub fn show(ui: &mut egui::Ui, rect: egui::Rect, m: &Model, lines: &[Line], row_h: f32, view: (f32, f32, f32)) -> Option<f32> {
    let resp = ui.interact(rect, ui.id().with("git-ruler"), Sense::click_and_drag());
    let p = ui.painter_at(rect);
    p.vline(rect.left() + 0.5, rect.y_range(), Stroke::new(1.0, theme::moss().gamma_multiply(0.6)));
    let total = (lines.len().max(1) as f32 * row_h).max(view.2).max(1.0);
    let y_of = |px: f32| rect.top() + px / total * rect.height();
    let (l, r, mid) = (rect.left() + 2.0, rect.right() - 1.0, rect.center().x + 0.5);
    for (a, b, kind) in ticks(m, lines) {
        let (t, bt) = (y_of(a as f32 * row_h), y_of((b + 1) as f32 * row_h));
        let bt = bt.max(t + 2.0);
        let spans: &[(f32, f32, egui::Color32)] = match kind {
            1 => &[(l, r, theme::ok())],
            2 => &[(l, r, theme::alt())],
            _ => &[(l, mid, theme::alt()), (mid, r, theme::ok())],
        };
        for &(x0, x1, c) in spans {
            p.rect_filled(egui::Rect::from_min_max(egui::pos2(x0, t), egui::pos2(x1, bt)), 1.0, c.gamma_multiply(0.7));
        }
    }
    let (off, vh, _) = view;
    let frame = egui::Rect::from_min_max(egui::pos2(rect.left() + 1.0, y_of(off)), egui::pos2(rect.right(), y_of(off + vh)));
    p.rect_filled(frame, 2.0, theme::text().gamma_multiply(0.06));
    p.rect_stroke(frame, 2.0, Stroke::new(1.0, theme::hush().gamma_multiply(0.5)), egui::StrokeKind::Inside);
    let resp = resp.on_hover_text("every change in the file - click to go there");
    if resp.clicked() || resp.dragged() {
        let y = resp.interact_pointer_pos()?.y;
        let at = (y - rect.top()) / rect.height() * total;
        return Some((at - vh / 2.0).max(0.0));
    }
    None
}
