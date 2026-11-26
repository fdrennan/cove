//! Cove's bars: every toolbar, tab strip and nav row draws its controls here,
//! so one control looks, sizes and behaves the same wherever it appears
//! (operator, 2026-11-25: "visually heavy, crowded, and inconsistent ... make
//! the design system sustainable so that future features do not reintroduce
//! the same clutter").
//!
//! The rules, which the functions below enforce rather than suggest:
//!
//! - A control is registered once (`icons::Ctl`: glyph, word, what it does),
//!   and drawn by `button` / `toggle` / `menu_item`, never as a framed
//!   `egui::Button` in a bar. Framed buttons are for dialogs and forms.
//! - Its `Weight` says how it is drawn:
//!   - `Primary`: the one thing the bar is for at this moment (Start a stopped
//!     app). Glyph and word, on a tint of the accent. At most one per bar.
//!   - `Secondary`: glyph and word at a desk; the glyph alone on a narrow
//!     screen, where the word is still its accessible name and its tip. A
//!     switch (`toggle`) keeps its word everywhere: its state is read.
//!   - `Icon`: a glyph everyone reads the same (back, forward, reload, open
//!     out, close, more): the glyph alone, everywhere.
//! - What a bar has no room for goes in its ⋯ (`more`), drawn with the same
//!   glyphs and words, never dropped.
//! - One row: `toolbar` never wraps. Its left holds where you are and what
//!   you act on; its right, ways out and the ⋯.
//! - Sizes come from `metrics`: 24 points tall at a desk, 36 on a touch screen
//!   or headset (device.rs), so a finger or a ray finds every target.

use eframe::egui::{self, Color32, Response, Sense, Stroke};

use super::icons::{self, Ctl};
use super::{device, theme};

/// How much a control matters in its bar; see the module's rules.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Weight {
    Primary,
    Secondary,
    Icon,
}

/// Every bar's measures, from the device and the room there is.
#[derive(Clone, Copy)]
pub struct Metrics {
    /// A control's height, and the least width of a glyph-only one.
    pub h: f32,
    pub glyph: f32,
    pub font: f32,
    /// Room either side of a control's content.
    pub pad: f32,
    /// A phone's width: Secondary controls show their glyph alone.
    pub narrow: bool,
}

/// Where Cove lays out for a phone (app.rs COMPACT_W).
pub const NARROW_W: f32 = 760.0;

pub fn metrics(ctx: &egui::Context) -> Metrics {
    let narrow = ctx.content_rect().width() < NARROW_W;
    if device::coarse() {
        Metrics { h: 36.0, glyph: 17.0, font: 13.5, pad: 9.0, narrow }
    } else if narrow {
        Metrics { h: 32.0, glyph: 16.0, font: 13.0, pad: 8.0, narrow }
    } else {
        Metrics { h: 24.0, glyph: 14.0, font: 12.5, pad: 6.0, narrow }
    }
}

/// Whether a control of this weight shows its word here.
pub fn shows_word(m: &Metrics, w: Weight) -> bool {
    match w {
        Weight::Primary => true,
        Weight::Secondary => !m.narrow,
        Weight::Icon => false,
    }
}

/// The ink and the fill a control takes in a state.
fn paint_state(ui: &egui::Ui, rect: egui::Rect, resp: &Response, enabled: bool, on: bool, primary: bool) -> (Color32, Color32) {
    let p = ui.painter();
    let hot = enabled && resp.hovered();
    let down = enabled && resp.is_pointer_button_down_on();
    let radius = 6.0;
    if primary || on {
        let a = if down { 0.30 } else if hot { 0.24 } else { 0.16 };
        p.rect_filled(rect, radius, theme::primary().gamma_multiply(a));
    } else if down {
        p.rect_filled(rect, radius, theme::moss());
    } else if hot {
        p.rect_filled(rect, radius, theme::lift(theme::surface_hi(), if theme::is_light() { 0.0 } else { 0.08 }));
    }
    if resp.has_focus() {
        p.rect_stroke(rect.shrink(0.5), radius, Stroke::new(1.5, theme::primary()), egui::StrokeKind::Inside);
    }
    let (glyph, text) = if !enabled {
        (theme::hush().gamma_multiply(0.6), theme::hush().gamma_multiply(0.6))
    } else if primary || on {
        (theme::primary(), theme::text_strong())
    } else if hot {
        (theme::text_strong(), theme::text_strong())
    } else {
        (theme::muted(), theme::text())
    };
    (glyph, text)
}

