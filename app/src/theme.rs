//! ndexr Regolith, drawn natively: the palette, type and shape of
//! Inlet 0012's `ndexr_regolith.rstheme` (operator, 2026-10-13: "sleek,
//! modern, we like regolith os"), so cove.ndexr.io and inlet.ndexr.io are one
//! look. Flat charcoal panes tiled on a darker ground with i3-style gaps,
//! square corners, one teal accent and an amber focus mark; Inter for the
//! interface, JetBrains Mono for code.
//!
//! The chrome is RStudio's, at RStudio's density: a pane is a bordered box
//! whose first row is its tab strip, then an optional toolbar row, then the
//! content (`pane`, `strip`, `tab`, `tool`).

use eframe::egui::{self, Align, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Layout, Rect, Response, Sense, Stroke, TextStyle, UiBuilder};

// ── the palettes ──
// One table, every look Cove has: ndexr Regolith (Inlet 0012's
// ndexr_regolith.rstheme), ndexr Light (the site's warm paper,
// src/css/_light.scss), and well-loved editor themes beside them (operator,
// 2026-10-16: "hard to read code in light mode, review theme settings and add
// multiple beautiful themes as options"). Every code colour and the body text
// clear 4.5:1 against what they sit on - the dim text (muted, hush, comment)
// too, since 2026-10-18: hush was 3:1 and it carries the small labels, hard to
// read for anyone with weaker eyes (operator: "make sure that we have those
// [accessibility issues] covered"). Accents 3:1. `tests::text_contrast` holds
// every look to it; a published colour that fell short (Tokyo Night's and
// Nord's comments, Gruvbox's red) is nudged just enough to pass. The person picks one
// at first start (setup.rs) and under View > Theme.

#[derive(Clone, Copy)]
pub struct Palette {
    pub name: &'static str,
    /// Built on egui's light visuals; light looks get opaque panes.
    pub light: bool,
    pub ground: u32,
    pub bg: u32,
    pub surface: u32,
    pub surface_hi: u32,
    pub moss: u32,
    pub text: u32,
    pub text_strong: u32,
    pub muted: u32,
    pub hush: u32,
    pub primary: u32,
    pub amber: u32,
    pub ok: u32,
    pub warn: u32,
    pub net: u32,
    pub alt: u32,
    /// Code: what it sits on, and its colours.
    pub code: u32,
    pub keyword: u32,
    pub operator: u32,
    pub number: u32,
    pub function: u32,
    pub string: u32,
    pub comment: u32,
}

