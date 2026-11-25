//! Opening a page in a new tab (operator, 2026-11-24: "the arrow that points
//! to the top right to open up the windows, they need to open up into new
//! tabs"). egui's `OpenUrl::new_tab` is `window.open(url, "_blank")` in a
//! browser, which the Quest Browser and some others turn into a separate
//! window. A click on a real `<a target="_blank">` is a link, and every browser
//! opens a link in a tab. egui runs a click's frame inside the browser's own
//! click event, so the link's click counts as the person's.

use eframe::egui;

/// Open `url` in a new tab: a browser tab in the browser build, the system's
/// browser on the desktop.
pub fn new_tab(ctx: &egui::Context, url: impl ToString) {
    let url = url.to_string();
    if !imp::link(&url) {
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use wasm_bindgen::JsCast;

    pub fn link(url: &str) -> bool {
        let go = || -> Option<()> {
            let doc = web_sys::window()?.document()?;
            let a: web_sys::HtmlElement = doc.create_element("a").ok()?.dyn_into().ok()?;
            a.set_attribute("href", url).ok()?;
            a.set_attribute("target", "_blank").ok()?;
            a.set_attribute("rel", "noopener").ok()?;
            a.style().set_property("display", "none").ok()?;
            let body = doc.body()?;
            body.append_child(&a).ok()?;
            a.click();
            body.remove_child(&a).ok()?;
            Some(())
        };
        go().is_some()
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    pub fn link(_: &str) -> bool {
        false
    }
}
