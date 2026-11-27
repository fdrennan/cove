//! Connect OpenAI (operator, 2026-11-27: "open AI is used and users need to
//! be able to do that as well"): the GPT models in the agent's picker on the
//! person's own OpenAI account, wherever there is no ndexr platform to run
//! them - OpenAI's Codex CLI installed for them, and their API key or their
//! ChatGPT plan. On ndexr's platform the GPT models already run, on ndexr's
//! key, and the window says so. The machine side is host/localagent/codex.rs.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::api::{OpenAiView, Query};
use crate::backend::Backend;
use crate::ui::kit::theme;

type Slot<T> = Arc<Mutex<Option<T>>>;

#[derive(Default)]
pub struct ConnectOpenAi {
    pub open: bool,
    view: Slot<Result<OpenAiView, String>>,
    asked: bool,
    busy: Arc<Mutex<Option<&'static str>>>,
    note: Slot<Result<String, String>>,
    key: String,
}

impl ConnectOpenAi {
    fn ask(&mut self, q: Query, doing: &'static str, backend: &Backend, ctx: &egui::Context) {
        *self.busy.lock().unwrap() = Some(doing);
        *self.note.lock().unwrap() = None;
        let (view, busy, note, b2, c2) = (self.view.clone(), self.busy.clone(), self.note.clone(), backend.clone(), ctx.clone());
        let after = move |r: Result<String, String>| {
            *note.lock().unwrap() = Some(r);
            // Where it stands now, whatever happened.
            let (view, busy) = (view.clone(), busy.clone());
            b2.query(Query::OpenAiStatus, &c2, move |r: Result<OpenAiView, String>| {
                *view.lock().unwrap() = Some(r);
                *busy.lock().unwrap() = None;
            });
        };
        match q {
            Query::CodexInstall => backend.query(q, ctx, move |r: Result<String, String>| after(r)),
            q => backend.query(q, ctx, move |r: Result<OpenAiView, String>| after(r.map(|v| if v.ready { format!("Connected: {}.", v.how) } else { "Removed.".to_owned() }))),
        }
    }

    /// `platform`: this Cove runs on ndexr's platform, where GPT runs on
    /// ndexr's key. `terminal`: a line to run in Cove's Terminal.
    pub fn show(&mut self, ctx: &egui::Context, backend: &Backend, platform: bool, terminal: &mut Option<String>) {
        if !self.open {
            self.asked = false;
            return;
        }
        let mut open = true;
        egui::Window::new("Connect OpenAI")
            .id(egui::Id::new("connect-openai"))
            .order(egui::Order::Foreground)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(500.0)
            .show(ctx, |ui| self.body(ui, ctx, backend, platform, terminal));
        if !open {
            self.open = false;
            self.asked = false;
        }
    }

    /// Its OpenAI ready to use (the setup step's Continue says so).
    pub fn ready(&self, platform: bool) -> bool {
        platform || self.view.lock().unwrap().as_ref().is_some_and(|v| v.as_ref().is_ok_and(|v| v.ready))
    }

