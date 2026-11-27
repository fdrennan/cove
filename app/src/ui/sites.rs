//! Sites: the open tabs side by side (operator, 2026-11-27: "for each domain
//! tab we have ... Sites will show what is currently selected in the tabs
//! (though the user could search for the others) - Sites could become what
//! the agents.ndexr.io site was but written and improved in rust here").
//!
//! agents.ndexr.io was a wall of "lenses" onto the platform's sites: each
//! tile one site, seen through one lens - its agent, its live page, its
//! terminal - in one-at-a-time or grid mode, each tile's lens its own. Here
//! the tiles are the person's open tabs, the selected one first, and each
//! has its own lens:
//!   Site    the live page (a frame laid over the tile in a browser; frames.rs)
//!   Agent   the site's thread - what was asked, what came back, whether it
//!           is working - and a box to give it the next turn
//!   Logs    the app's log, .cove/run/app.log
//! Focus shows the selected site large and the rest under it; Grid shows
//! them all the same size. Search reaches every site, and a result opens as
//! a tab. Each tile asks for its own data, only for the lens it shows.
//! The full list - access, sharing, deleting - is the All sites view (app.rs
//! connect_view), unchanged.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, RichText, Sense};

use crate::api::{AgentTurn, AppStatus, DomainInfo, FileText, Query};
use crate::backend::Backend;
use crate::ui::kit::theme;

#[cfg(target_arch = "wasm32")]
mod frames;

type Slot<T> = Arc<Mutex<Option<T>>>;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum Lens {
    #[default]
    Site,
    Agent,
    Logs,
}

impl Lens {
    const ALL: [Lens; 3] = [Lens::Site, Lens::Agent, Lens::Logs];
    fn word(self) -> &'static str {
        match self {
            Lens::Site => "Site",
            Lens::Agent => "Agent",
            Lens::Logs => "Logs",
        }
    }
    fn tip(self) -> &'static str {
        match self {
            Lens::Site => "the live page",
            Lens::Agent => "the site's agent: its thread, and a turn to give it",
            Lens::Logs => "the app's log (.cove/run/app.log)",
        }
    }
}

/// What one tile knows about its site, each asked on its own clock.
#[derive(Default)]
struct Tile {
    status: Slot<Result<AppStatus, String>>,
    turns: Slot<Result<Vec<AgentTurn>, String>>,
    log: Slot<Result<FileText, String>>,
    sent: Slot<Result<String, String>>,
    asked: HashMap<&'static str, f64>,
    prompt: String,
}

/// What the wall asks of the IDE: make a site the selected tab (open it as
/// one when it is not).
pub enum WallAct {
    Select(String),
}

#[derive(Default)]
pub struct SitesWall {
    /// Every site, as a list (app.rs connect_view), instead of the open tabs.
    pub all: bool,
    grid: bool,
    lens: HashMap<String, Lens>,
    tiles: HashMap<String, Tile>,
    pub find: String,
}

impl SitesWall {
    /// The two views' switch, above either: Open tabs (this wall) or All sites.
    pub fn switch(&mut self, ui: &mut egui::Ui, open: usize) {
        let items = [
            (format!("Open tabs · {open}"), "the sites open as tabs, each through its own lens".to_owned(), !self.all, true),
            ("All sites".to_owned(), "every site you may open: search, access, sharing".to_owned(), self.all, true),
        ];
        if let Some(i) = ui.horizontal(|ui| crate::ui::agent::segmented(ui, true, &items)).inner {
            self.all = i == 1;
        }
    }

