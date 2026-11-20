//! The front page's slides: what Cove is, the pictures, and the builds as
//! they happen, turning by themselves over the atlas (operator, 2026-10-17:
//! "images and data about the builds occurring can go onto the front page
//! where the graph is and swap through those there ... I made this post and I
//! want people to be able to observe and search it").
//!
//! The slides are data, not code: `showcase.json` beside the pictures in the
//! public view's showcase folder (server.rs `showcase_dir`, served at
//! /view/showcase.json and /view/showcase/<file>). Every front page reads the
//! same one - the public view, and a signed-in person's atlas - so editing that
//! file changes the front for everyone, with no build. A slide is one of:
//!
//!   {"kind":"text",   "title":"…", "body":"…"}
//!   {"kind":"image",  "src":"pawnee-butte.webp", "caption":"…"}
//!   {"kind":"builds", "title":"…"}                 the stack and the machines, live,
//!                                                  with builds per day for 30 days
//!   {"kind":"log",    "title":"…"}                 the busiest build machine's log, tailing
//!   {"kind":"atlas",  "lang":"Python", "focus":"pandas", "caption":"…"}
//!                                                  points the atlas at one package
//!                                                  and what it is built on
//!
//! and any may carry "secs" (how long it holds; 12 by default).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align2, RichText, Stroke};
use serde::Deserialize;

use crate::software::Catalog;
use crate::theme;

#[derive(Clone, Deserialize, Default)]
#[serde(default)]
pub struct Slide {
    kind: String,
    title: String,
    body: String,
    src: String,
    caption: String,
    lang: String,
    focus: String,
    secs: f64,
    /// An env set whose real settings the slide lists (host.rs
    /// env_tour_facts, /view/env.json): site libraries, repos, what it loads.
    set: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Deck {
    slides: Vec<Slide>,
}

/// What the atlas should do for the slide in front.
pub enum Point {
    /// Look at this catalog item.
    At(usize),
    /// Look at kernel floor `j` (the floors slide walks each in turn).
    Kernel(usize),
    /// Let go of whatever a slide pointed at.
    Release,
}

type Slot<T> = Arc<Mutex<Option<T>>>;

#[derive(Default)]
pub struct Showcase {
    deck: Slot<Result<Vec<Slide>, String>>,
    asked: bool,
    /// Pictures by file: decoded off the frame, made textures on it.
    decoded: Arc<Mutex<HashMap<String, Option<egui::ColorImage>>>>,
    tex: HashMap<String, egui::TextureHandle>,
    /// builds.json, asked again every minute while the builds slide shows.
    live: Slot<serde_json::Value>,
    live_at: f64,
    /// The log slide: which machine, its last lines, when last asked.
    log: Slot<(String, String, Vec<String>, crate::stage::Stage)>,
    log_at: f64,
    at: usize,
    since: f64,
    paused: bool,
    pub hidden: bool,
    /// The atlas is immersed (atlas.rs `immersive`): the message only - no
    /// card, pictures, dots or controls - low over the stack.
    pub bare: bool,
    /// The panel the atlas is drawn in, set by whoever shows it. Every card
    /// and button here sits in it, not on the screen: on a phone, anchored to
    /// the screen, "The story" lay over the bottom tabs and hid Account
    /// (operator screenshot, 2026-11-07).
    pub within: Option<egui::Rect>,
    /// The slide that last pointed the atlas, so it is let go only once.
    pointed: Option<usize>,
    /// Every public and signed-in domain, all owners (/view/domains.json),
    /// for a signed-in front page's canopy beside the person's own.
    canopy: Slot<Vec<crate::api::DomainInfo>>,
    canopy_at: f64,
    /// Facts a slide shows as lines, by key ("env:<set>", "floors"), asked
    /// once each: an env set's settings, the kernel floors.
    facts: Arc<Mutex<HashMap<String, Vec<String>>>>,
    facts_asked: std::collections::HashSet<String>,
}

/// The arch as people say it.
fn arch_word(a: &str) -> &str {
    match a {
        "x86_64" => "x86-64",
        "aarch64" => "arm64",
        a => a,
    }
}

/// The public view's address. In a browser on cove.ndexr.io it is this
/// page's own origin; anywhere else (a cell, the desktop app) the public site.
pub(crate) fn base() -> String {
    #[cfg(target_arch = "wasm32")]
    {
        if let Some(o) = web_sys::window().and_then(|w| w.location().origin().ok()) {
            if o.starts_with("https://cove.") || o.starts_with("http://127.0.0.1") || o.starts_with("http://localhost") {
                return format!("{o}/view");
            }
        }
    }
    "https://cove.ndexr.io/view".to_owned()
}

impl Showcase {
    fn bounds(&self, ctx: &egui::Context) -> egui::Rect {
        self.within.unwrap_or_else(|| ctx.content_rect())
    }

    /// The point `off` from the panel's `a` corner, for an area pivoted there.
    fn corner(&self, ctx: &egui::Context, a: Align2, off: egui::Vec2) -> egui::Pos2 {
        a.pos_in_rect(&self.bounds(ctx)) + off
    }

    fn load(&mut self, ctx: &egui::Context) {
        if self.asked {
            return;
        }
        self.asked = true;
        let (slot, ctx) = (self.deck.clone(), ctx.clone());
        ehttp::fetch(ehttp::Request::get(format!("{}/showcase.json", base())), move |r| {
            let v = match r {
                Ok(r) if r.ok => serde_json::from_slice::<Deck>(&r.bytes).map(|d| d.slides).map_err(|e| format!("showcase.json: {e}")),
                Ok(r) => Err(format!("showcase.json: {}", r.status)),
                Err(e) => Err(e),
            };
            *slot.lock().unwrap() = Some(v);
            ctx.request_repaint();
        });
    }

