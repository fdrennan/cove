//! The UI: one egui app, in the browser (wasm) and as a desktop window, both
//! talking to `app serve` over HTTP. Three tabs: Chaos Lab (chaos.rs, the
//! Lorenz system solved and drawn every frame - the first screen), Dependency
//! Universe (atlas.rs, the ndexr stack drawn live) and Media (left: the
//! files; right: what can be done to the one picked, and the jobs doing it).
//!
//! The two visuals fill the whole window (operator, 2026-10-24: "full screen
//! for the apps themselves ... beautiful and immersive"): the tabs float as a
//! pill in the corner and each visual's controls as a translucent card that
//! folds to its title (`float_card`). H hides them all.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align2, Color32, RichText, Stroke};

use crate::api::{AtlasView, Op, Query, State};

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Chaos,
    Atlas,
    Media,
}

pub struct App {
    /// Where `app serve` is: the page's own folder in a browser.
    base: String,
    tab: Tab,
    chaos: crate::chaos::Chaos,
    atlas: crate::atlas::Atlas,
    atlas_view: Slot<AtlasView>,
    atlas_asked: bool,
    state: Slot<State>,
    asked: f64,
    picked: Option<String>,
    start: String,
    end: String,
    note: Arc<Mutex<Option<String>>>,
    /// H pressed: the visual alone, no overlays.
    bare: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, base: String) -> Self {
        // set_theme, not set_visuals: with visuals alone the browser kept egui's light
        // panels (measured 2026-10-24, the atlas legend unreadable) - Cove's own fix.
        cc.egui_ctx.set_theme(egui::Theme::Dark);
        // Larger type: the controls float over a full-screen visual and are
        // read at a glance.
        cc.egui_ctx.all_styles_mut(|s| {
            for f in s.text_styles.values_mut() {
                f.size *= 1.15;
            }
        });
        App { base, tab: Tab::Chaos, chaos: Default::default(), atlas: Default::default(), atlas_view: Default::default(), atlas_asked: false, state: Default::default(), asked: f64::NEG_INFINITY, picked: None, start: "0".into(), end: "10".into(), note: Default::default(), bare: false }
    }

    fn ask<T: serde::de::DeserializeOwned + Send + 'static>(&self, q: &Query, ctx: &egui::Context, done: impl FnOnce(Result<T, String>) + Send + 'static) {
        let mut req = same_origin(ehttp::Request::post(format!("{}/api/q", self.base), serde_json::to_vec(q).unwrap_or_default()));
        req.headers.insert("Content-Type", "application/json");
        let ctx = ctx.clone();
        ehttp::fetch(req, move |r| {
            done(match r {
                Ok(r) if r.ok => serde_json::from_slice(&r.bytes).map_err(|e| e.to_string()),
                Ok(r) => Err(serde_json::from_slice::<serde_json::Value>(&r.bytes).ok().and_then(|v| v["error"].as_str().map(str::to_owned)).unwrap_or_else(|| format!("{}", r.status))),
                Err(e) => Err(e),
            });
            ctx.request_repaint();
        });
    }

    fn refresh(&mut self, ctx: &egui::Context, now: f64) {
        let busy = matches!(&*self.state.lock().unwrap(), Some(Ok(s)) if s.jobs.iter().any(|j| j.running));
        if now - self.asked < if busy { 1.0 } else { 5.0 } {
            return;
        }
        self.asked = now;
        let slot = self.state.clone();
        self.ask(&Query::State, ctx, move |r| *slot.lock().unwrap() = Some(r));
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }

    fn run(&mut self, op: Op, file: &str, ctx: &egui::Context) {
        let note = self.note.clone();
        let q = Query::Run { op, file: file.to_owned(), start: self.start.clone(), end: self.end.clone() };
        self.ask(&q, ctx, move |r: Result<crate::api::Job, String>| {
            *note.lock().unwrap() = Some(match r {
                Ok(j) => format!("started: {}", j.what),
                Err(e) => format!("not started: {e}"),
            });
        });
        self.asked = f64::NEG_INFINITY;
    }

    /// The Dependency Universe, full screen, once the catalog is in.
    fn atlas_tab(&mut self, root: &mut egui::Ui, ctx: &egui::Context) {
        if !self.atlas_asked {
            self.atlas_asked = true;
            let slot = self.atlas_view.clone();
            self.ask(&Query::Atlas, ctx, move |r| *slot.lock().unwrap() = Some(r));
        }
        let view = self.atlas_view.lock().unwrap().clone();
        match view {
            Some(Ok(v)) => self.atlas.ui(root, &v, self.bare),
            other => {
                egui::CentralPanel::default().frame(egui::Frame::new().fill(crate::chaos::DEEP)).show(root, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.4);
                        match other {
                            Some(Err(e)) => {
                                ui.colored_label(Color32::LIGHT_RED, format!("The stack's catalog did not load: {e}"));
                                if ui.button("Try again").clicked() {
                                    self.atlas_asked = false;
                                }
                            }
                            _ => {
                                ui.spinner();
                                ui.label(RichText::new("Reading the ndexr stack's catalog…").color(Color32::from_gray(200)));
                            }
                        }
                    });
                });
            }
        }
    }

    /// The tabs, as a pill floating top left over the visual.
    fn tabs_pill(&mut self, ctx: &egui::Context) {
        egui::Area::new(egui::Id::new("tabs-pill")).anchor(Align2::LEFT_TOP, egui::vec2(16.0, 16.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            glass().corner_radius(22).inner_margin(egui::Margin::symmetric(12, 6)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(host_name()).size(14.0).strong().color(Color32::WHITE));
                    ui.add_space(6.0);
                    for (t, name) in [(Tab::Chaos, "Chaos Lab"), (Tab::Atlas, "Dependency Universe"), (Tab::Media, "Media")] {
                        ui.selectable_value(&mut self.tab, t, RichText::new(name).size(14.0));
                    }
                });
            });
        });
    }

    /// Pick files on this device and send each into media/.
    fn upload(&self, ctx: &egui::Context) {
        let (base, note, ctx) = (self.base.clone(), self.note.clone(), ctx.clone());
        let pick = async move {
            let Some(files) = rfd::AsyncFileDialog::new().set_title("Add video or audio").pick_files().await else { return };
            for f in files {
                let name = f.file_name();
                let bytes = f.read().await;
                *note.lock().unwrap() = Some(format!("uploading {name}…"));
                let enc: String = name.bytes().map(|b| if b.is_ascii_alphanumeric() || b".-_".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") }).collect();
                let (note, ctx) = (note.clone(), ctx.clone());
                ehttp::fetch(same_origin(ehttp::Request::post(format!("{base}/api/upload?name={enc}"), bytes)), move |r| {
                    *note.lock().unwrap() = Some(match r {
                        Ok(r) if r.ok => format!("added {name}"),
                        Ok(r) => format!("{name}: {}", String::from_utf8_lossy(&r.bytes)),
                        Err(e) => format!("{name}: {e}"),
                    });
                    ctx.request_repaint();
                });
            }
        };
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(pick);
        #[cfg(not(target_arch = "wasm32"))]
        std::thread::spawn(move || pollster::block_on(pick));
    }
}

