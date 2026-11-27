//! The Terminal tab: a real terminal (operator, 2026-10-15: "apps need to be
//! able to open from terminal like ranger or vim or claude"). The Shell console
//! before it was line mode - each request ran to the end with no TTY, so
//! `claude` died with "Input must be provided either through stdin or as a
//! prompt argument" and vim, top and ranger could not draw at all.
//!
//! Server side (`pty`): a bash on a pseudo-terminal per session, run as the
//! session's person, in the stack with the session's module loaded and - for
//! `claude` - the person's own Claude in its environment (host.rs
//! `claude_env`). What it prints is kept (the last MiB) and fed to a vt100
//! screen of its own, so a tab that reconnects is repainted from that screen,
//! and the queries full-screen programs ask of a terminal (cursor position,
//! device attributes, background colour) are answered there.
//!
//! UI side (`TermPane`): its own vt100 screen fed by `Query::Term` - a long
//! poll that waits up to a quarter second for output - drawn cell by cell, and
//! keys sent as the bytes an xterm sends (`Query::TermWrite`), one write in
//! flight at a time so they arrive in order.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, text::LayoutJob, Color32, FontId, Key, Modifiers, Rect, RichText, Sense, Stroke, TextFormat};

use crate::api::{Query, TermChunk};
use crate::backend::Backend;
use crate::ui::kit::theme;

/// Lines kept above the screen in the tab (the server keeps none).
const SCROLLBACK: usize = 5000;
const FONT: f32 = 12.5;

// ── the server: a bash on a pty per session ─────────────────────────────────

#[cfg(all(unix, not(target_arch = "wasm32")))]
pub mod pty {
    use std::collections::HashMap;
    use std::fs::File;
    use std::io::{Read, Write};
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Arc, Condvar, Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use crate::api::TermChunk;

    /// What is kept of a terminal's output for a tab to catch up from.
    const KEEP: usize = 1 << 20;
    /// The most one answer carries; the rest comes on the next poll.
    const CHUNK: usize = 256 << 10;

    /// The bash a terminal starts: the person's own rc files, then the stack
    /// and the session's module, then the project folder.
    const RC: &str = r#"# Cove - the Terminal tab's bash (app/src/term.rs writes this file).
[ -r /etc/bash.bashrc ] && . /etc/bash.bashrc
[ -r "$HOME/.bashrc" ] && . "$HOME/.bashrc"
# Prepended in turn, so the person's own ~/.local/bin wins, then the host's
# /opt/ndexr/bin. claude itself is /usr/local/bin/claude, a link into CernVM-FS;
# Cove's shared copy goes last, for when the stack is not mounted.
for d in /opt/ndexr/bin "$HOME/.local/bin"; do
  case ":$PATH:" in *":$d:"*) ;; *) PATH="$d:$PATH" ;; esac