    fn picture(&mut self, src: &str, ctx: &egui::Context) -> Option<egui::TextureHandle> {
        if let Some(t) = self.tex.get(src) {
            return Some(t.clone());
        }
        let mut d = self.decoded.lock().unwrap();
        match d.get_mut(src) {
            None => {
                d.insert(src.to_owned(), None);
                drop(d);
                let (all, key, ctx2) = (self.decoded.clone(), src.to_owned(), ctx.clone());
                ehttp::fetch(ehttp::Request::get(format!("{}/showcase/{src}", base())), move |r| {
                    let img = r.ok().filter(|r| r.ok).and_then(|r| image::load_from_memory(&r.bytes).ok()).map(|img| {
                        let rgba = img.to_rgba8();
                        egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
                    });
                    if let Some(img) = img {
                        all.lock().unwrap().insert(key, Some(img));
                    }
                    ctx2.request_repaint();
                });
                None
            }
            Some(slot) => {
                let img = slot.take()?;
                drop(d);
                let t = ctx.load_texture(format!("showcase-{src}"), img, egui::TextureOptions::LINEAR);
                self.tex.insert(src.to_owned(), t.clone());
                Some(t)
            }
        }
    }

    /// The lines a slide lists under its caption, once they have come.
    fn fact_lines(&mut self, s: &Slide, ctx: &egui::Context) -> Vec<String> {
        let key = if s.kind == "floors" { "floors".to_owned() } else if !s.set.is_empty() { format!("env:{}", s.set) } else { return Vec::new() };
        if self.facts_asked.insert(key.clone()) {
            let (slot, c, k) = (self.facts.clone(), ctx.clone(), key.clone());
            let url = if s.kind == "floors" { format!("{}/kernels.json", base()) } else { format!("{}/env.json?set={}", base(), s.set) };
            ehttp::fetch(ehttp::Request::get(url), move |r| {
                let Some(v) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice::<serde_json::Value>(&r.bytes).ok()) else { return };
                let lines: Vec<String> = if k == "floors" {
                    // One kernel per arch: its floor, its C library, what it is proven on.
                    v.as_array().into_iter().flatten().map(|f| {
                        let s = |n: &str| f[n].as_str().unwrap_or("").to_owned();
                        let proven: Vec<String> = f["proven_on"].as_array().into_iter().flatten().filter_map(|p| p.as_str().map(str::to_owned)).collect();
                        format!("{:<7} Linux ≥ {} · glibc {} · headers {} · {}", arch_word(&s("arch")), s("min_kernel").trim_end_matches(".0"), s("glibc"), s("headers"),
                            if proven.is_empty() { "not yet run on a kernel here".to_owned() } else { format!("proven on {}", proven.join(", ")) })
                    }).collect()
                } else {
                    v["lines"].as_array().into_iter().flatten().filter_map(|l| l.as_str().map(str::to_owned)).collect()
                };
                slot.lock().unwrap().insert(k, lines);
                c.request_repaint();
            });
        }
        self.facts.lock().unwrap().get(&key).cloned().unwrap_or_default()
    }