    /// The wall. `open`: the open tabs, in order, as DomainInfo (a tab with
    /// none - not one of this person's sites yet - shows by name);
    /// `active`: the selected tab; `every`: every site, for the search.
    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, open: &[(String, Option<DomainInfo>)], active: Option<&str>, every: &[DomainInfo]) -> Option<WallAct> {
        let mut act = None;
        let now = ctx.input(|i| i.time);
        // The bar: search every site, then Focus or Grid.
        ui.horizontal(|ui| {
            let w = (ui.available_width() - 190.0).max(160.0);
            ui.add(egui::TextEdit::singleline(&mut self.find).hint_text("Search every site to open it as a tab").desired_width(w).margin(egui::vec2(10.0, 7.0)));
            let items = [
                ("Focus".to_owned(), "the selected tab large, the others under it".to_owned(), !self.grid, true),
                ("Grid".to_owned(), "every open tab the same size".to_owned(), self.grid, true),
            ];
            if let Some(i) = crate::ui::agent::segmented(ui, false, &items) {
                self.grid = i == 1;
            }
        });
        let find = self.find.trim().to_lowercase();
        if !find.is_empty() {
            let hits: Vec<&DomainInfo> = every.iter().filter(|d| [&d.host, &d.title, &d.owner].iter().any(|t| t.to_lowercase().contains(&find))).take(8).collect();
            egui::Frame::new().fill(theme::surface()).stroke(egui::Stroke::new(1.0, theme::pane_brd())).corner_radius(8).inner_margin(8.0).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if hits.is_empty() {
                    ui.label(RichText::new(format!("No site matches \"{}\".", self.find.trim())).size(13.0).color(theme::text()));
                }
                for d in hits {
                    let is_open = open.iter().any(|(h, _)| *h == d.host);
                    ui.horizontal(|ui| {
                        crate::ui::kit::frameworks::icon(ui, &d.framework, 16.0);
                        ui.label(RichText::new(&d.host).monospace().size(13.0).color(theme::text_strong()));
                        if !d.owner.is_empty() && d.path.is_empty() {
                            ui.label(RichText::new(format!("· {}", d.owner)).size(12.5).color(theme::text()));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button(if is_open { "Select" } else { "Open as a tab" }).clicked() {
                                act = Some(WallAct::Select(d.host.clone()));
                                self.find.clear();
                            }
                        });
                    });
                }
            });
        }
        ui.add_space(8.0);
        if open.is_empty() {
            ui.label(RichText::new("No tabs are open. Search above, or open a project from Workspace, and it shows here.").size(14.0).color(theme::text()));
            return act;
        }
        // The selected tab first, the rest in tab order.
        let mut order: Vec<&(String, Option<DomainInfo>)> = open.iter().collect();
        if let Some(a) = active {
            order.sort_by_key(|(h, _)| h != a);
        }
        let area = ui.available_rect_before_wrap();
        let gap = 10.0;
        let phone = area.width() < 700.0;
        let cols = if phone { 1 } else if self.grid { ((area.width() / 460.0).floor() as usize).clamp(1, 3) } else { ((area.width() / 380.0).floor() as usize).clamp(1, 4) };
        egui::ScrollArea::vertical().id_salt("sites-wall").auto_shrink([false, false]).show(ui, |ui| {
            let clip = ui.clip_rect();
            let w = ui.available_width();
            let mut i = 0;
            // Focus: the selected one across the width, most of the height.
            if !self.grid && active.is_some() && !phone {
                let h = (area.height() * 0.62).max(320.0);
                let (r, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
                let (host, d) = order[0];
                if let Some(a) = self.tile(ui, ctx, backend, r, clip, host, d.as_ref(), true, now) {
                    act = Some(a);
                }
                ui.add_space(gap);
                i = 1;
            }
            let rest = &order[i..];
            let rows = rest.len().div_ceil(cols);
            let fit = if self.grid { (area.height() - gap * (rows.max(1) as f32 - 1.0)) / rows.max(1) as f32 } else { 0.0 };
            let h = if phone { 420.0 } else if self.grid { fit.max(260.0) } else { 280.0 };
            for chunk in rest.chunks(cols) {
                let (row, _) = ui.allocate_exact_size(egui::vec2(w, h), Sense::hover());
                let tw = (w - gap * (cols as f32 - 1.0)) / cols as f32;
                for (k, (host, d)) in chunk.iter().enumerate() {
                    let r = egui::Rect::from_min_size(egui::pos2(row.left() + k as f32 * (tw + gap), row.top()), egui::vec2(tw, h));
                    if let Some(a) = self.tile(ui, ctx, backend, r, clip, host, d.as_ref(), Some(host.as_str()) == active, now) {
                        act = Some(a);
                    }
                }
                ui.add_space(gap);
            }
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(1000));
        act
    }

    /// One site's tile at `r`: its bar (state, name, lens) and what its lens shows.
    #[allow(clippy::too_many_arguments)]
    fn tile(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, r: egui::Rect, clip: egui::Rect, host: &str, d: Option<&DomainInfo>, selected: bool, now: f64) -> Option<WallAct> {
        let mut act = None;
        let lens = *self.lens.entry(host.to_owned()).or_default();
        let t = self.tiles.entry(host.to_owned()).or_default();
        // Its state, every 15 s, whatever the lens: the light in its bar.
        if let Some(d) = d {
            ask(t, "status", 15.0, now, backend, ctx, Query::AppStatus { host: d.host.clone() }, |t| t.status.clone());
        }
        let p = ui.painter().with_clip_rect(clip);
        p.rect(r, 10.0, theme::bg(), egui::Stroke::new(if selected { 1.5 } else { 1.0 }, if selected { theme::primary().gamma_multiply(0.8) } else { theme::pane_brd() }), egui::StrokeKind::Inside);
        let bar = egui::Rect::from_min_size(r.min, egui::vec2(r.width(), 36.0));
        let body = egui::Rect::from_min_max(egui::pos2(r.left() + 1.0, bar.bottom()), r.max - egui::vec2(1.0, 1.0));
        let name_r = egui::Rect::from_min_max(bar.min + egui::vec2(10.0, 4.0), egui::pos2(bar.right() - 200.0f32.min(bar.width() * 0.5) - 8.0, bar.bottom() - 4.0));
        let mut bui = ui.new_child(egui::UiBuilder::new().max_rect(name_r).layout(egui::Layout::left_to_right(egui::Align::Center)).id_salt(("tile-bar", host)));
        bui.set_clip_rect(bar.intersect(clip));
        let running = t.status.lock().unwrap().as_ref().and_then(|s| s.as_ref().ok()).map(|s| s.running);
        let (dot, state) = match running {
            Some(true) => (theme::ok(), "running"),
            Some(false) => (theme::hush(), "stopped"),
            None => (theme::hush(), ""),
        };
        let (dr, _) = bui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
        bui.painter().circle_filled(dr.center(), 4.0, dot);
        let name = bui.add(egui::Label::new(RichText::new(host).font(theme::display(13.5)).color(theme::text_strong())).truncate().sense(Sense::click()));
        if name.on_hover_text(if selected { format!("{host} is the selected tab{}", if state.is_empty() { String::new() } else { format!(" · {state}") }) } else { format!("make {host} the selected tab") }).clicked() && !selected {
            act = Some(WallAct::Select(host.to_owned()));
        }
        // The lens switch at the bar's right, read left to right: laid out from
        // the right it came out "Logs Agent Site".
        let sw = 190.0f32.min(bar.width() * 0.5);
        let sr = egui::Rect::from_min_max(egui::pos2(bar.right() - sw - 8.0, bar.top()), egui::pos2(bar.right() - 8.0, bar.bottom()));
        let mut sui = ui.new_child(egui::UiBuilder::new().max_rect(sr).layout(egui::Layout::left_to_right(egui::Align::Center).with_main_align(egui::Align::Max)).id_salt(("tile-lens", host)));
        sui.set_clip_rect(sr.intersect(clip));
        let items: Vec<(String, String, bool, bool)> = Lens::ALL.iter().map(|l| (l.word().to_owned(), l.tip().to_owned(), *l == lens, d.is_some() || *l == Lens::Site)).collect();
        if let Some(i) = crate::ui::agent::segmented(&mut sui, false, &items) {
            self.lens.insert(host.to_owned(), Lens::ALL[i]);
        }
        ui.painter().with_clip_rect(clip).hline(r.x_range(), bar.bottom(), egui::Stroke::new(1.0, theme::pane_brd()));
        let t = self.tiles.get_mut(host).expect("made above");
        let mut cui = ui.new_child(egui::UiBuilder::new().max_rect(body.shrink(10.0)).layout(egui::Layout::top_down(egui::Align::Min)).id_salt(("tile-body", host)));
        cui.set_clip_rect(body.intersect(clip));
        match (lens, d) {
            (Lens::Site, _) => site(&mut cui, ctx, backend, t, body, clip, host),
            (Lens::Agent, Some(d)) => agent(&mut cui, ctx, backend, t, d, now),
            (Lens::Logs, Some(d)) => logs(&mut cui, ctx, backend, t, d, now),
            (_, None) => {
                cui.label(RichText::new("Not one of your sites here: its agent and log are its owner's.").size(13.0).color(theme::text()));
            }
        }
        act
    }
}

