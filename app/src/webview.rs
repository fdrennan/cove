//! The desktop app's embedded browser: the App panel shows a domain's live
//! site inside the IDE window, never in another app (operator, 2026-10-14:
//! "both need to render through their ide not open up the external webpage").
//!
//! A native webview - WKWebView on macOS, WebKitGTK 4.1 on Linux - laid over
//! the panel's rectangle, the way the browser build lays an iframe there
//! (ide.rs site_frame). It is a top-level page, not a frame, so X-Frame-Options
//! and a site password's form do not stand in its way. Every webview shares
//! one context whose cookies are kept on disk (`CONTEXT`), so the ndexr
//! sign-in happens once, for every panel and every launch.
//!
//! Per frame: the App panel (and the Viewer, `place_in`) asks for a rectangle,
//! and after the frame `end_frame` creates, moves, reloads or hides each
//! panel's own webview - so the site and a rendered page can be side by side. Linux needs
//! GTK running on this thread and its loop advanced - done here every frame
//! the webview is shown, and only on X11 (wry's child webview).

use std::cell::RefCell;

use eframe::egui;

#[cfg_attr(not(feature = "webview"), allow(dead_code))]
struct Want {
    rect: egui::Rect,
    zoom: f32,
    url: String,
    seq: u64,
}

thread_local! {
    /// What each panel asked for this frame, by the panel's name.
    static WANT: RefCell<Vec<(&'static str, Want)>> = const { RefCell::new(Vec::new()) };
}

#[cfg(all(feature = "webview", any(target_os = "macos", target_os = "linux")))]
mod imp {
    use std::cell::RefCell;

    use wry::dpi::{LogicalPosition, LogicalSize};
    use wry::{Rect, WebContext, WebView, WebViewBuilder};

    pub struct Shown {
        pub view: WebView,
        pub url: String,
        pub seq: u64,
        pub visible: bool,
    }

    thread_local! {
        pub static VIEW: RefCell<Vec<(&'static str, Shown)>> = const { RefCell::new(Vec::new()) };
        /// Could not be made here (Wayland, no WebKitGTK): fall back for good.
        pub static BROKEN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        /// One context for every webview, its cookies kept on disk. wry
        /// gives a webview built without one a context of its own, and
        /// WebKitGTK keeps that context's cookies in memory only: each App
        /// or Viewer webview Cove made started signed out, so every reload
        /// was the console.ndexr.io sign-in again (operator, 2026-11-03: "I
        /// keep having to sign in through ndexr-console in the gui in local
        /// cove every time it refreshes"). WKWebView keeps its own on disk.
        static CONTEXT: RefCell<Option<WebContext>> = const { RefCell::new(None) };
    }

    /// $XDG_DATA_HOME/cove/webview, else ~/.local/share/cove/webview.
    fn data_dir() -> Option<std::path::PathBuf> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(std::path::PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share")))?;
        let dir = base.join("cove/webview");
        std::fs::create_dir_all(&dir).ok()?;
        Some(dir)
    }

    pub fn supported() -> bool {
        if BROKEN.with(|b| b.get()) {
            return false;
        }
        // wry's child webview is X11-only on Linux.
        !cfg!(target_os = "linux") || std::env::var_os("WAYLAND_DISPLAY").is_none() || std::env::var_os("DISPLAY").is_some()
    }

    pub fn bounds(rect: egui::Rect, zoom: f32) -> Rect {
        Rect {
            position: LogicalPosition::new((rect.left() * zoom) as f64, (rect.top() * zoom) as f64).into(),
            size: LogicalSize::new((rect.width() * zoom).max(1.0) as f64, (rect.height() * zoom).max(1.0) as f64).into(),
        }
    }

    use eframe::egui;

    #[cfg(target_os = "linux")]
    fn gtk_ready() -> bool {
        thread_local! { static INIT: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) }; }
        INIT.with(|i| {
            if i.get().is_none() {
                i.set(Some(gtk::init().is_ok()));
            }
            i.get() == Some(true)
        })
    }

    pub fn build(frame: &eframe::Frame, url: &str, b: Rect) -> Option<WebView> {
        #[cfg(target_os = "linux")]
        if !gtk_ready() {
            return None;
        }
        // Marked, so a Cove opened in it knows it is not the main one
        // (release.rs `embedded`: no update bar of its own).
        CONTEXT.with_borrow_mut(|c| {
            let ctx = c.get_or_insert_with(|| WebContext::new(data_dir()));
            match WebViewBuilder::new_with_web_context(ctx).with_url(url).with_initialization_script("window.__coveEmbedded = true;").with_bounds(b).build_as_child(frame) {
                Ok(v) => Some(v),
                Err(e) => {
                    eprintln!("webview: {e}");
                    None
                }
            }
        })
    }

    /// Linux: WebKitGTK paints and takes input through GTK's loop. Only once
    /// a webview has started GTK: before that, every frame panicked with "GTK
    /// has not been initialized" (the laptop, 2026-11-02, once WebKitGTK was
    /// installed and the binary could load).
    pub fn pump() {
        #[cfg(target_os = "linux")]
        while gtk::is_initialized_main_thread() && gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
    }
}

