//! Cove's icons, drawn as shapes, and its register of controls. The fonts Cove
//! ships (Inter, JetBrains Mono) have no emoji, so 🎙 and 📎 drew as boxes or
//! dots on a phone (operator, 2026-10-15: "could be improved and cleaner");
//! these draw the same everywhere, at any size.
//!
//! - `Icon` + `button`: the round buttons of the agent composer.
//! - `Glyph` + `paint`: line glyphs on a 16-unit grid.
//! - `Ctl` + `ctl::*`: every control a bar draws - its glyph, word and tip -
//!   registered once; bar.rs draws them (operator, 2026-11-25: "establish a
//!   centralized icon registry and shared navigation components").

use eframe::egui::{self, Color32, Sense, Stroke};

use crate::ui::kit::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    /// Attach a file.
    Plus,
    /// Speak.
    Mic,
    /// Stop (recording, a run).
    Stop,
    /// Send.
    Send,
    /// Listen: keep the mic open (a point with sound waves either side).
    Listen,
    /// Speak: a speaker with its waves - the agent answers aloud.
    Speak,
}

/// A round button `d` across: `fill` behind, the icon in `fg`.
pub fn button(ui: &mut egui::Ui, icon: Icon, d: f32, fill: Color32, fg: Color32, enabled: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(d, d), if enabled { Sense::click() } else { Sense::hover() });
    let p = ui.painter();
    let c = rect.center();
    let hot = enabled && resp.hovered();
    let fill = if !enabled { fill.gamma_multiply(0.45) } else if hot { fill.gamma_multiply(1.25) } else { fill };
    let fg = if enabled { fg } else { fg.gamma_multiply(0.45) };
    p.circle_filled(c, d / 2.0, fill);
    let s = d / 30.0; // drawn on a 30-unit grid
    let st = Stroke::new(1.8 * s, fg);
    match icon {
        Icon::Plus => {
            p.line_segment([c - egui::vec2(6.5 * s, 0.0), c + egui::vec2(6.5 * s, 0.0)], st);
            p.line_segment([c - egui::vec2(0.0, 6.5 * s), c + egui::vec2(0.0, 6.5 * s)], st);
        }
        Icon::Mic => {
            // The capsule, the cradle under it, the stem and the foot.
            let cap = egui::Rect::from_center_size(c - egui::vec2(0.0, 3.0 * s), egui::vec2(6.0 * s, 11.0 * s));
            p.rect_stroke(cap, 3.0 * s, st, egui::StrokeKind::Middle);
            let r = 6.5 * s;
            let ctr = c - egui::vec2(0.0, 1.5 * s);
            let pts: Vec<egui::Pos2> = (0..=12).map(|i| {
                let a = std::f32::consts::PI * i as f32 / 12.0;
                ctr + egui::vec2(-r * a.cos(), r * a.sin())
            }).collect();
            p.add(egui::Shape::line(pts, st));
            p.line_segment([ctr + egui::vec2(0.0, r), ctr + egui::vec2(0.0, r + 3.0 * s)], st);
            p.line_segment([ctr + egui::vec2(-3.5 * s, r + 3.0 * s), ctr + egui::vec2(3.5 * s, r + 3.0 * s)], st);
        }
        Icon::Stop => {
            p.rect_filled(egui::Rect::from_center_size(c, egui::vec2(9.0 * s, 9.0 * s)), 2.0 * s, fg);
        }
        Icon::Listen => {
            p.circle_filled(c, 2.2 * s, fg);
            for (r, a) in [(6.0 * s, 0.75_f32), (10.5 * s, 0.7)] {
                for side in [1.0_f32, -1.0] {
                    let pts: Vec<egui::Pos2> = (0..=8).map(|i| {
                        let t = -a + 2.0 * a * i as f32 / 8.0;
                        c + egui::vec2(side * r * t.cos(), r * t.sin())
                    }).collect();
                    p.add(egui::Shape::line(pts, st));
                }
            }
        }
        Icon::Speak => {
            // The speaker's box and cone, then two waves to its right.
            let o = c - egui::vec2(3.5 * s, 0.0);
            let body = [o + egui::vec2(-5.0 * s, -2.8 * s), o + egui::vec2(-2.0 * s, -2.8 * s), o + egui::vec2(2.5 * s, -6.5 * s),
                        o + egui::vec2(2.5 * s, 6.5 * s), o + egui::vec2(-2.0 * s, 2.8 * s), o + egui::vec2(-5.0 * s, 2.8 * s)];
            p.add(egui::Shape::convex_polygon(body.to_vec(), fg, Stroke::NONE));
            for (r, a) in [(5.5 * s, 0.7_f32), (9.5 * s, 0.65)] {
                let pts: Vec<egui::Pos2> = (0..=8).map(|i| {
                    let t = -a + 2.0 * a * i as f32 / 8.0;
                    o + egui::vec2(1.5 * s + r * t.cos(), r * t.sin())
                }).collect();
                p.add(egui::Shape::line(pts, st));
            }
        }
        Icon::Send => {
            // An arrow up.
            let top = c - egui::vec2(0.0, 7.0 * s);
            p.line_segment([c + egui::vec2(0.0, 7.0 * s), top], Stroke::new(2.2 * s, fg));
            p.line_segment([top, top + egui::vec2(-5.5 * s, 5.5 * s)], Stroke::new(2.2 * s, fg));
            p.line_segment([top, top + egui::vec2(5.5 * s, 5.5 * s)], Stroke::new(2.2 * s, fg));
        }
    }
    if hot {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// The quiet round button (attach, mic at rest).
pub fn quiet(ui: &mut egui::Ui, icon: Icon, d: f32) -> egui::Response {
    button(ui, icon, d, theme::surface_hi(), theme::muted(), true)
}

/// Line glyphs for rows, tabs and toolbars, drawn for the same reason as the
/// round buttons: ▸ ◆ ⤓ ⌂ ⤒ come from fonts, and are missing or misshapen on
/// a phone (operator, 2026-10-15: "may need to shift to icons on smaller
/// screens and devices"). Each fits a square `rect`.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Folder,
    File,
    /// A file a console runs (.R, .py, .sh, .qmd …).
    Code,
    /// A database or a table on disk.
    Data,
    /// An image or a PDF.
    Image,
    Up,
    Home,
    Refresh,
    Upload,
    Download,
    /// Full screen: four corners pointing out.
    Expand,
    /// Leave full screen: four corners pointing in.
    Collapse,
    /// Start: a play triangle.
    Play,
    /// Stop or interrupt: a square.
    Halt,
    /// Build: a hammer.
    Build,
    /// A log: lines of text.
    Logs,
    /// Open somewhere else: an arrow out of its box.
    Open,
    /// More: three dots.
    More,
    Back,
    Forward,
    /// Close or clear: a cross.
    Close,
    /// Live reload: the reload arrow round a dot.
    Live,
    /// New: a plus.
    Plus,
    /// Copy: two sheets, one over the other.
    Copy,
    /// Side by side: two columns of lines.
    Split,
    /// Unified: one column, lines out and in.
    Unified,
    /// Inline: one line with a word marked in it.
    Inline,
    ChevronUp,
    ChevronDown,
    Minus,
    /// Undo: an arrow turning back.
    Undo,
    /// Open every fold: arrows apart.
    Unfold,
}

