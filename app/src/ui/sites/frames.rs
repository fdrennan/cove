//! The Sites wall's live pages, in a browser: one <iframe> per site, laid
//! over its tile's rectangle (egui draws to one canvas and cannot hold a
//! page), as ide.rs site_frame does for the App pane. A site's frame is kept
//! by its name while it is on the wall, so moving between Focus and Grid, or
//! scrolling, does not load it again; a frame not placed in a frame of the
//! UI hides, and every one hides under a dialog or a menu.

use std::cell::RefCell;
use std::collections::BTreeSet;

use eframe::egui;

const PREFIX: &str = "cove-wall-";

thread_local! {
    static PLACED: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
}

fn id_of(host: &str) -> String {
    format!("{PREFIX}{}", host.chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect::<String>())
}

fn element(id: &str) -> Option<web_sys::Element> {
    let doc = web_sys::window()?.document()?;
    if let Some(e) = doc.get_element_by_id(id) {
        return Some(e);
    }
    let e = doc.create_element("iframe").ok()?;
    e.set_id(id);
    let _ = e.set_attribute("allow", "clipboard-read; clipboard-write; fullscreen; geolocation");
    let _ = e.set_attribute("loading", "lazy");
    doc.body()?.append_child(&e).ok()?;
    Some(e)
}

/// `rect` the page at its full size, `seen` the part of it on screen: what
/// lies outside is cut away (clip-path), so a tile half scrolled out shows its
/// page as it is, not squeezed into what is left.
pub fn place(ctx: &egui::Context, host: &str, rect: egui::Rect, seen: egui::Rect, src: &str) {
    let id = id_of(host);
    let Some(e) = element(&id) else { return };
    let z = ctx.zoom_factor();
    let (top, bottom) = ((seen.top() - rect.top()).max(0.0) * z, (rect.bottom() - seen.bottom()).max(0.0) * z);
    let (left, right) = ((seen.left() - rect.left()).max(0.0) * z, (rect.right() - seen.right()).max(0.0) * z);
    let style = format!(
        "position:absolute;left:{:.1}px;top:{:.1}px;width:{:.1}px;height:{:.1}px;border:0;background:#fff;z-index:2;display:block;clip-path:inset({top:.1}px {right:.1}px {bottom:.1}px {left:.1}px round 0 0 9px 9px)",
        rect.left() * z,
        rect.top() * z,
        rect.width() * z,
        rect.height() * z
    );
    if e.get_attribute("style").as_deref() != Some(style.as_str()) {
        let _ = e.set_attribute("style", &style);
    }
    if e.get_attribute("src").as_deref() != Some(src) {
        let _ = e.set_attribute("src", src);
    }
    PLACED.with(|p| p.borrow_mut().insert(id));
}

pub fn end_frame(hide: bool) {
    let placed = PLACED.with(|p| std::mem::take(&mut *p.borrow_mut()));
    let Some(doc) = web_sys::window().and_then(|w| w.document()) else { return };
    let Ok(list) = doc.query_selector_all(&format!("iframe[id^='{PREFIX}']")) else { return };
    for i in 0..list.length() {
        let Some(n) = list.item(i) else { continue };
        let Ok(e) = wasm_bindgen::JsCast::dyn_into::<web_sys::Element>(n) else { continue };
        if !hide && placed.contains(&e.id()) {
            continue;
        }
        if e.get_attribute("style").is_none_or(|s| !s.contains("display:none")) {
            let _ = e.set_attribute("style", "display:none");
        }
    }
}