/// Ask `q` again when `every` seconds have gone since the last time.
fn ask<T: serde::de::DeserializeOwned + Send + 'static>(t: &mut Tile, key: &'static str, every: f64, now: f64, backend: &Backend, ctx: &egui::Context, q: Query, slot: impl Fn(&Tile) -> Slot<Result<T, String>>) {
    if t.asked.get(key).is_some_and(|at| now - at < every) {
        return;
    }
    t.asked.insert(key, now);
    let s = slot(t);
    backend.query(q, ctx, move |r: Result<T, String>| {
        *s.lock().unwrap() = Some(r);
    });
}

/// The live page: where the app is seen when it runs through this Cove, else
/// its own address. In a browser a frame over the tile, kept loaded while
/// the tile is on screen; the desktop app opens it in the browser.
#[allow(unused_variables)]
fn site(ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, t: &Tile, body: egui::Rect, clip: egui::Rect, host: &str) {
    let seen = t.status.lock().unwrap().as_ref().and_then(|s| s.as_ref().ok()).map(|s| s.seen_at.clone()).unwrap_or_default();
    let url = if seen.is_empty() { format!("https://{host}/") } else { backend.viewer_url(&seen) };
    #[cfg(target_arch = "wasm32")]
    {
        // This Cove itself is never framed in itself.
        let here = web_sys::window().and_then(|w| w.location().host().ok()).unwrap_or_default();
        if here == host {
            ui.label(RichText::new(format!("{host} is this Cove: its page is the one you are in.")).size(13.0).color(theme::text()));
            return;
        }
        // Only whole on screen: a frame cannot be clipped by the scroll.
        if clip.contains_rect(body) {
            frames::place(ctx, host, body, &url);
        } else {
            ui.label(RichText::new("Scroll to show the page.").size(13.0).color(theme::text()));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        ui.label(RichText::new(&url).monospace().size(12.5).color(theme::text()));
        ui.add_space(6.0);
        if ui.button("Open in the browser ↗").clicked() {
            crate::ui::kit::links::new_tab(ctx, url);
        }
    }
}

/// The site's agent: its last turns - what was asked, what it said, the
/// tools it used - and a box to give it the next one. Asked every 8 s, every
/// 3 s while a turn is going.
fn agent(ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, t: &mut Tile, d: &DomainInfo, now: f64) {
    let busy = t.turns.lock().unwrap().as_ref().and_then(|r| r.as_ref().ok()).and_then(|v| v.last()).is_some_and(|x| !x.done);
    ask(t, "turns", if busy { 3.0 } else { 8.0 }, now, backend, ctx, Query::AgentHistory { domain: d.host.clone() }, |t| t.turns.clone());
    // The box first, at the top: the turns scroll under it.
    let mut send = false;
    ui.horizontal(|ui| {
        let w = (ui.available_width() - 70.0).max(80.0);
        let r = ui.add(egui::TextEdit::singleline(&mut t.prompt).hint_text(format!("Ask {}'s agent", crate::ui::ide::tabs::Tabs::short(&d.host))).desired_width(w));
        send = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        send |= ui.add_enabled(!t.prompt.trim().is_empty(), egui::Button::new("Send")).clicked();
    });
    if send && !t.prompt.trim().is_empty() {
        let prompt = std::mem::take(&mut t.prompt);
        let sent = t.sent.clone();
        backend.query(
            Query::AgentRun { domain: d.host.clone(), prompt, model: crate::ui::agent::default_model(), cwd: Some(d.path.clone()).filter(|p| !p.is_empty()), mode: None },
            ctx,
            move |r: Result<String, String>| {
                *sent.lock().unwrap() = Some(r);
            },
        );
        // The new turn in the thread at the next look.
        t.asked.remove("turns");
    }
    if let Some(Err(e)) = t.sent.lock().unwrap().clone() {
        ui.add(egui::Label::new(RichText::new(e).size(12.5).color(theme::warn())).wrap());
    }
    ui.add_space(6.0);
    match t.turns.lock().unwrap().clone() {
        None => {
            ui.spinner();
        }
        Some(Err(e)) => {
            ui.add(egui::Label::new(RichText::new(e).size(12.5).color(theme::warn())).wrap());
        }
        Some(Ok(v)) if v.is_empty() => {
            ui.label(RichText::new("No turns yet. Ask it something above.").size(13.0).color(theme::text()));
        }
        Some(Ok(v)) => {
            egui::ScrollArea::vertical().id_salt(("tile-turns", &d.host)).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                for turn in v.iter().rev().take(4).rev() {
                    turn_view(ui, turn);
                    ui.add_space(8.0);
                }
            });
        }
    }
}

