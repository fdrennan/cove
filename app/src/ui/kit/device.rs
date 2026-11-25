//! What Cove is being used with, when it changes how Cove should behave
//! (operator, 2026-11-24: "meta quest is also a very real device used often
//! for cove so we need it to work well for those users and be easier for them
//! to use"). A headset's browser (Meta Quest's, and the like) is read at arm's
//! length and pointed at with a ray or a pinch: coarse, with no hover to rely
//! on, like a touch screen. Asked once; neither changes while the page lives.

/// A headset's browser: Meta Quest, Pico, Wolvic.
pub fn headset() -> bool {
    static H: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *H.get_or_init(|| imp::user_agent().is_some_and(|ua| is_headset(&ua)))
}

/// A coarse pointer - a finger, a controller's ray - where hover cannot be
/// relied on and targets must be large.
pub fn coarse() -> bool {
    static C: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *C.get_or_init(|| headset() || crate::ui::kit::paste::wanted())
}

fn is_headset(ua: &str) -> bool {
    ["OculusBrowser", "Quest", "Pico", "Wolvic", "Mobile VR"].iter().any(|k| ua.contains(k))
}

#[cfg(target_arch = "wasm32")]
mod imp {
    pub fn user_agent() -> Option<String> {
        let w = web_sys::window()?;
        let nav = js_sys::Reflect::get(&w, &"navigator".into()).ok()?;
        js_sys::Reflect::get(&nav, &"userAgent".into()).ok()?.as_string()
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    pub fn user_agent() -> Option<String> {
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_quest_browser_is_a_headset() {
        let quest = "Mozilla/5.0 (X11; Linux x86_64; Quest 3) AppleWebKit/537.36 (KHTML, like Gecko) OculusBrowser/38.1.0.1.62 SamsungBrowser/4.0 Chrome/132.0.6834.163 VR Safari/537.36";
        assert!(super::is_headset(quest));
        assert!(!super::is_headset("Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/132.0 Safari/537.36"));
        assert!(!super::is_headset("Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 Mobile/15E148 Safari/604.1"));
    }
}
