//! Patched software on the stars (operator, 2026-10-22: "patched software
//! needs to be visible in the atlas"). Read once per catalog from
//! /view/patches.json (patches.rs): each module whose install recorded a
//! patch, with every patch, whose it is - the stack kit's or EasyBuild's
//! upstream recipe's - why, and its size.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::api::Patched;

/// Every patched module's versions, by its name.
#[derive(Default)]
pub struct Book {
    by: HashMap<String, Vec<Patched>>,
}

impl Book {
    fn from_rows(rows: Vec<Patched>) -> Book {
        let mut by: HashMap<String, Vec<Patched>> = HashMap::new();
        for r in rows {
            by.entry(r.name.clone()).or_default().push(r);
        }
        Book { by }
    }

    /// The patched versions of module `name`.
    pub fn of(&self, name: &str) -> &[Patched] {
        self.by.get(name).map_or(&[], Vec::as_slice)
    }

    /// Patched at all; and whether any patch is the kit's own.
    pub fn mark(&self, name: &str) -> Option<bool> {
        let v = self.of(name);
        (!v.is_empty()).then(|| v.iter().flat_map(|p| &p.patches).any(|p| p.ours))
    }
}

/// Asked once per catalog hash; the answer lands in `slot`.
#[derive(Default)]
pub struct Fetch {
    pub slot: Arc<Mutex<Option<Book>>>,
    asked: String,
}

impl Fetch {
    pub fn ask(&mut self, hash: &str, ctx: &eframe::egui::Context) {
        if self.asked == hash {
            return;
        }
        self.asked = hash.to_owned();
        let (slot, ctx2) = (self.slot.clone(), ctx.clone());
        ehttp::fetch(ehttp::Request::get(format!("{}/patches.json", crate::showcase::base())), move |r| {
            if let Some(rows) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice::<Vec<Patched>>(&r.bytes).ok()) {
                *slot.lock().unwrap() = Some(Book::from_rows(rows));
                ctx2.request_repaint();
            }
        });
    }
}

/// What a module was built from (/view/record.json), asked when its card
/// first shows, and the text of any patch someone opens to read. Clones share
/// one store, so the card's closure can hold one (operator, 2026-10-28: in
/// Cove, explore an application's upstream source, compare modifications,
/// inspect build provenance).
#[derive(Default, Clone)]
pub struct Records {
    by: Arc<Mutex<HashMap<String, Option<Vec<crate::api::BuildRecord>>>>>,
    texts: Arc<Mutex<HashMap<String, Option<String>>>>,
    /// The patch whose text is shown (`name/version/file`).
    pub open: Arc<Mutex<Option<String>>>,
}

/// A query value for a URL: everything but unreserved characters encoded, so
/// a version's `+` reaches the server as `%2B`, not a space.
fn enc(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

impl Records {
    /// Module `name`'s build records: asked once, None until they arrive.
    pub fn of(&self, name: &str, ctx: &eframe::egui::Context) -> Option<Vec<crate::api::BuildRecord>> {
        let mut by = self.by.lock().unwrap();
        if let Some(v) = by.get(name) {
            return v.clone();
        }
        by.insert(name.to_owned(), None);
        let (store, key, ctx2) = (self.by.clone(), name.to_owned(), ctx.clone());
        ehttp::fetch(ehttp::Request::get(format!("{}/record.json?name={}", crate::showcase::base(), enc(name))), move |r| {
            let rows = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice::<Vec<crate::api::BuildRecord>>(&r.bytes).ok()).unwrap_or_default();
            store.lock().unwrap().insert(key, Some(rows));
            ctx2.request_repaint();
        });
        None
    }

    /// One patch's text as the install applied it: asked once, None until it arrives.
    pub fn text(&self, name: &str, version: &str, file: &str, ctx: &eframe::egui::Context) -> Option<String> {
        let key = format!("{name}/{version}/{file}");
        let mut texts = self.texts.lock().unwrap();
        if let Some(v) = texts.get(&key) {
            return v.clone();
        }
        texts.insert(key.clone(), None);
        let (store, ctx2) = (self.texts.clone(), ctx.clone());
        let url = format!("{}/patch.txt?name={}&version={}&file={}", crate::showcase::base(), enc(name), enc(version), enc(file));
        ehttp::fetch(ehttp::Request::get(url), move |r| {
            let t = r.ok().filter(|r| r.ok).map(|r| String::from_utf8_lossy(&r.bytes).into_owned()).unwrap_or_else(|| "the patch could not be read from this machine's stack".to_owned());
            store.lock().unwrap().insert(key, Some(t));
            ctx2.request_repaint();
        });
        None
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_version_plus_survives_the_query() {
        assert_eq!(super::enc("2026.04.0+526-foss-2024a-R-4.6.1"), "2026.04.0%2B526-foss-2024a-R-4.6.1");
        assert_eq!(super::enc("a b/c"), "a%20b%2Fc");
    }
}