/// A card floating over a full-screen visual: translucent, foldable to its
/// title. Top right on a desk; along the bottom, full width, on a phone.
pub fn float_card(ctx: &egui::Context, id: &str, title: &str, open: &mut bool, add: impl FnOnce(&mut egui::Ui)) {
    let screen = ctx.content_rect();
    let narrow = screen.width() < 760.0;
    let w = if narrow { screen.width() - 24.0 } else { 330.0 };
    let (anchor, off) = if narrow { (Align2::CENTER_BOTTOM, egui::vec2(0.0, -12.0)) } else { (Align2::RIGHT_TOP, egui::vec2(-16.0, 16.0)) };
    egui::Area::new(egui::Id::new(id)).anchor(anchor, off).order(egui::Order::Foreground).show(ctx, |ui| {
        glass().show(ui, |ui| {
            ui.set_width(w - 28.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new(title).size(17.0).strong().color(Color32::WHITE));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let b = egui::Button::new(RichText::new(if *open { "–" } else { "+" }).size(16.0)).min_size(egui::vec2(32.0, 28.0));
                    if ui.add(b).on_hover_text(if *open { "fold" } else { "open" }).clicked() {
                        *open = !*open;
                    }
                });
            });
            if *open {
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(screen.height() * if narrow { 0.42 } else { 0.8 }).show(ui, add);
            }
        });
    });
}

/// The overlays' glass: dark, see-through, a hairline edge.
pub fn glass() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(10, 13, 22, 205))
        .stroke(Stroke::new(1.0, Color32::from_white_alpha(26)))
        .corner_radius(14)
        .inner_margin(14)
}

fn size(b: u64) -> String {
    match b {
        b if b >= 1 << 30 => format!("{:.1} GB", b as f64 / (1u64 << 30) as f64),
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b => format!("{} KB", b / 1024),
    }
}

