//! The media work: FFmpeg from the ndexr stack, driven from Rust. Inputs live
//! in media/, results go to out/ - both beside the binary's working folder
//! (the domain). Each piece of work is a job on its own thread; the UI polls.
//!
//! To add an operation: a variant in api.rs `Op`, its FFmpeg arguments in
//! `args` below, and it shows up as a button.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};

use crate::api::{ext, Job, MediaFile, Op, State, AUDIO, VIDEO};

pub const DIRS: [&str; 2] = ["media", "out"];

fn jobs() -> &'static Arc<Mutex<Vec<Job>>> {
    static J: OnceLock<Arc<Mutex<Vec<Job>>>> = OnceLock::new();
    J.get_or_init(Default::default)
}

pub fn state() -> State {
    let mut files = Vec::new();
    for d in DIRS {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        let mut v: Vec<MediaFile> = rd
            .flatten()
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                let x = ext(&name);
                let video = VIDEO.contains(&x.as_str());
                (video || AUDIO.contains(&x.as_str())).then(|| MediaFile { path: format!("{d}/{name}"), bytes: e.metadata().map(|m| m.len()).unwrap_or(0), video })
            })
            .collect();
        v.sort_by(|a, b| a.path.cmp(&b.path));
        files.extend(v);
    }
    let mut jobs = jobs().lock().unwrap().clone();
    jobs.reverse();
    State { files, jobs }
}

/// media/x or out/x, and nothing else: no folders below, no way up.
pub fn checked(file: &str) -> Result<PathBuf, String> {
    let (dir, name) = file.split_once('/').ok_or("expected media/<file> or out/<file>")?;
    if !DIRS.contains(&dir) || name.is_empty() || name.contains('/') || name.starts_with('.') {
        return Err(format!("{file}: not a file in media/ or out/"));
    }
    let p = Path::new(dir).join(name);
    p.is_file().then_some(p).ok_or_else(|| format!("{file}: no such file"))
}

/// A time FFmpeg reads: seconds (12.5) or h:mm:ss(.ms).
fn time(t: &str) -> Result<String, String> {
    let t = t.trim();
    if t.is_empty() || !t.chars().all(|c| c.is_ascii_digit() || c == ':' || c == '.') {
        return Err(format!("{t:?}: a time is seconds (12.5) or h:mm:ss"));
    }
    Ok(t.to_owned())
}

/// The voice and music range kept, the hiss and hum under it taken out, the
/// loudness evened to what streaming sites expect (-16 LUFS).
const CLEAN: &str = "highpass=f=70,lowpass=f=14000,afftdn=nf=-25:tn=1,loudnorm=I=-16:TP=-1.5:LRA=11";

/// FFmpeg's arguments for `op` on `input`, and where the result goes.
pub fn args(op: Op, input: &Path, start: &str, end: &str) -> Result<(Vec<String>, PathBuf), String> {
    let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("out").to_owned();
    let x = ext(&input.to_string_lossy());
    let video = VIDEO.contains(&x.as_str());
    let i = input.to_string_lossy().into_owned();
    let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    let (mut a, out) = match op {
        Op::Clean if video => (s(&["-i", &i, "-map", "0:v?", "-map", "0:a", "-c:v", "copy", "-af", CLEAN, "-c:a", "aac", "-b:a", "192k"]), format!("{stem}.clean.mp4")),
        Op::Clean => (s(&["-i", &i, "-af", CLEAN, "-ar", "48000"]), format!("{stem}.clean.wav")),
        Op::Cut => {
            let (from, to) = (time(start)?, time(end)?);
            let name = format!("{stem}.cut-{}-{}", from.replace(':', "."), to.replace(':', "."));
            // Re-encoded, so the cut lands on the exact frame, not the
            // keyframe before it.
            if video {
                (s(&["-ss", &from, "-to", &to, "-i", &i, "-c:v", "libx264", "-preset", "veryfast", "-crf", "20", "-c:a", "aac", "-b:a", "192k"]), format!("{name}.mp4"))
            } else {
                (s(&["-ss", &from, "-to", &to, "-i", &i]), format!("{name}.wav"))
            }
        }
        Op::Audio => (s(&["-i", &i, "-vn", "-ac", "2", "-ar", "48000"]), format!("{stem}.wav")),
    };
    let out = Path::new("out").join(out);
    a.push(out.to_string_lossy().into_owned());
    Ok((a, out))
}