    fn ask_live(&mut self, now: f64, ctx: &egui::Context) {
        if now - self.live_at < 60.0 {
            return;
        }
        self.live_at = now;
        let (slot, ctx) = (self.live.clone(), ctx.clone());
        ehttp::fetch(ehttp::Request::get(format!("{}/builds.json", base())), move |r| {
            if let Some(v) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice(&r.bytes).ok()) {
                *slot.lock().unwrap() = Some(v);
            }
            ctx.request_repaint();
        });
    }

    /// The atlas could not look yet (its scene was still being built): point
    /// again next frame.
    pub fn retry(&mut self) {
        self.pointed = None;
    }

    /// The most recently written build log's tail (buildlog.json), every 5 s
    /// while the log slide shows.
    fn ask_log(&mut self, now: f64, ctx: &egui::Context) {
        if now - self.log_at < 5.0 {
            return;
        }
        self.log_at = now;
        let (slot, ctx) = (self.log.clone(), ctx.clone());
        let b = base();
        ehttp::fetch(ehttp::Request::get(format!("{b}/buildlog.json")), move |r| {
            let Some(v) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice::<serde_json::Value>(&r.bytes).ok()) else { return };
            let newest = v["logs"].as_array().and_then(|l| l.iter()
                // A box that is on first (host.rs box_power), then one running, then the freshest.
                .max_by_key(|x| (x["power"].as_str().map_or(true, |p| p != "off"), x["size"].as_u64().unwrap_or(0) >= 1024, matches!(x["state"].as_str(), Some("running" | "building")), x["mtime"].as_u64().unwrap_or(0)))
                .cloned());
            let Some(l) = newest else { return };
            let key = l["key"].as_str().unwrap_or_default().to_owned();
            let state = l["state"].as_str().unwrap_or("").to_owned();
            let lane_step = l["step"].as_str().unwrap_or("").to_owned();
            ehttp::fetch(ehttp::Request::get(format!("{b}/buildlog.json?key={key}")), move |r| {
                if let Some(v) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice::<serde_json::Value>(&r.bytes).ok()) {
                    let text = v["text"].as_str().unwrap_or_default();
                    let lines: Vec<String> = text.lines().filter(|l| !l.trim().is_empty()).rev().take(8).map(|l| l.chars().take(140).collect()).collect::<Vec<_>>().into_iter().rev().collect();
                    // The stage in words (stage.rs); a step announced before
                    // the tail began comes from the machine's own state file.
                    let mut st = crate::stage::read(text);
                    if st.step.is_none() && st.error.is_none() {
                        if let Some(i) = crate::stage::STEPS.iter().position(|(n, _)| *n == lane_step) {
                            st.step = Some((i + 1, crate::stage::STEPS.len(), lane_step.clone()));
                            st.headline = crate::stage::STEPS[i].1.to_owned();
                        }
                    }
                    *slot.lock().unwrap() = Some((key, state, lines, st));
                }
                ctx.request_repaint();
            });
        });
    }

    /// Everyone's visible domains, asked again every minute (a new one joins
    /// within it); empty until the first answer.
    /// `url` is whose canopy: a session's own `domains.json` (that person's
    /// login, admin and owner sites too), or the public view's.
    pub fn canopy(&mut self, url: &str, ctx: &egui::Context) -> Vec<crate::api::DomainInfo> {
        let now = ctx.input(|i| i.time);
        if self.canopy_at == 0.0 || now - self.canopy_at > 60.0 {
            self.canopy_at = now.max(0.001);
            let (slot, ctx) = (self.canopy.clone(), ctx.clone());
            let mut req = ehttp::Request::get(url);
            // The session's cookie must go: it is what says who is looking.
            #[cfg(target_arch = "wasm32")]
            {
                req.credentials = ehttp::Credentials::SameOrigin;
            }
            ehttp::fetch(crate::backend::with_token(req), move |r| {
                if let Some(v) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice(&r.bytes).ok()) {
                    *slot.lock().unwrap() = Some(v);
                    ctx.request_repaint();
                }
            });
        }
        self.canopy.lock().unwrap().clone().unwrap_or_default()
    }

    /// Hold the slide where it is: the person is looking at something.
    pub fn hold(&mut self) {
        self.paused = true;
    }

    /// The card over the atlas's lower right. Returns where the atlas should look.
    pub fn ui(&mut self, ctx: &egui::Context, cat: Option<&Catalog>) -> Option<Point> {
        self.load(ctx);
        let now = ctx.input(|i| i.time);
        let slides = match &*self.deck.lock().unwrap() {
            Some(Ok(s)) if !s.is_empty() => s.clone(),
            _ => return self.pointed.take().map(|_| Point::Release),
        };
        if self.hidden && self.bare {
            return self.pointed.take().map(|_| Point::Release);
        }
        if self.hidden {
            let at = self.corner(ctx, Align2::RIGHT_BOTTOM, egui::vec2(-14.0, -14.0));
            egui::Area::new(egui::Id::new("showcase-shown"))
                .pivot(Align2::RIGHT_BOTTOM)
                .fixed_pos(at)
                .show(ctx, |ui| {
                    if ui.add(theme::primary_button("▶  The story")).on_hover_text("show the slides again").clicked() {
                        self.hidden = false;
                        self.since = now;
                    }
                });
            return self.pointed.take().map(|_| Point::Release);
        }
        if self.since == 0.0 {
            self.since = now;
        }
        self.at %= slides.len();
        let hold = Some(slides[self.at].secs).filter(|s| *s > 0.0).unwrap_or(12.0);
        // Time to look (operator, 2026-10-18: "I kind of just want to pause it
        // real quick and like soak it in ... I feel like I'm under a time
        // limit"): reaching into the atlas - a click, a drag, a zoom - holds
        // the slide where it is; Space holds it or lets it go, Page Up and
        // Page Down step. The controls themselves are Areas, so pressing
        // them is not reaching into the atlas.
        let (touched, keys) = ctx.input(|i| {
            let touched = i.pointer.any_pressed() || i.smooth_scroll_delta != egui::Vec2::ZERO;
            let k = |a| i.key_pressed(a);
            (touched, (k(egui::Key::Space), k(egui::Key::PageDown) as i32 - k(egui::Key::PageUp) as i32))
        });
        if touched && !ctx.is_pointer_over_egui() {
            self.paused = true;
        }
        if !ctx.egui_wants_keyboard_input() {
            if keys.0 {
                self.paused = !self.paused;
                self.since = now;
            }
            if keys.1 != 0 {
                self.step(keys.1, slides.len(), now);
            }
        }
        let hold = Some(slides[self.at].secs).filter(|s| *s > 0.0).unwrap_or(hold);
        if !self.paused && now - self.since > hold {
            self.at = (self.at + 1) % slides.len();
            self.since = now;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
        let slide = slides[self.at].clone();

        // ── where the atlas looks ──
        let mut point = None;
        if slide.kind == "atlas" {
            if self.pointed != Some(self.at) {
                if let Some(i) = cat.and_then(|c| c.find(if slide.lang.is_empty() { "Python" } else { &slide.lang }, &slide.focus)) {
                    self.pointed = Some(self.at);
                    point = Some(Point::At(i));
                }
            }
        } else if slide.kind == "floors" {
            // "Two kernels, two floors" with the camera elsewhere showed
            // nothing it named (operator, 2026-10-18): each kernel in turn,
            // half the slide each, its floor's panel open beside it.
            let j = usize::from(now - self.since > hold / 2.0);
            if self.pointed != Some(1000 + self.at * 2 + j) {
                self.pointed = Some(1000 + self.at * 2 + j);
                point = Some(Point::Kernel(j));
            }
        } else if self.pointed.take().is_some() {
            point = Some(Point::Release);
        }
        if slide.kind == "log" {
            self.ask_log(now, ctx);
        }
        if slide.kind == "builds" {
            self.ask_live(now, ctx);
        }

        let screen = self.bounds(ctx);
        if self.bare {
            self.bare_ui(ctx, &slide, cat, now, hold);
            let next = title_of(&slides[(self.at + 1) % slides.len()]);
            let step = self.bare_controls(ctx, slides.len(), &next, now, hold);
            self.step(step, slides.len(), now);
            return point;
        }
        // The card's inner width: on a phone the screen less the 12 px gaps and
        // the frame's own 14 px margins, else it ran 4 px wider than the
        // screen and its right anchor pushed it off the left edge
        // (screenshot, 2026-10-18).
        let w = if screen.width() < 640.0 { screen.width() - 24.0 - 30.0 } else { (screen.width() * 0.34).clamp(340.0, 460.0) };
        let mut step: i32 = 0;
        let at = self.corner(ctx, Align2::RIGHT_BOTTOM, egui::vec2(-12.0, -12.0));
        egui::Area::new(egui::Id::new("showcase"))
            .pivot(Align2::RIGHT_BOTTOM)
            .fixed_pos(at)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(theme::bg().gamma_multiply(0.92))
                    .stroke(Stroke::new(1.0, theme::pane_brd()))
                    .corner_radius(theme::WIN_RADIUS)
                    .inner_margin(14.0)
                    .show(ui, |ui| {
                        ui.set_width(w);
                        ui.set_max_width(w);
                        // Never taller than the screen leaves it: zoomed in
                        // (Ctrl + on a laptop) the slide ran off the top and
                        // its title went with it (operator, 2026-10-18). The
                        // bars above, the controls under and the gaps.
                        // On a phone, a fifth of the screen for the slide (operator,
                        // 2026-10-23, a 390 × 844 screenshot): the card took
                        // half of it and the stack it narrates sat behind.
                        // The slide scrolls inside.
                        let room = if screen.width() < 640.0 { (screen.height() * 0.2).max(110.0) } else { (screen.height() - 190.0).max(60.0) };
                        egui::ScrollArea::vertical().id_salt("showcase-slide").max_height(room).auto_shrink([false, true]).show(ui, |ui| {
                            self.slide_ui(ui, &slide, cat, w, ctx);
                        });
                        ui.add_space(10.0);
                        let next = title_of(&slides[(self.at + 1) % slides.len()]);
                        if !next.is_empty() && screen.width() >= 640.0 {
                            let when = if self.paused { "held - next".to_owned() } else { format!("next in {} s", (hold - (now - self.since)).max(0.0).ceil() as u32) };
                            ui.label(RichText::new(format!("{when}: {next}")).size(11.0).color(theme::hush()));
                            ui.add_space(4.0);
                        }
                        // ── the dots and the controls ──
                        // On a phone, buttons a thumb can hit (operator,
                        // 2026-10-18: "buttons are hard to push on mobile in
                        // places like the front visuals"): 44 × 38, bigger glyphs.
                        let touch = w < 520.0;
                        let ctl = |ui: &mut egui::Ui, glyph: &str, tip: &str| -> bool {
                            if touch {
                                ui.add(egui::Button::new(RichText::new(glyph).size(19.0)).min_size(egui::vec2(44.0, 38.0))).on_hover_text(tip).clicked()
                            } else {
                                theme::tool(ui, glyph, tip).clicked()
                            }
                        };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = if touch { 8.0 } else { 4.0 };
                            if ctl(ui, "‹", "the previous slide") {
                                step = -1;
                            }
                            if ctl(ui, if self.paused { "▶" } else { "⏸" }, if self.paused { "turn the slides again" } else { "hold this slide" }) {
                                self.paused = !self.paused;
                            }
                            if ctl(ui, "›", "the next slide") {
                                step = 1;
                            }
                            ui.add_space(6.0);
                            // A dot per slide is wider than a phone: say where
                            // it is instead.
                            if w < 420.0 {
                                ui.label(egui::RichText::new(format!("{} / {}", self.at + 1, slides.len())).size(if touch { 14.0 } else { 11.0 }).color(theme::hush()));
                            }
                            for k in (0..slides.len()).filter(|_| w >= 420.0) {
                                let (r, resp) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::click());
                                let c = if k == self.at { theme::primary() } else { theme::hush() };
                                ui.painter().circle_filled(r.center(), if k == self.at { 3.5 } else { 2.5 }, c);
                                if resp.clicked() {
                                    self.at = k;
                                    self.since = now;
                                }
                            }
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ctl(ui, "×", "hide the slides") {
                                    self.hidden = true;
                                }
                                // How far into this slide, as a hairline.
                                let f = if self.paused { 1.0 } else { ((now - self.since) / hold).clamp(0.0, 1.0) as f32 };
                                let (r, _) = ui.allocate_exact_size(egui::vec2(48.0, 2.0), egui::Sense::hover());
                                ui.painter().rect_filled(r, 1, theme::moss());
                                ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width() * f, r.height())), 1, theme::primary());
                            });
                        });
                    });
            });
        self.step(step, slides.len(), now);
        point
    }

    /// `by` slides on (back when negative), round the deck.
    fn step(&mut self, by: i32, n: usize, now: f64) {
        if by != 0 && n > 0 {
            self.at = ((self.at as i32 + by).rem_euclid(n as i32)) as usize;
            self.since = now;
        }
    }

    /// Immersed, the controls still there, small and to the right: back,
    /// hold, on, where in the deck, and what comes next - so the story can be
    /// stopped, read and walked at the reader's pace. Returns the step asked.
    fn bare_controls(&mut self, ctx: &egui::Context, n: usize, next: &str, now: f64, hold: f64) -> i32 {
        let mut step = 0;
        let screen = self.bounds(ctx);
        let touch = screen.width() < 640.0;
        let at = self.corner(ctx, Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -14.0));
        egui::Area::new(egui::Id::new("showcase-bare-ctl"))
            .order(egui::Order::Foreground)
            .pivot(Align2::RIGHT_BOTTOM)
            .fixed_pos(at)
            .show(ctx, |ui| {
                egui::Frame::new().fill(theme::bg().gamma_multiply(0.72)).corner_radius(99).inner_margin(egui::Margin::symmetric(10, 4)).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let b = |ui: &mut egui::Ui, g: &str, tip: &str| -> bool {
                            let size = if touch { egui::vec2(44.0, 38.0) } else { egui::vec2(30.0, 26.0) };
                            ui.add(egui::Button::new(RichText::new(g).size(if touch { 19.0 } else { 15.0 })).frame(false).min_size(size)).on_hover_text(tip).clicked()
                        };
                        if b(ui, "‹", "the previous slide (Page Up)") {
                            step = -1;
                        }
                        if b(ui, if self.paused { "▶" } else { "⏸" }, if self.paused { "go on (Space)" } else { "hold this slide (Space)" }) {
                            self.paused = !self.paused;
                            self.since = now;
                        }
                        if b(ui, "›", "the next slide (Page Down)") {
                            step = 1;
                        }
                        ui.label(RichText::new(format!("{} / {n}", self.at + 1)).monospace().size(11.0).color(theme::hush()));
                        if self.paused {
                            ui.label(RichText::new("held - take your time").size(11.5).color(theme::text()));
                        } else {
                            let left = (hold - (now - self.since)).max(0.0).ceil() as u32;
                            ui.label(RichText::new(format!("next in {left} s")).size(11.0).color(theme::hush()));
                        }
                        if !touch && !next.is_empty() {
                            ui.label(RichText::new(format!("· next: {next}")).size(11.5).color(theme::muted()));
                        }
                    });
                });
            });
        step
    }

    /// Immersed: the slide as a film title, low over the stack (operator,
    /// 2026-10-18: "text areas could be cooler in vis mode"). No box: the sky
    /// darkens toward the bottom edge so the words read over any of it. A
    /// kicker in the slide's own colour ("TOUR · 3", "LIVE"), the headline in
    /// the display face, the words easing in as the slide arrives, and the
    /// hairline for how long it holds. The build slides carry the machines
    /// themselves - what each is building, the log's last lines - not only a
    /// title. Pictures and charts are left to the full card.
    fn bare_ui(&mut self, ctx: &egui::Context, s: &Slide, cat: Option<&Catalog>, now: f64, hold: f64) {
        let screen = self.bounds(ctx);
        let w = (screen.width() - 64.0).min(720.0);
        let item = (s.kind == "atlas").then(|| cat.and_then(|c| c.find(if s.lang.is_empty() { "Python" } else { &s.lang }, &s.focus).map(|i| (c, i)))).flatten();
        // "Tour · 3 - the compiler everything shares" → kicker and headline.
        let split = |t: &str| -> (String, String) {
            match t.split_once(" - ") {
                Some((k, h)) if k.len() <= 32 => (k.trim().to_uppercase(), h.trim().to_owned()),
                _ => (String::new(), t.trim().to_owned()),
            }
        };
        let live = matches!(s.kind.as_str(), "builds" | "log");
        let (mut kicker, headline) = match s.kind.as_str() {
            "atlas" => {
                let name = item.map(|(c, i)| {
                    let sw = c.sw(i);
                    format!("{} {}", sw.name, sw.versions.last().map(String::as_str).unwrap_or(""))
                });
                if s.title.is_empty() { (String::new(), name.unwrap_or_default()) } else { split(&s.title) }
            }
            "builds" if s.title.is_empty() => (String::new(), "The stack, building".to_owned()),
            _ => split(&s.title),
        };
        if kicker.is_empty() {
            kicker = match s.kind.as_str() {
                "atlas" => item.map_or("THE STACK".to_owned(), |(c, i)| format!("THE STACK · {}", c.sw(i).kind.label().to_uppercase())),
                "builds" | "log" => "LIVE".to_owned(),
                "floors" => "THE STACK · KERNEL".to_owned(),
                _ => "COVE".to_owned(),
            };
        }
        let body = if s.kind == "text" { s.body.clone() } else { s.caption.clone() };
        // What the build slides show beneath their title: the machines that
        // are on, then (the log slide) the log's last lines.
        let mut ticker: Vec<(String, bool)> = Vec::new();
        if live {
            let v = self.live.lock().unwrap().clone();
            let at = v.as_ref().and_then(|v| v["generated"].as_i64()).unwrap_or(0);
            for l in v.as_ref().and_then(|v| v["lanes"].as_array()).into_iter().flatten() {
                // Off by the one answer every view gives (host.rs box_power).
                let off = match l["power"].as_str() {
                    Some(p) => p == "off",
                    None => l["state"].as_str() == Some("off") || at - l["mtime"].as_i64().unwrap_or(0) >= 15 * 60,
                };
                if off {
                    continue;
                }
                let key = l["key"].as_str().unwrap_or("?");
                let line = match (l["now"].as_str(), l["n"].as_u64(), l["of"].as_u64()) {
                    (Some(p), Some(n), Some(m)) if m > 0 => format!("{key}  ·  {p}  {n}/{m}"),
                    _ => format!("{key}  ·  {}", l["state"].as_str().unwrap_or("idle")),
                };
                ticker.push((line, false));
            }
            if s.kind == "log" {
                if let Some((key, _, lines, _)) = self.log.lock().unwrap().clone() {
                    ticker.retain(|(l, _)| !l.starts_with(&format!("{key} ")));
                    ticker.extend(lines.iter().rev().take(4).rev().map(|l| (l.clone(), true)));
                }
            }
            // Either live slide shows the machines: keep both answers fresh.
            self.ask_live(now, ctx);
            self.ask_log(now, ctx);
        }
        for l in self.fact_lines(s, ctx) {
            ticker.push((l, true));
        }
        // A live slide with no machine on and no log to show had shown its
        // title alone over nothing (operator, 2026-10-18). Say what is true
        // instead - and no filler: "right now" said nothing the slide's being
        // live does not (operator, 2026-10-18: "drop filler").
        let idle = live && ticker.is_empty();
        let headline = if idle { "No build machine is running".to_owned() } else { headline };
        if idle {
            ticker.push(("the stack is idle - Builds keeps every run it has made".to_owned(), true));
        }
        if headline.is_empty() && body.is_empty() && ticker.is_empty() {
            return;
        }
        let accent = match (s.kind.as_str(), item) {
            (_, Some((c, i))) => c.sw(i).kind.color(),
            ("builds" | "log", _) => theme::ok(),
            _ => theme::primary(),
        };
        // Arriving: a fade and a short rise; the words a beat after the title.
        let t = (now - self.since) as f32;
        let ease = |d: f32| {
            let x = ((t - d) / 0.7).clamp(0.0, 1.0);
            1.0 - (1.0 - x) * (1.0 - x) * (1.0 - x)
        };
        let (a_head, a_body) = (ease(0.0), ease(0.25));
        if a_body < 1.0 {
            ctx.request_repaint();
        }
        let white = |a: f32, k: f32| egui::Color32::from_white_alpha((255.0 * a * k) as u8);
        let fade = |c: egui::Color32, a: f32| egui::Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (255.0 * a) as u8);

        // The shade: the bottom of the sky darkened, no edge to it.
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Middle, egui::Id::new("showcase-shade")));
        let top = screen.bottom() - (screen.height() * 0.46).clamp(240.0, 420.0);
        let mut m = egui::Mesh::default();
        let (clear, dark) = (egui::Color32::TRANSPARENT, egui::Color32::from_black_alpha(185));
        m.colored_vertex(egui::pos2(screen.left(), top), clear);
        m.colored_vertex(egui::pos2(screen.right(), top), clear);
        m.colored_vertex(egui::pos2(screen.right(), screen.bottom()), dark);
        m.colored_vertex(egui::pos2(screen.left(), screen.bottom()), dark);
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        p.add(m);

        let rise = 10.0 * (1.0 - a_head);
        let at = self.corner(ctx, Align2::LEFT_BOTTOM, egui::vec2(if screen.width() < 640.0 { 20.0 } else { 56.0 }, -44.0 + rise));
        egui::Area::new(egui::Id::new("showcase-bare"))
            .order(egui::Order::Foreground)
            .pivot(Align2::LEFT_BOTTOM)
            .fixed_pos(at)
            .interactable(false)
            .show(ctx, |ui| {
                ui.set_max_width(w);
                // The kicker: a live dot or a rule, then the words spaced out.
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(egui::vec2(if live { 10.0 } else { 26.0 }, 12.0), egui::Sense::hover());
                    if live {
                        let pulse = 0.55 + 0.45 * ((now * 2.4).sin() as f32 * 0.5 + 0.5);
                        ui.painter().circle_filled(r.center(), 3.5, fade(accent, a_head * pulse));
                        ui.painter().circle_stroke(r.center(), 6.0, Stroke::new(1.0, fade(accent, a_head * 0.35 * pulse)));
                        ctx.request_repaint_after(std::time::Duration::from_millis(60));
                    } else {
                        ui.painter().line_segment([r.left_center(), r.right_center()], Stroke::new(1.5, fade(accent, a_head)));
                    }
                    let mut job = egui::text::LayoutJob::default();
                    job.append(&kicker, 0.0, egui::TextFormat { font_id: egui::FontId::monospace(11.5), color: fade(accent, a_head), extra_letter_spacing: 2.2, ..Default::default() });
                    ui.label(job);
                });
                ui.add_space(6.0);
                if !headline.is_empty() {
                    let mut h = headline.clone();
                    if let Some(c) = h.get(..1) {
                        h = format!("{}{}", c.to_uppercase(), &h[1..]);
                    }
                    ui.label(RichText::new(h).font(theme::display(if screen.width() < 640.0 { 22.0 } else { 30.0 })).color(white(a_head, 0.96)));
                }
                if let Some((c, i)) = item {
                    ui.add_space(6.0);
                    ui.label(RichText::new(c.live_facts(i)).font(egui::FontId::monospace(12.0)).color(fade(accent, a_body * 0.95)));
                }
                // Short (a laptop zoomed in, operator 2026-10-18): the first
                // paragraph and two ticker lines, or the title rose off the top.
                let short = screen.height() < 520.0;
                for para in body.split("\n\n").filter(|p| !p.trim().is_empty()).take(if short { 1 } else { usize::MAX }) {
                    ui.add_space(8.0);
                    ui.label(RichText::new(para.trim()).size(15.0).line_height(Some(22.0)).color(white(a_body, 0.78)));
                }
                if !ticker.is_empty() {
                    ui.add_space(10.0);
                    for (line, log) in ticker.iter().take(if short { 2 } else { 6 }) {
                        let c = if *log { white(a_body, 0.5) } else { fade(accent, a_body * 0.9) };
                        ui.add(egui::Label::new(RichText::new(line).monospace().size(if *log { 11.0 } else { 12.0 }).color(c)).truncate());
                    }
                }
                ui.add_space(12.0);
                // How long it holds: a hairline in the slide's colour, full
                // and still while held.
                let f = if self.paused { 1.0 } else { ((now - self.since) / hold).clamp(0.0, 1.0) as f32 };
                let (r, _) = ui.allocate_exact_size(egui::vec2(180.0, 2.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 1, white(a_head, 0.12));
                ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width() * f, r.height())), 1, fade(accent, a_head * 0.85));
            });
    }

    fn slide_ui(&mut self, ui: &mut egui::Ui, s: &Slide, cat: Option<&Catalog>, w: f32, ctx: &egui::Context) {
        let title = |ui: &mut egui::Ui, t: &str| {
            if !t.is_empty() {
                ui.label(RichText::new(t).size(17.0).strong().color(theme::text_strong()));
                ui.add_space(4.0);
            }
        };
        match s.kind.as_str() {
            "image" => {
                title(ui, &s.title);
                match self.picture(&s.src, ctx) {
                    Some(t) => {
                        let sz = t.size_vec2();
                        let h = (w * sz.y / sz.x.max(1.0)).min(ui.ctx().content_rect().height() * 0.42);
                        ui.add(egui::Image::new(&t).fit_to_exact_size(egui::vec2(h * sz.x / sz.y.max(1.0), h)).corner_radius(4));
                    }
                    None => {
                        ui.add_sized([w, w * 0.56], egui::Spinner::new());
                    }
                }
                if !s.caption.is_empty() {
                    ui.add_space(6.0);
                    ui.label(RichText::new(&s.caption).size(12.5).color(theme::muted()));
                }
            }
            "builds" => {
                title(ui, if s.title.is_empty() { "The stack, building" } else { &s.title });
                if let Some(c) = cat {
                    let (counts, steps, secs, boxes) = c.summary();
                    egui::Grid::new("showcase-counts").num_columns(5).min_col_width(64.0).spacing([14.0, 2.0]).show(ui, |ui| {
                        for (k, n) in [("Prefix", counts[0]), ("EasyBuild", counts[1]), ("R", counts[2]), ("Python", counts[3]), ("Rust", counts[4])] {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(thousands(n)).size(18.0).strong().color(theme::text_strong()));
                                ui.label(RichText::new(k).size(11.0).color(theme::muted()));
                            });
                        }
                        ui.end_row();
                    });
                    // What EasyBuild's number holds - the atlas shows three
                    // layers where this says one (operator, 2026-10-18: 425
                    // here, 312 in the layer list).
                    // Environments are their own layer, not EasyBuild's.
                    let eb: Vec<crate::software::Sw> = (0..c.len()).map(|i| c.sw(i)).filter(|s| s.kind == crate::software::Kind::EasyBuild).collect();
                    let sets = (0..c.len()).filter(|&i| c.sw(i).kind == crate::software::Kind::Env).count();
                    let langs = eb.iter().filter(|s| crate::atlas::language(s.name).is_some()).count();
                    ui.label(RichText::new(format!("EasyBuild: {} software · {langs} languages · and {sets} environments, a layer of their own", thousands(eb.len() - langs))).size(11.0).color(theme::hush()));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "{} builds recorded on {} machines - {} hours of compiling, every one from source.",
                            thousands(steps),
                            boxes,
                            thousands((secs / 3600) as usize)
                        ))
                        .size(12.5)
                        .color(theme::muted()),
                    );
                }
                if let Some(c) = cat {
                    let days = c.daily(30);
                    let top = days.iter().map(|d| d.1 + d.2).max().unwrap_or(0).max(1) as f32;
                    ui.add_space(8.0);
                    let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 58.0), egui::Sense::hover());
                    let p = ui.painter();
                    p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, theme::moss()));
                    let bw = r.width() / days.len().max(1) as f32;
                    let mut tip = None;
                    for (k, &(day, ok, bad)) in days.iter().enumerate() {
                        let x = r.left() + k as f32 * bw;
                        let h_ok = r.height() * ok as f32 / top;
                        let h_bad = r.height() * bad as f32 / top;
                        let col = egui::Rect::from_min_max(egui::pos2(x + 1.0, r.bottom() - h_ok), egui::pos2(x + bw - 1.0, r.bottom()));
                        p.rect_filled(col, 1, theme::primary());
                        if bad > 0 {
                            p.rect_filled(egui::Rect::from_min_max(egui::pos2(x + 1.0, col.top() - h_bad), egui::pos2(x + bw - 1.0, col.top())), 1, theme::amber());
                        }
                        if resp.hovered() && resp.hover_pos().is_some_and(|h| h.x >= x && h.x < x + bw) {
                            tip = Some(format!("{}: {} built, {} failed", day_label(day), thousands(ok), thousands(bad)));
                        }
                    }
                    if let Some(t) = tip {
                        resp.on_hover_text(t);
                    }
                    ui.label(RichText::new("builds finished per day, last 30 days - failures in amber").size(11.0).color(theme::hush()));
                }
                let live = self.live.lock().unwrap().clone();
                // Only the machines that are on: one switched off keeps its
                // last log forever, and on the front page that read as an
                // error (operator, 2026-10-18: "consider only showing live
                // ones"). On = written to in the last 15 minutes, not "off".
                let at = live.as_ref().and_then(|v| v["generated"].as_i64()).unwrap_or(0);
                let on = |l: &&serde_json::Value| l["state"].as_str() != Some("off") && at - l["mtime"].as_i64().unwrap_or(0) < 15 * 60;
                let lanes: Vec<&serde_json::Value> = live.as_ref().and_then(|v| v["lanes"].as_array()).map(|l| l.iter().filter(on).collect()).unwrap_or_default();
                if !lanes.is_empty() {
                    ui.add_space(8.0);
                    for l in lanes.into_iter().take(6) {
                        let key = l["key"].as_str().unwrap_or("?");
                        let state = l["state"].as_str().unwrap_or("");
                        let line = match (l["now"].as_str(), l["n"].as_u64(), l["of"].as_u64()) {
                            (Some(p), Some(n), Some(m)) if m > 0 => {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(key).monospace().size(11.5).color(theme::text()));
                                    let (r, _) = ui.allocate_exact_size(egui::vec2(70.0, 4.0), egui::Sense::hover());
                                    ui.painter().rect_filled(r, 2, theme::moss());
                                    ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width() * n as f32 / m as f32, r.height())), 2, theme::primary());
                                    ui.add(egui::Label::new(RichText::new(format!("{p} · {n}/{m}")).size(11.5).color(theme::muted())).truncate());
                                });
                                continue;
                            }
                            _ => format!("{key}  {}", if state.is_empty() { "idle" } else { state }),
                        };
                        ui.label(RichText::new(line).monospace().size(11.5).color(theme::muted()));
                    }
                }
                ui.add_space(4.0);
                ui.label(RichText::new("Packages and Builds, above, have all of it - search any of it.").size(11.5).color(theme::hush()));
            }
            "log" => {
                title(ui, if s.title.is_empty() { "The build machine" } else { &s.title });
                match self.log.lock().unwrap().clone() {
                    Some((key, state, lines, st)) => {
                        // The stage first, in words; the raw lines under it.
                        let step = st.step.as_ref().map_or(String::new(), |(n, m, _)| format!("step {n} of {m} · "));
                        ui.label(RichText::new(format!("{key} · {step}{}", if state.is_empty() { "log" } else { &state })).size(11.5).color(theme::muted()));
                        ui.label(RichText::new(&st.headline).size(15.0).strong().color(if st.error.is_some() { theme::alt() } else { theme::text_strong() }));
                        if let Some(e) = &st.error {
                            ui.add(egui::Label::new(RichText::new(e).size(11.5).color(theme::muted())).wrap());
                        } else if !st.doing.is_empty() {
                            ui.horizontal(|ui| {
                                ui.add(egui::Label::new(RichText::new(&st.doing).size(12.5).color(theme::amber())).truncate());
                                if let Some((n, of)) = st.progress {
                                    let txt = if of == 100 { format!("{n}%") } else { format!("{n} of {of}") };
                                    ui.label(RichText::new(txt).size(11.5).color(theme::muted()));
                                }
                            });
                            if let Some((n, of)) = st.progress.filter(|p| p.1 > 0) {
                                let (r, _) = ui.allocate_exact_size(egui::vec2(w - 4.0, 4.0), egui::Sense::hover());
                                ui.painter().rect_filled(r, 2, theme::moss());
                                ui.painter().rect_filled(egui::Rect::from_min_size(r.min, egui::vec2(r.width() * (n as f32 / of as f32).min(1.0), 4.0)), 2, theme::amber());
                            }
                        }
                        // The nine steps of a run, this one lit.
                        if let Some(at) = st.index() {
                            ui.add_space(4.0);
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                for (i, (name, words)) in crate::stage::STEPS.iter().enumerate() {
                                    let c = if i == at { theme::amber() } else if i < at { theme::primary() } else { theme::hush() };
                                    ui.label(RichText::new(*name).size(10.5).color(c)).on_hover_text(*words);
                                    if i + 1 < crate::stage::STEPS.len() {
                                        ui.label(RichText::new("›").size(10.5).color(theme::hush()));
                                    }
                                }
                            });
                        }
                        ui.add_space(4.0);
                        egui::Frame::new().fill(theme::ground()).corner_radius(4).inner_margin(8.0).show(ui, |ui| {
                            ui.set_width(w - 16.0);
                            for l in &lines {
                                ui.add(egui::Label::new(RichText::new(l).monospace().size(10.5).color(theme::text())).truncate());
                            }
                        });
                        ui.add_space(4.0);
                        ui.label(RichText::new("Builds › Logs, above, has every machine's whole log.").size(11.5).color(theme::hush()));
                    }
                    // No log has come: no machine has written one to show.
                    None => {
                        ui.label(RichText::new("No build machine is running. When one starts, its log shows here.").size(12.5).color(theme::muted()));
                    }
                }
            }
            "atlas" => {
                title(ui, &s.title);
                let found = cat.and_then(|c| c.find(if s.lang.is_empty() { "Python" } else { &s.lang }, &s.focus)).and_then(|i| cat.map(|c| (i, c)));
                if let Some((i, c)) = found {
                    let sw = c.sw(i);
                    ui.label(RichText::new(format!("{} {}", sw.name, sw.versions.last().map(String::as_str).unwrap_or(""))).size(14.0).strong().color(sw.kind.color()));
                    ui.label(RichText::new(format!("{} · {}", sw.kind.label(), c.live_facts(i))).size(12.0).color(theme::muted()));
                    ui.add_space(4.0);
                }
                if !s.caption.is_empty() {
                    ui.label(RichText::new(&s.caption).size(12.5).color(theme::text()));
                }
                facts_block(ui, &self.fact_lines(s, ui.ctx()), w);
            }
            "floors" => {
                title(ui, &s.title);
                if !s.caption.is_empty() {
                    ui.label(RichText::new(&s.caption).size(12.5).color(theme::text()));
                }
                facts_block(ui, &self.fact_lines(s, ui.ctx()), w);
            }
            _ => {
                title(ui, &s.title);
                // The card's own scroll holds it when the screen is short.
                for para in s.body.split("\n\n") {
                    ui.label(RichText::new(para.trim()).size(13.5).color(theme::text()));
                    ui.add_space(6.0);
                }
            }
        }
    }
}