fn turn_view(ui: &mut egui::Ui, turn: &AgentTurn) {
    ui.add(egui::Label::new(RichText::new(format!("› {}", one_line(crate::ui::agent::shown_prompt(&turn.prompt), 200))).size(13.0).color(theme::text_strong())).wrap());
    let tools = turn.events.iter().filter(|e| e.kind == "tool").count();
    let said: Vec<&str> = turn.events.iter().filter(|e| e.kind == "text").map(|e| e.text.as_str()).collect();
    let errs: Vec<&str> = turn.events.iter().filter(|e| e.kind == "error").map(|e| e.text.as_str()).collect();
    if let Some(last) = said.last() {
        ui.add(egui::Label::new(RichText::new(one_line(last, 600)).size(12.5).color(theme::text())).wrap());
    }
    for e in errs.iter().rev().take(1) {
        ui.add(egui::Label::new(RichText::new(one_line(e, 300)).size(12.5).color(theme::warn())).wrap());
    }
    let mut meta = Vec::new();
    if tools > 0 {
        meta.push(format!("{tools} step{}", if tools == 1 { "" } else { "s" }));
    }
    if !turn.done {
        meta.push("working…".to_owned());
    }
    if !meta.is_empty() {
        ui.label(RichText::new(meta.join(" · ")).size(12.0).color(if turn.done { theme::text() } else { theme::primary() }));
    }
}