    /// The window's content: also the OpenAI step of first-start setup
    /// (setup.rs), so the two never say different things.
    pub fn body(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, platform: bool, terminal: &mut Option<String>) {
        if !self.asked && !platform {
            self.asked = true;
            let slot = self.view.clone();
            backend.query(Query::OpenAiStatus, ctx, move |r: Result<OpenAiView, String>| {
                *slot.lock().unwrap() = Some(r);
            });
        }
        let busy = *self.busy.lock().unwrap();
        let mut act: Option<(Query, &'static str)> = None;
        (|ui: &mut egui::Ui| {
                ui.label(RichText::new("The GPT models in the agent's model picker run on OpenAI. Here they run on your own OpenAI account, through OpenAI's Codex - with the same projects, files and thread as Claude.").size(13.0).color(theme::text()));
                ui.add_space(8.0);
                if platform {
                    status(ui, true, "Ready on ndexr", "On ndexr's servers the GPT models run on ndexr's OpenAI account: pick one in the agent's model picker.");
                    return;
                }
                let v = match self.view.lock().unwrap().clone() {
                    None => {
                        ui.spinner();
                        return;
                    }
                    Some(Err(e)) => {
                        status(ui, false, "Could not check OpenAI here", &e);
                        return;
                    }
                    Some(Ok(v)) => v,
                };
                if v.ready {
                    status(ui, true, &format!("Connected · {}", v.how), "Pick a GPT model in the agent's model picker. OpenAI bills your account; ndexr does not meter it.");
                } else {
                    status(ui, false, if v.installed { "Codex is installed, not signed in" } else { "Codex is not installed yet" }, &v.detail);
                }
                ui.add_space(10.0);
                // 1. Codex itself.
                ui.label(RichText::new("1 · OpenAI's Codex").font(theme::display(13.5)).color(theme::text_strong()));
                if v.installed {
                    ui.label(RichText::new("Installed for you.").size(12.5).color(theme::text()));
                } else if ui.add_enabled(busy.is_none(), theme::primary_button(if busy == Some("install") { "Installing…" } else { "Install Codex" })).on_hover_text("OpenAI's installer: curl -fsSL https://chatgpt.com/codex/install.sh | sh, for your account only").clicked() {
                    act = Some((Query::CodexInstall, "install"));
                }
                ui.add_space(10.0);
                // 2. The account it runs on.
                ui.label(RichText::new("2 · Your OpenAI account").font(theme::display(13.5)).color(theme::text_strong()));
                ui.label(RichText::new("An API key from platform.openai.com, checked with OpenAI before it is kept. It stays on this computer, readable only by you.").size(12.5).color(theme::text()));
                ui.horizontal(|ui| {
                    let w = (ui.available_width() - 110.0).max(140.0);
                    ui.add(egui::TextEdit::singleline(&mut self.key).password(true).hint_text("sk-…").desired_width(w));
                    let ok = self.key.trim().starts_with("sk-") && busy.is_none();
                    if ui.add_enabled(ok, theme::primary_button(if busy == Some("connect") { "Checking…" } else { "Connect" })).clicked() {
                        act = Some((Query::OpenAiConnect { key: std::mem::take(&mut self.key) }, "connect"));
                    }
                });
                if ui.link("Get an API key at platform.openai.com ↗").clicked() {
                    crate::ui::kit::links::new_tab(ctx, "https://platform.openai.com/api-keys");
                }
                ui.add_space(6.0);
                ui.label(RichText::new("Or your ChatGPT plan: Codex signs in with a code you approve in any browser.").size(12.5).color(theme::text()));
                if ui.add_enabled(v.installed, egui::Button::new("Sign in with ChatGPT in the Terminal")).on_disabled_hover_text("install Codex first").clicked() {
                    *terminal = Some("codex login --device-auth".into());
                }
                if v.ready && v.how.starts_with("API key") {
                    ui.add_space(6.0);
                    if ui.add_enabled(busy.is_none(), egui::Button::new(RichText::new("Remove the key").color(theme::alt()))).clicked() {
                        act = Some((Query::OpenAiForget, "forget"));
                    }
                }
                if let Some(n) = self.note.lock().unwrap().clone() {
                    let (t, c) = match n {
                        Ok(t) => (t, theme::ok()),
                        Err(e) => (e, theme::warn()),
                    };
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(RichText::new(t).size(12.5).color(c)).wrap());
                }
        })(ui);
        if let Some((q, doing)) = act {
            self.ask(q, doing, backend, ctx);
        }
        // Signing in happens in the Terminal: ask again when they come back.
        if terminal.is_some() {
            self.asked = false;
        }
    }
}

fn status(ui: &mut egui::Ui, good: bool, head: &str, body: &str) {
    let c = if good { theme::ok() } else { theme::warn() };
    egui::Frame::new().fill(c.gamma_multiply(0.10)).stroke(egui::Stroke::new(1.0, c.gamma_multiply(0.5))).corner_radius(8.0).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(head).size(13.5).color(theme::text_strong()));
        if !body.is_empty() {
            ui.add(egui::Label::new(RichText::new(body).size(12.5).color(theme::text())).wrap());
        }
    });
}