pub fn paint(p: &egui::Painter, rect: egui::Rect, g: Glyph, c: Color32) {
    let s = rect.width().min(rect.height()) / 16.0; // a 16-unit grid
    let o = rect.center();
    let at = |x: f32, y: f32| o + egui::vec2(x * s, y * s);
    let st = Stroke::new((1.3 * s).max(1.0), c);
    let line = |pts: &[(f32, f32)]| {
        p.add(egui::Shape::line(pts.iter().map(|&(x, y)| at(x, y)).collect(), st));
    };
    let arrow = |down: bool| {
        let (a, b) = if down { (-5.0, 2.5) } else { (2.5, -5.0) };
        line(&[(0.0, a), (0.0, b)]);
        line(&[(-3.5, b + if down { -3.5 } else { 3.5 }), (0.0, b), (3.5, b + if down { -3.5 } else { 3.5 })]);
        line(&[(-5.5, 6.0), (5.5, 6.0)]);
    };
    match g {
        Glyph::Folder => {
            line(&[(-6.5, -4.5), (-2.0, -4.5), (-0.5, -3.0), (6.5, -3.0), (6.5, 5.0), (-6.5, 5.0), (-6.5, -4.5)]);
            p.rect_filled(egui::Rect::from_min_max(at(-6.5, -1.5), at(6.5, 5.0)), 0.0, c.gamma_multiply(0.35));
        }
        Glyph::File | Glyph::Code | Glyph::Data | Glyph::Image => {
            line(&[(-4.5, -6.5), (1.5, -6.5), (4.5, -3.5), (4.5, 6.5), (-4.5, 6.5), (-4.5, -6.5)]);
            line(&[(1.5, -6.5), (1.5, -3.5), (4.5, -3.5)]);
            match g {
                Glyph::Code => {
                    line(&[(-2.5, -0.5), (2.5, -0.5)]);
                    line(&[(-2.5, 2.0), (1.0, 2.0)]);
                    line(&[(-2.5, 4.5), (2.5, 4.5)]);
                }
                Glyph::Data => {
                    p.rect_filled(egui::Rect::from_min_max(at(-2.5, -1.0), at(2.5, 4.5)), 0.0, c.gamma_multiply(0.5));
                }
                Glyph::Image => line(&[(-3.0, 4.5), (-0.5, 1.0), (1.0, 3.0), (2.0, 2.0), (3.0, 4.5)]),
                _ => {}
            }
        }
        Glyph::Up => {
            line(&[(-4.0, 6.0), (2.5, 6.0), (2.5, -4.5)]);
            line(&[(-1.0, -1.0), (2.5, -4.5), (6.0, -1.0)]);
        }
        Glyph::Home => line(&[(-6.5, 0.0), (0.0, -6.0), (6.5, 0.0), (4.5, 0.0), (4.5, 6.0), (-4.5, 6.0), (-4.5, 0.0), (-6.5, 0.0)]),
        Glyph::Refresh => {
            let pts: Vec<egui::Pos2> = (0..=20).map(|i| {
                let a = 0.6 + 5.0 * i as f32 / 20.0;
                o + egui::vec2(5.5 * s * a.cos(), 5.5 * s * a.sin())
            }).collect();
            let end = *pts.last().unwrap();
            p.add(egui::Shape::line(pts, st));
            p.add(egui::Shape::line(vec![end + egui::vec2(-3.0 * s, -1.0 * s), end, end + egui::vec2(0.5 * s, 3.0 * s)], st));
        }
        Glyph::Upload => arrow(false),
        Glyph::Download => arrow(true),
        Glyph::Expand | Glyph::Collapse => {
            // One corner, turned four ways: out at the edge, or in at the middle.
            let (e, m) = if g == Glyph::Expand { (6.5, 2.5) } else { (2.5, 6.5) };
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                line(&[(sx * e, sy * m), (sx * e, sy * e), (sx * m, sy * e)]);
            }
        }
        Glyph::Play => {
            p.add(egui::Shape::convex_polygon(vec![at(-4.0, -6.0), at(6.0, 0.0), at(-4.0, 6.0)], c, Stroke::NONE));
        }
        Glyph::Halt => {
            p.rect_filled(egui::Rect::from_min_max(at(-5.0, -5.0), at(5.0, 5.0)), 1.5 * s, c);
        }
        Glyph::Build => {
            // The handle, then the head across its top.
            line(&[(-5.5, 6.5), (1.5, -0.5)]);
            p.add(egui::Shape::convex_polygon(vec![at(-1.5, -4.5), at(3.0, -7.0), at(7.0, -3.0), at(4.5, 1.5)], c, Stroke::NONE));
        }
        Glyph::Logs => {
            for (y, w) in [(-5.0, 6.5), (-1.7, 4.0), (1.6, 6.5), (4.9, 3.0)] {
                line(&[(-6.5, y), (-5.0, y)]);
                line(&[(-2.5, y), (w, y)]);
            }
        }
        Glyph::Open => {
            line(&[(-1.0, -5.5), (-6.0, -5.5), (-6.0, 6.0), (5.5, 6.0), (5.5, 1.0)]);
            line(&[(0.0, 0.0), (6.5, -6.5)]);
            line(&[(1.5, -6.5), (6.5, -6.5), (6.5, -1.5)]);
        }
        Glyph::More => {
            for x in [-5.5, 0.0, 5.5] {
                p.circle_filled(at(x, 0.0), 1.5 * s, c);
            }
        }
        Glyph::Back => line(&[(3.0, -6.0), (-3.0, 0.0), (3.0, 6.0)]),
        Glyph::Forward => line(&[(-3.0, -6.0), (3.0, 0.0), (-3.0, 6.0)]),
        Glyph::Close => {
            line(&[(-5.0, -5.0), (5.0, 5.0)]);
            line(&[(-5.0, 5.0), (5.0, -5.0)]);
        }
        Glyph::Plus => {
            line(&[(-6.0, 0.0), (6.0, 0.0)]);
            line(&[(0.0, -6.0), (0.0, 6.0)]);
        }
        Glyph::Copy => {
            line(&[(-2.5, -6.5), (6.5, -6.5), (6.5, 3.0), (-2.5, 3.0), (-2.5, -6.5)]);
            line(&[(-4.5, -3.5), (-6.5, -3.5), (-6.5, 6.5), (3.5, 6.5), (3.5, 5.0)]);
        }
        Glyph::Split => {
            line(&[(-6.5, -6.0), (-6.5, 6.0), (6.5, 6.0), (6.5, -6.0), (-6.5, -6.0)]);
            line(&[(0.0, -6.0), (0.0, 6.0)]);
            for y in [-2.5, 1.0] {
                line(&[(-4.5, y), (-2.0, y)]);
                line(&[(2.0, y), (4.5, y)]);
            }
        }
        Glyph::Unified => {
            line(&[(-6.5, -6.0), (-6.5, 6.0), (6.5, 6.0), (6.5, -6.0), (-6.5, -6.0)]);
            for (y, w) in [(-3.0, 3.0), (0.0, 4.5), (3.0, 2.0)] {
                line(&[(-4.5, y), (w, y)]);
            }
        }
        Glyph::Inline => {
            line(&[(-6.5, -6.0), (-6.5, 6.0), (6.5, 6.0), (6.5, -6.0), (-6.5, -6.0)]);
            line(&[(-4.5, 0.0), (4.5, 0.0)]);
            p.rect_filled(egui::Rect::from_min_max(at(-1.0, -2.0), at(3.0, 2.0)), 1.0 * s, c.gamma_multiply(0.6));
        }
        Glyph::ChevronUp => line(&[(-5.0, 2.5), (0.0, -2.5), (5.0, 2.5)]),
        Glyph::ChevronDown => line(&[(-5.0, -2.5), (0.0, 2.5), (5.0, -2.5)]),
        Glyph::Minus => line(&[(-6.0, 0.0), (6.0, 0.0)]),
        Glyph::Undo => {
            let pts: Vec<egui::Pos2> = (0..=14).map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 14.0;
                o + egui::vec2(1.0 * s + 5.0 * s * a.cos(), 0.5 * s + 5.0 * s * a.sin())
            }).collect();
            p.add(egui::Shape::line(pts, st));
            line(&[(1.0, -4.5), (-5.0, -4.5)]);
            line(&[(-2.5, -7.0), (-5.0, -4.5), (-2.5, -2.0)]);
        }
        Glyph::Unfold => {
            line(&[(-4.5, -3.0), (0.0, -7.0), (4.5, -3.0)]);
            line(&[(-4.5, 3.0), (0.0, 7.0), (4.5, 3.0)]);
            line(&[(-6.0, 0.0), (6.0, 0.0)]);
        }
        Glyph::Live => {
            let pts: Vec<egui::Pos2> = (0..=20).map(|i| {
                let a = 0.6 + 5.0 * i as f32 / 20.0;
                o + egui::vec2(6.0 * s * a.cos(), 6.0 * s * a.sin())
            }).collect();
            let end = *pts.last().unwrap();
            p.add(egui::Shape::line(pts, st));
            p.add(egui::Shape::line(vec![end + egui::vec2(-3.0 * s, -1.0 * s), end, end + egui::vec2(0.5 * s, 3.0 * s)], st));
            p.circle_filled(o, 2.0 * s, c);
        }
    }
}

