//! Link your Claude organization (Anthropic Linked billing, operator
//! 2026-11-27): the person's Claude organization pays for their Claude use
//! through ndexr, instead of a key or a token pasted into Cove. The link
//! lives at auth.ndexr.io (ndexr.ndexr.io crate/src/bin/auth_svc_claude.rs):
//!
//!   GET  https://auth.ndexr.io/claude/connect?next=<url>  Claude's approval,
//!        then back to <url> with ?claude=linked or ?claude=<error>
//!   GET  https://auth.ndexr.io/claude/status      {signed_in, linked, linked_at, ...}
//!   POST https://auth.ndexr.io/claude/disconnect  revoke at Claude, then forget
//!
//! All three read the .ndexr.io ndx_token cookie, so only a page on an ndexr
//! host can use them, asked from the browser with its cookies (ehttp sends
//! none). A self-installed Cove, or the desktop app, has no ndexr sign-in to
//! link: the setup step does not offer it there.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, RichText};

use crate::ui::kit::theme;

const AUTH: &str = "https://auth.ndexr.io/claude";

#[derive(Clone, Default, serde::Deserialize)]
struct LinkView {
    #[serde(default)]
    signed_in: bool,
    #[serde(default)]
    linked: bool,
    #[serde(default)]
    linked_at: Option<f64>,
}

type Slot<T> = Arc<Mutex<Option<T>>>;

#[derive(Default)]
pub struct ClaudeLink {
    got: Slot<Result<LinkView, String>>,
    asked: bool,
    busy: Arc<Mutex<bool>>,
    /// What the return from Claude's approval said (?claude=...), once.
    returned: Option<Option<String>>,
}

/// Linking is offered: Cove is a page on an ndexr host, signed in to ndexr.
pub fn available() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window().and_then(|w| w.location().hostname().ok()).is_some_and(|h| h == "ndexr.io" || h.ends_with(".ndexr.io"))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
}

impl ClaudeLink {
    fn ask(&mut self, ctx: &egui::Context) {
        self.asked = true;
        let (got, busy, ctx) = (self.got.clone(), self.busy.clone(), ctx.clone());
        call("GET", "status", move |r| {
            *got.lock().unwrap() = Some(r.and_then(|v| serde_json::from_value(v).map_err(|e| e.to_string())));
            *busy.lock().unwrap() = false;
            ctx.request_repaint();
        });
    }

    /// The setup step's row: where the link stands, and Link or Disconnect.
    pub fn show(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.returned.is_none() {
            self.returned = Some(returned());
        }
        if !self.asked {
            self.ask(ctx);
        }
        let busy = *self.busy.lock().unwrap();
        let got = self.got.lock().unwrap().clone();
        match (&self.returned, &got) {
            (Some(Some(w)), _) if w == "linked" => note(ui, true, "Claude approved the link."),
            (Some(Some(w)), _) => note(ui, false, &format!("The link did not finish: {}.", words(w))),
            _ => {}
        }
        match got {
            None => {
                ui.spinner();
            }
            Some(Err(e)) => note(ui, false, &format!("Could not ask auth.ndexr.io: {e}")),
            Some(Ok(v)) if !v.signed_in => note(ui, false, "Sign in to ndexr first, then link your organization."),
            Some(Ok(v)) if v.linked => {
                let since = v.linked_at.map(|t| format!(" since {} UTC", crate::ui::software::fmt_date(t as i64))).unwrap_or_default();
                note(ui, true, &format!("Your Claude organization is linked{since}."));
                if ui.add_enabled(!busy, egui::Button::new(RichText::new(if busy { "Disconnecting…" } else { "Disconnect" }).color(theme::alt())))
                    .on_hover_text("revoke the link at Claude: your organization stops paying through ndexr")
                    .clicked()
                {
                    *self.busy.lock().unwrap() = true;
                    let (got, busy, ctx2) = (self.got.clone(), self.busy.clone(), ctx.clone());
                    call("POST", "disconnect", move |r| {
                        *got.lock().unwrap() = Some(r.and_then(|v| serde_json::from_value(v).map(|l: LinkView| LinkView { signed_in: true, ..l }).map_err(|e| e.to_string())));
                        *busy.lock().unwrap() = false;
                        ctx2.request_repaint();
                    });
                }
            }
            Some(Ok(_)) => {
                if ui.add(theme::primary_button("Link your Claude organization")).on_hover_text("Claude asks an admin of your organization to approve, then brings you back here").clicked() {
                    connect();
                }
            }
        }
    }
}

/// What a `?claude=` error code means, in words.
fn words(code: &str) -> String {
    match code {
        "access_denied" => "the approval was declined".into(),
        "user_profile_already_linked" => "that Claude organization is already linked to another ndexr account".into(),
        "server_error" => "ndexr could not finish it - try again".into(),
        c => c.replace('_', " "),
    }
}

fn note(ui: &mut egui::Ui, good: bool, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(12.5).color(if good { theme::ok() } else { theme::warn() })).wrap());
}

/// `?claude=` on this page's address: the return from Claude's approval.
fn returned() -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let search = web_sys::window()?.location().search().ok()?;
        web_sys::UrlSearchParams::new_with_str(&search).ok()?.get("claude")
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// Go to Claude's approval, coming back to this page.
fn connect() {
    #[cfg(target_arch = "wasm32")]
    if let Some(w) = web_sys::window() {
        let here = w.location().href().unwrap_or_default();
        let next = js_sys::encode_uri_component(&here).as_string().unwrap_or_default();
        let _ = w.location().set_href(&format!("{AUTH}/connect?next={next}"));
    }
}

/// One call to auth.ndexr.io with the page's cookies; `done` gets its JSON.
fn call(method: &'static str, path: &'static str, done: impl FnOnce(Result<serde_json::Value, String>) + 'static) {
    #[cfg(target_arch = "wasm32")]
    wasm_bindgen_futures::spawn_local(async move {
        done(fetch(method, path).await);
    });
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = (method, path, AUTH);
        done(Err("linking runs in the browser only".into()));
    }
}

#[cfg(target_arch = "wasm32")]
async fn fetch(method: &str, path: &str) -> Result<serde_json::Value, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    let js = |e: wasm_bindgen::JsValue| e.as_string().unwrap_or_else(|| format!("{e:?}"));
    let opts = web_sys::RequestInit::new();
    opts.set_method(method);
    opts.set_credentials(web_sys::RequestCredentials::Include);
    let req = web_sys::Request::new_with_str_and_init(&format!("{AUTH}/{path}"), &opts).map_err(js)?;
    let w = web_sys::window().ok_or("no window")?;
    let resp: web_sys::Response = JsFuture::from(w.fetch_with_request(&req)).await.map_err(js)?.dyn_into().map_err(js)?;
    let text = JsFuture::from(resp.text().map_err(js)?).await.map_err(js)?.as_string().unwrap_or_default();
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| format!("HTTP {}", resp.status()))?;
    if !resp.ok() {
        return Err(v["error"].as_str().map(str::to_owned).unwrap_or_else(|| format!("HTTP {}", resp.status())));
    }
    Ok(v)
}

#[cfg(test)]
mod tests {
    #[test]
    fn errors_read_as_words() {
        assert_eq!(super::words("access_denied"), "the approval was declined");
        assert!(super::words("user_profile_already_linked").contains("another ndexr account"));
        assert_eq!(super::words("invalid_request"), "invalid request");
    }
}