fn draw(ui: &mut egui::Ui, glyph: icons::Glyph, word_of: &str, tip_of: &str, weight: Weight, enabled: bool, on: Option<bool>) -> Response {
    let m = metrics(ui.ctx());
    // A switch keeps its word unless it is an Icon: what it turns on must be
    // read, not guessed from a lit glyph.
    let word = (shows_word(&m, weight) || (on.is_some() && weight != Weight::Icon)).then_some(word_of).filter(|w| !w.is_empty());
    let font = egui::FontId::proportional(m.font);
    let galley = word.map(|w| ui.painter().layout_no_wrap(w.to_owned(), font, Color32::WHITE));
    let content = m.glyph + galley.as_ref().map_or(0.0, |g| g.size().x + 6.0);
    let w = (content + m.pad * 2.0).max(m.h);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, m.h), if enabled { Sense::click() } else { Sense::hover() });
    let (gc, tc) = paint_state(ui, rect, &resp, enabled, on == Some(true), weight == Weight::Primary);
    let left = rect.center().x - content / 2.0;
    let g = egui::Rect::from_center_size(egui::pos2(left + m.glyph / 2.0, rect.center().y), egui::vec2(m.glyph, m.glyph));
    icons::paint(ui.painter(), g, glyph, gc);
    if let Some(gal) = galley {
        ui.painter().galley_with_override_text_color(egui::pos2(g.right() + 6.0, rect.center().y - gal.size().y / 2.0), gal, tc);
    }
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    // Its name for a screen reader, the word whether or not it is shown.
    let (name, sel) = (if word_of.is_empty() { tip_of.to_owned() } else { word_of.to_owned() }, on);
    resp.widget_info(|| match sel {
        Some(s) => egui::WidgetInfo::selected(egui::WidgetType::Checkbox, enabled, s, &name),
        None => egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name),
    });
    // A glyph alone is named in its tip; a shown word needs only what it does.
    let tip = if word.is_some() || word_of.is_empty() { tip_of.to_owned() } else { format!("{word_of} - {tip_of}") };
    resp.on_hover_text(tip)
}

/// A control in a bar.
pub fn button(ui: &mut egui::Ui, ctl: &Ctl, weight: Weight) -> Response {
    draw(ui, ctl.glyph, ctl.word, ctl.tip, weight, true, None)
}

/// Greyed and unclickable unless `enabled`.
pub fn button_if(ui: &mut egui::Ui, enabled: bool, ctl: &Ctl, weight: Weight) -> Response {
    draw(ui, ctl.glyph, ctl.word, ctl.tip, weight, enabled, None)
}

/// A control whose tip names what it acts on ("restart R - a clean session").
pub fn button_tip(ui: &mut egui::Ui, enabled: bool, ctl: &Ctl, weight: Weight, tip: &str) -> Response {
    draw(ui, ctl.glyph, ctl.word, tip, weight, enabled, None)
}

/// A glyph alone, named by its tip: what `theme::tool` draws for a symbol.
pub fn glyph(ui: &mut egui::Ui, g: icons::Glyph, tip: &str, enabled: bool) -> Response {
    draw(ui, g, "", tip, Weight::Icon, enabled, None)
}

/// A glyph alone at a rect the caller laid out - inside a row of a list
/// that paints by rect (the diff's hunk and fold lines). Same states and
/// name as `glyph`.
pub fn glyph_at(ui: &mut egui::Ui, rect: egui::Rect, id: egui::Id, g: icons::Glyph, tip: &str) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let (gc, _) = paint_state(ui, rect, &resp, true, false, false);
    let side = (rect.height() * 0.62).min(16.0);
    icons::paint(ui.painter(), egui::Rect::from_center_size(rect.center(), egui::vec2(side, side)), g, gc);
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let name = tip.to_owned();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &name));
    resp.on_hover_text(tip)
}

/// A switch: lit while `on`. Clicked = flipped by the caller.
pub fn toggle(ui: &mut egui::Ui, ctl: &Ctl, on: bool, weight: Weight) -> Response {
    draw(ui, ctl.glyph, ctl.word, ctl.tip, weight, true, Some(on))
}

/// The bar's ⋯: what it has no room for, as menu rows.
pub fn more<R>(ui: &mut egui::Ui, tip: &str, body: impl FnOnce(&mut egui::Ui) -> R) -> Option<R> {
    let m = metrics(ui.ctx());
    let btn = egui::Button::new("").frame(false).min_size(egui::vec2(m.h, m.h));
    let (r, inner) = egui::containers::menu::MenuButton::from_button(btn).ui(ui, |ui| {
        theme::chrome_menu(ui, 220.0);
        body(ui)
    });
    let g = egui::Rect::from_center_size(r.rect.center(), egui::vec2(m.glyph, m.glyph));
    let c = if r.hovered() { theme::text_strong() } else { theme::muted() };
    icons::paint(ui.painter(), g, icons::Glyph::More, c);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "More"));
    r.on_hover_text(tip);
    inner.map(|i| i.inner)
}

