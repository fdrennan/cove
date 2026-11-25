//! Pictures for the agent (operator, 2026-10-15: "the app needs a send screen
//! shot to agent in the app panel ... chat should be able to receive pasted
//! images ... to quickly give visual information to agent with context on
//! what to fix"). Two ways in, both through the page (web/index.html
//! `window.coveShots`):
//!
//!   capture_site()   asks the App panel's site to draw itself - every ndexr
//!                    site loads javascript/preview_capture_child.js, which
//!                    answers a trusted parent (exec, admin, cove) with an
//!                    html2canvas JPEG. A cross-origin frame cannot be read
//!                    from outside; it can be asked.
//!   a paste          an image pasted anywhere in Cove (a screenshot on the
//!                    clipboard) is caught before egui sees it.
//!
//! Ide drains them every frame (`take`) and hands each to the Agents pane as
//! an attachment with a thumbnail. The desktop app has neither yet.

/// What came back from the page.
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))] // the desktop has no page
pub enum Shot {
    /// The App panel's site, as a data URL.
    Site(String),
    /// An image pasted into Cove, as a data URL.
    Pasted(String),
    /// A file picked with `pick` (Send a request's Attach files): its name
    /// and its data URL.
    File(String, String),
    Error(String),
}

#[cfg(target_arch = "wasm32")]
mod js {
    use wasm_bindgen::{JsCast, JsValue};

    pub fn shots() -> Option<JsValue> {
        let w = web_sys::window()?;
        let m = js_sys::Reflect::get(&w, &"coveShots".into()).ok()?;
        (!m.is_undefined() && !m.is_null()).then_some(m)
    }

    pub fn call(name: &str, arg: Option<&str>) -> Option<JsValue> {
        let m = shots()?;
        let f: js_sys::Function = js_sys::Reflect::get(&m, &name.into()).ok()?.dyn_into().ok()?;
        match arg {
            Some(a) => f.call1(&m, &JsValue::from_str(a)).ok(),
            None => f.call0(&m).ok(),
        }
    }
}

/// The next picture (or failure) the page has, once.
pub fn take() -> Option<Shot> {
    #[cfg(target_arch = "wasm32")]
    {
        let v = js::call("take", None)?.as_string()?;
        return Some(if let Some(e) = v.strip_prefix("ERROR:") {
            Shot::Error(e.to_owned())
        } else if let Some((n, u)) = v.strip_prefix("FILE:").and_then(|f| f.split_once('\n')) {
            Shot::File(n.to_owned(), u.to_owned())
        } else if let Some(p) = v.strip_prefix("PASTE:") {
            Shot::Pasted(p.to_owned())
        } else {
            Shot::Site(v)
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    None
}

/// Ask the page for files from the person's computer; each arrives through
/// `take` as a `Shot::File`.
pub fn pick() {
    #[cfg(target_arch = "wasm32")]
    let _ = js::call("pick", None);
}

/// A file name for a picture taken now: `screenshot-2026-10-15-1842.jpg`.
pub fn name(prefix: &str, data_url: &str) -> String {
    let ext = match data_url.split(';').next().unwrap_or("").trim_start_matches("data:") {
        "image/png" => "png",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "jpg",
    };
    #[cfg(target_arch = "wasm32")]
    let secs = (js_sys::Date::now() / 1000.0) as i64;
    #[cfg(not(target_arch = "wasm32"))]
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64);
    let days = secs.div_euclid(86400);
    let s = secs.rem_euclid(86400);
    // civil_from_days (Hinnant), as software.rs's civil
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{prefix}-{y}-{m:02}-{d:02}-{:02}{:02}{:02}.{ext}", s / 3600, (s % 3600) / 60, s % 60)
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_picture_is_named_for_its_kind() {
        let n = super::name("screenshot", "data:image/jpeg;base64,AAAA");
        assert!(n.starts_with("screenshot-20") && n.ends_with(".jpg"), "{n}");
        assert!(super::name("pasted", "data:image/png;base64,AAAA").ends_with(".png"));
    }
}
