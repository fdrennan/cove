//! Where each package's machine code came from, on the stars themselves
//! (operator, 2026-10-21: the atlas is "a system for modeling scientific
//! software within an organization in order to perfectly clean and understand
//! ... the system from the bottom up and for catching and understanding the
//! use of external binaries in a regulatory setting").
//!
//! Read once per catalog from /view/provenance.json (host.rs `provenance`):
//! the site libraries' own install records - a Python wheel's tag, an R
//! package's DESCRIPTION. A binary built somewhere else and downloaded (a
//! manylinux wheel from PyPI, a package manager's R binary) is marked on its
//! star, counted in the rail, and said on its card with the evidence.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::api::Provenance;

/// Every package's provenance rows, by (language, catalog name).
#[derive(Default)]
pub struct Book {
    by: HashMap<(String, String), Vec<Provenance>>,
}

impl Book {
    pub fn from_rows(rows: Vec<Provenance>) -> Book {
        let mut by: HashMap<(String, String), Vec<Provenance>> = HashMap::new();
        for r in rows {
            by.entry((r.language.clone(), crate::api::pkg_key(&r.name))).or_default().push(r);
        }
        Book { by }
    }

    /// A package's rows (R or Python, by its catalog name).
    pub fn of(&self, language: &str, name: &str) -> &[Provenance] {
        self.by.get(&(language.to_owned(), crate::api::pkg_key(name))).map_or(&[], Vec::as_slice)
    }

    /// Came as a binary built elsewhere, in at least one set.
    pub fn external(&self, language: &str, name: &str) -> bool {
        self.of(language, name).iter().any(|r| r.how == "external")
    }

    /// What the next release does about it (binary-plan.tsv): its fate -
    /// "source", "remove", "keep", or "" when nobody has decided - and how.
    pub fn fate(&self, language: &str, name: &str) -> (String, String) {
        self.of(language, name).iter().find(|r| r.how == "external").map(|r| (r.fate.clone(), r.plan.clone())).unwrap_or_default()
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
        ehttp::fetch(ehttp::Request::get(format!("{}/provenance.json", crate::showcase::base())), move |r| {
            if let Some(rows) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice::<Vec<Provenance>>(&r.bytes).ok()) {
                *slot.lock().unwrap() = Some(Book::from_rows(rows));
                ctx2.request_repaint();
            }
        });
    }
}