/// FFmpeg from PATH, else from the stack: loaded and run in one shell, since
/// a module loaded in one shell is gone in the next.
fn ffmpeg(args: &[String]) -> Command {
    let mut c = Command::new("bash");
    c.arg("-c")
        .arg("command -v ffmpeg >/dev/null || { . /opt/ndexr/stack/use.sh && ml FFmpeg; } >/dev/null 2>&1; \
              command -v ffmpeg >/dev/null || { echo 'ffmpeg: not on PATH, and ml FFmpeg did not load it'; exit 127; }; \
              exec ffmpeg -hide_banner -nostdin -y \"$@\"")
        .arg("ffmpeg")
        .args(args);
    c
}

pub fn run(op: Op, file: &str, start: &str, end: &str) -> Result<Job, String> {
    let input = checked(file)?;
    let (a, out) = args(op, &input, start, end)?;
    std::fs::create_dir_all("out").map_err(|e| format!("out/: {e}"))?;
    let all = jobs().clone();
    let job = {
        let mut v = all.lock().unwrap();
        let j = Job { id: v.len() as u64 + 1, what: format!("{} · {file}", op.label()), out: out.to_string_lossy().into_owned(), running: true, ..Default::default() };
        v.push(j.clone());
        j
    };
    let id = job.id;
    let mut child = ffmpeg(&a).stdout(Stdio::null()).stderr(Stdio::piped()).spawn().map_err(|e| format!("ffmpeg: {e}"))?;
    let err = child.stderr.take();
    std::thread::spawn(move || {
        let set = |f: &dyn Fn(&mut Job)| {
            if let Some(j) = all.lock().unwrap().iter_mut().find(|j| j.id == id) {
                f(j);
            }
        };
        if let Some(err) = err {
            // FFmpeg rewrites its progress line with \r; split on both.
            for line in BufReader::new(err).split(b'\r').flatten() {
                for l in String::from_utf8_lossy(&line).lines().map(str::trim).filter(|l| !l.is_empty()) {
                    let l = l.to_owned();
                    set(&|j| {
                        j.tail.push(l.clone());
                        let n = j.tail.len();
                        if n > 12 {
                            j.tail.drain(..n - 12);
                        }
                    });
                }
            }
        }
        let ok = child.wait().map(|s| s.success()).unwrap_or(false);
        set(&|j| {
            j.running = false;
            j.ok = Some(ok);
        });
    });
    Ok(job)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_outside_media_and_out() {
        assert!(checked("../Cargo.toml").is_err());
        assert!(checked("media/../../x.wav").is_err());
        assert!(checked("src/main.rs").is_err());
        assert!(checked("media/.hidden.wav").is_err());
    }

    #[test]
    fn a_cut_names_its_times_and_rejects_words() {
        let (a, out) = args(Op::Cut, Path::new("media/song.wav"), "1:05", "1:30.5").unwrap();
        assert_eq!(out, Path::new("out/song.cut-1.05-1.30.5.wav"));
        assert_eq!(a[..4], ["-ss", "1:05", "-to", "1:30.5"]);
        assert!(args(Op::Cut, Path::new("media/song.wav"), "1; rm -rf /", "2").is_err());
    }

    #[test]
    fn cleaning_a_video_keeps_its_picture() {
        let (a, out) = args(Op::Clean, Path::new("media/clip.mov"), "", "").unwrap();
        assert_eq!(out, Path::new("out/clip.clean.mp4"));
        assert!(a.windows(2).any(|w| w == ["-c:v", "copy"]));
    }
}