/// Index 0 is the default dark look and 5 the default light one
/// (setup.rs `ThemePref::palette`).
pub const PALETTES: &[Palette] = &[
    Palette { name: "ndexr Regolith", light: false, ground: 0x15171c, bg: 0x1d2027, surface: 0x191c22, surface_hi: 0x252932, moss: 0x2f343e, text: 0xd5dae3, text_strong: 0xeef1f5, muted: 0x98a0ae, hush: 0x8a909e, primary: 0x5ec4c0, amber: 0xe8a75d, ok: 0xa3c585, warn: 0xe8a75d, net: 0x7aa7e0, alt: 0xe5767a, code: 0x1a1d23, keyword: 0xb99ce0, operator: 0x5ec4c0, number: 0xe8a75d, function: 0x7aa7e0, string: 0xa3c585, comment: 0x8990a0 },
    Palette { name: "Tokyo Night", light: false, ground: 0x16161e, bg: 0x1a1b26, surface: 0x1f2335, surface_hi: 0x292e42, moss: 0x3b4261, text: 0xc0caf5, text_strong: 0xe3e8ff, muted: 0xa9b1d6, hush: 0x8c94bd, primary: 0x7aa2f7, amber: 0xe0af68, ok: 0x9ece6a, warn: 0xe0af68, net: 0x7dcfff, alt: 0xf7768e, code: 0x1a1b26, keyword: 0xbb9af7, operator: 0x89ddff, number: 0xff9e64, function: 0x7aa2f7, string: 0x9ece6a, comment: 0x8b94c1 },
    Palette { name: "Nord", light: false, ground: 0x242933, bg: 0x2e3440, surface: 0x2b303b, surface_hi: 0x3b4252, moss: 0x434c5e, text: 0xd8dee9, text_strong: 0xeceff4, muted: 0xb9c0cf, hush: 0xa5aec1, primary: 0x88c0d0, amber: 0xebcb8b, ok: 0xa3be8c, warn: 0xebcb8b, net: 0x81a1c1, alt: 0xd0757e, code: 0x2e3440, keyword: 0x81a1c1, operator: 0x8fbcbb, number: 0xb792b0, function: 0x88c0d0, string: 0xa3be8c, comment: 0xa4aec1 },
    Palette { name: "Catppuccin Mocha", light: false, ground: 0x11111b, bg: 0x1e1e2e, surface: 0x181825, surface_hi: 0x313244, moss: 0x45475a, text: 0xcdd6f4, text_strong: 0xeef1ff, muted: 0xa6adc8, hush: 0x949ab4, primary: 0x94e2d5, amber: 0xfab387, ok: 0xa6e3a1, warn: 0xf9e2af, net: 0x89b4fa, alt: 0xf38ba8, code: 0x1e1e2e, keyword: 0xcba6f7, operator: 0x89dceb, number: 0xfab387, function: 0x89b4fa, string: 0xa6e3a1, comment: 0x949ab3 },
    Palette { name: "Gruvbox Dark", light: false, ground: 0x1d2021, bg: 0x282828, surface: 0x232323, surface_hi: 0x3c3836, moss: 0x504945, text: 0xebdbb2, text_strong: 0xfbf1c7, muted: 0xc0b195, hush: 0xafa088, primary: 0x8ec07c, amber: 0xfabd2f, ok: 0xb8bb26, warn: 0xfe8019, net: 0x83a598, alt: 0xfb4934, code: 0x282828, keyword: 0xfb5d4a, operator: 0x8ec07c, number: 0xd3869b, function: 0xfabd2f, string: 0xb8bb26, comment: 0xafa089 },
    Palette { name: "ndexr Light", light: true, ground: 0xe9e3d6, bg: 0xfdfbf6, surface: 0xf3eee4, surface_hi: 0xffffff, moss: 0xe0d8c7, text: 0x1c1f2b, text_strong: 0x0f1220, muted: 0x4f5567, hush: 0x606576, primary: 0x1a716d, amber: 0xa45f0c, ok: 0x27783f, warn: 0xa45f0c, net: 0x335f96, alt: 0xb73434, code: 0xfdfbf6, keyword: 0x6b3fa8, operator: 0x17706c, number: 0x965009, function: 0x275a9c, string: 0x36702a, comment: 0x606575 },
    Palette { name: "GitHub Light", light: true, ground: 0xeaeef2, bg: 0xffffff, surface: 0xf6f8fa, surface_hi: 0xffffff, moss: 0xd0d7de, text: 0x1f2328, text_strong: 0x010409, muted: 0x565f69, hush: 0x646c75, primary: 0x0969da, amber: 0x9a6700, ok: 0x1a7f37, warn: 0x9a6700, net: 0x0969da, alt: 0xcf222e, code: 0xffffff, keyword: 0xcf222e, operator: 0x0550ae, number: 0x0550ae, function: 0x8250df, string: 0x0a3069, comment: 0x646c75 },
    Palette { name: "Catppuccin Latte", light: true, ground: 0xdce0e8, bg: 0xeff1f5, surface: 0xe6e9ef, surface_hi: 0xffffff, moss: 0xccd0da, text: 0x4c4f69, text_strong: 0x2c2f45, muted: 0x52556e, hush: 0x5f627a, primary: 0x1e66f5, amber: 0xb85a0a, ok: 0x2e7d1e, warn: 0xa55400, net: 0x1b6fa3, alt: 0xc8102f, code: 0xeff1f5, keyword: 0x7d2fe0, operator: 0x0f767c, number: 0xb54c08, function: 0x1e5fe6, string: 0x2e7d1e, comment: 0x5f6279 },
    // Operator, 2026-10-26: "change the theme to hawaiian". Night over the
    // lagoon: deep Pacific blue panes, sand-coloured text, a turquoise accent,
    // sunset orange, palm green and hibiscus pink.
    Palette { name: "Hawaiian", light: false, ground: 0x081826, bg: 0x0f2537, surface: 0x0c2031, surface_hi: 0x173349, moss: 0x23465f, text: 0xf3ebdc, text_strong: 0xfffaf0, muted: 0xcac1ae, hush: 0xb7ae9c, primary: 0x2fd3c4, amber: 0xffb347, ok: 0x84d87c, warn: 0xff9f43, net: 0x6cc4ff, alt: 0xff6f8e, code: 0x0e2335, keyword: 0xff94bc, operator: 0x2fd3c4, number: 0xffb347, function: 0x6cc4ff, string: 0x9fe28e, comment: 0xb3ab97 },
];

static CUR: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The palette in effect.
pub fn pal() -> &'static Palette {
    &PALETTES[CUR.load(std::sync::atomic::Ordering::Relaxed).min(PALETTES.len() - 1)]
}

/// Which palette is in effect (an index into `PALETTES`).
pub fn current() -> usize {
    CUR.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether a light palette is in effect.
pub fn is_light() -> bool {
    pal().light
}

pub const fn rgb(h: u32) -> Color32 {
    Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)
}

