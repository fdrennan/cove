//! Environments › a draft of a manifest (operator, 2026-11-01: "make
//! environment manifests directly editable ... editing should operate on a
//! draft of the source manifest, not directly mutate a published environment
//! ... validate changes ... review diff → approve → build → publish").
//!
//! A draft starts as a copy of the manifest's records. Packages are added,
//! dropped and pinned in it; nothing here touches the published manifest.
//! Every change is checked against the stack's own catalog - whether the
//! stack has built it, at which versions, for which machines, whether it has
//! failed, what it brings with it and what still needs a package dropped -
//! and the difference is reviewed. Propose sends the draft, as the
//! manifest.dcf the CLI reads, to the ndexr team as a request: approval,
//! the build and the publish are ndexr's (`ndexr pkgs manifest`, then a build
//! and a new immutable revision; the one it replaces stays). The draft is
//! never built or published from here.

use std::sync::{Arc, Mutex};

use eframe::egui::{self, Color32, RichText};

use super::manifests::catalog_index;
use super::{Catalog, SoftwarePane};
use crate::api::Query;
use crate::backend::Backend;
use crate::manifest::{self, Detail, Lang, Record};
use crate::theme;

#[derive(Default)]
pub struct Draft {
    /// The manifest it is a draft of.
    pub base: String,
    pub records: Vec<Record>,
    new_pkg: String,
    new_lang: Option<Lang>,
    new_ver: String,
    review: bool,
    /// What the person says about the change, sent with it.
    why: String,
    sent: Arc<Mutex<Option<Result<String, String>>>>,
}

/// How a check came out: fine, worth knowing, or in the way.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Ok,
    Note,
    Problem,
}

/// What the catalog says about one change.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub package: String,
    pub level: Level,
    pub words: String,
}

/// Every change from `base` to `draft`, checked against the catalog: an added
/// or re-pinned package the stack has built (and at which versions, for which
/// machines), or not yet; one whose builds have all failed; the packages it
/// brings with it (resolved, not asked for); and a dropped one that a kept
/// one still needs.
pub fn check(base: &[Record], draft: &[Record], cat: &Catalog) -> Vec<Finding> {
    let d = manifest::diff(base, draft);
    let mut out = Vec::new();
    let asked: Vec<&Record> = d.added.iter().chain(d.changed.iter().map(|(_, b)| b)).collect();
    for r in asked {
        let f = |level, words: String| Finding { package: r.package.clone(), level, words };
        let Some(i) = catalog_index(cat, r) else {
            let from = if r.source.is_empty() { "its source".to_owned() } else { r.source.clone() };
            out.push(f(Level::Note, format!("not built by the stack yet: the build fetches it from {from}")));
            continue;
        };
        let it = &cat.items[i];
        if !r.version.trim().is_empty() && !it.v.iter().any(|v| v == r.version.trim()) {
            let have = if it.v.is_empty() { "none".to_owned() } else { it.v.join(", ") };
            out.push(f(Level::Note, format!("{} would be a new version: the stack has built {have}", r.version.trim())));
        }
        if let Some(st) = &it.st {
            if st.ok == 0 && st.fail > 0 {
                out.push(f(Level::Problem, format!("every build of it has failed so far ({} tries)", st.fail)));
            }
        }
        if !it.a.is_empty() {
            out.push(f(Level::Ok, format!("built for {}", it.a.join(", "))));
        }
        let deps: Vec<&str> = it.dep.iter().filter_map(|&k| cat.items.get(k as usize)).map(|x| x.n.as_str()).collect();
        if !deps.is_empty() {
            let shown: Vec<&str> = deps.iter().take(8).copied().collect();
            let more = if deps.len() > shown.len() { format!(" and {} more", deps.len() - shown.len()) } else { String::new() };
            out.push(f(Level::Ok, format!("brings {} it depends on: {}{more}", deps.len(), shown.join(", "))));
        }
    }
    // A dropped package that a kept one depends on comes back as that one's
    // dependency: say so rather than let the drop look done.
    for r in &d.removed {
        let Some(gone) = catalog_index(cat, r) else { continue };
        let needs: Vec<&str> = draft
            .iter()
            .filter_map(|k| catalog_index(cat, k))
            .filter(|&i| cat.items[i].dep.contains(&(gone as u32)))
            .map(|i| cat.items[i].n.as_str())
            .take(5)
            .collect();
        if !needs.is_empty() {
            out.push(Finding { package: r.package.clone(), level: Level::Note, words: format!("still needed by {}: it comes back as their dependency", needs.join(", ")) });
        }
    }
    out
}

