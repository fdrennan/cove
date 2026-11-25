//! The Builds tab's Logs view (operator, 2026-10-15: "users should be able to
//! watch the live logs through cove.ndexr.io in software somewhere, live is
//! good"). Every build machine's run log, as `ndexr aws server watch` keeps it
//! on the host (host.rs build_log): pick a machine and its log follows the
//! build - asked every 2 s from the byte it last had, so a poll costs only what
//! was written since. A box that has gone away still shows what it said.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align, Color32, FontId, Layout, RichText};
use serde::Deserialize;

use super::{fmt_ago, fmt_dur, now_secs, segment, SoftwarePane};
use crate::backend::Backend;
use crate::ui::kit::theme;

#[derive(Deserialize, Default, Clone)]
#[serde(default)]
struct LogInfo {
    key: String,
    size: u64,
    mtime: i64,
    state: Option<String>,
    step: Option<String>,
    started: Option<String>,
    host: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Reply {
    logs: Vec<LogInfo>,
    key: Option<String>,
    from: u64,
    next: u64,
    size: u64,
    reset: bool,
    text: String,
}

/// More than this and the oldest lines go: a day of compiling is millions.
const KEEP: usize = 60_000;

#[derive(Default)]
pub struct Logs {
    slot: Arc<Mutex<Option<Result<Reply, String>>>>,
    asked: f64,
    waiting: bool,
    list: Vec<LogInfo>,
    err: Option<String>,
    key: Option<String>,
    lines: Vec<String>,
    /// The byte the next ask starts at; None asks for the tail.
    next: Option<u64>,
    size: u64,
    paused: bool,
    filter: String,
    wrap: bool,
    /// The widest line this log has shown, in bytes: only ever grows while
    /// the same log is open, so the oldest lines draining off (KEEP) never
    /// narrows it and flips the horizontal bar.
    widest: usize,
}

pub(super) fn tone(line: &str) -> Color32 {
    let l = line.trim_start();
    let low = l.to_ascii_lowercase();
    if l.starts_with("[ok]") || l.starts_with("* DONE") || l.contains(" — done") {
        theme::ok()
    } else if low.starts_with("error") || low.contains(" error:") || low.contains("[failed]") || low.contains("failed (rc") || low.starts_with("fatal") {
        theme::alt()
    } else if low.contains("warning:") || low.starts_with("warn") {
        theme::amber()
    } else if l.starts_with("[ndexr") || l.starts_with("==>") || l.starts_with("* now installing") || l.starts_with("  ·") {
        theme::primary()
    } else {
        theme::muted()
    }
}

impl Logs {
    fn take(&mut self) -> bool {
        let Some(r) = self.slot.lock().unwrap().take() else { return false };
        self.waiting = false;
        match r {
            Err(e) => self.err = Some(e),
            Ok(r) => {
                self.err = None;
                self.list = r.logs;
                if self.key.is_none() {
                    // First look: a machine that is building, then the one that
                    // wrote last - never a stub of a few lines (a stopped box's
                    // "no public IP" note led the public page, 2026-10-18).
                    let running = |l: &LogInfo| matches!(l.state.as_deref(), Some("running" | "building"));
                    self.key = self.list.iter().max_by_key(|l| (l.size >= 1024, running(l), l.mtime)).map(|l| l.key.clone());
                    return true;
                }
                if r.key.is_some() && r.key == self.key {
                    if r.reset {
                        self.lines.clear();
                        self.widest = 0;
                    }
                    self.lines.extend(r.text.lines().map(str::to_owned));
                    if self.lines.len() > KEEP {
                        self.lines.drain(..self.lines.len() - KEEP);
                    }
                    self.next = Some(r.next);
                    self.size = r.size;
                    // Still behind (a big burst): ask again at once.
                    return r.next < r.size;
                }
            }
        }
        false
    }

    fn ask(&mut self, backend: &Backend, ctx: &egui::Context, now: f64) {
        self.asked = now;
        self.waiting = true;
        let slot = self.slot.clone();
        backend.build_log(ctx, self.key.clone(), self.next, move |r| {
            let parsed = r.and_then(|b| serde_json::from_slice::<Reply>(&b).map_err(|e| format!("buildlog.json did not read: {e}")));
            *slot.lock().unwrap() = Some(parsed);
        });
    }

    fn select(&mut self, key: String) {
        if self.key.as_deref() != Some(&key) {
            self.key = Some(key);
            self.lines.clear();
            self.widest = 0;
            self.next = None;
            self.size = 0;
        }
    }
}

impl SoftwarePane {
    pub(super) fn logs_view(&mut self, ui: &mut egui::Ui, backend: &Backend, ctx: &egui::Context) {
        let lg = &mut self.logs;
        let now = ctx.input(|i| i.time);
        let behind = lg.take();
        let running = lg.key.as_deref().and_then(|k| lg.list.iter().find(|l| l.key == k)).is_some_and(|l| l.state.as_deref() == Some("running"));
        let every = if lg.paused { f64::INFINITY } else if running { 2.0 } else { 10.0 };
        if !lg.waiting && (behind || now - lg.asked >= every) {
            lg.ask(backend, ctx, now);
        }
        if every.is_finite() {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(every.min(2.0)));
        }