macro_rules! token {
    ($(#[$m:meta])* $name:ident) => {
        $(#[$m])*
        pub fn $name() -> Color32 {
            rgb(pal().$name)
        }
    };
}

token!(/// ground: the desktop behind the panes - menu bar, toolbar, the gaps.
    ground);
token!(/// pane: what a pane's content sits on.
    bg);
token!(/// Between the two: tab strips, hovered rows.
    surface);
token!(/// raised: menus, dialogs, buttons.
    surface_hi);
token!(/// line: hairlines.
    moss);
token!(/// text.
    text);
token!(text_strong);
token!(/// dim: secondary text.
    muted);
token!(/// mute: eyebrows, dividers, idle controls.
    hush);
token!(/// the one accent - links, focus, primary.
    primary);
token!(/// the focus mark (the active tab, the cursor), and warnings.
    amber);
token!(ok);
token!(warn);
// ── status accents ──
token!(net);
token!(alt);

// ── the RStudio chrome, in those tokens ──
/// A pane's tab strip.
pub fn pane_hdr() -> Color32 {
    surface()
}
/// Pane borders and the rules under strips.
pub fn pane_brd() -> Color32 {
    moss()
}
/// Height of a pane's tab strip, and of its toolbar row.
pub const TAB_H: f32 = 26.0;
pub const TOOL_H: f32 = 24.0;

/// Regolith's corners: square, everywhere.
const SQUARE: CornerRadius = CornerRadius::ZERO;

/// A pane is a window over ndexr.io's night photograph (backdrop.rs): its
/// --wm-radius; the photo shows in the gaps between windows, not through them.
pub const WIN_RADIUS: u8 = 7;

/// Opaque (operator's review, 2026-11-14: "use an opaque, nearly
/// textureless surface behind text, code, and menus"): the photograph shows
/// in the gaps between windows and behind the Atlas, never through a pane.
pub fn win_alpha() -> f32 {
    1.0
}
/// The gap between windows (ndexr.io's --wm-gap).
pub const WIN_GAP: i8 = 7;

/// The display face: Inter SemiBold, for headlines and numbers.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("display".into()))
}

fn fonts() -> FontDefinitions {
    let mut f = FontDefinitions::default();
    for (name, bytes) in [
        ("inter", &include_bytes!("../assets/fonts/Inter-Regular.ttf")[..]),
        ("inter-semibold", &include_bytes!("../assets/fonts/Inter-SemiBold.ttf")[..]),
        ("jetbrains-mono", &include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf")[..]),
        // The symbol blocks the UI draws with - arrows, box drawing, shapes,
        // dingbats, fullwidth forms - which neither Inter nor egui's fonts
        // fully have (operator, 2026-10-18: "we're missing icons": ＋ and ◐
        // were empty boxes). Subsets of DejaVu Sans and Noto Sans Symbols 2;
        // assets/fonts/README says how they were cut.
        ("symbols-dejavu", &include_bytes!("../assets/fonts/DejaVuSans-symbols.ttf")[..]),
        ("symbols-noto", &include_bytes!("../assets/fonts/NotoSansSymbols2-symbols.ttf")[..]),
    ] {
        f.font_data.insert(name.into(), FontData::from_static(bytes).into());
    }
    // Ours first; egui's defaults stay behind as fallbacks (emoji), and the
    // symbol fonts last, so they only ever fill a gap.
    f.families.entry(FontFamily::Proportional).or_default().insert(0, "inter".into());
    f.families.entry(FontFamily::Monospace).or_default().insert(0, "jetbrains-mono".into());
    for fam in [FontFamily::Proportional, FontFamily::Monospace] {
        let v = f.families.entry(fam).or_default();
        v.push("symbols-dejavu".into());
        v.push("symbols-noto".into());
    }
    let mut display = vec!["inter-semibold".to_owned()];
    display.extend(f.families[&FontFamily::Proportional].iter().skip(1).cloned());
    f.families.insert(FontFamily::Name("display".into()), display);
    f
}

/// Fonts and the default dark palette, at start; `set_palette` switches afterwards.
pub fn apply(ctx: &egui::Context) {
    ctx.set_fonts(fonts());
    set_palette(ctx, 0);
}

/// Put palette `i` (an index into `PALETTES`) in effect, for every widget and token.
pub fn set_palette(ctx: &egui::Context, i: usize) {
    CUR.store(i.min(PALETTES.len() - 1), std::sync::atomic::Ordering::Relaxed);
    let light = is_light();
    let mut style = (*ctx.global_style()).clone();
    let r = SQUARE;
    let shade = if light { 40 } else { 140 };

    let v = &mut style.visuals;
    *v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
    v.override_text_color = Some(text());
    v.panel_fill = ground();
    // Menus and dialogs a clear step above the panes, seen even out of the
    // corner of an eye (the review: "too many surfaces blend together").
    v.window_fill = lift(surface_hi(), if light { 0.0 } else { 0.05 });
    // Dialogs and floating windows rounded like the menus, with room inside
    // (operator, 2026-11-16: "windows need to be updated for cleaner more
    // organized visuals"): a square edge and a 6 px margin read as a pane
    // that had come loose.
    v.window_corner_radius = egui::CornerRadius::same(12);
    // Menus as a browser's: rounded, roomy (operator, 2026-11-14: "menus
    // should feel a bit like chrome, no more feeling like rstudio").
    v.menu_corner_radius = egui::CornerRadius::same(10);
    v.window_shadow = egui::epaint::Shadow { offset: [0, 12], blur: 40, spread: 0, color: Color32::from_black_alpha(shade) };
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(shade * 3 / 4) };
    v.extreme_bg_color = ground();
    v.faint_bg_color = surface();
    v.code_bg_color = surface();
    v.hyperlink_color = primary();
    v.selection.bg_fill = { let p = primary(); Color32::from_rgba_unmultiplied(p.r(), p.g(), p.b(), if light { 0x38 } else { 0x40 }) };
    v.selection.stroke = Stroke::new(1.0, primary());
    v.window_stroke = Stroke::new(1.0, moss());
    v.text_cursor.stroke = Stroke::new(2.0, amber());

    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_fill = surface();
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, moss());
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, muted());
    v.widgets.inactive.bg_fill = surface_hi();
    v.widgets.inactive.weak_bg_fill = surface_hi();
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, text());
    v.widgets.hovered.bg_fill = surface_hi();
    v.widgets.hovered.weak_bg_fill = surface_hi();
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, primary());
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, text_strong());
    v.widgets.active.bg_fill = primary();
    v.widgets.active.weak_bg_fill = primary();
    v.widgets.active.fg_stroke = Stroke::new(1.0, if light { surface_hi() } else { ground() });

    style.text_styles = [
        (TextStyle::Heading, display(18.0)),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (TextStyle::Button, FontId::new(12.5, FontFamily::Proportional)),
        (TextStyle::Small, FontId::new(11.5, FontFamily::Proportional)),
        (TextStyle::Monospace, FontId::new(12.5, FontFamily::Monospace)),
    ]
    .into();
    // RStudio's density: a toolbar row is 24 px, not a web form.
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);
    style.spacing.interact_size.y = 22.0;
    style.spacing.menu_margin = egui::Margin::symmetric(6, 8);
    style.spacing.window_margin = egui::Margin::same(14);
    // Gentle motion: panels and collapsibles ease, they do not snap.
    style.animation_time = 0.18;
    // The splitters between windows grab across the whole gap, not egui's
    // 3 px sliver at its edge (operator, 2026-10-15: "it can be hard to access
    // the borders to click and move them around").
    style.interaction.resize_grab_radius_side = f32::from(WIN_GAP as u8);

    let theme = if light { egui::Theme::Light } else { egui::Theme::Dark };
    ctx.set_style_of(theme, style);
    ctx.set_theme(theme);
}

