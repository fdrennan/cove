//! Picking files on this device, for Attach, Upload and Messages, and
//! saving one there (Export).
//!
//! On the desktop it is rfd's system dialog. In a browser rfd puts up a box of
//! its own first - a white page-styled "Choose Files / Ok / Cancel" - to have
//! a click to open the picker from (operator, 2026-11-01, screenshots of it on
//! a phone). A browser lets a page open its picker shortly after a tap, and
//! egui acts on a tap within a frame, so here the phone's own picker opens at
//! once from a file input made for the purpose.

/// One file picked: its name and its bytes.
pub struct Picked {
    name: String,
    #[cfg(target_arch = "wasm32")]
    data: Vec<u8>,
    #[cfg(not(target_arch = "wasm32"))]
    handle: rfd::FileHandle,
}

impl Picked {
    pub fn file_name(&self) -> String {
        self.name.clone()
    }

    pub async fn read(&self) -> Vec<u8> {
        #[cfg(target_arch = "wasm32")]
        return self.data.clone();
        #[cfg(not(target_arch = "wasm32"))]
        return self.handle.read().await;
    }
}

/// Files picked by the person (`many` or one), None when they picked none.
/// `title` names the dialog where the system shows one.
#[cfg(not(target_arch = "wasm32"))]
pub async fn files(title: &str, many: bool) -> Option<Vec<Picked>> {
    let d = rfd::AsyncFileDialog::new().set_title(title);
    let got = if many { d.pick_files().await? } else { vec![d.pick_file().await?] };
    Some(got.into_iter().map(|h| Picked { name: h.file_name(), handle: h }).collect())
}

#[cfg(target_arch = "wasm32")]
pub async fn files(_title: &str, many: bool) -> Option<Vec<Picked>> {
    use wasm_bindgen::{closure::Closure, JsCast, JsValue};
    let doc = web_sys::window()?.document()?;
    let input: web_sys::HtmlInputElement = doc.create_element("input").ok()?.dyn_into().ok()?;
    input.set_type("file");
    input.set_multiple(many);
    let _ = input.style().set_property("display", "none");
    doc.body()?.append_child(&input).ok()?;
    // Settled by a choice ("change") or by closing the picker ("cancel").
    let settled = js_sys::Promise::new(&mut |resolve, _| {
        for event in ["change", "cancel"] {
            let r = resolve.clone();
            let cb = Closure::once_into_js(move || {
                let _ = r.call0(&JsValue::NULL);
            });
            let _ = input.add_event_listener_with_callback(event, cb.unchecked_ref());
        }
    });
    // Before anything is awaited: still within the tap that asked for it.
    input.click();
    let _ = wasm_bindgen_futures::JsFuture::from(settled).await;
    let mut out = Vec::new();
    if let Some(list) = input.files() {
        for i in 0..list.length() {
            let Some(f) = list.get(i) else { continue };
            let Ok(buf) = wasm_bindgen_futures::JsFuture::from(f.array_buffer()).await else { continue };
            out.push(Picked { name: f.name(), data: js_sys::Uint8Array::new(&buf).to_vec() });
        }
    }
    input.remove();
    (!out.is_empty()).then_some(out)
}

/// Text saved as a file the person names - an exported patch: the system's
/// save dialog on the desktop, a download in a browser. Returns at once.
pub fn save_text(name: &str, text: String) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        let name = name.to_owned();
        std::thread::spawn(move || {
            if let Some(h) = pollster::block_on(rfd::AsyncFileDialog::new().set_file_name(&name).save_file()) {
                let _ = std::fs::write(h.path(), text);
            }
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let parts = js_sys::Array::of1(&text.into());
        let Ok(blob) = web_sys::Blob::new_with_str_sequence(&parts) else { return };
        let Ok(url) = web_sys::Url::create_object_url_with_blob(&blob) else { return };
        if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
            if let Ok(a) = doc.create_element("a") {
                let _ = a.set_attribute("href", &url);
                let _ = a.set_attribute("download", name);
                if let Ok(a) = a.dyn_into::<web_sys::HtmlElement>() {
                    a.click();
                }
            }
        }
        let _ = web_sys::Url::revoke_object_url(&url);
    }
}
