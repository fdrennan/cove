//! Drag to select on a chart (operator, 2026-10-15: "across the graphs we can
//! select horizontal or vertical space where it makes sense to look at things
//! more clearly"). A drag paints a band and, let go, zooms to it - plotly's
//! rule for which way: a nearly flat drag takes a stretch of time, a nearly
//! upright one a stretch of the vertical axis, anything else the box. A chart
//! whose vertical axis means nothing to zoom passes `allow_y = false` and every
//! drag is a stretch of time. Shift+drag or the middle button pans instead.

use eframe::egui::{self, Id, Pos2, Rect, Stroke};

use crate::theme;

/// What a let-go drag selected, in screen coordinates.
pub enum Pick {
    X(f32, f32),
    Y(f32, f32),
    Both(Rect),
}

pub enum Drag {
    None,
    /// Pixels moved this frame, to pan by.
    Pan(egui::Vec2),
    /// Mid-drag: the band is painted, nothing changes yet.
    Brushing,
    Pick(Pick),
}

/// Under this many pixels across (or up) a drag stays one-dimensional.
const FLAT: f32 = 12.0;

fn pick(a: Pos2, b: Pos2, allow_y: bool) -> Option<Pick> {
    let d = b - a;
    if d.x.abs() < 3.0 && d.y.abs() < 3.0 {
        return None;
    }
    let (x0, x1) = (a.x.min(b.x), a.x.max(b.x));
    let (y0, y1) = (a.y.min(b.y), a.y.max(b.y));
    Some(if !allow_y || d.y.abs() < FLAT {
        Pick::X(x0, x1)
    } else if d.x.abs() < FLAT {
        Pick::Y(y0, y1)
    } else {
        Pick::Both(Rect::from_min_max(egui::pos2(x0, y0), egui::pos2(x1, y1)))
    })
}

/// Follow a drag on `resp` over `plot`. `id` names the chart, so a drag that
/// began on one of its rows (the gantt's rows are each their own widget) is
/// the same brush wherever the pointer goes.
pub fn drag(ui: &egui::Ui, resp: &egui::Response, plot: Rect, id: Id, allow_y: bool) -> Drag {
    let key = id.with("brush-origin");
    let panning = ui.input(|i| i.modifiers.shift) || resp.dragged_by(egui::PointerButton::Middle);
    if resp.drag_started() && !panning {
        let at = ui.input(|i| i.pointer.press_origin()).or(resp.interact_pointer_pos());
        ui.ctx().data_mut(|d| d.insert_temp(key, at.map(|p| p.clamp(plot.min, plot.max))));
    }
    let origin: Option<Pos2> = ui.ctx().data(|d| d.get_temp::<Option<Pos2>>(key)).flatten();
    let now = resp.interact_pointer_pos().or(ui.input(|i| i.pointer.latest_pos())).map(|p| p.clamp(plot.min, plot.max));
    if resp.drag_stopped() {
        ui.ctx().data_mut(|d| d.remove::<Option<Pos2>>(key));
        return match (origin, now) {
            (Some(a), Some(b)) => pick(a, b, allow_y).map_or(Drag::None, Drag::Pick),
            _ => Drag::None,
        };
    }
    if !resp.dragged() {
        return Drag::None;
    }
    let (Some(a), Some(b)) = (origin.filter(|_| !panning), now) else {
        return Drag::Pan(resp.drag_delta());
    };
    let band = match pick(a, b, allow_y) {
        None => return Drag::Brushing,
        Some(Pick::X(x0, x1)) => Rect::from_x_y_ranges(x0..=x1, plot.y_range()),
        Some(Pick::Y(y0, y1)) => Rect::from_x_y_ranges(plot.x_range(), y0..=y1),
        Some(Pick::Both(r)) => r,
    };
    // Painted above the chart's own layers, so it shows over the gantt's rows.
    let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, key)).with_clip_rect(plot);
    p.rect_filled(band, 0.0, theme::primary().gamma_multiply(0.14));
    p.rect_stroke(band, 0.0, Stroke::new(1.0, theme::primary().gamma_multiply(0.7)), egui::StrokeKind::Inside);
    Drag::Brushing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drag_picks_its_direction() {
        let a = egui::pos2(100.0, 100.0);
        assert!(pick(a, egui::pos2(101.0, 101.0), true).is_none());
        assert!(matches!(pick(a, egui::pos2(300.0, 105.0), true), Some(Pick::X(100.0, 300.0))));
        assert!(matches!(pick(a, egui::pos2(104.0, 20.0), true), Some(Pick::Y(20.0, 100.0))));
        assert!(matches!(pick(a, egui::pos2(300.0, 200.0), true), Some(Pick::Both(_))));
        // A chart with nothing to zoom vertically: every drag is time.
        assert!(matches!(pick(a, egui::pos2(300.0, 200.0), false), Some(Pick::X(100.0, 300.0))));
    }
}