/// `c` moved `f` of the way toward the text colour: a surface lifted (on a
/// dark look, lighter).
pub fn lift(c: Color32, f: f32) -> Color32 {
    let t = text_strong();
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * f).round() as u8;
    Color32::from_rgb(mix(c.r(), t.r()), mix(c.g(), t.g()), mix(c.b(), t.b()))
}

/// What a process is doing - one colour each, always with a mark and a word
/// (the operator's review, 2026-11-14: "define a strict semantic system ...
/// never rely on color alone"). Cyan: selected, focused, executing. Green:
/// running well, connected. Amber: needs attention, pending. Red: failed, or
/// an action that destroys. Slate: idle, stopped, disabled, informational.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Active,
    Good,
    Attention,
    Failed,
    Idle,
}

impl State {
    pub fn color(self) -> Color32 {
        match self {
            State::Active => primary(),
            State::Good => ok(),
            State::Attention => amber(),
            State::Failed => alt(),
            State::Idle => hush(),
        }
    }
}

/// A state's mark (drawn: a ring turning while active, a filled dot when
/// good, a diamond for attention, a cross for failed, a hollow ring when idle)
/// and its word. Returns the label's response.
pub fn state_mark(ui: &mut egui::Ui, st: State, word: &str) -> Response {
    let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), Sense::hover());
    let (c, p) = (r.center(), ui.painter());
    let col = st.color();
    match st {
        State::Active => {
            let t = ui.input(|i| i.time) as f32;
            let pts: Vec<egui::Pos2> = (0..=18).map(|k| {
                let a = t * 4.0 + k as f32 / 18.0 * std::f32::consts::PI * 1.5;
                c + egui::vec2(a.cos(), a.sin()) * 5.0
            }).collect();
            p.add(egui::Shape::line(pts, Stroke::new(2.0, col)));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
        State::Good => {
            p.circle_filled(c, 4.5, col);
        }
        State::Attention => {
            let d = 5.0;
            p.add(egui::Shape::convex_polygon(vec![c + egui::vec2(0.0, -d), c + egui::vec2(d, 0.0), c + egui::vec2(0.0, d), c + egui::vec2(-d, 0.0)], col, Stroke::NONE));
        }
        State::Failed => {
            let d = 4.0;
            p.line_segment([c + egui::vec2(-d, -d), c + egui::vec2(d, d)], Stroke::new(2.0, col));
            p.line_segment([c + egui::vec2(-d, d), c + egui::vec2(d, -d)], Stroke::new(2.0, col));
        }
        State::Idle => {
            p.circle_stroke(c, 4.5, Stroke::new(1.5, col));
        }
    }
    ui.label(egui::RichText::new(word).size(12.5).color(if st == State::Idle { text() } else { col }))
}