/// A row in a ⋯ or any Cove menu: the control's glyph, then its word.
pub fn menu_item(ui: &mut egui::Ui, ctl: &Ctl, enabled: bool) -> bool {
    let tall = if device::coarse() { 40.0 } else { 30.0 };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(180.0), tall), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, theme::lift(theme::surface_hi(), if theme::is_light() { 0.0 } else { 0.12 }));
    }
    let ink = if enabled { theme::text() } else { theme::hush() };
    let g = egui::Rect::from_center_size(rect.left_center() + egui::vec2(20.0, 0.0), egui::vec2(15.0, 15.0));
    icons::paint(ui.painter(), g, ctl.glyph, if enabled { theme::muted() } else { theme::hush() });
    ui.painter().text(rect.left_center() + egui::vec2(38.0, 0.0), egui::Align2::LEFT_CENTER, ctl.word, egui::FontId::proportional(13.0), ink);
    let name = ctl.word.to_owned();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &name));
    let clicked = resp.on_hover_text(ctl.tip).clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// One bar on one row that never wraps: `right()` is laid out from the right
/// edge first and keeps its room; `left()` then gets what is left, clipped.
/// Draw the right side first, then take the left.
pub struct Toolbar {
    right: egui::Ui,
    inner: egui::Rect,
    clip: egui::Rect,
    left_ui: Option<egui::Ui>,
}

impl Toolbar {
    pub fn new(ui: &mut egui::Ui, fill: Color32) -> Toolbar {
        Toolbar::shaped(ui, fill, 0)
    }

    /// A pane's head: the bar at the top of a window (theme::pane), in the
    /// pane-header ink and rounded with the window, so it is the window's
    /// own top, not a strip laid over it with square corners.
    pub fn top(ui: &mut egui::Ui) -> Toolbar {
        Toolbar::shaped(ui, theme::pane_hdr(), theme::WIN_RADIUS)
    }

    fn shaped(ui: &mut egui::Ui, fill: Color32, round: u8) -> Toolbar {
        let m = metrics(ui.ctx());
        let h = m.h + 6.0;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::hover());
        ui.painter().rect_filled(rect, egui::CornerRadius { nw: round, ne: round, sw: 0, se: 0 }, fill);
        ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, theme::pane_brd()));
        let inner = rect.shrink2(egui::vec2(6.0, 3.0));
        let clip = ui.clip_rect();
        let mut right = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::right_to_left(egui::Align::Center)));
        right.spacing_mut().item_spacing.x = 2.0;
        right.set_clip_rect(inner.intersect(clip));
        let left_ui = Some(ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center))));
        Toolbar { right, inner, clip, left_ui }
    }

    /// The right end, laid out right to left: ways out and the ⋯.
    pub fn right(&mut self) -> &mut egui::Ui {
        &mut self.right
    }

    /// The rest of the row, left to right: where you are, what you act on.
    pub fn left(&mut self) -> egui::Ui {
        let used = self.right.min_rect().width();
        let room = self.inner.with_max_x((self.inner.right() - used - 8.0).max(self.inner.left()));
        let mut l = self.left_ui.take().expect("Toolbar::left taken twice");
        l.set_max_width(room.width());
        l.set_clip_rect(room.intersect(self.clip));
        l.spacing_mut().item_spacing.x = 4.0;
        l
    }
}

/// A bar's name: the title strong, then a count or a short state, quieter.
/// One type size for every head (History, a version, a file), so the heads
/// of a window line up and read as one family.
pub fn title(ui: &mut egui::Ui, title: &str, detail: &str) {
    let m = metrics(ui.ctx());
    ui.add(egui::Label::new(egui::RichText::new(title).font(theme::display(m.font + 0.5)).color(theme::text_strong())).truncate());
    if !detail.is_empty() {
        let g = ui.painter().layout_no_wrap(detail.to_owned(), egui::FontId::proportional(m.font - 1.5), theme::muted());
        let (r, _) = ui.allocate_exact_size(g.size() + egui::vec2(12.0, 4.0), Sense::hover());
        ui.painter().rect_filled(r, 8.0, theme::lift(theme::surface_hi(), 0.04));
        ui.painter().galley(r.center() - g.size() / 2.0, g, theme::muted());
    }
}