done
case ":$PATH:" in *:/usr/local/lib/cove/bin:*) ;; *) PATH="$PATH:/usr/local/lib/cove/bin" ;; esac
# A shared claude cannot update itself (read-only); publishing refreshes it.
case "$(command -v claude 2>/dev/null)" in "$HOME"/*) ;; *) export DISABLE_AUTOUPDATER=1 ;; esac
if [ -n "${COVE_USE_SH:-}" ] && [ -r "$COVE_USE_SH" ]; then
  set +u
  . "$COVE_USE_SH" >/dev/null 2>&1 || echo "could not enter the ndexr stack through $COVE_USE_SH"
  if [ -n "${COVE_ENV_SCRIPT:-}" ] && [ -r "$COVE_ENV_SCRIPT" ]; then
    # The load, resolved once (envs.rs): `ml` still layers on top of it.
    . "$COVE_ENV_SCRIPT" >/dev/null 2>&1 || echo "could not load $COVE_MODULES"
  elif [ -n "${COVE_MODULES:-}" ]; then
    # shellcheck disable=SC2086
    ml $COVE_MODULES >/dev/null 2>&1 || echo "could not load $COVE_MODULES"
  fi
fi
cd "${COVE_CWD:-$HOME}" 2>/dev/null || cd
[ -n "${COVE_CLAUDE:-}" ] && printf '\033[2mclaude here runs on %s\033[0m\n' "$COVE_CLAUDE"
# Claude Code is installed per person (only the operator's account had it,
# 2026-10-15): until it is, `claude` offers the official installer, into
# ~/.local/bin, and asks first.
if ! command -v claude >/dev/null 2>&1; then
  claude() {
    # Claude Code may have arrived since this shell started (Cove installs it
    # in the background): use it rather than asking to install it again.
    if [ -x "$HOME/.local/bin/claude" ]; then
      unset -f claude && hash -r && PATH="$HOME/.local/bin:$PATH" command claude "$@"
      return
    fi
    printf 'Claude Code is not installed for %s yet. Install it into ~/.local/bin now (claude.ai/install.sh)? [y/N] ' "$USER"
    local a
    read -r a
    case $a in
      y | Y | yes)
        curl -fsSL https://claude.ai/install.sh | bash && unset -f claude && hash -r && command claude "$@" ;;
      *) return 1 ;;
    esac
  }
fi
"#;

    /// The answers a terminal owes the programs in it, which vt100 leaves to
    /// its embedder: without them vim and claude wait on a reply that never
    /// comes, or guess the background is white.
    #[derive(Default)]
    pub(super) struct Replies(pub Vec<u8>);

    impl vt100::Callbacks for Replies {
        fn unhandled_csi(&mut self, screen: &mut vt100::Screen, i1: Option<u8>, _i2: Option<u8>, params: &[&[u16]], c: char) {
            let p0 = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
            match (i1, c) {
                // Where is the cursor (DSR 6), are you there (DSR 5).
                (None, 'n') if p0 == 6 => {
                    let (r, c) = screen.cursor_position();
                    self.0.extend(format!("\x1b[{};{}R", r + 1, c + 1).bytes());
                }
                (None, 'n') if p0 == 5 => self.0.extend(b"\x1b[0n"),
                // What are you (DA1, DA2): a VT220 with colour, as xterm says.
                (None, 'c') if p0 == 0 => self.0.extend(b"\x1b[?62;22c"),
                (Some(b'>'), 'c') => self.0.extend(b"\x1b[>1;10;0c"),
                _ => {}
            }
        }

        fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
            // OSC 10 / 11 "?": the text and background colours, which claude
            // and vim read to pick their dark or light look.
            if params.len() >= 2 && params[1] == b"?" {
                let rgb = match params[0] {
                    b"10" => super::DEFAULT_FG,
                    b"11" => super::DEFAULT_BG,
                    _ => return,
                };
                let h = |v: u8| format!("{v:02x}{v:02x}");
                self.0.extend(format!("\x1b]{};rgb:{}/{}/{}\x1b\\", String::from_utf8_lossy(params[0]), h(rgb.r()), h(rgb.g()), h(rgb.b())).bytes());
            }
        }
    }

    struct Term {
        master: File,
        child: Child,
        /// Output from byte `base` on (`KEEP` at most).
        buf: Vec<u8>,
        base: u64,
        screen: vt100::Parser<Replies>,
        size: (u16, u16),
        alive: bool,
        note: String,
    }

    impl Term {
        fn end(&self) -> u64 {
            self.base + self.buf.len() as u64
        }

        /// The terminal's foreground program, "" when it is the shell: the
        /// shell leads its own process group (setsid), so any other
        /// foreground group is a program it started - R, vim, top. Named
        /// from /proc. A typed line handed to a busy terminal went into R
        /// as R code (operator, 2026-11-15: "ran vim CONTEXT.md and it ran
        /// it in the R session").
        fn running(&self) -> String {
            // SAFETY: tcgetpgrp reads the foreground group of an open tty fd.
            let pg = unsafe { libc::tcgetpgrp(self.master.as_raw_fd()) };
            if !self.alive || pg <= 0 || pg as u32 == self.child.id() {
                return String::new();
            }
            std::fs::read_to_string(format!("/proc/{pg}/comm")).map(|s| s.trim().to_owned()).unwrap_or_else(|_| "a program".into())
        }
    }

    struct Slot {
        t: Mutex<Term>,
        more: Condvar,
    }

    fn terms() -> &'static Mutex<HashMap<u32, Arc<Slot>>> {
        static T: OnceLock<Mutex<HashMap<u32, Arc<Slot>>>> = OnceLock::new();
        T.get_or_init(Default::default)
    }

    /// The session's terminal: what `id` 0 attaches to.
    static MAIN: AtomicU32 = AtomicU32::new(0);
    static NEXT: AtomicU32 = AtomicU32::new(1);

    fn winsize(rows: u16, cols: u16) -> libc::winsize {
        libc::winsize { ws_row: rows, ws_col: cols, ws_xpixel: 0, ws_ypixel: 0 }
    }

    fn open(rows: u16, cols: u16) -> Result<(u32, Arc<Slot>), String> {
        let ws = winsize(rows, cols);
        let (mut m, mut s) = (0, 0);
        // SAFETY: openpty fills two fds; we own them from here on.
        if unsafe { libc::openpty(&mut m, &mut s, std::ptr::null_mut(), std::ptr::null_mut(), &ws as *const _ as *mut _) } != 0 {
            return Err(format!("no pseudo-terminal: {}", std::io::Error::last_os_error()));
        }
        let (master, slave) = unsafe { (File::from_raw_fd(m), File::from_raw_fd(s)) };
        // The master is ours alone: nothing the shell starts may hold it open.
        unsafe { libc::fcntl(m, libc::F_SETFD, libc::FD_CLOEXEC) };

        let st = crate::host::saved_settings();
        let cwd = crate::host::project_dir(&st);
        let dir = crate::host::kernel::cache_dir();
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let rc = dir.join("term.bashrc");
        std::fs::write(&rc, RC).map_err(|e| format!("{}: {e}", rc.display()))?;
        let mods = st.module(crate::api::Lang::Shell).to_owned();
        // On EESSI or Compute Canada's stack the Terminal enters that stack
        // and loads nothing: the picked modules are the ndexr stack's names.
        let stack = crate::host::stacks::chosen(&st.stack_source);
        let mods = if stack.key == "ndexr" && mods.split_whitespace().all(crate::host::kernel::valid_module) { mods } else { String::new() };
        // Resolved once and replayed (envs.rs); "" = the rc file loads it itself.
        let script = if mods.trim().is_empty() { String::new() } else { crate::host::envs::resolve(&mods).map(|r| r.script.display().to_string()).unwrap_or_default() };
        let (claude, label) = crate::host::claude_env();

        let mut cmd = Command::new("bash");
        crate::host::runner::forget_loaded_modules(&mut cmd);
        cmd.args(["--rcfile", &rc.display().to_string(), "-i"])
            .current_dir(if std::path::Path::new(&cwd).is_dir() { cwd.clone() } else { std::env::var("HOME").unwrap_or_else(|_| "/".into()) })
            .env("TERM", "xterm-256color")
            .env("COLORTERM", "truecolor")
            .env("COVE", "1")
            .env("COVE_USE_SH", crate::host::stacks::enter_script(&st.stack_source, &dir).map(|p| p.display().to_string()).unwrap_or_default())
            .env("COVE_STACK", stack.key)
            .env("COVE_MODULES", &mods)
            .env("COVE_ENV_SCRIPT", &script)
            .env("COVE_CWD", &cwd)
            .env("COVE_CLAUDE", &label)
            .env_remove("NO_COLOR")
            .env_remove("PAGER")
            .envs(claude)
            .stdin(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
            .stdout(Stdio::from(slave.try_clone().map_err(|e| e.to_string())?))
            .stderr(Stdio::from(slave));
        if std::env::var_os("LANG").is_none() {
            cmd.env("LANG", "C.UTF-8");
        }
        unsafe {
            use std::os::unix::process::CommandExt;
            // A session of its own with the pty as its controlling terminal:
            // ^C, ^Z and job control reach what runs in it.
            cmd.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = cmd.spawn().map_err(|e| format!("could not start bash: {e}"))?;
        let mut reader = master.try_clone().map_err(|e| e.to_string())?;
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        let slot = Arc::new(Slot {
            t: Mutex::new(Term {
                master,
                child,
                buf: Vec::new(),
                base: 0,
                screen: vt100::Parser::new_with_callbacks(rows, cols, 0, Replies::default()),
                size: (rows, cols),
                alive: true,
                note: label,
            }),
            more: Condvar::new(),
        });
        let s2 = slot.clone();
        std::thread::spawn(move || {
            let mut chunk = vec![0u8; 64 << 10];
            loop {
                // EIO once the last process on the pty has gone.
                let n = reader.read(&mut chunk).unwrap_or(0);
                let mut t = s2.t.lock().unwrap();
                if n == 0 {
                    t.alive = false;
                    let _ = t.child.try_wait();
                    drop(t);
                    s2.more.notify_all();
                    return;
                }
                t.buf.extend_from_slice(&chunk[..n]);
                if t.buf.len() > KEEP {
                    let cut = t.buf.len() - KEEP;
                    t.buf.drain(..cut);
                    t.base += cut as u64;
                }
                t.screen.process(&chunk[..n]);
                let replies = std::mem::take(&mut t.screen.callbacks_mut().0);
                if !replies.is_empty() {
                    let _ = t.master.write_all(&replies);
                }
                drop(t);
                s2.more.notify_all();
            }
        });
        terms().lock().unwrap().insert(id, slot.clone());
        Ok((id, slot))
    }

    /// Terminal `id`; 0 (or one this server no longer has, after a restart)
    /// is the session's terminal, started if it is not running.
    fn get(id: u32, rows: u16, cols: u16) -> Result<(u32, Arc<Slot>, bool), String> {
        let found = terms().lock().unwrap().get(&id).cloned();
        if let Some(s) = found {
            return Ok((id, s, false));
        }
        let main = MAIN.load(Ordering::Relaxed);
        let live = terms().lock().unwrap().get(&main).cloned().filter(|s| s.t.lock().unwrap().alive);
        if let Some(s) = live {
            return Ok((main, s, true));
        }
        let (id, s) = open(rows, cols)?;
        MAIN.store(id, Ordering::Relaxed);
        Ok((id, s, true))
    }

    /// What terminal `id` printed after `offset`, waiting up to a quarter
    /// second for something new. A tab that is behind what is kept (or new)
    /// gets the screen as it stands, `reset`.
    pub fn read(id: u32, offset: u64, rows: u16, cols: u16) -> Result<TermChunk, String> {
        let (rows, cols) = (rows.clamp(2, 500), cols.clamp(10, 1000));
        let (id, slot, fresh) = get(id, rows, cols)?;
        let mut t = slot.t.lock().unwrap();
        if t.size != (rows, cols) {
            let ws = winsize(rows, cols);
            // SAFETY: TIOCSWINSZ reads one winsize; the kernel sends SIGWINCH.
            unsafe { libc::ioctl(t.master.as_raw_fd(), libc::TIOCSWINSZ as _, &ws) };
            t.screen.screen_mut().set_size(rows, cols);
            t.size = (rows, cols);
        }
        if fresh || offset < t.base || offset > t.end() {
            let data = String::from_utf8_lossy(&t.screen.screen().state_formatted()).into_owned();
            return Ok(TermChunk { id, offset: t.end(), data, reset: true, alive: t.alive, note: t.note.clone(), running: t.running() });
        }
        let until = Instant::now() + Duration::from_millis(250);
        while t.alive && offset == t.end() {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                break;
            }
            t = slot.more.wait_timeout(t, left).unwrap().0;
        }
        let from = (offset - t.base) as usize;
        let bytes = &t.buf[from..(from + CHUNK).min(t.buf.len())];
        // Whole characters only: a sequence cut at the end waits for the next poll.
        let cut = match std::str::from_utf8(bytes) {
            Err(e) if e.error_len().is_none() => e.valid_up_to(),
            _ => bytes.len(),
        };
        let data = String::from_utf8_lossy(&bytes[..cut]).into_owned();
        Ok(TermChunk { id, offset: offset + cut as u64, data, reset: false, alive: t.alive, note: t.note.clone(), running: t.running() })
    }

    /// Keys typed into terminal `id`.
    pub fn write(id: u32, input: &str) -> Result<bool, String> {
        let slot = terms().lock().unwrap().get(&id).cloned().ok_or("that terminal has ended")?;
        let mut t = slot.t.lock().unwrap();
        if !t.alive {
            return Err("that terminal has ended".into());
        }
        t.master.write_all(input.as_bytes()).map_err(|e| e.to_string())?;
        Ok(true)
    }

    /// End terminal `id` and everything running in it.
    pub fn kill(id: u32) -> bool {
        let Some(slot) = terms().lock().unwrap().remove(&id) else { return false };
        let mut t = slot.t.lock().unwrap();
        let pid = t.child.id() as i32;
        // SAFETY: the shell leads its own session and process group (setsid).
        unsafe { libc::kill(-pid, libc::SIGHUP) };
        let _ = t.child.kill();
        let _ = t.child.wait();
        t.alive = false;
        true
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_terminal_answers_what_programs_ask_it() {
            let mut p = vt100::Parser::new_with_callbacks(24, 80, 0, Replies::default());
            p.process(b"ab\x1b[6n\x1b[c\x1b]11;?\x07");
            let r = String::from_utf8(std::mem::take(&mut p.callbacks_mut().0)).unwrap();
            assert!(r.starts_with("\x1b[1;3R\x1b[?62;22c\x1b]11;rgb:1d1d/2020/2727"), "{r:?}");
        }

        #[test]
        fn a_program_on_the_pty_has_a_terminal() {
            let (id, _) = open(30, 100).unwrap();
            // A tab that has never seen it (id 0) gets the screen, reset.
            let first = read(0, 0, 30, 100).unwrap();
            assert!(first.reset);
            kill(first.id);
            let mut c = read(id, 0, 30, 100).unwrap();
            assert!(c.alive);
            write(id, "stty size; tty -s && echo has-a-tty; exit\r").unwrap();
            let mut all = String::new();
            let until = Instant::now() + Duration::from_secs(20);
            while c.alive && Instant::now() < until {
                c = read(id, c.offset, 30, 100).unwrap();
                all.push_str(&c.data);
            }
            assert!(all.contains("30 100") && all.contains("has-a-tty"), "{all:?}");
            kill(id);
        }

        #[test]
        fn the_terminal_says_what_program_it_is_running() {
            let (id, _) = open(24, 80).unwrap();
            let mut c = read(id, 0, 24, 80).unwrap();
            // The shell's own prompt first: nothing in front of it.
            let until = Instant::now() + Duration::from_secs(10);
            while c.running != "" && Instant::now() < until {
                c = read(id, c.offset, 24, 80).unwrap();
            }
            assert_eq!(c.running, "", "a shell at its prompt runs nothing");
            write(id, "sleep 3\r").unwrap();
            let until = Instant::now() + Duration::from_secs(10);
            while c.running != "sleep" && Instant::now() < until {
                c = read(id, c.offset, 24, 80).unwrap();
            }
            assert_eq!(c.running, "sleep");
            let until = Instant::now() + Duration::from_secs(10);
            while !c.running.is_empty() && Instant::now() < until {
                c = read(id, c.offset, 24, 80).unwrap();
            }
            assert_eq!(c.running, "", "back at the prompt when it ends");
            kill(id);
        }

        /// `cargo test -- --ignored claude_draws`: claude's own screen comes up
        /// in the terminal (it needs this machine's claude and its login).
        #[test]
        #[ignore]
        fn claude_draws_its_screen() {
            let (id, _) = open(40, 120).unwrap();
            write(id, "claude\r").unwrap();
            let mut screen = vt100::Parser::new(40, 120, 0);
            let mut off = 0;
            let until = Instant::now() + Duration::from_secs(25);
            while Instant::now() < until {
                let c = read(id, off, 40, 120).unwrap();
                screen.process(c.data.as_bytes());
                off = c.offset;
            }
            let text = screen.screen().contents();
            println!("{text}");
            kill(id);
            assert!(!text.contains("Input must be provided"), "{text}");
        }
    }
}

#[cfg(all(not(unix), not(target_arch = "wasm32")))]
pub mod pty {
    use crate::api::TermChunk;
    pub fn read(_: u32, _: u64, _: u16, _: u16) -> Result<TermChunk, String> {
        Err("the terminal runs in your ndexr cell - sign in and it connects there".into())
    }
    pub fn write(_: u32, _: &str) -> Result<bool, String> {
        Err("no terminal here".into())
    }
    pub fn kill(_: u32) -> bool {
        false
    }
}

// ── the tab: a vt100 screen, drawn ──────────────────────────────────────────

/// The pane's own colours for "the terminal's default", so what claude and vim
/// read back (OSC 10 / 11) is what is on screen. Regolith's, in the light theme
/// too: the server answers those queries and cannot know the window's theme,
/// so a light pane would get programs drawing for a dark one.
pub const DEFAULT_FG: Color32 = Color32::from_rgb(0xd5, 0xda, 0xe3);
pub const DEFAULT_BG: Color32 = Color32::from_rgb(0x1d, 0x20, 0x27);

/// The 16 ANSI colours, Regolith-toned to sit with the rest of the IDE.
const ANSI: [Color32; 16] = [
    Color32::from_rgb(0x15, 0x17, 0x1c),
    Color32::from_rgb(0xe5, 0x76, 0x7a),
    Color32::from_rgb(0xa3, 0xc5, 0x85),
    Color32::from_rgb(0xe8, 0xa7, 0x5d),
    Color32::from_rgb(0x7a, 0xa7, 0xe0),
    Color32::from_rgb(0xc3, 0x8f, 0xd8),
    Color32::from_rgb(0x5e, 0xc4, 0xc0),
    Color32::from_rgb(0xd5, 0xda, 0xe3),
    Color32::from_rgb(0x5d, 0x64, 0x72),
    Color32::from_rgb(0xf0, 0x8d, 0x91),
    Color32::from_rgb(0xbb, 0xdb, 0x9c),
    Color32::from_rgb(0xf2, 0xbf, 0x7e),
    Color32::from_rgb(0x9c, 0xc1, 0xf0),
    Color32::from_rgb(0xd8, 0xaa, 0xe9),
    Color32::from_rgb(0x82, 0xdc, 0xd8),
    Color32::from_rgb(0xee, 0xf1, 0xf5),
];

fn color(c: vt100::Color, default: Color32) -> Color32 {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Rgb(r, g, b) => Color32::from_rgb(r, g, b),
        vt100::Color::Idx(i) if i < 16 => ANSI[i as usize],
        // The 6×6×6 cube, then 24 greys.
        vt100::Color::Idx(i) if i < 232 => {
            let i = i - 16;
            let v = |n: u8| if n == 0 { 0 } else { 55 + n * 40 };
            Color32::from_rgb(v(i / 36), v((i / 6) % 6), v(i % 6))
        }
        vt100::Color::Idx(i) => {
            let g = 8 + (i - 232) * 10;
            Color32::from_rgb(g, g, g)
        }
    }
}

/// The long-lived token `claude setup-token` prints (sk-ant-oat…), read off
/// the screen. It is ~108 characters, so in a narrow pane it runs past the
/// right edge onto the next row (`wrapped_run`).
pub fn oauth_token_in(rows: &[String], cols: u16) -> Option<String> {
    wrapped_run(rows, cols, "sk-ant-oat", |c| c.is_ascii_alphanumeric() || c == '-' || c == '_').filter(|t| t.len() >= 60)
}

/// The sign-in link `claude setup-token` prints - ~400 characters, always
/// over several rows, which made it hard to copy whole.
pub fn signin_link_in(rows: &[String], cols: u16) -> Option<String> {
    wrapped_run(rows, cols, "https://claude.", |c| c.is_ascii_graphic()).filter(|u| u.contains("oauth"))
}

/// The last run of `keep` characters starting at `start` on the screen,
/// carried on from row to row while it fills its row to the right edge.
fn wrapped_run(rows: &[String], cols: u16, start: &str, keep: impl Fn(char) -> bool) -> Option<String> {
    for (i, row) in rows.iter().enumerate().rev() {
        let Some(at) = row.find(start) else { continue };
        let mut out: String = row[at..].chars().take_while(|c| keep(*c)).collect();
        let mut end = row[..at].chars().count() + out.chars().count();
        let mut j = i;
        while end + 1 >= cols as usize && j + 1 < rows.len() {
            j += 1;
            let next = rows[j].trim_start();
            let more: String = next.chars().take_while(|c| keep(*c)).collect();
            if more.is_empty() {
                break;
            }
            end = rows[j].chars().count() - next.chars().count() + more.chars().count();
            out.push_str(&more);
        }
        return Some(out);
    }
    None
}

/// The bytes an xterm sends for a key, or None for one a terminal ignores.
pub fn key_bytes(key: Key, m: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let mods = 1 + (m.shift as u8) + 2 * (m.alt as u8) + 4 * (m.ctrl as u8);
    let cursor = |c: char| -> Vec<u8> {
        if mods > 1 {
            format!("\x1b[1;{mods}{c}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{c}").into_bytes()
        } else {
            format!("\x1b[{c}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> { if mods > 1 { format!("\x1b[{n};{mods}~") } else { format!("\x1b[{n}~") }.into_bytes() };
    if m.ctrl && !m.alt {
        let name = key.name();
        if name.len() == 1 && name.as_bytes()[0].is_ascii_alphabetic() {
            return Some(vec![name.as_bytes()[0].to_ascii_uppercase() - b'@']);
        }
        match key {
            Key::Space | Key::Num2 => return Some(vec![0]),
            Key::OpenBracket => return Some(vec![0x1b]),
            Key::Backslash => return Some(vec![0x1c]),
            Key::CloseBracket => return Some(vec![0x1d]),
            Key::Slash | Key::Minus => return Some(vec![0x1f]),
            _ => {}
        }
    }
    let alt = |b: Vec<u8>| if m.alt { [vec![0x1b], b].concat() } else { b };
    Some(match key {
        Key::Enter => alt(b"\r".to_vec()),
        Key::Backspace => alt(if m.ctrl { vec![0x08] } else { vec![0x7f] }),
        Key::Tab if m.shift => b"\x1b[Z".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::Escape => b"\x1b".to_vec(),
        Key::ArrowUp => cursor('A'),
        Key::ArrowDown => cursor('B'),
        Key::ArrowRight => cursor('C'),
        Key::ArrowLeft => cursor('D'),
        Key::Home => cursor('H'),
        Key::End => cursor('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => b"\x1bOP".to_vec(),
        Key::F2 => b"\x1bOQ".to_vec(),
        Key::F3 => b"\x1bOR".to_vec(),
        Key::F4 => b"\x1bOS".to_vec(),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        _ => return None,
    })
}

struct Client {
    screen: vt100::Parser,
    id: u32,
    offset: u64,
    alive: bool,
    started: bool,
    note: String,
    /// TermChunk::running, as last read.
    running: String,
    error: Option<String>,
    /// Keys typed and not yet sent; `writing` while a write is in flight.
    out: Vec<u8>,
    /// A line to type once a Ctrl+C has been written and the shell has had a
    /// moment (`after_at`, egui seconds; 0 = not yet timed). Sent in the same
    /// write as the ^C, the tty's interrupt flushed the input behind it and
    /// the line arrived without its first letters: setup's `claude
    /// setup-token` ran as `laude setup-token` (operator, 2026-10-15).
    after: Vec<u8>,
    after_at: f64,
    /// Local web addresses servers started here printed (`api::local_url`),
    /// for the Viewer; and the end of the last chunk, where one may be cut.
    urls: Vec<String>,
    carry: String,
}

impl Default for Client {
    fn default() -> Self {
        Client { screen: vt100::Parser::new(24, 80, SCROLLBACK), id: 0, offset: 0, alive: false, started: false, note: String::new(), running: String::new(), error: None, out: Vec::new(), after: Vec::new(), after_at: 0.0, urls: Vec::new(), carry: String::new() }
    }
}

pub struct TermPane {
    st: Arc<Mutex<Client>>,
    reading: Arc<AtomicBool>,
    writing: Arc<AtomicBool>,
    /// The grid size the pane has room for.
    size: (u16, u16),
    /// A drag-selection: (row, col) where it started and where it is.
    sel: Option<((u16, u16), (u16, u16))>,
    /// Wheel movement not yet a whole line.
    wheel: f32,
}

impl Default for TermPane {
    fn default() -> Self {
        TermPane { st: Default::default(), reading: Default::default(), writing: Default::default(), size: (24, 80), sel: None, wheel: 0.0 }
    }
}

impl TermPane {
    /// Local web addresses printed since the last ask.
    pub fn take_urls(&self) -> Vec<String> {
        std::mem::take(&mut self.st.lock().unwrap().urls)
    }

    /// The program the terminal is running, "" at its shell's prompt.
    pub fn running(&self) -> String {
        self.st.lock().unwrap().running.clone()
    }

    /// The tab's light: a terminal is open.
    pub fn alive(&self) -> bool {
        self.st.lock().unwrap().alive
    }

    /// What the terminal says about itself (the tab's tooltip, the strip).
    pub fn note(&self) -> String {
        let s = self.st.lock().unwrap();
        match &s.error {
            Some(e) => e.clone(),
            None if !s.started => "starting…".into(),
            None if !s.alive => "the shell ended - ⟳ starts a new one".into(),
            None if s.note.is_empty() => format!("bash · {}×{}", self.size.1, self.size.0),
            None => format!("bash · {}×{} · claude on {}", self.size.1, self.size.0, s.note),
        }
    }

    /// Type `code` into the terminal and run it (a ▶ Terminal block, a shell
    /// line from the editor).
    /// Ctrl+C, then `code` typed and run once the interrupt has landed - for
    /// a line that must reach a fresh prompt whatever was waiting there (a
    /// `claude setup-token` still at its code prompt took the next line as
    /// the code).
    pub fn interrupt_then_line(&mut self, code: &str) {
        let mut s = self.st.lock().unwrap();
        s.out.push(0x03);
        s.after = code.trim_end().replace('\n', "\r").into_bytes();
        s.after.push(b'\r');
        s.after_at = 0.0;
    }

    pub fn type_line(&mut self, code: &str) {
        let mut s = self.st.lock().unwrap();
        s.out.extend(code.trim_end().replace('\n', "\r").bytes());
        s.out.push(b'\r');
    }

    /// The screen as text, one string per row, and its width - what setup
    /// reads `claude setup-token`'s token from (`oauth_token_in`).
    pub fn screen_rows(&self) -> (Vec<String>, u16) {
        let s = self.st.lock().unwrap();
        let screen = s.screen.screen();
        let (_, cols) = screen.size();
        (screen.rows(0, cols).collect(), cols)
    }

    /// End this terminal; the next frame starts a fresh one.
    pub fn restart(&mut self, backend: &Backend, ctx: &egui::Context) {
        let id = {
            let mut s = self.st.lock().unwrap();
            let id = s.id;
            *s = Client::default();
            id
        };
        if id != 0 {
            backend.query(Query::TermKill { id }, ctx, |_: Result<bool, String>| {});
        }
    }

    fn pump(&mut self, backend: &Backend, ctx: &egui::Context) {
        // Keys first, one write at a time so they arrive in order.
        if !self.writing.load(Ordering::Relaxed) {
            // A line held behind a ^C: timed from when the ^C has been
            // written, sent 0.3 s later as its own write.
            {
                let now = ctx.input(|i| i.time);
                let mut s = self.st.lock().unwrap();
                if s.out.is_empty() && !s.after.is_empty() {
                    if s.after_at == 0.0 {
                        s.after_at = now + 0.3;
                        ctx.request_repaint_after(std::time::Duration::from_millis(320));
                    } else if now >= s.after_at {
                        let line = std::mem::take(&mut s.after);
                        s.out = line;
                        s.after_at = 0.0;
                    } else {
                        ctx.request_repaint_after(std::time::Duration::from_millis(60));
                    }
                }
            }
            let (id, out) = {
                let mut s = self.st.lock().unwrap();
                if s.id == 0 || s.out.is_empty() { (0, Vec::new()) } else { (s.id, std::mem::take(&mut s.out)) }
            };
            if id != 0 {
                self.writing.store(true, Ordering::Relaxed);
                let (writing, st) = (self.writing.clone(), self.st.clone());
                let input = String::from_utf8_lossy(&out).into_owned();
                backend.query(Query::TermWrite { id, input }, ctx, move |r: Result<bool, String>| {
                    writing.store(false, Ordering::Relaxed);
                    if let Err(e) = r {
                        st.lock().unwrap().error = Some(e);
                    }
                });
            }
        }
        // Then the long poll, always one out while the tab is on screen.
        if self.reading.swap(true, Ordering::Relaxed) {
            return;
        }
        let (id, offset) = {
            let s = self.st.lock().unwrap();
            (s.id, s.offset)
        };
        let (rows, cols) = self.size;
        let (reading, st) = (self.reading.clone(), self.st.clone());
        backend.query(Query::Term { id, offset, rows, cols }, ctx, move |r: Result<TermChunk, String>| {
            reading.store(false, Ordering::Relaxed);
            let mut s = st.lock().unwrap();
            match r {
                Ok(c) => {
                    if c.reset {
                        let (r, k) = s.screen.screen().size();
                        s.screen = vt100::Parser::new(r, k, SCROLLBACK);
                    }
                    s.screen.process(c.data.as_bytes());
                    // A server started here (quarto preview, mkdocs serve,
                    // python -m http.server …) goes to the Viewer, as
                    // Positron's app launcher sends it.
                    // Whole lines only: a chunk can end inside an address.
                    let text = format!("{}{}", s.carry, c.data);
                    let (done, rest) = text.rsplit_once('\n').unwrap_or(("", &text));
                    for line in done.split('\n') {
                        if let Some(u) = crate::api::local_url(line) {
                            if !s.urls.contains(&u) {
                                s.urls.push(u);
                            }
                        }
                    }
                    s.carry = rest.chars().rev().take(400).collect::<Vec<_>>().into_iter().rev().collect();
                    s.started = true;
                    s.error = None;
                    (s.id, s.offset, s.alive, s.note, s.running) = (c.id, c.offset, c.alive, c.note, c.running);
                }
                Err(e) => s.error = Some(e),
            }
        });
    }

    /// The terminal, filling `ui`.
    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend) {
        let font = FontId::monospace(FONT);
        let (cw, rh) = ui.fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
        let (cw, rh) = (cw.max(5.0), rh.max(8.0));
        let avail = ui.available_rect_before_wrap();
        let (rect, _) = ui.allocate_exact_size(avail.size(), Sense::hover());
        // The area answers to the same id the focus is asked for. Allocated
        // under an automatic id, nothing on screen claimed "cove-term", so egui
        // dropped its focus at the end of every frame and the next frame took
        // it back: keys typed in the frames between were lost (operator,
        // 2026-10-15: "slow to type ... flickering"; `echo hello cove
        // terminal` arrived as `echo ove termn`) and the cursor blinked
        // between its focused and unfocused look.
        let id = egui::Id::new("cove-term");
        let resp = ui.interact(rect, id, Sense::click_and_drag());
        let inner = rect.shrink2(egui::vec2(8.0, 4.0));
        let size = (((inner.height() / rh).floor() as u16).max(2), ((inner.width() / cw).floor() as u16).max(10));
        if size != self.size {
            self.size = size;
            self.st.lock().unwrap().screen.screen_mut().set_size(size.0, size.1);
        }
        let focused = ui.memory(|m| m.has_focus(id));
        if resp.clicked() || resp.drag_started() {
            ui.memory_mut(|m| m.request_focus(id));
        }
        if focused {
            // Tab, arrows and Esc belong to the program, not to egui's focus moves.
            ui.memory_mut(|m| m.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: true }));
        } else if ui.memory(|m| m.focused().is_none()) {
            // Shown with nothing else focused: typing goes here.
            ui.memory_mut(|m| m.request_focus(id));
        }

        let cell_at = |p: egui::Pos2| -> (u16, u16) {
            let r = ((p.y - inner.top()) / rh).floor().clamp(0.0, (size.0 - 1) as f32) as u16;
            let c = ((p.x - inner.left()) / cw).floor().clamp(0.0, (size.1 - 1) as f32) as u16;
            (r, c)
        };

        {
            let mut s = self.st.lock().unwrap();
            let (app_cursor, paste_brackets, mouse) = {
                let sc = s.screen.screen();
                (sc.application_cursor(), sc.bracketed_paste(), sc.mouse_protocol_mode() != vt100::MouseProtocolMode::None)
            };
            let alt_screen = s.screen.screen().alternate_screen();
            let sgr = s.screen.screen().mouse_protocol_encoding() == vt100::MouseProtocolEncoding::Sgr;
            if focused {
                let mut copy = false;
                ui.input_mut(|i| {
                    i.events.retain(|e| {
                        match e {
                            egui::Event::Text(t) => s.out.extend(t.bytes()),
                            egui::Event::Paste(t) => {
                                let t = t.replace("\r\n", "\r").replace('\n', "\r");
                                if paste_brackets {
                                    s.out.extend(b"\x1b[200~");
                                    s.out.extend(t.bytes());
                                    s.out.extend(b"\x1b[201~");
                                } else {
                                    s.out.extend(t.bytes());
                                }
                            }
                            // Ctrl+Shift+C copies the selection; Ctrl+V comes as Paste.
                            egui::Event::Key { key: Key::C, pressed: true, modifiers, .. } if modifiers.ctrl && modifiers.shift => copy = true,
                            egui::Event::Key { key: Key::V, pressed: true, modifiers, .. } if modifiers.ctrl || modifiers.mac_cmd => {}
                            egui::Event::Key { key: Key::C, pressed: true, modifiers, .. } if modifiers.mac_cmd => copy = true,
                            egui::Event::Key { key, pressed: true, modifiers, .. } => match key_bytes(*key, *modifiers, app_cursor) {
                                Some(b) => s.out.extend(b),
                                None => return true,
                            },
                            egui::Event::Key { .. } | egui::Event::Copy | egui::Event::Cut => {}
                            _ => return true,
                        }
                        false
                    });
                });
                if copy {
                    if let Some(((r0, c0), (r1, c1))) = self.sel.map(|(a, b)| if a <= b { (a, b) } else { (b, a) }) {
                        ui.ctx().copy_text(s.screen.screen().contents_between(r0, c0, r1, c1 + 1));
                    }
                }
            }

            // The wheel: the program's when it asked for the mouse, the arrow
            // keys on a full-screen program that did not, else the scrollback.
            if resp.hovered() {
                self.wheel += ui.input(|i| i.smooth_scroll_delta.y);
                let lines = (self.wheel / rh).trunc() as i32;
                self.wheel -= lines as f32 * rh;
                if lines != 0 {
                    if mouse {
                        let (r, c) = ui.input(|i| i.pointer.hover_pos()).map_or((0, 0), cell_at);
                        let b = if lines > 0 { 64 } else { 65 };
                        for _ in 0..lines.unsigned_abs().min(10) {
                            if sgr {
                                s.out.extend(format!("\x1b[<{b};{};{}M", c + 1, r + 1).bytes());
                            } else {
                                s.out.extend([0x1b, b'[', b'M', 32 + b, 33 + c.min(222) as u8, 33 + r.min(222) as u8]);
                            }
                        }
                    } else if alt_screen {
                        let k = if lines > 0 { Key::ArrowUp } else { Key::ArrowDown };
                        for _ in 0..lines.unsigned_abs().min(10) {
                            s.out.extend(key_bytes(k, Modifiers::NONE, app_cursor).unwrap_or_default());
                        }
                    } else {
                        let now = s.screen.screen().scrollback();
                        let to = (now as i64 + lines as i64).max(0) as usize;
                        s.screen.screen_mut().set_scrollback(to);
                    }
                }
            }
            // Typing goes to the live screen, not the history.
            if focused && !s.out.is_empty() && s.screen.screen().scrollback() > 0 {
                s.screen.screen_mut().set_scrollback(0);
            }
            // Clicks for a program that asked for them (SGR only); otherwise a drag selects.
            if mouse && sgr {
                if let Some(p) = resp.interact_pointer_pos().filter(|_| resp.drag_started() || resp.clicked()) {
                    let (r, c) = cell_at(p);
                    s.out.extend(format!("\x1b[<0;{};{}M\x1b[<0;{};{}m", c + 1, r + 1, c + 1, r + 1).bytes());
                }
                self.sel = None;
            } else if resp.drag_started() {
                if let Some(p) = resp.interact_pointer_pos() {
                    self.sel = Some((cell_at(p), cell_at(p)));
                }
            } else if resp.dragged() {
                if let (Some(p), Some((a, _))) = (resp.interact_pointer_pos(), self.sel) {
                    self.sel = Some((a, cell_at(p)));
                }
            } else if resp.clicked() {
                self.sel = None;
            }
            if resp.drag_stopped() {
                if let Some(((r0, c0), (r1, c1))) = self.sel.map(|(a, b)| if a <= b { (a, b) } else { (b, a) }) {
                    let text = s.screen.screen().contents_between(r0, c0, r1, c1 + 1);
                    if !text.trim().is_empty() {
                        ui.ctx().copy_text(text);
                    }
                }
            }

            paint(ui, rect, inner, &s.screen, cw, rh, &font, focused, self.sel);
            if let Some(e) = &s.error {
                ui.painter().text(inner.left_bottom() - egui::vec2(0.0, 4.0), egui::Align2::LEFT_BOTTOM, e, FontId::proportional(11.5), theme::alt());
            } else if !s.started {
                ui.painter().text(inner.left_top(), egui::Align2::LEFT_TOP, "starting a terminal…", FontId::proportional(12.0), theme::hush());
            }
        }
        self.pump(backend, ctx);
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// The screen, cell by cell: backgrounds in runs, text in runs of one look,
/// each run placed at its own column so a glyph from a fallback font cannot
/// push the rest of its line off the grid.
#[allow(clippy::too_many_arguments)]
fn paint(ui: &egui::Ui, rect: Rect, inner: Rect, p: &vt100::Parser, cw: f32, rh: f32, font: &FontId, focused: bool, sel: Option<((u16, u16), (u16, u16))>) {
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, DEFAULT_BG);
    let sc = p.screen();
    let (rows, cols) = sc.size();
    let sel = sel.map(|(a, b)| if a <= b { (a, b) } else { (b, a) });
    let selected = |r: u16, c: u16| sel.is_some_and(|(a, b)| (r, c) >= a && (r, c) <= b);
    for r in 0..rows {
        let y = inner.top() + r as f32 * rh;
        let mut c = 0;
        while c < cols {
            let Some(cell) = sc.cell(r, c) else { break };
            let (mut fg, mut bg) = (color(cell.fgcolor(), DEFAULT_FG), color(cell.bgcolor(), DEFAULT_BG));
            if cell.bold() && matches!(cell.fgcolor(), vt100::Color::Idx(0..=7)) {
                if let vt100::Color::Idx(i) = cell.fgcolor() {
                    fg = ANSI[i as usize + 8];
                }
            }
            if cell.inverse() ^ selected(r, c) {
                std::mem::swap(&mut fg, &mut bg);
                if bg == DEFAULT_BG {
                    bg = theme::bg();
                }
            }
            if cell.dim() {
                fg = fg.gamma_multiply(0.6);
            }
            let w = if cell.is_wide() { 2 } else { 1 };
            if bg != DEFAULT_BG {
                painter.rect_filled(Rect::from_min_size(egui::pos2(inner.left() + c as f32 * cw, y), egui::vec2(cw * w as f32, rh)), 0.0, bg);
            }
            let text = cell.contents();
            if !text.is_empty() && text != " " {
                // One run: this cell and those after it that look the same
                // and are plain ASCII.
                let mut run = text.to_owned();
                let ascii = text.is_ascii();
                let mut k = c + w;
                while ascii && k < cols {
                    let Some(n) = sc.cell(r, k) else { break };
                    let t = n.contents();
                    if !t.is_ascii() || n.is_wide() || n.fgcolor() != cell.fgcolor() || n.bgcolor() != cell.bgcolor() || n.bold() != cell.bold() || n.italic() != cell.italic() || n.underline() != cell.underline() || n.inverse() != cell.inverse() || n.dim() != cell.dim() || selected(r, k) != selected(r, c) {
                        break;
                    }
                    if n.bgcolor() != vt100::Color::Default && !n.inverse() {
                        break;
                    }
                    run.push_str(if t.is_empty() { " " } else { t });
                    k += 1;
                }
                // The run's background past its first cell: an inverse run
                // (top's header, a selected menu line) had only its first cell
                // filled, the rest drawn in the background's own colour.
                if bg != DEFAULT_BG && k > c + w {
                    painter.rect_filled(Rect::from_min_size(egui::pos2(inner.left() + (c + w) as f32 * cw, y), egui::vec2(cw * (k - c - w) as f32, rh)), 0.0, bg);
                }
                let mut job = LayoutJob::default();
                job.append(&run, 0.0, TextFormat { font_id: font.clone(), color: fg, italics: cell.italic(), underline: if cell.underline() { Stroke::new(1.0, fg) } else { Stroke::NONE }, ..Default::default() });
                let g = ui.fonts_mut(|f| f.layout_job(job));
                painter.galley(egui::pos2(inner.left() + c as f32 * cw, y), g, fg);
                c = k;
                continue;
            }
            c += w;
        }
    }
    if sc.scrollback() == 0 && !sc.hide_cursor() {
        let (r, c) = sc.cursor_position();
        let at = Rect::from_min_size(egui::pos2(inner.left() + c as f32 * cw, inner.top() + r as f32 * rh), egui::vec2(cw, rh));
        if focused {
            painter.rect_filled(at, 0.0, theme::amber().gamma_multiply(0.75));
            if let Some(cell) = sc.cell(r, c).filter(|x| !x.contents().is_empty()) {
                painter.text(at.left_top(), egui::Align2::LEFT_TOP, cell.contents(), font.clone(), DEFAULT_BG);
            }
        } else {
            painter.rect_stroke(at, 0.0, Stroke::new(1.0, theme::amber()), egui::StrokeKind::Inside);
        }
    }
    if sc.scrollback() > 0 {
        let t = format!("↑ {} lines back · type to return", sc.scrollback());
        painter.text(rect.right_top() + egui::vec2(-10.0, 4.0), egui::Align2::RIGHT_TOP, t, FontId::proportional(11.0), theme::hush());
    }
}

/// The strip's words for the terminal (used by ide.rs beside its tools).
pub fn strip_label(t: &TermPane) -> RichText {
    RichText::new(t.note()).monospace().size(11.0).color(theme::hush())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_setup_token_even_wrapped() {
        let t = format!("sk-ant-oat01-{}", "Ab3_-x".repeat(16));
        // On one row.
        let rows = vec!["Your OAuth token (valid for 1 year):".to_owned(), String::new(), format!("{t}"), String::new(), "Store this token securely.".to_owned()];
        assert_eq!(oauth_token_in(&rows, 200).as_deref(), Some(t.as_str()));
        // Run over a 60-column pane.
        let rows = vec![t[..60].to_owned(), t[60..].to_owned(), String::new(), "Store this token securely.".to_owned()];
        assert_eq!(oauth_token_in(&rows, 60).as_deref(), Some(t.as_str()));
        // Indented, over three rows, and the next row's words not taken.
        let rows = vec![format!("  {}", &t[..38]), t[38..78].to_owned(), t[78..].to_owned(), "Store".to_owned()];
        assert_eq!(oauth_token_in(&rows, 40).as_deref(), Some(t.as_str()));
        assert_eq!(oauth_token_in(&["nothing here".to_owned()], 80), None);
        // The sign-in link, over three full rows of a 40-column pane.
        let url = format!("https://claude.com/cai/oauth/authorize?code=true&client_id={}", "x".repeat(60));
        let rows = vec!["Browser didn't open? Use the url below".to_owned(), String::new(), url[..40].to_owned(), url[40..80].to_owned(), url[80..].to_owned(), String::new(), " Paste code here if prompted >".to_owned()];
        assert_eq!(signin_link_in(&rows, 40).as_deref(), Some(url.as_str()));
    }

    #[test]
    fn keys_are_the_bytes_an_xterm_sends() {
        let n = Modifiers::NONE;
        assert_eq!(key_bytes(Key::Enter, n, false), Some(b"\r".to_vec()));
        assert_eq!(key_bytes(Key::C, Modifiers::CTRL, false), Some(vec![3]));
        assert_eq!(key_bytes(Key::ArrowUp, n, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_bytes(Key::ArrowUp, n, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(key_bytes(Key::ArrowLeft, Modifiers::CTRL, false), Some(b"\x1b[1;5D".to_vec()));
        assert_eq!(key_bytes(Key::Tab, Modifiers::SHIFT, false), Some(b"\x1b[Z".to_vec()));
        assert_eq!(key_bytes(Key::Delete, n, false), Some(b"\x1b[3~".to_vec()));
        assert_eq!(key_bytes(Key::A, n, false), None, "letters come as Text");
    }

    #[test]
    fn colours_cover_the_256() {
        assert_eq!(color(vt100::Color::Idx(1), DEFAULT_FG), ANSI[1]);
        assert_eq!(color(vt100::Color::Idx(16), DEFAULT_FG), Color32::from_rgb(0, 0, 0));
        assert_eq!(color(vt100::Color::Idx(231), DEFAULT_FG), Color32::from_rgb(255, 255, 255));
        assert_eq!(color(vt100::Color::Idx(255), DEFAULT_FG), Color32::from_rgb(238, 238, 238));
    }
}