/// A slide's facts: real settings, set as code, on the ground colour.
fn facts_block(ui: &mut egui::Ui, lines: &[String], w: f32) {
    if lines.is_empty() {
        return;
    }
    ui.add_space(6.0);
    egui::Frame::new().fill(theme::ground()).corner_radius(4).inner_margin(8.0).show(ui, |ui| {
        ui.set_width(w - 16.0);
        for l in lines {
            ui.add(egui::Label::new(RichText::new(l).monospace().size(10.5).color(theme::text())).wrap());
        }
    });
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (k, ch) in s.chars().enumerate() {
        if k > 0 && (s.len() - k) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// "Oct 17" for a UTC day start.
fn day_label(t: i64) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    // Civil from days (Howard Hinnant's algorithm).
    let z = t.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    format!("{} {d}", M[(m - 1) as usize])
}

#[cfg(test)]
mod tests {
    #[test]
    fn day_labels() {
        assert_eq!(super::day_label(1_792_195_200), "Oct 17");
        assert_eq!(super::day_label(0), "Jan 1");
    }
}

/// A slide's name for "next": its title, else its picture's caption, else
/// the package it points at.
fn title_of(s: &Slide) -> String {
    let t = if !s.title.is_empty() { s.title.as_str() } else if !s.caption.is_empty() { s.caption.as_str() } else { s.focus.as_str() };
    let t = t.lines().next().unwrap_or("");
    if t.chars().count() > 60 {
        format!("{}…", t.chars().take(58).collect::<String>())
    } else {
        t.to_owned()
    }
}
