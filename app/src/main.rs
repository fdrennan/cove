#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! Cove — a system for building and managing very large numbers of
//! scientific applications, kernel to user: immutable over time, customisable
//! without breaking earlier environments, for regulated and scientific work
//! (operator, 2026-10-15; never "an R and Python ide"). Agents are one way of
//! using it: they weave the stack's software, the consoles, the terminal and
//! the live app into new applications. One UI, three ways to run it:
//!
//!   cove                 the desktop app, driving this machine
//!   cove --connect URL   the desktop app, driving a server
//!   cove serve           the server: the UI as WebAssembly + /api
//!   cove multiuser       one server for a machine's people (root): PAM
//!                             sign-in, a `serve` per person run as them
//!   (wasm32)                      the UI in a browser, driving its server

#[cfg(not(target_arch = "wasm32"))]
mod account;
mod tabs;
#[cfg(not(target_arch = "wasm32"))]
mod boxcomplete;
mod spoken;
#[cfg(not(target_arch = "wasm32"))]
mod localagent;
mod manage;
#[cfg(unix)]
mod localserve;
mod localstack;
mod account_tab;
mod settings_tab;
mod access;
mod agent;
mod ansi;
mod api;
#[cfg(not(target_arch = "wasm32"))]
mod apps;
#[cfg(unix)]
mod record;
mod app;
mod atlas;
mod backdrop;
mod backend;
#[cfg(not(target_arch = "wasm32"))]
mod boxes;
mod db;
mod dock;
// Server-side but for TYPES, which the Settings tab's Type menu reads.
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
mod dotcove;
mod edcomplete;
mod editkeys;
#[cfg(not(target_arch = "wasm32"))]
mod envs;
mod find;
mod frameworks;
mod gitview;
#[cfg(not(target_arch = "wasm32"))]
mod host;
#[cfg(not(target_arch = "wasm32"))]
mod zippack;
#[cfg(not(target_arch = "wasm32"))]
mod vitals;
mod icons;
mod ide;
#[cfg(not(target_arch = "wasm32"))]
mod lsp;
mod inbox;
#[cfg(not(target_arch = "wasm32"))]
mod kernel;
#[cfg(not(target_arch = "wasm32"))]
mod messages;
#[cfg(not(target_arch = "wasm32"))]
mod recap;
#[cfg(not(target_arch = "wasm32"))]
mod brief;
#[cfg(not(target_arch = "wasm32"))]
mod patches;
#[cfg(not(target_arch = "wasm32"))]
mod pkgflow;
mod manifest;
mod md;
mod pick;
mod names;
mod paste;
mod recents;
mod release;
#[cfg(not(target_arch = "wasm32"))]
mod runner;
#[cfg(all(unix, not(target_arch = "wasm32")))]
mod limits;
#[cfg(all(unix, not(target_arch = "wasm32")))]
mod multiuser;
#[cfg(all(unix, not(target_arch = "wasm32")))]
mod server;
#[cfg(all(unix, not(target_arch = "wasm32")))]
mod shares;
mod software;
mod source;
mod stage;
#[cfg(not(target_arch = "wasm32"))]
mod stack;
mod syntax;
mod starters;
#[cfg(not(target_arch = "wasm32"))]
mod storage;
mod term;
mod setup;
mod shots;
mod showcase;
mod theme;
mod tour;
#[cfg(target_arch = "wasm32")]
mod view;
mod voice;
#[cfg(not(target_arch = "wasm32"))]
mod update;
#[cfg(not(target_arch = "wasm32"))]
mod webview;

/// Cove was "ndexr ide" until 2026-10-14, and kept a person's settings, server
/// token and run-on pick in ~/.config/ndexr/ide*. Moved once to the cove names
/// so nobody is asked again or loses their token; a cove file already there wins.
#[cfg(not(target_arch = "wasm32"))]
fn carry_over_ide_config() {
    let Some(base) = std::env::var_os("XDG_CONFIG_HOME").map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config")))
    else {
        return;
    };
    let dir = base.join("ndexr");
    for (old, new) in [("ide.json", "cove.json"), ("ide-token", "cove-token"), ("ide-run-on", "cove-run-on")] {
        let (old, new) = (dir.join(old), dir.join(new));
        if old.is_file() && !new.exists() {
            let _ = std::fs::rename(&old, &new);
        }
    }
}