/// The App panel wants the site at `rect`. False when this build or display
/// has no embedded browser (built without `webview`, Windows, Wayland): the
/// panel then offers a link.
pub fn place(ctx: &egui::Context, rect: egui::Rect, url: &str, seq: u64) -> bool {
    place_in("app", ctx, rect, url, seq)
}

/// Same, for the panel named `slot` (each panel keeps a webview of its own).
pub fn place_in(slot: &'static str, ctx: &egui::Context, rect: egui::Rect, url: &str, seq: u64) -> bool {
    #[cfg(all(feature = "webview", any(target_os = "macos", target_os = "linux")))]
    if imp::supported() {
        WANT.with(|w| {
            let mut w = w.borrow_mut();
            w.retain(|(s, _)| *s != slot);
            w.push((slot, Want { rect, zoom: ctx.zoom_factor(), url: url.to_owned(), seq }));
        });
        return true;
    }
    let _ = (slot, ctx, rect, url, seq);
    false
}

/// After every frame: show the webview where the panel asked this frame (made
/// on first use, reloaded when the panel's ⟳ or a save bumps `seq`), or hide
/// it - when the App tab is not showing, or a dialog is open (`hide`).
pub fn end_frame(ctx: &egui::Context, frame: &eframe::Frame, hide: bool) {
    let mut want = WANT.with(|w| std::mem::take(&mut *w.borrow_mut()));
    if hide {
        want.clear();
    }
    #[cfg(all(feature = "webview", any(target_os = "macos", target_os = "linux")))]
    imp::VIEW.with(|v| {
        let mut views = v.borrow_mut();
        // Not asked for this frame: hidden, kept for when its panel shows again.
        for (slot, s) in views.iter_mut() {
            if s.visible && !want.iter().any(|(w, _)| w == slot) {
                let _ = s.view.set_visible(false);
                s.visible = false;
            }
        }
        let shown = !want.is_empty();
        for (slot, w) in want {
            let b = imp::bounds(w.rect, w.zoom);
            match views.iter_mut().find(|(s, _)| *s == slot) {
                Some((_, s)) => {
                    let _ = s.view.set_bounds(b);
                    if !s.visible {
                        let _ = s.view.set_visible(true);
                        s.visible = true;
                    }
                    if s.url != w.url {
                        let _ = s.view.load_url(&w.url);
                        s.url = w.url;
                        s.seq = w.seq;
                    } else if s.seq != w.seq {
                        let _ = s.view.reload();
                        s.seq = w.seq;
                    }
                }
                None => match imp::build(frame, &w.url, b) {
                    Some(view) => views.push((slot, imp::Shown { view, url: w.url, seq: w.seq, visible: true })),
                    None => imp::BROKEN.with(|x| x.set(true)),
                },
            }
        }
        // Keep GTK's loop turning while a page is on screen.
        if shown && cfg!(target_os = "linux") {
            ctx.request_repaint_after(std::time::Duration::from_millis(16));
        }
        imp::pump();
    });
    #[cfg(not(all(feature = "webview", any(target_os = "macos", target_os = "linux"))))]
    let _ = (ctx, frame, want);
}
