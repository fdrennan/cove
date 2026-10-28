//! Speak to an agent (operator, 2026-10-15: "All agents need to allow for
//! audio submission and translation like we do with exec.ndexr.io"). One mic
//! button, used by every agent box - the Agents tab and the ask bar under a
//! file: tap to record, tap again to stop; the clip goes to `transcribe`
//! (host.rs transcribe_as: exec's Whisper path and the person's dictation
//! vocabulary) and the text lands in the box, after anything already typed,
//! to read over before sending - as exec does.
//!
//! The recording is the browser's (web/index.html `window.coveMic`:
//! getUserMedia + MediaRecorder, webm on Chrome, mp4 on iOS). The desktop app
//! has no microphone path yet, so the button only shows in a browser.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::backend::Backend;
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    Recording,
    /// Stopped; the browser is still packing the clip.
    Packing,
    Transcribing,
}

#[derive(Default)]
pub struct Mic {
    phase: Phase,
    since: f64,
    done: Arc<Mutex<Option<Result<String, String>>>>,
    /// Why the last try failed, until the next one.
    error: Option<String>,
}

#[cfg(target_arch = "wasm32")]
mod js {
    use wasm_bindgen::{JsCast, JsValue};

    fn mic() -> Option<JsValue> {
        let w = web_sys::window()?;
        let m = js_sys::Reflect::get(&w, &"coveMic".into()).ok()?;
        (!m.is_undefined() && !m.is_null()).then_some(m)
    }

    pub fn supported() -> bool {
        let Some(w) = web_sys::window() else { return false };
        mic().is_some()
            && js_sys::Reflect::get(&w, &"navigator".into())
                .and_then(|n| js_sys::Reflect::get(&n, &"mediaDevices".into()))
                .is_ok_and(|d| !d.is_undefined())
    }

    pub fn call(name: &str) -> Option<JsValue> {
        let m = mic()?;
        let f: js_sys::Function = js_sys::Reflect::get(&m, &name.into()).ok()?.dyn_into().ok()?;
        f.call0(&m).ok()
    }

