//! __HOST__ - a Rust app, started from Cove's template.
//!
//!   app serve [port]   the site: API + media work + the browser UI (PORT, default 8800)
//!   app [url]          the same UI as a desktop window, talking to a server
//!                      (APP_URL, default http://127.0.0.1:8800)

mod api;
mod app;
mod atlas;
mod chaos;
#[cfg(not(target_arch = "wasm32"))]
mod catalog;
#[cfg(not(target_arch = "wasm32"))]
mod media;
#[cfg(not(target_arch = "wasm32"))]
mod server;

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("serve") {
        let port = args.get(1).cloned().or_else(|| std::env::var("PORT").ok()).and_then(|p| p.parse().ok()).unwrap_or(8800);
        server::serve(port);
        return Ok(());
    }
    let base = args.first().cloned().or_else(|| std::env::var("APP_URL").ok()).unwrap_or_else(|| "http://127.0.0.1:8800".into());
    let base = base.trim_end_matches('/').to_owned();
    eframe::run_native(
        "__HOST__",
        eframe::NativeOptions { viewport: eframe::egui::ViewportBuilder::default().with_inner_size([1100.0, 700.0]), ..Default::default() },
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, base)))),
    )
}

/// In a browser: the page's canvas #app; the server is the page's own folder,
/// so the app works at / and behind a prefix alike.
#[cfg(target_arch = "wasm32")]
fn main() {
    use wasm_bindgen::JsCast;
    wasm_bindgen_futures::spawn_local(async {
        let window = web_sys::window().expect("no window");
        let document = window.document().expect("no document");
        let canvas = document.get_element_by_id("app").expect("no #app canvas").dyn_into::<web_sys::HtmlCanvasElement>().expect("#app is not a canvas");
        let loc = window.location();
        let origin = loc.origin().unwrap_or_default();
        let path = loc.pathname().unwrap_or_default();
        let base = format!("{origin}{}", &path[..path.rfind('/').unwrap_or(0)]);
        let started = eframe::WebRunner::new()
            .start(canvas, eframe::WebOptions::default(), Box::new(move |cc| Ok(Box::new(app::App::new(cc, base)) as Box<dyn eframe::App>)))
            .await;
        if let Some(l) = document.get_element_by_id("loading") {
            match started {
                Ok(()) => l.remove(),
                Err(e) => l.set_inner_html(&format!("The app did not start: {e:?}")),
            }
        }
    });
}
