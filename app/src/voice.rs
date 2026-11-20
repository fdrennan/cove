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
    /// The domain folder the mic works for. Set by its box each frame: every
    /// recording is kept in <keep>/supporting_docs/ with its transcript beside
    /// it (operator, 2026-11-01: "all uploaded audio should get stored in
    /// supporting docs ... voice overs, game sounds"); None keeps nothing.
    pub keep: Option<String>,
    /// The recording just kept, waiting for its transcript: (folder, name).
    kept: Option<(String, String)>,
    /// The transcript that just landed in the box, for its owner to take: the
    /// composer remembers it as spoken (spoken.rs).
    last: Option<String>,
}

/// A recording's extension, from its data URL's type.
fn clip_ext(data_url: &str) -> &'static str {
    match data_url.split(';').next().unwrap_or("").trim_start_matches("data:") {
        m if m.contains("mp4") || m.contains("m4a") => "m4a",
        m if m.contains("ogg") => "ogg",
        m if m.contains("wav") => "wav",
        _ => "webm",
    }
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
    /// The transcript that landed in the box since the last call.
    pub fn take_last(&mut self) -> Option<String> {
        self.last.take()
    }

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
                        self.kept = None;
                        if let Some(dir) = self.keep.clone().filter(|d| !d.is_empty()) {
                            if let Some(bytes) = clip.split_once(";base64,").and_then(|(_, b)| crate::api::b64_decode(b)) {
                                let stamp = crate::shots::name("voice", "");
                                let name = format!("{}.{}", stamp.rsplit_once('.').map_or(stamp.as_str(), |x| x.0), clip_ext(&clip));
                                let docs = format!("{dir}/supporting_docs");
                                backend.upload(&docs, &name, bytes, ctx, |r| {
                                    if let Err(e) = r {
                                        eprintln!("cove: recording not kept: {e}");
                                    }
                                });
                                self.kept = Some((docs, name));
                            }
                        }
                        let done = self.done.clone();
                        backend.transcribe(ctx, clip, move |r| *done.lock().unwrap() = Some(r));
                    }
                }
            }
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }
        if let Some(r) = self.done.lock().unwrap().take() {
            self.phase = Phase::Idle;
            // The transcript goes beside the kept recording, as <name>.md.
            if let (Some((docs, name)), Ok(t)) = (self.kept.take(), &r) {
                if !t.trim().is_empty() {
                    let md = format!("# {name}\n\nTranscript of {name}, recorded with the mic in Cove.\n\n{}\n", t.trim());
                    backend.upload(&docs, &format!("{name}.md"), md.into_bytes(), ctx, |_| {});
                }
            }
            match r {
                Ok(t) if !t.trim().is_empty() => {
                    let t = t.trim();
                    self.last = Some(t.to_owned());
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

/// What the one Listen button shows (operator, 2026-10-31: "a nice clean
/// single icon button, no text ... hover over to explain it ... to the right
/// of the mic", in the IDE's console and in Go). A click moves Off → Listen →
/// Act as I talk → Off.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListenMode {
    #[default]
    Off,
    /// The mic is open; each minute is transcribed and kept.
    Listen,
    /// The mic is open; each piece goes to the agent as it is heard.
    Act,
}

/// The Listen button, `d` across: quiet when off, filled and slowly pulsing
/// while it listens (green to keep, blue to act). `enabled` is false while a
/// stopped recording is still being transcribed.
pub fn listen_button(ui: &mut egui::Ui, mode: ListenMode, enabled: bool, tip: &str, d: f32) -> egui::Response {
    let now = ui.input(|i| i.time);
    let pulse = |c: egui::Color32| if (now * 1.5).floor() as i64 % 2 == 0 { c } else { c.gamma_multiply(0.75) };
    let r = match mode {
        ListenMode::Off if enabled => crate::icons::quiet(ui, crate::icons::Icon::Listen, d),
        ListenMode::Off => crate::icons::button(ui, crate::icons::Icon::Listen, d, theme::surface_hi(), theme::muted(), false),
        ListenMode::Listen => crate::icons::button(ui, crate::icons::Icon::Listen, d, pulse(theme::ok()), theme::bg(), true),
        ListenMode::Act => crate::icons::button(ui, crate::icons::Icon::Listen, d, pulse(theme::primary()), theme::bg(), true),
    };
    if mode != ListenMode::Off {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
    }
    r.on_hover_text(tip)
}

/// Speak: the agent answers aloud, beside Listen in every composer (operator,
/// 2026-11-05: a "voice toggle like speak out loud ... and maybe a stop
/// speaking button that shows up while it's talking"). One switch for every
/// agent pane, kept across restarts (app.rs Saved). What is said, and whether
/// anything is, is brief.rs `spoken`'s.
pub mod speak {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ON: AtomicBool = AtomicBool::new(false);
    pub fn on() -> bool {
        ON.load(Ordering::Relaxed)
    }
    pub fn set(v: bool) {
        ON.store(v, Ordering::Relaxed);
    }
    /// Only where there is a player to say it (the browser).
    pub fn shown() -> bool {
        super::play::supported()
    }
}

/// The Speak button: off (quiet), on (lit), or Stop while it is talking.
pub fn speak_button(ui: &mut egui::Ui, d: f32) {
    use crate::icons::{button, quiet, Icon};
    if !speak::shown() {
        return;
    }
    if play::playing() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(300));
        if button(ui, Icon::Stop, d, theme::primary(), theme::bg(), true).on_hover_text("stop talking - what it said is in the chat").clicked() {
            play::stop();
        }
        return;
    }
    let on = speak::on();
    let r = if on { button(ui, Icon::Speak, d, theme::primary(), theme::bg(), true) } else { quiet(ui, Icon::Speak, d) };
    let tip = if on {
        "Speak is on - the agent says its answer aloud when you asked it something or it needs you; the rest stays in the chat.\nClick: quiet"
    } else {
        "Speak - the agent answers aloud when you asked it something or it needs you; the full reply stays in the chat"
    };
    if r.on_hover_text(tip).clicked() {
        speak::set(!on);
    }
}