/// A frameless toolbar button: the glyph, then `label` when there is room
/// for words. The tip always names it.
pub fn tool(ui: &mut egui::Ui, g: Glyph, label: Option<&str>, tip: &str) -> egui::Response {
    let font = egui::FontId::proportional(12.0);
    let galley = label.map(|l| ui.painter().layout_no_wrap(l.to_owned(), font, theme::text()));
    let w = 18.0 + galley.as_ref().map_or(0.0, |g| g.size().x + 4.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w + 6.0, 20.0), Sense::click());
    let hot = resp.hovered();
    if hot {
        ui.painter().rect_filled(rect, 3.0, theme::surface_hi());
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let c = if hot { theme::text_strong() } else { theme::muted() };
    let icon = egui::Rect::from_center_size(egui::pos2(rect.left() + 12.0, rect.center().y), egui::vec2(14.0, 14.0));
    paint(ui.painter(), icon, g, c);
    if let Some(gal) = galley {
        ui.painter().galley(egui::pos2(icon.right() + 4.0, rect.center().y - gal.size().y / 2.0), gal, c);
    }
    resp.on_hover_text(tip)
}

/// A control, registered once: its glyph, its word (shown, or its accessible
/// name and tip when only the glyph is), and what it does. Bars draw these
/// through bar.rs; a new control is a new constant here, not a new button.
#[derive(Clone, Copy)]
pub struct Ctl {
    pub glyph: Glyph,
    pub word: &'static str,
    pub tip: &'static str,
}

