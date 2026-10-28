//! Packages › Manifests, and the manifest filter over Find and Tree.
//!
//! Operator, 2026-10-15: "the different manifests really do become an
//! important filter and viewer in the software sections and where they show
//! up, the reason being is that we need to create manifests of combinations of
//! language packages for different purposes on the stack we have built
//! below". Chosen the same day: a manifest filter on Packages, a Manifests
//! view, and the Install tab picking by manifest (install.rs).
//!
//!   Manifests  every env manifest - its family (R + Python, R, Python, a
//!              purpose), the module it is used through and the modules it
//!              builds on (set.json), its R and Python record counts, and what
//!              this machine has built of it (`ndexr pkgs status`). One picked
//!              shows its records (Package, Language, Source, Version, Group),
//!              searchable; "compare with" shows what a second one adds, drops
//!              and moves.
//!   filter     one or several manifests picked in the chip row restrict Find
//!              and Tree to their records - R and Python by name (Python as
//!              pip normalizes it), EasyBuild by the modules the set loads.
//!
//! Everything comes from the machine (manifest.rs host, api.rs
//! Query::Manifests / Query::Manifest): fetched once, a manifest's records
//! when it is first opened or filtered by, kept for the session.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use eframe::egui::{self, Align, Color32, FontId, Layout, RichText, Sense, Stroke};

use super::{chip, region, segment, stat, stat_c, thousands, Catalog, Kind, SoftwarePane, View};
use crate::api::Query;
use crate::backend::Backend;
use crate::manifest::{self, count_words, Detail, Lang, Listing, Record, Summary};
use crate::theme;

type Slot<T> = Arc<Mutex<Option<Result<T, String>>>>;

/// What a filter lets through: R, Python and Rust records by (language, key),
/// and the modules the sets load.
pub struct Allowed {
    names: HashSet<(Lang, String)>,
    modules: HashSet<String>,
}

/// Whether catalog row `i` is in the filter's manifests.
pub fn admits(a: &Allowed, cat: &Catalog, i: usize) -> bool {
    let it = &cat.items[i];
    match cat.kind[i] {
        Kind::R => a.names.contains(&(Lang::R, cat.lname[i].clone())),
        Kind::Python => a.names.contains(&(Lang::Python, manifest::py_norm(&it.n))),
        Kind::Rust => a.names.contains(&(Lang::Rust, manifest::crate_norm(&it.n))),
        Kind::Julia => a.names.contains(&(Lang::Julia, cat.lname[i].clone())),
        Kind::EasyBuild | Kind::Env => a.modules.contains(&it.m.to_lowercase()) || a.modules.contains(&cat.lname[i]),
        Kind::Prefix | Kind::Other => false,
    }
}

#[derive(Default)]
pub struct Man {
    list: Slot<Listing>,
    asked: bool,
    /// When the list was last asked: while a set is busy it is asked again,
    /// or a set seen mid-sync said "building here" for the rest of the
    /// session (operator screenshot, 2026-10-19, R/4.5.3-bioc3.22).
    listed_at: f64,
    details: Arc<Mutex<HashMap<String, Result<Arc<Detail>, String>>>>,
    asking: HashSet<String>,
    /// The manifest open in the view, and the one it is compared with.
    sel: Option<String>,
    compare: Option<String>,
    query: String,
    /// 0 every record, 1 R, 2 Python.
    lang: u8,
    /// Every set the kit carries, not only the kept ones and the new
    /// EasyBuild-style ones.
    pub show_all: bool,
    /// The manifests Find and Tree are restricted to.
    pub filter: Vec<String>,
}

impl Man {
    pub fn load(&mut self, backend: &Backend, ctx: &egui::Context) {
        if self.asked {
            return;
        }
        self.asked = true;
        self.listed_at = ctx.input(|i| i.time);
        let slot = self.list.clone();
        backend.query(Query::Manifests, ctx, move |r: Result<Listing, String>| {
            *slot.lock().unwrap() = Some(r);
        });
    }