/// What the request says: the change, what the catalog says about it, and
/// what ndexr is asked to do with the attached manifest.
pub fn request_words(base: &str, d: &manifest::Diff, findings: &[Finding], why: &str) -> String {
    let mut w = format!("A change to the environment {base}, as a draft of its manifest (attached as manifest.dcf).\n");
    if !why.trim().is_empty() {
        w.push_str(&format!("\nWhy: {}\n", why.trim()));
    }
    let names = |rs: &mut dyn Iterator<Item = String>| rs.collect::<Vec<_>>().join(", ");
    if !d.added.is_empty() {
        w.push_str(&format!("\nAdd ({}): {}\n", d.added.len(), names(&mut d.added.iter().map(|r| if r.version.is_empty() { r.package.clone() } else { format!("{} {}", r.package, r.version) }))));
    }
    if !d.removed.is_empty() {
        w.push_str(&format!("Drop ({}): {}\n", d.removed.len(), names(&mut d.removed.iter().map(|r| r.package.clone()))));
    }
    if !d.changed.is_empty() {
        w.push_str(&format!("Version ({}): {}\n", d.changed.len(), names(&mut d.changed.iter().map(|(a, b)| format!("{} {} -> {}", b.package, or_newest(&a.version), or_newest(&b.version))))));
    }
    let worth: Vec<&Finding> = findings.iter().filter(|f| f.level != Level::Ok).collect();
    if !worth.is_empty() {
        w.push_str("\nChecked against the stack's catalog:\n");
        for f in worth {
            w.push_str(&format!("- {}: {}\n", f.package, f.words));
        }
    }
    w.push_str(&format!("\nTo build it: ndexr pkgs manifest {base} --from the attached manifest.dcf, then build and publish it as a new revision. The revision it replaces stays as it is."));
    w
}

impl Draft {
    pub fn of(d: &Detail) -> Draft {
        Draft { base: d.name.clone(), records: d.records.clone(), ..Default::default() }
    }
}

impl SoftwarePane {
    /// The draft of the manifest open: add, drop and pin; what the catalog
    /// says; the difference; and Propose. Returns true when it was discarded.
    pub(super) fn draft_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, base: &Detail, cat: Option<&Catalog>, q: &str) -> bool {
        let Some(dr) = self.man.draft.as_mut() else { return true };
        let d = manifest::diff(&base.records, &dr.records);
        let findings = cat.map(|c| check(&base.records, &dr.records, c)).unwrap_or_default();
        let changes = d.added.len() + d.removed.len() + d.changed.len();
        let mut discard = false;
        let mut propose = false;
        theme::strip(ui, 30.0, theme::surface(), |ui| {
            ui.add_space(10.0);
            ui.label(RichText::new(format!("Draft of {}", base.name)).size(13.5).strong().color(theme::text_strong()));
            ui.label(RichText::new(format!("+{}", d.added.len())).size(13.0).color(theme::ok()));
            ui.label(RichText::new(format!("−{}", d.removed.len())).size(13.0).color(theme::alt()));
            ui.label(RichText::new(format!("~{}", d.changed.len())).size(13.0).color(theme::amber()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                discard = ui.button("Discard").on_hover_text("drop the draft; the manifest was never touched").clicked();
                let review = if dr.review { "Edit" } else { "Review the change" };
                if ui.add_enabled(changes > 0, egui::Button::new(review)).clicked() {
                    dr.review = !dr.review;
                }
            });
        });
        if discard {
            self.man.draft = None;
            return true;
        }
        let sent = dr.sent.lock().unwrap().clone();
        if dr.review {
            egui::ScrollArea::vertical().id_salt("draft-review").auto_shrink([false, false]).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                    ui.label(RichText::new("The change").size(14.0).strong().color(theme::text_strong()));
                    for r in &d.added {
                        line(ui, "+", theme::ok(), &r.package, &r.language, if r.version.is_empty() { "newest".to_owned() } else { r.version.clone() });
                    }
                    for r in &d.removed {
                        line(ui, "−", theme::alt(), &r.package, &r.language, r.version.clone());
                    }
                    for (a, b) in &d.changed {
                        line(ui, "~", theme::amber(), &b.package, &b.language, format!("{} → {}", or_newest(&a.version), or_newest(&b.version)));
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("What the stack's catalog says").size(14.0).strong().color(theme::text_strong()));
                    if cat.is_none() {
                        ui.label(RichText::new("The catalog is still loading: the change is checked once it is here.").size(13.0).color(theme::text()));
                    }
                    for f in &findings {
                        let (mark, c) = match f.level {
                            Level::Ok => ("✓", theme::ok()),
                            Level::Note => ("•", theme::amber()),
                            Level::Problem => ("✕", theme::alt()),
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(mark).size(13.0).color(c));
                            ui.label(RichText::new(&f.package).monospace().size(12.5).color(theme::text_strong()));
                            ui.label(RichText::new(&f.words).size(13.0).color(theme::text_strong()));
                        });
                    }
                    ui.add_space(10.0);
                    ui.label(RichText::new("Why (sent with it)").size(13.0).color(theme::text_strong()));
                    ui.add(egui::TextEdit::multiline(&mut dr.why).hint_text("what this change is for").desired_rows(2).desired_width(f32::INFINITY));
                    ui.add_space(6.0);
                    match &sent {
                        Some(Ok(m)) => {
                            ui.label(RichText::new(format!("✓ {m}")).size(13.5).color(theme::ok()));
                        }
                        Some(Err(e)) => {
                            ui.label(RichText::new(format!("Not sent: {e}")).size(13.5).color(theme::alt()));
                        }
                        None => {}
                    }
                    let blocked = findings.iter().any(|f| f.level == Level::Problem);
                    let b = ui.add_enabled(changes > 0 && !matches!(sent, Some(Ok(_))), theme::primary_button("Propose to ndexr"));
                    if b.on_hover_text(if blocked { "it can be sent, but the ✕ above will likely fail its build" } else { "sends the draft to the ndexr team as a request: they approve, build and publish it as a new revision" }).clicked() {
                        propose = true;
                    }
                });
            });
        } else {
            draft_editor(ui, dr, cat, q);
        }
        if propose {
            let dr = self.man.draft.as_mut().expect("drawn above");
            let words = request_words(&dr.base, &d, &findings, &dr.why);
            let file = crate::api::Attachment { name: "manifest.dcf".into(), data: crate::api::b64_encode(manifest::to_dcf(&dr.records).as_bytes()) };
            let (slot, c) = (dr.sent.clone(), ctx.clone());
            *slot.lock().unwrap() = None;
            backend.query(Query::SendRequest { domain: "ndexr.ndexr.io".into(), what: words, trying: format!("change the environment {}", dr.base), blocked: false, files: vec![file] }, ctx, move |r: Result<serde_json::Value, String>| {
                *slot.lock().unwrap() = Some(r.map(|id| format!("sent to the ndexr team - request #{id} in Messages")));
                c.request_repaint();
            });
        }
        false
    }
}