    /// A finished clip (a data URL, or "ERROR:why"), once.
    pub fn take() -> Option<String> {
        call("take")?.as_string()
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod js {
    pub fn supported() -> bool {
        false
    }
    pub fn call(_: &str) -> Option<()> {
        None
    }
    pub fn take() -> Option<String> {
        None
    }
}

impl Mic {
    /// There is a microphone to offer here (a browser that can record).
    pub fn shown() -> bool {
        js::supported()
    }

    /// The mic, at a 30 px square; a finished transcript is added to `text`.
    /// Draws nothing where there is no microphone to use.
    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, text: &mut String) {
        self.ui_sized(ui, ctx, backend, text, 32.0);
    }

    /// The same mic at `d` px: Go mode's big record button (app.rs go_view).
    /// Big, it is filled, so it reads as the thing to press.
    pub fn ui_sized(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, text: &mut String, d: f32) {
        if !js::supported() {
            return;
        }
        let now = ui.input(|i| i.time);
        // The browser's clip, once it is packed.
        if matches!(self.phase, Phase::Recording | Phase::Packing) {
            if let Some(clip) = js::take() {
                match clip.strip_prefix("ERROR:") {
                    Some(why) => {
                        self.error = Some(format!("microphone: {why}"));
                        self.phase = Phase::Idle;
                    }
                    None => {
                        self.phase = Phase::Transcribing;
                        let done = self.done.clone();
                        backend.transcribe(ctx, clip, move |r| *done.lock().unwrap() = Some(r));
                    }
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if let Some(r) = self.done.lock().unwrap().take() {
            self.phase = Phase::Idle;
            match r {
                Ok(t) if !t.trim().is_empty() => {
                    let t = t.trim();
                    if text.trim().is_empty() {
                        *text = t.to_owned();
                    } else {
                        text.push_str("\n\n");
                        text.push_str(t);
                    }
                }
                Ok(_) => self.error = Some("heard nothing - try again, closer to the mic".into()),
                Err(e) => self.error = Some(e),
            }
        }

        let tip = match self.phase {
            Phase::Idle => "speak instead of typing - tap, talk, tap again; the words land in the box to read over",
            Phase::Recording => "listening - tap to stop and transcribe",
            Phase::Packing | Phase::Transcribing => "turning it into text",
        };
        let r = match self.phase {
            Phase::Idle if d > 40.0 => crate::icons::button(ui, crate::icons::Icon::Mic, d, theme::primary(), theme::bg(), true),
            Phase::Idle => crate::icons::quiet(ui, crate::icons::Icon::Mic, d),
            Phase::Recording => {
                // A slow pulse while it listens, and how long it has been.
                let on = (now * 2.0).floor() as i64 % 2 == 0;
                let r = crate::icons::button(ui, crate::icons::Icon::Stop, d, if on { theme::alt() } else { theme::alt().gamma_multiply(0.75) }, theme::bg(), true);
                let s = (now - self.since).max(0.0) as u64;
                ui.label(RichText::new(format!("{}:{:02}", s / 60, s % 60)).monospace().size(11.5).color(theme::alt()));
                r
            }
            Phase::Packing | Phase::Transcribing => {
                ui.add_sized([d, d], egui::Spinner::new().size(d / 2.0));
                ui.label(RichText::new("transcribing…").size(11.0).color(theme::muted()))
            }
        };
        let r = match &self.error {
            Some(e) => r.on_hover_text(format!("{tip}\n\nlast try: {e}")),
            None => r.on_hover_text(tip),
        };
        if r.clicked() {
            match self.phase {
                Phase::Idle => {
                    self.error = None;
                    js::call("start");
                    self.phase = Phase::Recording;
                    self.since = now;
                }
                Phase::Recording => {
                    js::call("stop");
                    self.phase = Phase::Packing;
                }
                _ => {}
            }
        }
        if self.phase == Phase::Recording {
            ctx.request_repaint_after(std::time::Duration::from_millis(250));
        }
    }

    /// The last failure, for a line under the box (cleared by the next try).
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }
}

/// Go mode's Listen (operator, 2026-10-18: "set it up so that it's listening
/// and it's continuously taking information from that stream ... take chunks
/// of that audio and act on it if it's confident enough ... but then also
/// submit the final audio, so have it transcribed and ultimately stored ...
/// only within the Go mode"). The page keeps the mic open
/// (web/index.html `window.coveListen`); every CHUNK_SECS a piece is
/// transcribed and handed back as a `Heard::Chunk`; Stop hands back the whole
/// recording and its transcript as `Heard::Whole`. What is done with them -
/// the agent, supporting_docs/ - is ide.rs go_agent's.
#[derive(Default)]
pub struct Listen {
    since: f64,
    /// Pieces at the transcriber, and the whole once it is there.
    out: Arc<Mutex<Vec<Heard>>>,
    pending: usize,
    /// The whole recording, waiting for its transcript.
    whole: bool,
}

/// What Listen heard.
pub enum Heard {
    Chunk(String),
    /// The whole recording (a data URL) and what it said.
    Whole { audio: String, text: Result<String, String> },
    Error(String),
}

/// Seconds of talk in each piece transcribed as it goes.
pub const CHUNK_SECS: u32 = 60;

#[cfg(target_arch = "wasm32")]
mod listen_js {
    use wasm_bindgen::{JsCast, JsValue};

    fn obj() -> Option<JsValue> {
        let w = web_sys::window()?;
        let m = js_sys::Reflect::get(&w, &"coveListen".into()).ok()?;
        (!m.is_undefined() && !m.is_null()).then_some(m)
    }

    pub fn call(name: &str, arg: Option<f64>) -> Option<String> {
        let m = obj()?;
        let f: js_sys::Function = js_sys::Reflect::get(&m, &name.into()).ok()?.dyn_into().ok()?;
        match arg {
            Some(a) => f.call1(&m, &JsValue::from_f64(a)).ok()?.as_string(),
            None => f.call0(&m).ok()?.as_string(),
        }
    }

    pub fn supported() -> bool {
        obj().is_some() && super::js::supported()
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod listen_js {
    pub fn call(_: &str, _: Option<f64>) -> Option<String> {
        None
    }
    pub fn supported() -> bool {
        false
    }
}

impl Listen {
    /// There is a microphone to keep open here (a browser that records).
    pub fn shown() -> bool {
        listen_js::supported()
    }

    /// "off", "starting", "on" or "packing" (stopped, the whole being packed).
    pub fn state(&self) -> String {
        listen_js::call("status", None).unwrap_or_else(|| "off".into())
    }

    /// Still working: listening, packing, or transcribing what it heard.
    pub fn busy(&self) -> bool {
        self.state() != "off" || self.pending > 0 || self.whole
    }

    pub fn start(&mut self, now: f64) {
        self.since = now;
        listen_js::call("start", Some(CHUNK_SECS as f64));
    }

    pub fn stop(&mut self) {
        listen_js::call("stop", None);
    }

    /// How long it has been listening, as m:ss.
    pub fn clock(&self, now: f64) -> String {
        let s = (now - self.since).max(0.0) as u64;
        format!("{}:{:02}", s / 60, s % 60)
    }

    /// Pieces transcribing now.
    pub fn pending(&self) -> usize {
        self.pending
    }

    /// Hand the page's pieces to the transcriber; what has come back, in the
    /// order it came.
    pub fn poll(&mut self, ctx: &egui::Context, backend: &Backend) -> Vec<Heard> {
        if let Some(e) = listen_js::call("error", None) {
            self.out.lock().unwrap().push(Heard::Error(format!("microphone: {e}")));
        }
        while let Some(clip) = listen_js::call("seg", None) {
            self.pending += 1;
            let out = self.out.clone();
            backend.transcribe(ctx, clip, move |r| {
                out.lock().unwrap().push(match r {
                    Ok(t) => Heard::Chunk(t),
                    Err(e) => Heard::Error(format!("a piece could not be transcribed: {e}")),
                })
            });
        }
        if let Some(audio) = listen_js::call("final", None) {
            self.whole = true;
            let out = self.out.clone();
            let a = audio.clone();
            backend.transcribe(ctx, audio, move |r| out.lock().unwrap().push(Heard::Whole { audio: a, text: r }));
        }
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        let got: Vec<Heard> = std::mem::take(&mut *self.out.lock().unwrap());
        for h in &got {
            match h {
                Heard::Chunk(_) | Heard::Error(_) => self.pending = self.pending.saturating_sub(1),
                Heard::Whole { .. } => self.whole = false,
            }
        }
        got
    }
}

/// The briefing's recording, for Go (web/index.html `window.covePlay`). The
/// desktop app has no player: there the briefing is read.
pub mod play {
    #[cfg(target_arch = "wasm32")]
    fn call(name: &str, arg: Option<&str>) -> Option<wasm_bindgen::JsValue> {
        use wasm_bindgen::JsCast;
        let w = web_sys::window()?;
        let o = js_sys::Reflect::get(&w, &"covePlay".into()).ok().filter(|o| !o.is_undefined() && !o.is_null())?;
        let f: js_sys::Function = js_sys::Reflect::get(&o, &name.into()).ok()?.dyn_into().ok()?;
        match arg {
            Some(a) => f.call1(&o, &a.into()).ok(),
            None => f.call0(&o).ok(),
        }
    }

    pub fn supported() -> bool {
        #[cfg(target_arch = "wasm32")]
        return call("playing", None).is_some();
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    /// Play a base64 mp3, in place of anything playing.
    pub fn start(b64: &str) {
        #[cfg(target_arch = "wasm32")]
        let _ = call("play", Some(b64));
        #[cfg(not(target_arch = "wasm32"))]
        let _ = b64;
    }

    pub fn stop() {
        #[cfg(target_arch = "wasm32")]
        let _ = call("stop", None);
    }

    pub fn playing() -> bool {
        #[cfg(target_arch = "wasm32")]
        return call("playing", None).and_then(|v| v.as_bool()).unwrap_or(false);
        #[cfg(not(target_arch = "wasm32"))]
        false
    }

    /// "1:05 / 4:32", or "" with nothing loaded.
    pub fn clock() -> String {
        #[cfg(target_arch = "wasm32")]
        return call("clock", None).and_then(|v| v.as_string()).unwrap_or_default();
        #[cfg(not(target_arch = "wasm32"))]
        String::new()
    }
}