    /// Ask for the list again every 15 s while any set is busy; the details
    /// already read stay.
    fn follow_busy(&mut self, backend: &Backend, ctx: &egui::Context) {
        let busy = matches!(&*self.list.lock().unwrap(), Some(Ok(l)) if l.sets.iter().any(|s| s.built.as_ref().is_some_and(|b| b.busy)));
        if !busy {
            return;
        }
        let now = ctx.input(|i| i.time);
        ctx.request_repaint_after(std::time::Duration::from_secs(15));
        if now - self.listed_at < 15.0 {
            return;
        }
        self.listed_at = now;
        let slot = self.list.clone();
        backend.query(Query::Manifests, ctx, move |r: Result<Listing, String>| {
            // A failed re-ask keeps what is shown.
            if r.is_ok() {
                *slot.lock().unwrap() = Some(r);
            }
        });
    }

    fn reload(&mut self, backend: &Backend, ctx: &egui::Context) {
        *self.list.lock().unwrap() = None;
        self.details.lock().unwrap().clear();
        self.asking.clear();
        self.asked = false;
        self.load(backend, ctx);
    }

    pub fn listing(&self) -> Option<Result<Listing, String>> {
        self.list.lock().unwrap().clone()
    }

    /// The sets shown by default: the kept combinations and named sets, and
    /// every EasyBuild-style one; all of them with `show_all`.
    pub fn shown<'a>(&self, l: &'a Listing) -> Vec<&'a Summary> {
        l.sets.iter().filter(|s| self.show_all || s.kept).collect()
    }

    /// Ask for a manifest's records unless they are here or on their way.
    pub fn want(&mut self, name: &str, backend: &Backend, ctx: &egui::Context) {
        if self.details.lock().unwrap().contains_key(name) || !self.asking.insert(name.to_owned()) {
            return;
        }
        let (slot, n) = (self.details.clone(), name.to_owned());
        backend.query(Query::Manifest { name: n.clone() }, ctx, move |r: Result<Detail, String>| {
            slot.lock().unwrap().insert(n, r.map(Arc::new));
        });
    }

    pub fn detail(&self, name: &str) -> Option<Result<Arc<Detail>, String>> {
        self.details.lock().unwrap().get(name).cloned()
    }

    pub fn fetch_filter(&mut self, backend: &Backend, ctx: &egui::Context) {
        for n in self.filter.clone() {
            self.want(&n, backend, ctx);
        }
    }

    /// What the results were built for: the picked manifests, and how many
    /// of them have arrived (the results change when one does).
    pub fn filter_sig(&self) -> String {
        if self.filter.is_empty() {
            return String::new();
        }
        let have = self.filter.iter().filter(|n| matches!(self.detail(n), Some(Ok(_)))).count();
        format!("{}#{have}", self.filter.join(","))
    }

    /// The filter, once every picked manifest's records are here; None is no
    /// filter (none picked, or still arriving - the line under the chips says).
    pub fn allowed(&self) -> Option<Allowed> {
        if self.filter.is_empty() {
            return None;
        }
        let mut a = Allowed { names: HashSet::new(), modules: HashSet::new() };
        for n in &self.filter {
            let Some(Ok(d)) = self.detail(n) else { return None };
            for r in &d.records {
                a.names.insert((r.lang(), r.key()));
            }
            a.modules.insert(d.env_module.to_lowercase());
            for m in &d.modules {
                a.modules.insert(m.to_lowercase());
            }
        }
        Some(a)
    }

    /// The chip row's picker: "manifest: any ▾", or the ones picked.
    pub fn filter_picker(&mut self, ui: &mut egui::Ui) {
        let label = match self.filter.len() {
            0 => "manifest: any ▾".to_owned(),
            1 => format!("manifest: {} ▾", self.filter[0]),
            n => format!("manifests: {} ▾", n),
        };
        let lit = !self.filter.is_empty();
        let loading = lit && self.allowed().is_none() && self.filter.iter().all(|n| !matches!(self.detail(n), Some(Err(_))));
        let tip = if lit {
            format!("only what {} list{}{}", self.filter.join(" + "), if self.filter.len() == 1 { "s" } else { "" }, if loading { " (reading them…)" } else { "" })
        } else {
            "restrict Find and Tree to one or several env manifests' records".to_owned()
        };
        let listing = self.listing();
        let mut show_all = self.show_all;
        ui.menu_button(RichText::new(label).size(11.0).color(if lit { theme::amber() } else { theme::muted() }), |ui| {
            // Wide enough for a manifest's name, on one line: the popup took
            // the button's width and wrapped every name a letter a line
            // (operator, 2026-10-15: "manifest is hard to read").
            ui.set_min_width(340.0);
            ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
            ui.set_max_height(440.0);
            if lit && ui.button("any manifest (clear)").clicked() {
                self.filter.clear();
            }
            match &listing {
                None => {
                    ui.spinner();
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e).size(11.0).color(theme::alt()));
                }
                Some(Ok(l)) => {
                    ui.separator();
                    egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                        let picked = self.filter.clone();
                        for s in l.sets.iter().filter(|s| show_all || s.kept || picked.contains(&s.name)) {
                            let mut on = self.filter.contains(&s.name);
                            let changed = ui
                                .horizontal(|ui| {
                                    let c = ui.checkbox(&mut on, RichText::new(&s.name).monospace().size(11.5).color(theme::text())).changed();
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.label(RichText::new(count_words(s.counts())).size(10.5).color(theme::hush()));
                                    });
                                    c
                                })
                                .inner;
                            if changed {
                                if on {
                                    self.filter.push(s.name.clone());
                                } else {
                                    self.filter.retain(|x| x != &s.name);
                                }
                            }
                        }
                    });
                    let hidden = l.sets.iter().filter(|s| !s.kept).count();
                    if hidden > 0 {
                        ui.separator();
                        ui.checkbox(&mut show_all, RichText::new(format!("every set the kit carries (+{hidden})")).size(11.0));
                    }
                }
            }
        })
        .response
        .on_hover_text(tip);
        self.show_all = show_all;
        if loading {
            ui.spinner();
        }
    }
}