        let mut pick: Option<String> = None;
        theme::strip(ui, 24.0, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.spacing_mut().button_padding = egui::vec2(8.0, 2.0);
            if lg.list.is_empty() && lg.err.is_none() {
                ui.spinner();
            }
            for l in &lg.list {
                let on = lg.key.as_deref() == Some(&l.key);
                let dot = match l.state.as_deref() {
                    Some("running") => "● ",
                    Some("failed") => "✕ ",
                    _ => "○ ",
                };
                let tip = format!(
                    "{} · {}{} · {} written, {}",
                    l.host.as_deref().unwrap_or(&l.key),
                    l.state.as_deref().unwrap_or("no run state"),
                    l.step.as_deref().map(|s| format!(" › {s}")).unwrap_or_default(),
                    human_bytes(l.size),
                    fmt_ago(now_secs(), l.mtime),
                );
                if segment(ui, &format!("{dot}{}", l.key), &tip, on).clicked() {
                    pick = Some(l.key.clone());
                }
            }
            theme::tool_sep(ui);
            let follow = if lg.paused { "▶ follow" } else { "⏸ pause" };
            if theme::tool(ui, follow, if lg.paused { "follow the log again" } else { "stop following, to read" }).clicked() {
                lg.paused = !lg.paused;
            }
            if theme::tool_if(ui, lg.wrap, "wrap", "wrap long lines (compiler lines run to thousands of characters)").clicked() {
                lg.wrap = !lg.wrap;
            }
            // The whole log as text: drag-selecting stops at what is on screen
            // (operator, 2026-10-17: "in logs I can't seem to copy text there
            // at all").
            if theme::tool(ui, "copy", "copy the lines shown (all of them, or the ones the filter keeps) to the clipboard").clicked() {
                let needle = lg.filter.trim().to_ascii_lowercase();
                let text: Vec<&str> = lg.lines.iter().map(String::as_str).filter(|l| needle.is_empty() || l.to_ascii_lowercase().contains(&needle)).collect();
                ui.ctx().copy_text(text.join("\n"));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(6.0);
                let w = ui.available_width().clamp(80.0, 260.0);
                ui.add(egui::TextEdit::singleline(&mut lg.filter).hint_text("only lines with…").desired_width(w));
            });
        });
        if let Some(k) = pick {
            lg.select(k);
            lg.ask(backend, ctx, now);
        }

        // What the picked machine is doing, in a line.
        if let Some(l) = lg.key.as_deref().and_then(|k| lg.list.iter().find(|l| l.key == k)) {
            theme::strip(ui, 20.0, theme::surface(), |ui| {
                ui.add_space(8.0);
                let (word, c) = match l.state.as_deref() {
                    Some("running") => ("building", theme::amber()),
                    Some("failed") => ("failed", theme::alt()),
                    Some(s) => (s, theme::ok()),
                    None => ("no run state", theme::hush()),
                };
                ui.label(RichText::new(word).size(11.0).color(c));
                if let Some(s) = &l.step {
                    ui.label(RichText::new(format!("step {s}")).size(11.0).color(theme::muted()));
                }
                if let Some(t) = l.started.as_deref().and_then(parse_rfc3339) {
                    ui.label(RichText::new(format!("started {} ago", fmt_dur(now_secs() - t))).size(11.0).color(theme::hush()));
                }
                // Left-aligned like the log under it, after the run's own words
                // (operator, 2026-10-18: "that section should be left justified
                // for example like the logs themselves").
                {
                    ui.label(RichText::new("·").size(11.0).color(theme::hush()));
                    let fresh = now_secs() - l.mtime;
                    // A running box whose log has gone quiet is compiling, not
                    // stuck: say so, or the view reads as frozen (2026-10-15).
                    let quiet = fresh > 120 && l.state.as_deref() == Some("running");
                    let c = if fresh < 120 { theme::ok() } else if quiet { theme::amber() } else if fresh < 1800 { theme::muted() } else { theme::hush() };
                    let text = if quiet {
                        format!("{} · quiet {} - still compiling; R and pip print a package's log when it finishes", human_bytes(lg.size.max(l.size)), fmt_dur(fresh))
                    } else {
                        format!("{} · last line {}", human_bytes(lg.size.max(l.size)), fmt_ago(now_secs(), l.mtime))
                    };
                    ui.add(egui::Label::new(RichText::new(text).size(10.5).color(c)).truncate())
                        .on_hover_text("A long compile (duckdb, rstanarm) can stay silent for many minutes; the box is still working. Asked again every 2 s.");
                }
            });
        }
        if let Some(e) = &lg.err {
            egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                ui.label(RichText::new(e).color(theme::alt()));
            });
            if lg.list.is_empty() {
                return;
            }
        }

        let needle = lg.filter.trim().to_ascii_lowercase();
        let shown: Vec<usize> = if needle.is_empty() { (0..lg.lines.len()).collect() } else { (0..lg.lines.len()).filter(|&i| lg.lines[i].to_ascii_lowercase().contains(&needle)).collect() };
        let font = FontId::monospace(11.5);
        let row_h = ui.fonts_mut(|f| f.row_height(&font)) + 1.0;
        let wrap = lg.wrap;
        ui.spacing_mut().item_spacing.y = 0.0;
        let area = egui::ScrollArea::both().id_salt("bl-log").auto_shrink([false, false]).stick_to_bottom(!lg.paused);
        let out = if wrap {
            egui::ScrollArea::vertical().id_salt("bl-log-wrap").auto_shrink([false, false]).stick_to_bottom(!lg.paused).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    // Wrapped lines have no fixed height: the last 4,000 only.
                    for &i in shown.iter().skip(shown.len().saturating_sub(4000)) {
                        let l = &lg.lines[i];
                        ui.add(egui::Label::new(RichText::new(l).font(font.clone()).color(tone(l))).wrap());
                    }
                    });
                });
            })
        } else {
            // One width for the whole log, its longest line: measured from the
            // rows in view only, the width crossed the window's as new lines
            // scrolled in, the horizontal bar came and went, and the bottom
            // shook (operator, 2026-10-18: "when scrolled to the bottom it
            // looks blurry from moving back and forth so fast").
            let char_w = ui.fonts_mut(|f| f.glyph_width(&font, '0'));
            // Bytes, not chars: O(1) a line, every frame, on 100k-line logs.
            let widest = shown.iter().map(|&i| lg.lines[i].len()).max().unwrap_or(0);
            let widest = if needle.is_empty() {
                lg.widest = lg.widest.max(widest);
                lg.widest
            } else {
                widest
            };
            let content_w = 16.0 + widest as f32 * char_w;
            // Labels, not painted text: a drag selects across the lines and
            // Ctrl/Cmd+C copies them.
            // Every row exactly row_h tall, allocated before the label goes in
            // it: show_rows sizes the whole log as rows × row_h, and a row that
            // came out a fraction taller (a label's own height) made the drawn
            // log disagree with that every frame - stuck to the bottom, the
            // view jumped between the two and the text shimmered (came back
            // 2026-10-18: "the blurry stuff in builds > logs").
            area.show_rows(ui, row_h, shown.len(), |ui, range| {
                ui.set_min_width(content_w);
                for k in range {
                    let l = &lg.lines[shown[k]];
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(content_w, row_h), egui::Sense::hover());
                    let cell = egui::Rect::from_min_max(rect.min + egui::vec2(8.0, 0.0), rect.max);
                    // Left-aligned in its row: `put` centres a widget in the
                    // rect, so each line sat in the middle of a row as wide as
                    // the longest line (operator, 2026-10-18: "the logs
                    // section is centered text, these are logs, that looks awful").
                    let label = egui::Label::new(RichText::new(if l.is_empty() { " " } else { l.as_str() }).font(font.clone()).color(tone(l))).extend();
                    ui.scope_builder(egui::UiBuilder::new().max_rect(cell).layout(Layout::left_to_right(Align::Center)), |ui| ui.add(label));
                }
            })
        };
        if lg.lines.is_empty() && lg.err.is_none() && lg.key.is_some() {
            ui.painter().text(out.inner_rect.center(), egui::Align2::CENTER_CENTER, if lg.waiting { "reading the log…" } else { "nothing written yet" }, FontId::proportional(12.0), theme::hush());
        }
    }
}