/// A menu laid out as Chrome lays its menus (operator, 2026-11-14: "menus
/// should feel a bit like chrome, no more feeling like rstudio"): rows a
/// finger tall with room either side, a soft fill under the pointer instead
/// of an outline, shortcuts quiet at the right, and air around separators.
/// Call it first thing in a menu's closure; it changes that menu only.
pub fn chrome_menu(ui: &mut egui::Ui, min_width: f32) {
    // A command menu is narrow (the review: 280-340 px; it measured the old
    // one at 600 and its Help cascade another 270).
    let room = (ui.ctx().content_rect().width() - 16.0).max(200.0);
    ui.set_min_width(min_width.min(320.0).min(room));
    ui.set_max_width(320.0f32.min(room));
    let s = ui.style_mut();
    s.spacing.button_padding = egui::vec2(14.0, 7.0);
    s.spacing.interact_size.y = 30.0;
    s.spacing.item_spacing.y = 0.0;
    s.spacing.menu_spacing = 2.0;
    let w = &mut s.visuals.widgets;
    for st in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        st.corner_radius = egui::CornerRadius::same(6);
        st.bg_stroke = Stroke::NONE;
    }
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    let hover = lift(surface_hi(), if is_light() { 0.0 } else { 0.12 });
    let hover = if is_light() { surface() } else { hover };
    w.hovered.weak_bg_fill = hover;
    w.hovered.bg_fill = hover;
    w.hovered.fg_stroke = Stroke::new(1.0, text_strong());
    w.open.weak_bg_fill = hover;
    w.active.weak_bg_fill = moss();
    w.active.fg_stroke = Stroke::new(1.0, text_strong());
    s.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, moss());
}

/// A separator with Chrome's breathing room.
/// A section of a menu, opening beside it the way an IDE's menus do
/// (operator, 2026-11-18: "hover over options shows highlight and auto open
/// the menu like normal menubar ide" - the in-place dropdowns before this
/// closed the whole menu on every click). Hover opens it at a desk, a tap on a
/// phone, and neither closes the menu. Its state - the theme in use, R · busy -
/// stands on the right before the arrow, in the same ink. `keep_open`: a
/// click inside does not close it either (Theme: try one, then another).
pub fn menu_fold<R>(ui: &mut egui::Ui, title: &str, state: &str, keep_open: bool, body: impl FnOnce(&mut egui::Ui) -> R) {
    use egui::containers::menu::{MenuConfig, SubMenuButton};
    let arrow = SubMenuButton::RIGHT_ARROW;
    let right = if state.is_empty() {
        arrow.to_owned()
    } else {
        let state: String = if state.chars().count() > 22 { state.chars().take(21).chain(['…']).collect() } else { state.to_owned() };
        format!("{state}   {arrow}")
    };
    let mut sub = SubMenuButton::from_button(egui::Button::new(title).right_text(right));
    if keep_open {
        sub = sub.config(MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside));
    }
    sub.ui(ui, |ui| {
        chrome_menu(ui, 240.0);
        body(ui)
    });
}

/// A theme in the menu's Theme dropdown: a chip in the look's own colours
/// (its ground, pane, keyword and string), its name, and a tick when it is
/// the one in use. Clicked = picked.
pub fn theme_row(ui: &mut egui::Ui, name: &str, pl: &Palette, picked: bool, tall: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), if tall { 38.0 } else { 30.0 }), egui::Sense::click());
    let p = ui.painter();
    if picked || resp.hovered() {
        p.rect_filled(r, 6.0, if picked { primary().gamma_multiply(0.18) } else { surface() });
    }
    let chip = egui::Rect::from_min_size(r.left_center() + egui::vec2(14.0, -8.0), egui::vec2(30.0, 16.0));
    p.rect(chip, 4.0, rgb(pl.ground), Stroke::new(1.0, rgb(pl.moss)), egui::StrokeKind::Inside);
    p.rect_filled(egui::Rect::from_min_max(chip.center_top() + egui::vec2(0.0, 3.0), chip.right_bottom() - egui::vec2(3.0, 3.0)), 2.0, rgb(pl.code));
    p.circle_filled(chip.left_center() + egui::vec2(6.0, 0.0), 3.0, rgb(pl.keyword));
    p.circle_filled(chip.left_center() + egui::vec2(12.5, 0.0), 3.0, rgb(pl.string));
    p.text(chip.right_center() + egui::vec2(10.0, 0.0), egui::Align2::LEFT_CENTER, name, egui::FontId::proportional(13.0), if picked { text_strong() } else { text() });
    if picked {
        // A drawn tick: the font's ✓ is not one every build carries.
        let c = r.right_center() - egui::vec2(20.0, 0.0);
        p.line(vec![c + egui::vec2(-5.0, 0.0), c + egui::vec2(-1.5, 3.5), c + egui::vec2(5.0, -4.0)], Stroke::new(2.0, primary()));
    }
    resp.clicked()
}

/// Text a person can select, scroll and copy: a read-only text box - drag,
/// Shift-click, Ctrl+A then Ctrl+C - for logs and job output (operator,
/// 2026-11-19: "unable to select and scroll up or select all within one of
/// those boxes"). A column of labels selected one line at a time and had no
/// select-all. `job` carries its colours; `wrap` false keeps long lines
/// whole, for a horizontal scroll.
pub fn selectable_job(ui: &mut egui::Ui, job: egui::text::LayoutJob, wrap: bool) -> egui::Response {
    let text = job.text.clone();
    let mut layouter = move |ui: &egui::Ui, _t: &dyn egui::TextBuffer, w: f32| {
        let mut j = job.clone();
        j.wrap.max_width = if wrap { w } else { f32::INFINITY };
        ui.fonts_mut(|f| f.layout_job(j))
    };
    let mut s: &str = &text;
    ui.add(
        egui::TextEdit::multiline(&mut s)
            .layouter(&mut layouter)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .desired_rows(1)
            .desired_width(if wrap { ui.available_width() } else { f32::INFINITY }),
    )
}