/// "r4.6_py3.13-release" reads as itself; a set.json's modules as words.
fn modules_line(s: &Summary) -> String {
    let mut m = vec![s.env_module.clone()];
    m.extend(s.modules.iter().cloned());
    if !s.bioc.is_empty() {
        m.push(format!("Bioconductor {}", s.bioc));
    }
    m.join(" · ")
}

/// The catalog kind a manifest language's records are.
pub(super) fn lang_kind(l: Lang) -> Kind {
    match l {
        Lang::R => Kind::R,
        Lang::Python => Kind::Python,
        Lang::Rust => Kind::Rust,
        Lang::Julia => Kind::Julia,
    }
}

fn built_words(s: &Summary) -> (String, Color32) {
    match &s.built {
        Some(b) if b.busy && b.running > 0 => (format!("building here · {} running · {}", b.running, count_words(b.counts())), theme::amber()),
        // The lock with no build step running: a sync, or a run that is ending.
        Some(b) if b.busy => (format!("in use here · {}", count_words(b.counts())), theme::amber()),
        Some(b) if b.failed > 0 => (format!("built here · {} · {} failed", count_words(b.counts()), b.failed), theme::alt()),
        Some(b) => (format!("built here · {}", count_words(b.counts())), theme::ok()),
        None => ("not built here".into(), theme::hush()),
    }
}

fn family_color(f: &str) -> Color32 {
    match f {
        "R" => Kind::R.color(),
        "Python" => Kind::Python.color(),
        "Rust" => Kind::Rust.color(),
        "Julia" => Kind::Julia.color(),
        "R + Python" => theme::primary(),
        _ => theme::amber(),
    }
}

fn record_matches(r: &Record, q: &str, lang: u8) -> bool {
    (lang == 0 || Lang::ALL.get(lang as usize - 1) == Some(&r.lang()))
        && (q.is_empty() || r.package.to_lowercase().contains(q) || r.group.to_lowercase().contains(q) || r.source.to_lowercase().contains(q) || r.description.to_lowercase().contains(q))
}

