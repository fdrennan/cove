//! What is typed in a message box and not sent yet, kept as it is typed
//! (operator, 2026-11-22: "between the moment prior (like writing text in a
//! thread) or uploading something ... as much information is stored in real
//! time about the session that a restart is no longer scary"). Cove's server
//! restarts with each update and the page reloads; the box comes back as it
//! was. One draft per key (a site's host, or Exec's): the browser's own
//! storage in a browser, a small file on the desktop.

/// The draft kept for `key`, or "".
pub fn load(key: &str) -> String {
    imp::load(&name(key)).unwrap_or_default()
}

/// Keep `text` for `key`; an empty one is forgotten.
pub fn save(key: &str, text: &str) {
    imp::save(&name(key), text);
}

fn name(key: &str) -> String {
    let k: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    format!("cove-draft-{k}")
}

#[cfg(target_arch = "wasm32")]
mod imp {
    fn store() -> Option<web_sys::Storage> {
        web_sys::window()?.local_storage().ok()?
    }
    pub fn load(name: &str) -> Option<String> {
        store()?.get_item(name).ok()?
    }
    pub fn save(name: &str, text: &str) {
        if let Some(s) = store() {
            let _ = if text.trim().is_empty() { s.remove_item(name) } else { s.set_item(name, text) };
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::path::PathBuf;

    fn path(name: &str) -> PathBuf {
        PathBuf::from(crate::host::home()).join(".local/state/cove/drafts").join(name)
    }
    pub fn load(name: &str) -> Option<String> {
        std::fs::read_to_string(path(name)).ok()
    }
    pub fn save(name: &str, text: &str) {
        let p = path(name);
        if text.trim().is_empty() {
            let _ = std::fs::remove_file(p);
        } else if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d).and_then(|_| std::fs::write(&p, text));
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn a_draft_comes_back_and_an_empty_one_goes() {
        let key = format!("test-{}.example.org", std::process::id());
        super::save(&key, "half a message");
        assert_eq!(super::load(&key), "half a message");
        super::save(&key, "  ");
        assert_eq!(super::load(&key), "");
    }
}