/// A scrolling log whose text can be selected (operator, 2026-11-19: "I cant
/// select and scroll up"): a drag inside it selects - it never scrolls the
/// log, which took the gesture before - and while a selection is dragged past
/// the log's top or bottom edge the log scrolls that way, faster the further
/// out, as a text editor does. The wheel scrolls it as always. `area` is the
/// caller's ScrollArea (its id, height, stick-to-bottom).
pub fn log_scroll<R>(area: egui::ScrollArea, ui: &mut egui::Ui, body: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let out = area.scroll_source(egui::scroll_area::ScrollSource { scroll_bar: true, mouse_wheel: true, drag: egui::scroll_area::DragScroll::Never }).show(ui, body);
    let r = out.inner_rect;
    let (down, at, from) = ui.input(|i| (i.pointer.primary_down(), i.pointer.interact_pos(), i.pointer.press_origin()));
    if let (true, Some(at), Some(from)) = (down, at, from) {
        if r.contains(from) {
            let dy = if at.y < r.top() { at.y - r.top() } else if at.y > r.bottom() { at.y - r.bottom() } else { 0.0 };
            if dy != 0.0 {
                let mut st = out.state;
                st.offset.y = (st.offset.y + dy.clamp(-40.0, 40.0) * 0.5).clamp(0.0, (out.content_size.y - r.height()).max(0.0));
                st.store(ui.ctx(), out.id);
                ui.ctx().request_repaint();
            }
        }
    }
    out.inner
}

/// `lines` in one ink, as `selectable_job`.
pub fn selectable_lines(ui: &mut egui::Ui, lines: &[String], font: egui::FontId, color: Color32, wrap: bool) -> egui::Response {
    let mut job = egui::text::LayoutJob::default();
    job.append(&lines.join("\n"), 0.0, egui::TextFormat { font_id: font, color, ..Default::default() });
    selectable_job(ui, job, wrap)
}

pub fn menu_sep(ui: &mut egui::Ui) {
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);
}

/// A filled call-to-action in the brand signal, like .btn-primary.
pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.to_owned()).color(bg()).font(display(13.0))).fill(primary()).corner_radius(SQUARE)
}

/// A pane: one bordered box filling the space it is given - and never more.
/// It draws in a child bounded by that space and claims exactly it from the
/// parent: the panes sit in resizable panels, which grow to whatever their
/// content asks for, so a pane asking a few pixels too many grew the Console
/// a little on every repaint while output streamed (2026-10-13).
pub fn pane<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let rect = ui.available_rect_before_wrap();
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    let inner = egui::Frame::new()
        .fill(bg().gamma_multiply(win_alpha()))
        .stroke(Stroke::new(1.0, pane_brd()))
        .corner_radius(CornerRadius::same(WIN_RADIUS))
        .show(&mut child, |ui| {
            ui.set_min_size(ui.available_size());
            ui.set_clip_rect(ui.max_rect().intersect(rect));
            add(ui)
        })
        .inner;
    ui.allocate_rect(rect, Sense::hover());
    inner
}

/// A full-width row of height `h` on `fill`, ruled underneath, laid out left
/// to right: a pane's tab strip (`TAB_H`, `pane_hdr()`) or toolbar (`TOOL_H`, `bg()`).
pub fn strip<R>(ui: &mut egui::Ui, h: f32, fill: Color32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    // A toolbar on a phone wraps onto more lines (operator, 2026-10-24:
    // "could be smoother management on mobile", IMG_8268: Builds' views ran
    // to "Que" at the edge and the hidden sideways scroll gave no sign there
    // was more). Every choice is on screen and a tap away. Tab strips keep
    // one row: their tabs are the panes'.
    let w = ui.available_width();
    if w < 560.0 && h != TAB_H {
        let top = ui.cursor().min;
        let bg = ui.painter().add(egui::Shape::Noop);
        // One row's height: further rows wrap below it, and a widget sized to
        // available_height gets the row, not infinity - an unbounded child sent
        // the row, and everything after the strip, off the page (2026-10-24,
        // Builds and Go drew nothing below their first wrapped strip).
        let mut child = ui.new_child(UiBuilder::new().max_rect(egui::Rect::from_min_size(top, egui::vec2(w, h - 1.0))).layout(Layout::left_to_right(Align::Center).with_main_wrap(true)));
        child.spacing_mut().item_spacing = egui::vec2(2.0, 4.0);
        child.set_min_height(h - 1.0);
        let out = add(&mut child);
        let bottom = child.min_rect().bottom();
        let bottom = if bottom.is_finite() { bottom.min(top.y + 8.0 * h) } else { top.y + h - 1.0 };
        let used = egui::Rect::from_min_max(top, egui::pos2(top.x + w, bottom.max(top.y + h - 1.0) + 1.0));
        ui.painter().set(bg, egui::epaint::RectShape::filled(used, 0, fill));
        ui.painter().hline(used.x_range(), used.bottom() - 0.5, Stroke::new(1.0, pane_brd()));
        ui.allocate_rect(used, Sense::hover());
        return out;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
    // A tab strip is a window's top row: rounded with it, and as see-through.
    let top = if h == TAB_H { WIN_RADIUS } else { 0 };
    ui.painter().rect_filled(rect, CornerRadius { nw: top, ne: top, sw: 0, se: 0 }, fill);
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, pane_brd()));
    let inner = rect.with_max_y(rect.bottom() - 1.0);
    // A child the parent does not grow with, clipped to the strip: a row of
    // buttons wider than a phone used to widen egui's layout for everything
    // under it, so a chart's right end (now) and the composer's Send sat off
    // the screen with no way to reach them (operator, 2026-10-18: "the right
    // side of the screen keeps having things that go off of that"). A narrow
    // tab strip scrolls sideways instead.
    let mut child = ui.new_child(UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
    child.set_clip_rect(inner.intersect(ui.clip_rect()));
    child.spacing_mut().item_spacing.x = 2.0;
    if inner.width() < 560.0 {
        egui::ScrollArea::horizontal()
            .id_salt(("strip", inner.min.x as i32, inner.min.y as i32))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .max_height(inner.height())
            .show(&mut child, |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.set_min_height(inner.height());
                ui.horizontal_centered(|ui| add(ui)).inner
            })
            .inner
    } else {
        add(&mut child)
    }
}