impl SoftwarePane {
    /// Packages › Manifests. `cat` (when it has arrived) lets a record open
    /// its software.
    pub(super) fn manifests_view(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, backend: &Backend, cat: Option<&Catalog>) {
        self.man.load(backend, ctx);
        self.man.follow_busy(backend, ctx);
        let mut reload = false;
        theme::strip(ui, 24.0, theme::bg(), |ui| {
            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.x = 4.0;
            for (v, name) in [(0u8, "all records"), (1, "R"), (2, "Python"), (3, "Rust"), (4, "Julia")] {
                if segment(ui, name, "which records the list and the comparison show", self.man.lang == v).clicked() {
                    self.man.lang = v;
                }
            }
            theme::tool_sep(ui);
            if theme::tool_if(ui, self.man.show_all, "every kit set", "also the sets the kit carries that were not kept (the kept: the CLI's pkgs.sets - the R, Python and Rust language sets)").clicked() {
                self.man.show_all = !self.man.show_all;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_space(4.0);
                reload = theme::tool(ui, "⟳", "ask the ndexr CLI again").clicked();
                let w = ui.available_width().clamp(80.0, 320.0);
                ui.add(egui::TextEdit::singleline(&mut self.man.query).hint_text("search manifests and records…").desired_width(w));
            });
        });
        if reload {
            self.man.reload(backend, ctx);
        }
        let l = match self.man.listing() {
            None => {
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spinner();
                    ui.label(RichText::new("asking the ndexr CLI for its manifests…").color(theme::muted()));
                });
                return;
            }
            Some(Err(e)) => {
                egui::Frame::new().inner_margin(egui::Margin::same(10)).show(ui, |ui| {
                    ui.label(RichText::new(e).color(theme::alt()));
                });
                return;
            }
            Some(Ok(l)) => l,
        };
        if self.man.sel.is_none() {
            self.man.sel = self.man.shown(&l).first().map(|s| s.name.clone());
        }
        if let Some(n) = self.man.sel.clone() {
            self.man.want(&n, backend, ctx);
        }
        if let Some(n) = self.man.compare.clone() {
            self.man.want(&n, backend, ctx);
        }

        let rect = ui.available_rect_before_wrap();
        ui.allocate_rect(rect, Sense::hover());
        let wide = rect.width() >= 720.0;
        let (left, right) = if wide {
            let w = (rect.width() * 0.36).clamp(260.0, 420.0);
            (rect.with_max_x(rect.left() + w), rect.with_min_x(rect.left() + w + 1.0))
        } else {
            let h = (rect.height() * 0.38).max(140.0);
            (rect.with_max_y(rect.top() + h), rect.with_min_y(rect.top() + h + 1.0))
        };
        if wide {
            ui.painter().vline(right.left() - 0.5, rect.y_range(), Stroke::new(1.0, theme::pane_brd()));
        } else {
            ui.painter().hline(rect.x_range(), right.top() - 0.5, Stroke::new(1.0, theme::pane_brd()));
        }
        let q = self.man.query.trim().to_lowercase();
        region(ui, left, |ui| self.manifest_list(ui, &l, &q));
        region(ui, right, |ui| self.manifest_detail(ui, &l, &q, cat));
    }

    fn manifest_list(&mut self, ui: &mut egui::Ui, l: &Listing, q: &str) {
        let shown: Vec<Summary> = self.man.shown(l).into_iter().filter(|s| q.is_empty() || s.name.to_lowercase().contains(q) || s.env_module.to_lowercase().contains(q)).cloned().collect();
        theme::strip(ui, 20.0, theme::surface(), |ui| {
            ui.add_space(8.0);
            let words = format!("{} manifests · ndexr {} · {}", shown.len(), if l.cli.is_empty() { "?" } else { &l.cli }, l.triple);
            ui.label(RichText::new(words).size(10.5).color(theme::hush()));
        });
        if !l.note.is_empty() {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(8, 4)).show(ui, |ui| {
                ui.label(RichText::new(&l.note).size(10.5).color(theme::amber()));
            });
        }
        let row_h = 40.0;
        let mut picked = None;
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::ScrollArea::vertical().id_salt("mf-list").auto_shrink([false, false]).show_rows(ui, row_h, shown.len(), |ui, range| {
            for s in &shown[range] {
                let (r, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
                let on = self.man.sel.as_deref() == Some(&s.name);
                let cmp = self.man.compare.as_deref() == Some(&s.name);
                let p = ui.painter_at(r);
                if on {
                    p.rect_filled(r, 0.0, theme::surface_hi());
                    p.vline(r.left() + 1.0, r.y_range(), Stroke::new(2.0, theme::amber()));
                } else if resp.hovered() {
                    p.rect_filled(r, 0.0, theme::surface_hi().gamma_multiply(0.6));
                }
                if cmp {
                    p.vline(r.left() + 1.0, r.y_range(), Stroke::new(2.0, theme::net()));
                }
                let fam = manifest::family(&s.name);
                let fc = family_color(fam);
                p.circle_filled(egui::pos2(r.left() + 12.0, r.top() + 13.0), 4.0, fc);
                p.text(egui::pos2(r.left() + 22.0, r.top() + 13.0), egui::Align2::LEFT_CENTER, &s.name, FontId::monospace(12.0), theme::text_strong());
                p.text(egui::pos2(r.right() - 8.0, r.top() + 13.0), egui::Align2::RIGHT_CENTER, count_words(s.counts()), FontId::proportional(11.0), theme::muted());
                let (bw, bc) = built_words(s);
                p.text(egui::pos2(r.left() + 22.0, r.top() + 29.0), egui::Align2::LEFT_CENTER, format!("{fam} · {}", if s.from == "repo" { "repo.ndexr.io" } else { "kit" }), FontId::proportional(10.5), fc.gamma_multiply(0.85));
                p.text(egui::pos2(r.right() - 8.0, r.top() + 29.0), egui::Align2::RIGHT_CENTER, bw, FontId::proportional(10.5), bc);
                if resp.on_hover_text(modules_line(s)).clicked() {
                    picked = Some(s.name.clone());
                }
            }
        });
        if let Some(n) = picked {
            if self.man.compare.as_deref() == Some(&n) {
                self.man.compare = None;
            }
            self.man.sel = Some(n);
        }
    }

    fn manifest_detail(&mut self, ui: &mut egui::Ui, l: &Listing, q: &str, cat: Option<&Catalog>) {
        let Some(name) = self.man.sel.clone() else {
            ui.add_space(14.0);
            ui.vertical_centered(|ui| ui.label(RichText::new("Pick a manifest.").color(theme::hush())));
            return;
        };
        let summary = l.sets.iter().find(|s| s.name == name).cloned().unwrap_or_default();
        let mut go_find = false;
        let mut go_install = false;
        let mut compare = self.man.compare.clone();
        egui::Frame::new().inner_margin(egui::Margin { left: 10, right: 10, top: 8, bottom: 4 }).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&name).font(theme::display(15.0)).color(theme::text_strong()));
                let fam = manifest::family(&name);
                chip(ui, fam, family_color(fam), false);
                if summary.kept {
                    chip(ui, "kept", theme::ok(), false).on_hover_text("in the CLI's pkgs.sets: the combinations the operator kept (2026-10-15) and the named sets");
                }
            });
            ui.label(RichText::new(modules_line(&summary)).monospace().size(11.0).color(theme::muted()));
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                // Its own languages only: an R set has no "0 Python records".
                let langs = summary.counts();
                for (l, n) in Lang::ALL.iter().zip(langs).filter(|(_, n)| *n > 0) {
                    stat(ui, &thousands(n), &format!("{} records", l.word()));
                }
                if langs.iter().all(|n| *n == 0) {
                    stat(ui, "0", "records");
                }
                let (bw, bc) = built_words(&summary);
                stat_c(ui, bw.split(" · ").next().unwrap_or(""), "on this machine", bc);
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("filter Packages by it").on_hover_text("Find and Tree show only this manifest's records").clicked() {
                    go_find = true;
                }
                if ui.button("install it…").on_hover_text("Software › Install, with this one picked").clicked() {
                    go_install = true;
                }
                let now = compare.clone().unwrap_or_else(|| "nothing".into());
                egui::ComboBox::from_id_salt("mf-compare").selected_text(RichText::new(format!("compare with: {now}")).size(11.5)).width(240.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut compare, None, "nothing");
                    for s in self.man.shown(l) {
                        if s.name != name {
                            ui.selectable_value(&mut compare, Some(s.name.clone()), &s.name);
                        }
                    }
                });
            });
        });
        self.man.compare = compare;
        if go_find {
            self.man.filter = vec![name.clone()];
            self.view = View::Find;
        }
        if go_install {
            self.inst.picks.insert(name.clone());
            self.last[0] = View::Manifests;
            self.view = View::Install;
        }
        let d = match self.man.detail(&name) {
            None => {
                ui.horizontal(|ui| {
                    ui.add_space(10.0);
                    ui.spinner();
                    ui.label(RichText::new("reading its records…").color(theme::muted()));
                });
                return;
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(theme::alt()));
                return;
            }
            Some(Ok(d)) => d,
        };
        let lang = self.man.lang;
        if let Some(other) = self.man.compare.clone() {
            match self.man.detail(&other) {
                Some(Ok(o)) => self.compare_view(ui, &d, &o, q, lang),
                Some(Err(e)) => {
                    ui.label(RichText::new(e).color(theme::alt()));
                }
                None => {
                    ui.spinner();
                }
            }
            return;
        }
        let recs: Vec<&Record> = d.records.iter().filter(|r| record_matches(r, q, lang)).collect();
        if let Some(i) = record_table(ui, "mf-recs", &recs, cat) {
            self.open(i);
            self.view = View::Find;
        }
    }

    fn compare_view(&mut self, ui: &mut egui::Ui, a: &Detail, b: &Detail, q: &str, lang: u8) {
        let ra: Vec<Record> = a.records.iter().filter(|r| record_matches(r, q, lang)).cloned().collect();
        let rb: Vec<Record> = b.records.iter().filter(|r| record_matches(r, q, lang)).cloned().collect();
        let d = manifest::diff(&ra, &rb);
        theme::strip(ui, 22.0, theme::surface(), |ui| {
            ui.add_space(10.0);
            ui.label(RichText::new(format!("{} → {}", a.name, b.name)).monospace().size(11.5).color(theme::text()));
            theme::tool_sep(ui);
            ui.label(RichText::new(format!("+{} added", d.added.len())).size(11.0).color(theme::ok()));
            ui.label(RichText::new(format!("−{} dropped", d.removed.len())).size(11.0).color(theme::alt()));
            ui.label(RichText::new(format!("~{} moved", d.changed.len())).size(11.0).color(theme::amber()));
            ui.label(RichText::new(format!("{} the same", d.same)).size(11.0).color(theme::hush()));
        });
        egui::ScrollArea::vertical().id_salt("mf-diff").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(10, 6)).show(ui, |ui| {
                let line = |ui: &mut egui::Ui, mark: &str, c: Color32, r: &Record, v: String| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(mark).monospace().color(c));
                        ui.label(RichText::new(&r.package).monospace().size(12.0).color(theme::text_strong()));
                        ui.label(RichText::new(&r.language).size(10.5).color(lang_kind(r.lang()).color()));
                        ui.label(RichText::new(v).monospace().size(11.0).color(theme::muted()));
                    });
                };
                if d.added.is_empty() && d.removed.is_empty() && d.changed.is_empty() {
                    ui.label(RichText::new("No difference in what is shown.").color(theme::hush()));
                }
                for r in &d.added {
                    line(ui, "+", theme::ok(), r, format!("{} · {}", r.version, r.source));
                }
                for r in &d.removed {
                    line(ui, "−", theme::alt(), r, format!("{} · {}", r.version, r.source));
                }
                for (x, y) in &d.changed {
                    line(ui, "~", theme::amber(), y, format!("{} → {}", x.version, y.version));
                }
            });
        });
    }
}

