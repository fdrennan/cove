//! What the UI asks the server, and what comes back. Shared by both ends, so
//! a new question is one variant here, one arm in server.rs and one button in
//! app.rs.
// The file rules (ext, safe_name, VIDEO, AUDIO) are the server's; the browser
// build compiles this file without it.
#![cfg_attr(target_arch = "wasm32", allow(dead_code))]

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "q", rename_all = "snake_case")]
pub enum Query {
    /// The ndexr stack as stars: every package, its layer and what it needs
    /// (catalog.rs). The app's first screen (atlas.rs).
    Atlas,
    /// Every file in media/ and out/, and the jobs.
    State,
    /// Start one piece of media work on a file (media/x or out/x).
    Run { op: Op, file: String, #[serde(default)] start: String, #[serde(default)] end: String },
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Background noise out, voice range kept, loudness evened: the audio of
    /// a song or a video, ready to publish.
    Clean,
    /// A piece from start to end (seconds or h:mm:ss), cut cleanly.
    Cut,
    /// The sound of a video as a WAV, to work on by itself.
    Audio,
}

impl Op {
    pub fn label(self) -> &'static str {
        match self {
            Op::Clean => "Clean audio",
            Op::Cut => "Cut",
            Op::Audio => "Extract audio",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MediaFile {
    /// "media/song.wav" or "out/song.clean.wav"
    pub path: String,
    pub bytes: u64,
    pub video: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Job {
    pub id: u64,
    pub what: String,
    pub out: String,
    pub running: bool,
    pub ok: Option<bool>,
    /// The tool's last lines, so a failure says why.
    pub tail: Vec<String>,
}

/// One package of the stack, as the atlas draws it.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Star {
    pub name: String,
    /// Index into AtlasView::layers.
    pub layer: u8,
    pub group: String,
    /// What it is for, in a few words.
    pub purpose: String,
    pub archs: Vec<String>,
    /// Builds that finished ok / failed, across the build machines.
    pub ok: u32,
    pub failed: u32,
    /// What it needs: indices into AtlasView::stars.
    pub deps: Vec<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AtlasView {
    /// Bottom of the stack first: the compatibility layer, then EasyBuild,
    /// then each language's packages.
    pub layers: Vec<String>,
    pub stars: Vec<Star>,
    /// Where the catalog came from, and when it was made (unix seconds).
    pub source: String,
    pub generated: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct State {
    pub files: Vec<MediaFile>,
    pub jobs: Vec<Job>,
}

pub const VIDEO: &[&str] = &["mp4", "mov", "mkv", "webm", "m4v", "avi"];
pub const AUDIO: &[&str] = &["wav", "mp3", "m4a", "aac", "flac", "ogg", "opus"];

pub fn ext(path: &str) -> String {
    path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

/// A name a person gave a file, made safe to keep: no folders, no leading dot.
pub fn safe_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next()?.trim();
    let s: String = base.chars().map(|c| if c.is_ascii_alphanumeric() || "._- ".contains(c) { c } else { '_' }).collect();
    let s = s.trim_start_matches('.').trim().to_owned();
    let e = ext(&s);
    (!s.is_empty() && (VIDEO.contains(&e.as_str()) || AUDIO.contains(&e.as_str()))).then_some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_kept_inside_media() {
        assert_eq!(safe_name("../../etc/passwd.wav").as_deref(), Some("passwd.wav"));
        assert_eq!(safe_name("C:\\songs\\My Song!.mp3").as_deref(), Some("My Song_.mp3"));
        assert_eq!(safe_name(".hidden.mp4").as_deref(), Some("hidden.mp4"));
        assert_eq!(safe_name("notes.txt"), None);
    }
}