/// One tab in a strip, RStudio's way: the active tab takes the pane's colour
/// and Regolith's amber focus mark along its top. `dot` paints a status light before
/// the label; `closable` adds an × that reports through the second value.
pub fn tab(ui: &mut egui::Ui, label: &str, active: bool, dot: Option<Color32>, closable: bool) -> (Response, bool) {
    let color = if active { text_strong() } else { muted() };
    let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::proportional(12.0), color);
    let lead = if dot.is_some() { 14.0 } else { 0.0 };
    let tail = if closable { 16.0 } else { 0.0 };
    let size = egui::vec2(galley.size().x + 20.0 + lead + tail, ui.available_height());
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let p = ui.painter();
    if active {
        p.rect_filled(rect, 0.0, bg());
        p.hline(rect.x_range(), rect.top() + 1.0, Stroke::new(2.0, amber()));
    } else if resp.hovered() {
        p.rect_filled(rect, 0.0, surface_hi());
    }
    p.vline(rect.right() - 0.5, rect.y_range(), Stroke::new(1.0, pane_brd()));
    let x = rect.left() + 10.0;
    if let Some(c) = dot {
        p.circle_filled(egui::pos2(x + 4.0, rect.center().y), 3.5, c);
    }
    p.galley(egui::pos2(x + lead, rect.center().y - galley.size().y / 2.0), galley, color);
    let mut closed = false;
    if closable {
        let r = Rect::from_center_size(egui::pos2(rect.right() - 13.0, rect.center().y), egui::vec2(14.0, 14.0));
        let x = ui.interact(r, resp.id.with("close"), Sense::click());
        let c = if x.hovered() { text_strong() } else { hush() };
        if x.hovered() {
            ui.painter().rect_filled(r, 0.0, surface_hi());
        }
        let d = 3.5;
        ui.painter().line_segment([r.center() + egui::vec2(-d, -d), r.center() + egui::vec2(d, d)], Stroke::new(1.2, c));
        ui.painter().line_segment([r.center() + egui::vec2(-d, d), r.center() + egui::vec2(d, -d)], Stroke::new(1.2, c));
        closed = x.on_hover_text("close").clicked();
    }
    (resp, closed)
}

/// A frameless toolbar button, as RStudio's pane toolbars draw them.
pub fn tool(ui: &mut egui::Ui, text: &str, tip: &str) -> Response {
    ui.add(egui::Button::new(egui::RichText::new(text).size(12.0)).frame(false)).on_hover_text(tip)
}

/// Same, greyed out unless `on`.
pub fn tool_if(ui: &mut egui::Ui, on: bool, text: &str, tip: &str) -> Response {
    ui.add_enabled(on, egui::Button::new(egui::RichText::new(text).size(12.0)).frame(false)).on_hover_text(tip)
}

/// A thin upright rule between groups of toolbar buttons.
pub fn tool_sep(ui: &mut egui::Ui) {
    let (r, _) = ui.allocate_exact_size(egui::vec2(9.0, 16.0), Sense::hover());
    ui.painter().vline(r.center().x, r.y_range(), Stroke::new(1.0, pane_brd()));
}