/// A panic is also appended to ~/.cache/ndexr/cove/crash.log, with its
/// backtrace: launched from the app menu, stderr goes nowhere, and a crash on
/// the laptop left no trace at all (2026-11-02, "cove crashed when clicking on
/// a new sub directory").
#[cfg(not(target_arch = "wasm32"))]
fn keep_crashes() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        use std::io::Write;
        let path = kernel::cache_dir().join("crash.log");
        let when = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        let thread = std::thread::current().name().unwrap_or("?").to_owned();
        let trace = std::backtrace::Backtrace::force_capture();
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let _ = writeln!(f, "--- cove {} · unix {when} · thread '{thread}'\n{info}\n{trace}", env!("CARGO_PKG_VERSION"));
        }
        default(info);
    }));
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result<()> {
    // Windows has no HOME; every path here (~/.config/ndexr, ~/.cache/ndexr)
    // is written against it, so it is the person's profile folder.
    #[cfg(windows)]
    if std::env::var_os("HOME").is_none() {
        if let Some(p) = std::env::var_os("USERPROFILE") {
            std::env::set_var("HOME", p);
        }
    }
    carry_over_ide_config();
    keep_crashes();
    let args: Vec<String> = std::env::args().collect();
    let mut backend = backend::Backend::Local;
    // `--folder <dir>` opens a folder as the project, as cove.ndexr.io/?folder= does.
    let folder = args.iter().position(|a| a == "--folder").and_then(|i| args.get(i + 1)).cloned();
    match args.get(1).map(String::as_str) {
        Some("--update") => std::process::exit(update::cli()),
        Some("--version") => {
            println!("cove {} ({})", update::VERSION, update::platform());
            return Ok(());
        }
        #[cfg(unix)]
        Some("serve") => std::process::exit(server::main(&args[2..])),
        #[cfg(unix)]
        Some("multiuser") => std::process::exit(multiuser::main(&args[2..])),
        // Compiled starters, built once for everyone (host.rs seeds).
        #[cfg(unix)]
        Some("seed") => std::process::exit(host::seed_cli(&args[2..])),
        // Move projects made before .cove/ into it (dotcove.rs adopt):
        // `cove dotcove <project folder>...`, run once over every project.
        #[cfg(not(target_arch = "wasm32"))]
        Some("dotcove") => std::process::exit(host::dotcove_cli(&args[2..])),
        // `cove record [dir]`: the software a running project uses, saved to
        // its .cove/record.json (record.rs). Up runs it once the app answers.
        #[cfg(unix)]
        Some("record") => std::process::exit(record::cli(&args[2..])),
        // `cove setup-stack [--yes]`: mount the ndexr stack from a terminal
        // (stack.rs setup_cli) - a server's, or the installer's --stack.
        #[cfg(unix)]
        Some("setup-stack") => std::process::exit(stack::setup_cli(&args[2..])),
        Some("--connect") => match args.get(2) {
            Some(url) => {
                // The server's token (~/.config/ndexr/cove-token there):
                // `--token T`, or COVE_CONNECT_TOKEN, so it stays out of `ps`.
                let token = args.iter().position(|a| a == "--token").and_then(|i| args.get(i + 1)).cloned().or_else(|| std::env::var("COVE_CONNECT_TOKEN").ok());
                if let Some(t) = token {
                    backend::set_server_token(&t);
                }
                backend = backend::Backend::Http { base: url.trim_end_matches('/').to_owned() }
            }
            None => {
                eprintln!("--connect needs a URL, e.g. --connect http://127.0.0.1:8788");
                std::process::exit(2);
            }
        },
        _ => {}
    }
    // Where this window runs (account.rs `choose`): signed in, the person's
    // cell - on every machine, unless "this computer" was picked. A cell that
    // cannot be woken falls back here, and the app says why.
    let mut startup_note = None;
    // A server saved on this computer (account menu › Connect to a server): a
    // managed one or the person's company's. No ndexr account needed. One that
    // does not answer leaves the window here, and it says why.
    if matches!(backend, backend::Backend::Local) && account::choose() == account::RunOn::Server {
        if let Some((base, token)) = account::saved_server() {
            match account::probe_server(&base, &token) {
                Ok(()) => {
                    backend::set_server_token(&token);
                    backend = backend::Backend::Http { base };
                }
                Err(e) => startup_note = Some(format!("running on this computer: {e}")),
            }
        }
    }
    if matches!(backend, backend::Backend::Local) && account::find_key().is_some() && account::choose() == account::RunOn::Cell {
        match account::connect_cell() {
            Ok((base, session)) => backend = backend::Backend::Cell { base, session },
            Err(e) => startup_note = Some(format!("running on this computer: {e}")),
        }
    }
    // The App panel's site is a WebKitGTK view made a child of Cove's own
    // window, which wry can do only on X11. Under Wayland, winit made a
    // Wayland window, the child failed, and the panel said "This build has
    // no embedded browser - the site opens in yours" (operator's Gentoo
    // laptop, 2026-11-20: "needs to go away and the site needs to show up
    // there"). Where the session also runs XWayland (DISPLAY set - GNOME,
    // KDE and Sway all do by default), the window and WebKit both use it.
    #[cfg(all(feature = "webview", target_os = "linux"))]
    if std::env::var_os("WAYLAND_DISPLAY").is_some() && std::env::var_os("DISPLAY").is_some() && std::env::var_os("COVE_WAYLAND").is_none() {
        std::env::remove_var("WAYLAND_DISPLAY");
        std::env::set_var("GDK_BACKEND", "x11");
    }
    let options = eframe::NativeOptions {
        renderer: renderer(),
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Cove")
            .with_app_id("io.ndexr.cove")
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([860.0, 540.0])
            // The tent on its tile (assets/cove-mark.svg): the taskbar and
            // switcher showed eframe's own icon without it.
            .with_icon(eframe::icon_data::from_png_bytes(include_bytes!("../assets/cove-app-256.png")).unwrap_or_default()),
        ..Default::default()
    };
    eframe::run_native(
        "Cove",
        options,
        Box::new(move |cc| {
            let mut app = app::App::new(cc, backend, folder);
            if let Some(n) = startup_note {
                app.startup_note(n, &cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )
}

/// wgpu → Metal on macOS. On Linux, OpenGL: it runs everywhere ndexr does,
/// including VNC desktops and cluster nodes with no GPU (Mesa llvmpipe), where
/// wgpu's Vulkan path has nothing to bind to. NDEXR_RENDERER=wgpu|glow overrides.
#[cfg(not(target_arch = "wasm32"))]
fn renderer() -> eframe::Renderer {
    match std::env::var("NDEXR_RENDERER").as_deref() {
        Ok("wgpu") => eframe::Renderer::Wgpu,
        Ok("glow") => eframe::Renderer::Glow,
        _ if cfg!(target_os = "macos") => eframe::Renderer::Wgpu,
        _ => eframe::Renderer::Glow,
    }
}

/// In a browser: the page served by `cove serve` has a canvas with
/// id "ndexr"; the backend is the page's own origin.
#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().and_then(|w| w.document()).expect("no document");
        let canvas = document
            .get_element_by_id("ndexr")
            .expect("no #ndexr canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#ndexr is not a canvas");
        // The page's own folder, not only its origin: behind a prefix (a cell's
        // code-server /proxy/<port>/) /api/q is beside the page, not at /.
        let loc = web_sys::window().map(|w| w.location());
        let origin = loc.as_ref().and_then(|l| l.origin().ok()).unwrap_or_default();
        let dir = loc.as_ref().and_then(|l| l.pathname().ok()).map(|p| p[..p.rfind('/').unwrap_or(0)].to_owned()).unwrap_or_default();
        let backend = backend::Backend::Http { base: format!("{origin}{dir}") };
        // ?folder=/abs/path opens that folder as the project, like code-server's
        // https://code.ndexr.io/?folder=… (operator, 2026-10-13); the tab is
        // named for it so several folders in several tabs tell apart.
        let folder = web_sys::window()
            .and_then(|w| w.location().search().ok())
            .and_then(|s| web_sys::UrlSearchParams::new_with_str(&s).ok())
            .and_then(|p| p.get("folder"))
            .map(|f| if f.len() > 1 { f.trim_end_matches('/').to_owned() } else { f })
            .filter(|f| f.starts_with('/'));
        if let Some(f) = &folder {
            let name = f.rsplit('/').next().filter(|n| !n.is_empty()).unwrap_or("/");
            document.set_title(&format!("{name} · Cove"));
        }
        // The public view (view.rs): /view/, the page nobody signed in for.
        let public = dir == "/view";
        if public {
            document.set_title("Cove - the stack, in public");
        }
        let started = eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(move |cc| {
                    if public {
                        Ok(Box::new(view::ViewApp::new(cc, backend)) as Box<dyn eframe::App>)
                    } else {
                        Ok(Box::new(app::App::new(cc, backend, folder)) as Box<dyn eframe::App>)
                    }
                }),
            )
            .await;
        if let Err(e) = started {
            web_sys::console::error_1(&e);
        }
    });
}