/// Go mode's Listen (operator, 2026-10-18: "set it up so that it's listening
/// and it's continuously taking information from that stream ... take chunks
/// of that audio and act on it if it's confident enough ... but then also
/// submit the final audio, so have it transcribed and ultimately stored ...
/// only within the Go mode"). The page keeps the mic open
/// (web/index.html `window.coveListen`); every 10 s to 5 min (PIECES) a piece is
/// transcribed and handed back as a `Heard::Chunk`; Stop hands back the whole
/// recording and its transcript as `Heard::Whole`. What is done with them -
/// the agent, supporting_docs/ - is ide.rs go_agent's.
#[derive(Default)]
pub struct Listen {
    since: f64,
    /// Seconds per piece, as started.
    secs: u32,
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

/// How often Listen cuts a piece, transcribes it and - acting as I talk -
/// hands it to the agent, in seconds: three choices and nothing to type
/// (operator, 2026-11-13: "whenever I push one minute ... it wants me to put
/// in a numeric input ... I think it should be simpler ... 30 second, one
/// minute, or five minutes, as the only options"). It replaced a minutes
/// field, 1 to 10 (2026-11-11), which replaced the Steady / In batches /
/// Presenting paces (2026-11-08).
///
/// 10 s and 15 s added 2026-11-19 (operator: "i cant push 30s to shrink the
/// response time"): 30 s was the floor, and on a phone the three buttons ran
/// off the agent bar's edge with only "30 s" showing, so it is one dropdown.
pub const PIECES: [u32; 5] = [10, 15, 30, 60, 300];

/// The choice nearest `secs` - what a saved or older value becomes.
pub fn piece(secs: u32) -> u32 {
    *PIECES.iter().min_by_key(|p| p.abs_diff(secs)).unwrap_or(&60)
}

/// "30 s", "1 min", "5 min".
pub fn piece_words(secs: u32) -> String {
    if secs < 60 { format!("{secs} s") } else { format!("{} min", secs / 60) }
}

/// The same span in a sentence: "30 seconds", "minute", "5 minutes" (as in
/// "the last minute").
pub fn piece_span(secs: u32) -> String {
    match secs {
        s if s < 60 => format!("{s} seconds"),
        60 => "minute".to_owned(),
        s => format!("{} minutes", s / 60),
    }
}

/// The choices as one dropdown beside Model and Auto, the one in use shown.
/// Left of it, while Listen is on, the time until the piece being heard is cut.
pub fn piece_field(ui: &mut egui::Ui, secs: &mut u32, left: Option<u32>) {
    if let Some(s) = left {
        ui.label(RichText::new(format!("{}:{:02}", s / 60, s % 60)).monospace().size(11.0).color(theme::hush()))
            .on_hover_text("until Listen transcribes what it is hearing and, acting as you talk, the agent gets it");
    }
    egui::ComboBox::from_id_salt(("listen-piece", ui.id()))
        .width(58.0)
        .selected_text(RichText::new(piece_words(*secs)).size(11.5))
        .show_ui(ui, |ui| {
            for p in PIECES {
                ui.selectable_value(secs, p, piece_words(p));
            }
        })
        .response
        .on_hover_text(format!("Listen transcribes what you said every {} and, acting as you talk, hands it to the agent - shorter answers sooner", piece_words(*secs)));
}

/// The model a Presenting turn runs on: Sonnet, on the same plan
/// (subscription or API) as the agent's own pick.
pub fn present_model(current: &str) -> String {
    if current.starts_with("sub-") { "sub-sonnet" } else { "sonnet" }.to_owned()
}

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

    /// Open the mic, cutting a piece every `secs` seconds.
    pub fn start(&mut self, now: f64, secs: u32) {
        self.since = now;
        self.secs = secs;
        listen_js::call("start", Some(secs as f64));
    }

    /// Cut a piece every `secs` from now on, keeping the one being heard: a
    /// change to the minutes field reaches a running Listen.
    pub fn every(&mut self, now: f64, secs: u32) {
        if self.secs != secs {
            self.since = now;
            self.secs = secs;
            listen_js::call("every", Some(secs as f64));
        }
    }

    /// Seconds until the piece being recorded is cut and transcribed.
    pub fn next_piece_in(&self, now: f64) -> u32 {
        let secs = self.secs.max(1);
        secs - ((now - self.since).max(0.0) as u32 % secs)
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

#[cfg(test)]
mod minutes_tests {
    const PIECES_WORDS: [&str; 5] = ["10 s", "15 s", "30 s", "1 min", "5 min"];

    #[test]
    fn minutes() {
        assert_eq!(super::PIECES, [10, 15, 30, 60, 300]);
        // Saved values from before (minutes 1 to 10, or seconds) land on the
        // nearest choice; 30 s and up keep what they were.
        assert_eq!([10, 12, 15, 30, 45, 60, 120, 179, 181, 600].map(super::piece), [10, 10, 15, 30, 30, 60, 60, 60, 300, 300]);
        assert_eq!(PIECES_WORDS, super::PIECES.map(super::piece_words));
        assert_eq!([super::piece_span(30), super::piece_span(60), super::piece_span(300)], ["30 seconds", "minute", "5 minutes"]);
        assert_eq!(super::present_model("sub-opus"), "sub-sonnet");
        assert_eq!(super::present_model("opus"), "sonnet");
    }
}