/// The records as a table: Package, Language, Source, Version, Group. A row
/// whose software the catalog has opens it (the index comes back).
fn record_table(ui: &mut egui::Ui, salt: &str, recs: &[&Record], cat: Option<&Catalog>) -> Option<usize> {
    let row_h = 20.0;
    let w = ui.available_width();
    let cols = [0.0, 0.34, 0.46, 0.62, 0.78];
    let x = |c: usize, left: f32| left + 10.0 + (w - 20.0) * cols[c];
    theme::strip(ui, 20.0, theme::surface(), |ui| {
        let r = ui.max_rect();
        let p = ui.painter();
        for (c, h) in ["Package", "Language", "Source", "Version", "Group"].iter().enumerate() {
            p.text(egui::pos2(x(c, r.left()), r.center().y), egui::Align2::LEFT_CENTER, *h, FontId::proportional(10.5), theme::hush());
        }
        p.text(egui::pos2(r.right() - 8.0, r.center().y), egui::Align2::RIGHT_CENTER, format!("{} records", thousands(recs.len())), FontId::proportional(10.5), theme::hush());
    });
    if recs.is_empty() {
        ui.add_space(12.0);
        ui.vertical_centered(|ui| ui.label(RichText::new("No records match.").color(theme::hush())));
        return None;
    }
    let find = |r: &Record| -> Option<usize> {
        let cat = cat?;
        let (k, key) = (lang_kind(r.lang()), r.key());
        (0..cat.items.len()).find(|&i| {
            cat.kind[i] == k
                && match r.lang() {
                    Lang::Python => manifest::py_norm(&cat.items[i].n) == key,
                    Lang::Rust => manifest::crate_norm(&cat.items[i].n) == key,
                    Lang::R | Lang::Julia => cat.lname[i] == key,
                }
        })
    };
    let mut picked = None;
    ui.spacing_mut().item_spacing.y = 0.0;
    egui::ScrollArea::vertical().id_salt(salt).auto_shrink([false, false]).show_rows(ui, row_h, recs.len(), |ui, range| {
        for r in &recs[range] {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
            let p = ui.painter_at(rect);
            if resp.hovered() {
                p.rect_filled(rect, 0.0, theme::surface_hi());
            }
            let y = rect.center().y;
            let lc = lang_kind(r.lang()).color();
            let cell = |c: usize, t: &str, color: Color32, mono: bool| {
                let font = if mono { FontId::monospace(11.5) } else { FontId::proportional(11.0) };
                // The last column runs to the edge; the others are cut to theirs.
                let room = if c + 1 < cols.len() { ((x(c + 1, rect.left()) - x(c, rect.left())) / 6.5).max(4.0) as usize } else { usize::MAX };
                let t: String = if t.chars().count() > room { t.chars().take(room.saturating_sub(1)).chain(['…']).collect() } else { t.to_owned() };
                p.text(egui::pos2(x(c, rect.left()), y), egui::Align2::LEFT_CENTER, t, font, color);
            };
            cell(0, &r.package, theme::text_strong(), true);
            cell(1, &r.language, lc, false);
            cell(2, &r.source, theme::muted(), false);
            cell(3, &r.version, theme::text(), true);
            cell(4, &r.group, theme::hush(), false);
            let tip = [&r.description, &r.note, &r.url].iter().filter(|s| !s.is_empty()).map(|s| s.as_str()).collect::<Vec<_>>().join("\n");
            let resp = if tip.is_empty() { resp } else { resp.on_hover_text(tip) };
            if resp.clicked() {
                picked = find(r);
            }
        }
    });
    picked
}