fn or_newest(v: &str) -> &str {
    if v.trim().is_empty() { "newest" } else { v }
}

fn line(ui: &mut egui::Ui, mark: &str, c: Color32, package: &str, language: &str, what: String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(mark).monospace().size(13.0).color(c));
        ui.label(RichText::new(package).monospace().size(12.5).color(theme::text_strong()));
        ui.label(RichText::new(language).size(12.0).color(theme::text()));
        ui.label(RichText::new(what).monospace().size(12.0).color(theme::text_strong()));
    });
}

/// Adding a package, then every record: its version to pin (blank: the
/// newest), and ✕ to drop it. A row the catalog lacks says so.
fn draft_editor(ui: &mut egui::Ui, dr: &mut Draft, cat: Option<&Catalog>, q: &str) {
    let langs: Vec<Lang> = {
        let mut v: Vec<Lang> = dr.records.iter().map(Record::lang).collect();
        v.sort();
        v.dedup();
        if v.is_empty() { Lang::ALL.to_vec() } else { v }
    };
    let lang = *dr.new_lang.get_or_insert(langs[0]);
    let mut add = false;
    ui.horizontal_wrapped(|ui| {
        ui.add_space(10.0);
        let r = ui.add(egui::TextEdit::singleline(&mut dr.new_pkg).hint_text("add a package").desired_width(180.0));
        add |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        egui::ComboBox::from_id_salt("draft-lang").selected_text(lang.word()).width(80.0).show_ui(ui, |ui| {
            for l in Lang::ALL {
                ui.selectable_value(&mut dr.new_lang, Some(l), l.word());
            }
        });
        ui.add(egui::TextEdit::singleline(&mut dr.new_ver).hint_text("version (blank: newest)").desired_width(150.0));
        add |= ui.add_enabled(!dr.new_pkg.trim().is_empty(), egui::Button::new("Add")).clicked();
    });
    if add && !dr.new_pkg.trim().is_empty() {
        let r = Record { package: dr.new_pkg.trim().to_owned(), language: lang.word().to_owned(), version: dr.new_ver.trim().to_owned(), ..Default::default() };
        // One record per package: adding one that is there pins it instead.
        match dr.records.iter_mut().find(|x| x.lang() == r.lang() && x.key() == r.key()) {
            Some(x) => x.version = r.version,
            None => dr.records.insert(0, r),
        }
        dr.new_pkg.clear();
        dr.new_ver.clear();
    }
    let ql = q.trim().to_lowercase();
    let shown: Vec<usize> = (0..dr.records.len()).filter(|&i| ql.is_empty() || dr.records[i].package.to_lowercase().contains(&ql)).collect();
    let mut drop = None;
    let row_h = 26.0;
    egui::ScrollArea::vertical().id_salt("draft-recs").auto_shrink([false, false]).show_rows(ui, row_h, shown.len(), |ui, range| {
        for &i in &shown[range] {
            let known = cat.map(|c| catalog_index(c, &dr.records[i]).is_some());
            ui.horizontal(|ui| {
                ui.set_height(row_h);
                ui.add_space(10.0);
                let r = &mut dr.records[i];
                ui.add_sized([200.0, 18.0], egui::Label::new(RichText::new(&r.package).monospace().size(12.5).color(theme::text_strong())).truncate());
                ui.add_sized([60.0, 18.0], egui::Label::new(RichText::new(&r.language).size(12.0).color(theme::text())));
                ui.add(egui::TextEdit::singleline(&mut r.version).hint_text("newest").desired_width(110.0).font(egui::TextStyle::Monospace));
                if known == Some(false) {
                    ui.label(RichText::new("not built by the stack yet").size(12.0).color(theme::amber()));
                }
                if ui.small_button("✕").on_hover_text("drop it from the draft").clicked() {
                    drop = Some(i);
                }
            });
        }
    });
    if let Some(i) = drop {
        dr.records.remove(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Catalog {
        let j = br#"{"schema":1,"generated":0,"hash":"h","t0":1791990000,
            "boxes":[{"name":"four-box-intel","arch":"x86_64","lane":"y"}],
            "runs":[{"id":"20261014T150000Z-a","box":0,"itype":"m7i","start":0,"end":0}],
            "sets":["r4.6_py3.13-release"],
            "items":[{"k":"R","n":"ggplot2","v":["3.5.1"],"a":["x86_64","aarch64"],"dep":[1]},
                     {"k":"R","n":"scales","v":["1.3.0"],"a":["x86_64"]},
                     {"k":"R","n":"brokenpkg","st":{"n":2,"ok":0,"fail":2}}],
            "steps":{"i":[],"r":[],"t":[],"d":[],"s":"","m":[]}}"#;
        Catalog::parse(j).unwrap()
    }

    fn rec(p: &str, v: &str) -> Record {
        Record { package: p.into(), language: "R".into(), version: v.into(), ..Default::default() }
    }

    #[test]
    fn a_change_is_checked_against_the_catalog() {
        let c = cat();
        let base = vec![rec("ggplot2", ""), rec("scales", "")];
        // Pin ggplot2 to a version never built, add one the stack lacks and
        // one that has only failed, and drop scales, which ggplot2 needs.
        let draft = vec![rec("ggplot2", "4.0.0"), rec("newthing", ""), rec("brokenpkg", "")];
        let f = check(&base, &draft, &c);
        let says = |p: &str, w: &str| f.iter().any(|x| x.package == p && x.words.contains(w));
        assert!(says("ggplot2", "4.0.0 would be a new version: the stack has built 3.5.1"), "{f:?}");
        assert!(says("ggplot2", "built for x86_64, aarch64"));
        assert!(says("ggplot2", "brings 1 it depends on: scales"));
        assert!(says("newthing", "not built by the stack yet"));
        assert!(f.iter().any(|x| x.package == "brokenpkg" && x.level == Level::Problem));
        assert!(says("scales", "still needed by ggplot2"));
    }

    #[test]
    fn the_request_says_what_changes_and_what_to_run() {
        let base = vec![rec("ggplot2", "3.5.1"), rec("scales", "")];
        let draft = vec![rec("ggplot2", "4.0.0"), rec("dplyr", "")];
        let d = manifest::diff(&base, &draft);
        let w = request_words("env/R/4.6.1-bioc3.24", &d, &[], "faster joins");
        assert!(w.contains("Why: faster joins"));
        assert!(w.contains("Add (1): dplyr"));
        assert!(w.contains("Drop (1): scales"));
        assert!(w.contains("Version (1): ggplot2 3.5.1 -> 4.0.0"));
        assert!(w.contains("ndexr pkgs manifest env/R/4.6.1-bioc3.24 --from the attached manifest.dcf"));
        assert!(w.contains("The revision it replaces stays as it is."));
    }
}