impl Ctl {
    pub const fn new(glyph: Glyph, word: &'static str, tip: &'static str) -> Ctl {
        Ctl { glyph, word, tip }
    }
    /// The same control with a tip that names its object.
    pub fn tip(self, tip: &'static str) -> Ctl {
        Ctl { tip, ..self }
    }
}

pub mod ctl {
    use super::{Ctl, Glyph};
    pub const START: Ctl = Ctl::new(Glyph::Play, "Start", "start the app and wait until it answers (.cove/up.sh)");
    pub const RESTART: Ctl = Ctl::new(Glyph::Refresh, "Restart", "stop it (.cove/down.sh), then start it again (.cove/up.sh)");
    pub const STOP: Ctl = Ctl::new(Glyph::Halt, "Stop", "stop the app (.cove/down.sh); the agents keep working");
    pub const BUILD: Ctl = Ctl::new(Glyph::Build, "Build", "compile or check it (.cove/build.sh); it never starts or stops the app");
    pub const LOGS: Ctl = Ctl::new(Glyph::Logs, "Logs", "the app's own output, and the last Build's");
    pub const JOB_LOG: Ctl = Ctl::new(Glyph::Logs, "View log", "what the running job prints");
    pub const LIVE: Ctl = Ctl::new(Glyph::Live, "Live reload", "on: the site reloads when its files change; off: only when you ask");
    pub const RELOAD_SITE: Ctl = Ctl::new(Glyph::Refresh, "Reload", "reload the site");
    pub const OPEN_OUT: Ctl = Ctl::new(Glyph::Open, "Open", "open in a new browser tab");
    pub const COPY_ADDRESS: Ctl = Ctl::new(Glyph::Copy, "Copy address", "copy the site's address");
    pub const INTERRUPT: Ctl = Ctl::new(Glyph::Halt, "Interrupt", "interrupt · Esc");
    pub const FULLSCREEN: Ctl = Ctl::new(Glyph::Expand, "Full screen", "full screen (F11)");
    pub const EXIT_FULLSCREEN: Ctl = Ctl::new(Glyph::Collapse, "Exit full screen", "exit full screen (F11)");
    // The Git window's diff (ui/ide/gitview).
    pub const DIFF_SPLIT: Ctl = Ctl::new(Glyph::Split, "Side by side", "the old file on the left, the new on the right");
    pub const DIFF_UNIFIED: Ctl = Ctl::new(Glyph::Unified, "Unified", "one column: removed lines, then the lines that took their place");
    pub const DIFF_INLINE: Ctl = Ctl::new(Glyph::Inline, "Inline", "an edited line as one line, its removed words struck through");
    pub const PREV_CHANGE: Ctl = Ctl::new(Glyph::ChevronUp, "Previous change", "the change before · Shift+F7 or Alt+↑");
    pub const NEXT_CHANGE: Ctl = Ctl::new(Glyph::ChevronDown, "Next change", "the change after · F7 or Alt+↓");
    pub const UNFOLD: Ctl = Ctl::new(Glyph::Unfold, "Every line", "show the unchanged lines too, or fold them again");
    pub const STAGE: Ctl = Ctl::new(Glyph::Plus, "Stage", "put it in the next version");
    pub const UNSTAGE: Ctl = Ctl::new(Glyph::Minus, "Unstage", "take it back out of the next version; the file keeps it");
    pub const REVERT: Ctl = Ctl::new(Glyph::Undo, "Revert", "take it out of the file");
    pub const COPY_PATCH: Ctl = Ctl::new(Glyph::Copy, "Copy patch", "the patch, to paste anywhere");
    pub const EXPORT_PATCH: Ctl = Ctl::new(Glyph::Download, "Export patch", "save the patch as a file");
    pub const APPLY_PATCH: Ctl = Ctl::new(Glyph::Upload, "Apply a patch", "paste or open a patch and apply it to the project's files");
    pub const NEW_TAB: Ctl = Ctl::new(Glyph::Plus, "New tab", "go to a project, or create one (Projects)");
    pub const VERSIONS: Ctl = Ctl::new(Glyph::Undo, "Versions", "every saved version of this project: see what changed, restore one");
}
