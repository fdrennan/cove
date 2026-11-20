//! Paste on a phone (operator, 2026-11-16: "I can't tap to paste in the agent
//! thread window"). egui types through one hidden input, so iOS never offers
//! its Paste menu over a Cove box. A composer draws its own Paste button and
//! `place`s web/index.html's `covePaste` button, transparent, over it every
//! frame: the tap lands on a real button, the browser lets it read the
//! clipboard, and `take` hands the text to that composer. Touch screens only
//! (`wanted`): a desk has Ctrl/⌘+V, which egui already reads.

use eframe::egui;

/// A touch screen, where the composer shows Paste.
pub fn wanted() -> bool {
    imp::call("wanted", &[]).is_some_and(|s| !s.is_empty())
}

/// Lay the real button over `rect` (egui points) for this frame.
pub fn place(ctx: &egui::Context, id: &str, rect: egui::Rect) {
    let z = ctx.zoom_factor();
    let n = |v: f32| (v * z) as f64;
    imp::place(id, [n(rect.left()), n(rect.top()), n(rect.width()), n(rect.height())]);
}

/// What was pasted into `id` since the last frame: the text, or Err(why).
pub fn take(id: &str) -> Option<Result<String, String>> {
    let t = imp::call("take", &[id])?;
    Some(match t.strip_prefix("ERROR:") {
        Some(e) => Err(e.to_owned()),
        None => Ok(t),
    })
}

/// After every frame: buttons not placed this frame step aside.
pub fn end_frame() {
    let _ = imp::call("end", &[]);
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use wasm_bindgen::{JsCast, JsValue};

    fn obj() -> Option<JsValue> {
        let w = web_sys::window()?;
        let m = js_sys::Reflect::get(&w, &"covePaste".into()).ok()?;
        (!m.is_undefined() && !m.is_null()).then_some(m)
    }

    fn func(name: &str) -> Option<(JsValue, js_sys::Function)> {
        let m = obj()?;
        let f = js_sys::Reflect::get(&m, &name.into()).ok()?.dyn_into().ok()?;
        Some((m, f))
    }

    pub fn call(name: &str, args: &[&str]) -> Option<String> {
        let (m, f) = func(name)?;
        let a: js_sys::Array = args.iter().map(|s| JsValue::from_str(s)).collect();
        f.apply(&m, &a).ok()?.as_string()
    }

    pub fn place(id: &str, r: [f64; 4]) {
        if let Some((m, f)) = func("place") {
            let a = js_sys::Array::of5(&id.into(), &r[0].into(), &r[1].into(), &r[2].into(), &r[3].into());
            let _ = f.apply(&m, &a);
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    pub fn call(_: &str, _: &[&str]) -> Option<String> {
        None
    }
    pub fn place(_: &str, _: [f64; 4]) {}
}