/// The maximize / restore control at a pane's top right, drawn rather than
/// borrowed from a font: one box when it would zoom, two when it would restore.
pub fn zoom_button(ui: &mut egui::Ui, zoomed: bool) -> bool {
    let (r, resp) = ui.allocate_exact_size(egui::vec2(20.0, 18.0), Sense::click());
    let c = if resp.hovered() { text_strong() } else { hush() };
    let s = Stroke::new(1.0, c);
    let p = ui.painter();
    if zoomed {
        p.rect_stroke(Rect::from_center_size(r.center() + egui::vec2(-1.5, 1.5), egui::vec2(8.0, 7.0)), 0.0, s, egui::StrokeKind::Inside);
        p.rect_stroke(Rect::from_center_size(r.center() + egui::vec2(1.5, -1.5), egui::vec2(8.0, 7.0)), 0.0, s, egui::StrokeKind::Inside);
    } else {
        p.rect_stroke(Rect::from_center_size(r.center(), egui::vec2(10.0, 9.0)), 0.0, s, egui::StrokeKind::Inside);
        p.hline((r.center().x - 5.0)..=(r.center().x + 5.0), r.center().y - 2.5, s);
    }
    resp.on_hover_text(if zoomed { "restore all panes (Ctrl+Shift+0)" } else { "zoom this pane" }).clicked()
}

#[cfg(test)]
mod tests {
    use super::PALETTES;

    /// WCAG 2 contrast: text 4.5:1 on every ground it sits on, accents 3:1.
    #[test]
    fn text_contrast() {
        fn lum(h: u32) -> f64 {
            let c = |s: u32| {
                let x = ((h >> s) & 255) as f64 / 255.0;
                if x <= 0.03928 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * c(16) + 0.7152 * c(8) + 0.0722 * c(0)
        }
        let ratio = |a: u32, b: u32| {
            let (a, b) = (lum(a), lum(b));
            (a.max(b) + 0.05) / (a.min(b) + 0.05)
        };
        for p in PALETTES {
            for ground in [p.bg, p.surface, p.surface_hi, p.ground] {
                for (name, fg, floor) in [("text", p.text, 4.5), ("muted", p.muted, 4.5), ("hush", p.hush, 4.5), ("comment", p.comment, 4.5), ("primary", p.primary, 3.0), ("amber", p.amber, 3.0), ("alt", p.alt, 3.0), ("ok", p.ok, 3.0)] {
                    let r = ratio(fg, ground);
                    assert!(r >= floor, "{}: {name} {fg:06x} on {ground:06x} is {r:.2}:1, under {floor}:1", p.name);
                }
            }
        }
    }

    /// Every symbol a string in the UI uses can be drawn by the fonts we
    /// ship - else it is an empty box on screen (operator screenshot,
    /// 2026-10-18: "we're missing icons": ＋ and ◐ were boxes).
    #[test]
    fn every_symbol_in_the_ui_has_a_glyph() {
        use eframe::egui::{epaint::text::{Fonts, TextOptions}, FontFamily, FontId};
        let mut fonts = Fonts::new(TextOptions::default(), super::fonts());
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![];
        for d in [src.clone(), src.join("software")] {
            for e in std::fs::read_dir(d).unwrap().flatten() {
                if e.path().extension().is_some_and(|x| x == "rs") {
                    files.push(e.path());
                }
            }
        }
        let mut missing = std::collections::BTreeMap::<char, String>::new();
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            for line in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
                // Only inside string literals: comments may say what they like.
                for (k, part) in line.split('"').enumerate() {
                    if k % 2 == 0 {
                        continue;
                    }
                    // The face the line sets it in: mono where it says so.
                    let fam = if line.contains("monospace()") || line.contains("FontId::monospace") { FontFamily::Monospace } else { FontFamily::Proportional };
                    for c in part.chars().filter(|c| !c.is_ascii() && !c.is_alphabetic()) {
                        if !fonts.has_glyph(&FontId::new(12.0, fam.clone()), c) {
                            missing.entry(c).or_insert_with(|| format!("{} ({fam:?})", f.file_name().unwrap().to_string_lossy()));
                        }
                    }
                }
            }
        }
        assert!(missing.is_empty(), "no glyph for: {missing:?}");
    }

    fn lum(h: u32) -> f64 {
        let f = |c: u32| {
            let x = c as f64 / 255.0;
            if x <= 0.03928 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * f((h >> 16) & 255) + 0.7152 * f((h >> 8) & 255) + 0.0722 * f(h & 255)
    }

    fn contrast(a: u32, b: u32) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    /// Code and body text read at 4.5:1 (WCAG AA) in every look; dim text and
    /// the accent at 3:1. A palette edit that breaks this fails here, not on
    /// someone's screen (operator, 2026-10-16: "hard to read code in light mode").
    #[test]
    fn every_palette_is_readable() {
        for p in PALETTES {
            for (what, c) in [("keyword", p.keyword), ("operator", p.operator), ("number", p.number), ("function", p.function), ("string", p.string), ("comment", p.comment), ("text", p.text)] {
                let r = contrast(c, p.code);
                assert!(r >= 4.5, "{}: {what} {c:06x} on {:06x} is {r:.2}:1", p.name, p.code);
            }
            assert!(contrast(p.muted, p.bg) >= 4.5, "{}: muted", p.name);
            for (what, c) in [("hush", p.hush), ("primary", p.primary), ("alt", p.alt)] {
                assert!(contrast(c, p.bg) >= 3.0, "{}: {what}", p.name);
            }
        }
    }
}