impl eframe::App for App {
    fn ui(&mut self, root: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &root.ctx().clone();
        let now = ctx.input(|i| i.time);
        // H: every overlay away, the visual alone (and back).
        if !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::H)) {
            self.bare = !self.bare;
        }
        if self.tab != Tab::Media {
            if self.tab == Tab::Chaos {
                self.chaos.ui(root, self.bare);
            } else {
                self.atlas_tab(root, ctx);
            }
            if !self.bare {
                self.tabs_pill(ctx);
            }
            return;
        }
        egui::Panel::top("bar").show(root, |ui| {
            ui.horizontal(|ui| {
                ui.heading(host_name());
                ui.add_space(12.0);
                ui.selectable_value(&mut self.tab, Tab::Chaos, "Chaos Lab").on_hover_text("the Lorenz system, solved as you watch");
                ui.selectable_value(&mut self.tab, Tab::Atlas, "Dependency Universe").on_hover_text("every package of the ndexr stack and what it needs, live");
                ui.selectable_value(&mut self.tab, Tab::Media, "Media").on_hover_text("clean, cut and extract audio with the stack's FFmpeg");
                if self.tab == Tab::Media {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("+ Add video or audio").clicked() {
                            self.upload(ctx);
                        }
                        if let Some(n) = self.note.lock().unwrap().as_ref() {
                            ui.label(RichText::new(n).weak());
                        }
                    });
                }
            });
        });
        self.refresh(ctx, now);
        let state = self.state.lock().unwrap().clone();
        let narrow = ctx.content_rect().width() < 640.0;
        let files = egui::Panel::left("files").resizable(true).default_size(if narrow { 160.0 } else { 280.0 });
        files.show(root, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| match &state {
                None => {
                    ui.spinner();
                }
                Some(Err(e)) => {
                    ui.colored_label(egui::Color32::LIGHT_RED, e);
                }
                Some(Ok(s)) => {
                    for dir in ["media", "out"] {
                        ui.label(RichText::new(if dir == "media" { "YOUR FILES" } else { "RESULTS" }).small().weak());
                        for f in s.files.iter().filter(|f| f.path.starts_with(dir)) {
                            let name = f.path.split_once('/').map(|x| x.1).unwrap_or(&f.path);
                            let label = format!("{} {name}  ·  {}", if f.video { "▶" } else { "♪" }, size(f.bytes));
                            if ui.selectable_label(self.picked.as_deref() == Some(f.path.as_str()), label).clicked() {
                                self.picked = Some(f.path.clone());
                            }
                        }
                        if !s.files.iter().any(|f| f.path.starts_with(dir)) {
                            ui.label(RichText::new(if dir == "media" { "nothing yet - Add video or audio, top right" } else { "nothing made yet" }).weak());
                        }
                        ui.add_space(8.0);
                    }
                }
            });
        });
        egui::CentralPanel::default().show(root, |ui| {
            let Some(file) = self.picked.clone() else {
                ui.label("Pick a file on the left.");
                return;
            };
            ui.heading(&file);
            ui.hyperlink_to("Open / download", format!("{}/files/{file}", self.base));
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                if ui.button(Op::Clean.label()).on_hover_text("take out background noise and hum, even out the loudness").clicked() {
                    self.run(Op::Clean, &file, ctx);
                }
                if ui.button(Op::Audio.label()).on_hover_text("the sound by itself, as a WAV").clicked() {
                    self.run(Op::Audio, &file, ctx);
                }
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("from");
                ui.add(egui::TextEdit::singleline(&mut self.start).desired_width(70.0));
                ui.label("to");
                ui.add(egui::TextEdit::singleline(&mut self.end).desired_width(70.0));
                if ui.button(Op::Cut.label()).on_hover_text("seconds (12.5) or h:mm:ss").clicked() {
                    self.run(Op::Cut, &file, ctx);
                }
            });
            ui.separator();
            ui.label(RichText::new("JOBS").small().weak());
            if let Some(Ok(s)) = &state {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for j in &s.jobs {
                        ui.horizontal(|ui| {
                            if j.running {
                                ui.spinner();
                            } else if j.ok == Some(true) {
                                ui.colored_label(egui::Color32::LIGHT_GREEN, "✔");
                            } else {
                                ui.colored_label(egui::Color32::LIGHT_RED, "✘");
                            }
                            ui.label(&j.what);
                            if j.ok == Some(true) {
                                if ui.link(&j.out).clicked() {
                                    self.picked = Some(j.out.clone());
                                }
                            }
                        });
                        if j.ok == Some(false) || j.running {
                            if let Some(l) = j.tail.last() {
                                ui.label(RichText::new(l).monospace().small().weak());
                            }
                        }
                    }
                });
            }
        });
    }
}

/// A request to this app's own server that carries the page's sign-in
/// cookie. ehttp's fetch omits credentials by default, so behind a login or
/// admin gate every API call came back as a redirect to sign-in and the page
/// said "Failed to fetch" while the page itself had loaded (du.ndexr.io
/// 2026-11-08, kalshi.ndexr.io 2026-11-17).
#[allow(unused_mut)]
fn same_origin(mut req: ehttp::Request) -> ehttp::Request {
    #[cfg(target_arch = "wasm32")]
    {
        req.credentials = ehttp::Credentials::SameOrigin;
    }
    req
}

/// The name this project was laid out with (Cove fills `__HOST__` in).
pub const LAID: &str = "__HOST__";

/// This app's name, read when it runs, so a binary Cove compiled ahead of
/// time from this template (a seed) names the project it is in: in a browser
/// the page's title (web/index.html, laid for this project), on a desktop or
/// a server APP_HOST (.cove/rust/env.sh), else the name it was laid out with.
pub fn host_name() -> String {
    #[cfg(target_arch = "wasm32")]
    if let Some(t) = web_sys::window().and_then(|w| w.document()).map(|d| d.title()).filter(|t| !t.trim().is_empty()) {
        return t;
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(h) = std::env::var("APP_HOST").ok().filter(|h| !h.trim().is_empty()) {
        return h;
    }
    LAID.to_owned()
}