fn human_bytes(n: u64) -> String {
    match n {
        n if n >= 1 << 30 => format!("{:.1} GB", n as f64 / (1u64 << 30) as f64),
        n if n >= 1 << 20 => format!("{:.1} MB", n as f64 / (1u64 << 20) as f64),
        n if n >= 1 << 10 => format!("{} KB", n >> 10),
        n => format!("{n} B"),
    }
}

/// `2026-10-14T22:47:49Z` as unix seconds (the run state's own stamp).
fn parse_rfc3339(s: &str) -> Option<i64> {
    let (d, t) = s.trim_end_matches('Z').split_once('T')?;
    let mut d = d.split('-').map(|x| x.parse::<i64>().ok());
    let (y, m, dd) = (d.next()??, d.next()??, d.next()??);
    let mut t = t.split(':').map(|x| x.split('.').next().and_then(|x| x.parse::<i64>().ok()));
    let (h, mi, se) = (t.next()??, t.next()??, t.next().flatten().unwrap_or(0));
    Some(super::days(y, m, dd) * 86400 + h * 3600 + mi * 60 + se)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stamps_and_tones() {
        assert_eq!(parse_rfc3339("1970-01-02T00:00:10Z"), Some(86410));
        assert_eq!(tone("  [ok] R        dashboardthemes"), theme::ok());
        assert_eq!(tone("Error: only 48 GiB free"), theme::alt());
        assert_eq!(human_bytes(47_677_020), "45.5 MB");
    }
}