/// The app's log, its last lines, asked every 4 s.
fn logs(ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, t: &mut Tile, d: &DomainInfo, now: f64) {
    if d.path.is_empty() {
        ui.label(RichText::new("No folder here to read a log from.").size(13.0).color(theme::text()));
        return;
    }
    ask(t, "log", 4.0, now, backend, ctx, Query::AppLog { path: d.path.clone(), build: false }, |t| t.log.clone());
    match t.log.lock().unwrap().clone() {
        None => {
            ui.spinner();
        }
        Some(Err(e)) => {
            ui.add(egui::Label::new(RichText::new(e).size(12.5).color(theme::warn())).wrap());
        }
        Some(Ok(f)) if f.text.trim().is_empty() => {
            ui.label(RichText::new("The log is empty: the app has not been started here.").size(13.0).color(theme::text()));
        }
        Some(Ok(f)) => {
            let lines: Vec<&str> = f.text.lines().collect();
            let tail = lines[lines.len().saturating_sub(200)..].join("\n");
            egui::ScrollArea::both().id_salt(("tile-log", &d.host)).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(tail).monospace().size(11.5).color(line_color(&f.text))).extend());
            });
        }
    }
}

/// The log's colour: red when its last lines hold an error.
fn line_color(text: &str) -> Color32 {
    let tail: String = text.lines().rev().take(5).collect::<Vec<_>>().join("\n").to_lowercase();
    if tail.contains("error") || tail.contains("panicked") || tail.contains("traceback") {
        theme::warn()
    } else {
        theme::text()
    }
}

fn one_line(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() > max {
        format!("{}…", flat.chars().take(max).collect::<String>())
    } else {
        flat
    }
}

/// After every frame: the wall's pages stay only while the wall placed them
/// this frame, and never over a dialog or a menu (`hide`).
pub fn end_frame(hide: bool) {
    #[cfg(target_arch = "wasm32")]
    frames::end_frame(hide);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = hide;
}
