//! The Builds tab's dimensions and its focus (operator,
//! 2026-10-15: "color by paradigm and ideally have a 3d perspective - paradigm
//! and manifest and language are colors or groups ... so we can see other than
//! just arch stats ... creating branches in the builds section which look tree
//! like and where we can look at different parts of that tree").
//!
//!   Dims      every build step coded once per catalog on five dimensions -
//!             machine, arch, paradigm, manifest, language - so a split, a
//!             colour or a filter is a lookup, never a string compare.
//!   Branch    a path down those dimensions (arm › R › r4.6 rel): the focus.
//!             Curves show only the builds under it; a line's name under
//!             the curves focuses it. (The Branches tree
//!             view was removed, 2026-10-18: "Branches tab under builds can be
//!             completely removed".) The 3D view went 2026-11-01 (operator:
//!             "remove 3D entirely. It adds another navigation model without
//!             making package locations or build dependencies clearer").
//!
//! The steps are the catalog's (ndexr.ndexr.io aggregate/software.rs). The
//! manifest is the box's own record (unibuild_steps.env_set) where it sent one,
//! else the single manifest that lists the package; language is the spec's
//! `ln`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use eframe::egui::{self, Color32, RichText};

use super::{box_color, short_arch, short_box, short_set, Catalog, Kind};
use crate::theme;

// ── the dimensions ─────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Dim {
    Machine,
    Arch,
    Paradigm,
    Manifest,
    Language,
}

impl Dim {
    pub const ALL: [(Dim, &'static str, &'static str); 5] = [
        (Dim::Machine, "machine", "each build machine"),
        (Dim::Arch, "arch", "x86_64, aarch64, and the GPU box's CUDA lane"),
        (Dim::Paradigm, "paradigm", "Gentoo Prefix, EasyBuild, R, Python"),
        (Dim::Manifest, "manifest", "the env manifest an R or Python build was for; the stack's own builds are 'the stack'"),
        (Dim::Language, "language", "what building it compiles: C/C++, Rust, Fortran… or R / Python alone"),
    ];

    pub fn label(self) -> &'static str {
        Dim::ALL.iter().find(|d| d.0 == self).map_or("", |d| d.1)
    }

    fn ix(self) -> usize {
        self as usize
    }
}

/// The values that mean "not known" sort last and draw grey.
const QUIET: [&str; 4] = ["the stack", "unrecorded", "unknown", "meta"];

/// A language's colour: fixed, so Rust is Rust on every chart.
fn lang_color(l: &str) -> Color32 {
    match l {
        "C/C++" => Color32::from_rgb(0x6f, 0x9f, 0xd8),
        "Rust" => Kind::Rust.color(),
        "R" => Kind::R.color(),
        "Python" => Kind::Python.color(),
        "Fortran" => Color32::from_rgb(0xd9, 0xcf, 0x6b),
        "Go" => Color32::from_rgb(0x6f, 0xd6, 0xd6),
        "Java" => Color32::from_rgb(0xe7, 0x83, 0xb5),
        "Perl" => Color32::from_rgb(0xa8, 0xc2, 0x7a),
        "binary" => Color32::from_rgb(0x9a, 0x9a, 0xa8),
        _ => theme::hush(),
    }
}

/// One dimension, coded: its values and each step's value.
struct Coded {
    names: Vec<String>,
    colors: Vec<Color32>,
    code: Vec<u16>,
}

/// Every step on every dimension, worked out once per catalog.
pub struct Dims {
    hash: String,
    by: [Coded; 5],
    /// The steps that count: a run lists the steps it inherited from the runs
    /// before it, so each (machine, software, start) once - as the curves do.
    pub unique: Vec<u32>,
}

impl Dims {
    pub fn new(cat: &Catalog) -> Dims {
        let n = cat.st_i.len().min(cat.st_t.len()).min(cat.st_d.len()).min(cat.st_s.len());
        let mut seen: HashSet<(usize, u32, i64)> = HashSet::new();
        let mut unique = Vec::with_capacity(n);
        for k in 0..n {
            if seen.insert((cat.box_of_step(k), cat.st_i[k], cat.st_t[k])) {
                unique.push(k as u32);
            }
        }
        // Machine and arch keep the catalog's order, so a machine has the
        // colour the Timeline gives it; the others go by how many builds.
        let machine = Coded {
            names: cat.boxes.iter().map(|b| short_box(&b.name).to_owned()).collect(),
            colors: (0..cat.boxes.len()).map(box_color).collect(),
            code: (0..n).map(|k| cat.box_of_step(k) as u16).collect(),
        };
        let value = |d: Dim, k: usize| -> String {
            let i = cat.st_i[k] as usize;
            match d {
                Dim::Arch => short_arch(&cat.box_arch(cat.box_of_step(k))).to_owned(),
                Dim::Paradigm => cat.kind[i].label().to_owned(),
                Dim::Manifest => match (cat.step_set(k), cat.kind[i]) {
                    (Some(s), _) => short_set(s),
                    (None, k) if k.is_lang() => "unrecorded".to_owned(),
                    (None, _) => "the stack".to_owned(),
                },
                Dim::Language => cat.lang(i).to_owned(),
                Dim::Machine => unreachable!(),
            }
        };
        let code = |d: Dim| -> Coded {
            let vals: Vec<String> = (0..n).map(|k| value(d, k)).collect();
            let mut count: HashMap<&str, usize> = HashMap::new();
            for &k in &unique {
                *count.entry(vals[k as usize].as_str()).or_default() += 1;
            }
            for v in &vals {
                count.entry(v.as_str()).or_default();
            }
            let mut names: Vec<&str> = count.keys().copied().collect();
            match d {
                Dim::Paradigm => names.sort_by_key(|n| Kind::ALL.iter().position(|k| k.label() == *n)),
                Dim::Arch => names.sort(),
                _ => names.sort_by(|a, b| QUIET.contains(a).cmp(&QUIET.contains(b)).then(count[b].cmp(&count[a])).then(a.cmp(b))),
            }
            let ix: HashMap<&str, u16> = names.iter().enumerate().map(|(p, n)| (*n, p as u16)).collect();
            let colors = names
                .iter()
                .enumerate()
                .map(|(p, name)| match d {
                    _ if QUIET.contains(name) => theme::hush(),
                    Dim::Paradigm => Kind::ALL.iter().find(|k| k.label() == *name).map_or(theme::hush(), |k| k.color()),
                    Dim::Language => lang_color(name),
                    Dim::Arch => match *name {
                        "x86" => theme::primary(),
                        "arm" => theme::amber(),
                        "cuda" => theme::ok(),
                        _ => box_color(p + 3),
                    },
                    _ => Catalog::group_color(p),
                })
                .collect();
            let code = vals.iter().map(|v| ix[v.as_str()]).collect();
            Coded { names: names.into_iter().map(str::to_owned).collect(), colors, code }
        };
        let by = [machine, code(Dim::Arch), code(Dim::Paradigm), code(Dim::Manifest), code(Dim::Language)];
        Dims { hash: cat.hash.clone(), by, unique }
    }

    pub fn code(&self, d: Dim, k: usize) -> u16 {
        self.by[d.ix()].code[k]
    }

    pub fn name(&self, d: Dim, c: u16) -> &str {
        self.by[d.ix()].names.get(c as usize).map_or("?", String::as_str)
    }

    pub fn color(&self, d: Dim, c: u16) -> Color32 {
        self.by[d.ix()].colors.get(c as usize).copied().unwrap_or(theme::hush())
    }

    pub fn values(&self, d: Dim) -> usize {
        self.by[d.ix()].names.len()
    }

    /// Step `k` lies under the branch.
    pub fn under(&self, branch: &[(Dim, u16)], k: usize) -> bool {
        branch.iter().all(|&(d, c)| self.code(d, k) == c)
    }
}

// ── the state the views share ──────────────────────────────────────────────

pub struct State {
    pub dims: Option<Arc<Dims>>,
    /// What the curves split into lines by.
    pub split: Dim,
    /// The focus: a path down the dimensions.
    pub branch: Vec<(Dim, u16)>,
}

impl Default for State {
    fn default() -> Self {
        State {
            dims: None,
            split: Dim::Machine,
            branch: Vec::new(),
        }
    }
}

impl State {
    pub fn dims_for(&mut self, cat: &Catalog) -> Arc<Dims> {
        match &self.dims {
            Some(d) if d.hash == cat.hash => d.clone(),
            _ => {
                let d = Arc::new(Dims::new(cat));
                // A new catalog may code the values differently.
                if self.dims.is_some() {
                    self.branch.clear();
                }
                self.dims = Some(d.clone());
                d
            }
        }
    }

    /// The key the curves are cached under: split and focus.
    pub fn sig(&self) -> String {
        format!("{:?}{:?}", self.split, self.branch)
    }
}

/// The focus as breadcrumbs: every crumb steps back up to it, ✕ clears.
pub fn branch_strip(ui: &mut egui::Ui, dims: &Dims, branch: &mut Vec<(Dim, u16)>) {
    theme::strip(ui, 22.0, theme::surface(), |ui| {
        ui.add_space(6.0);
        ui.spacing_mut().item_spacing.x = 4.0;
        let mut cut: Option<usize> = None;
        let all = ui.add(egui::Button::new(RichText::new("all builds").size(11.5).color(if branch.is_empty() { theme::text_strong() } else { theme::muted() })).frame(false));
        if all.on_hover_text("every build: the focus is the whole fleet").clicked() {
            cut = Some(0);
        }
        for (n, &(d, c)) in branch.iter().enumerate() {
            ui.label(RichText::new("›").size(11.0).color(theme::hush()));
            let r = super::chip(ui, &format!("{} {}", d.label(), dims.name(d, c)), dims.color(d, c), n + 1 == branch.len());
            if r.on_hover_text("back up to here").clicked() {
                cut = Some(n + 1);
            }
        }
        if !branch.is_empty() && theme::tool(ui, "✕", "the whole fleet again").clicked() {
            cut = Some(0);
        }
        if let Some(n) = cut {
            branch.truncate(n);
        }
        if branch.is_empty() {
            ui.label(RichText::new("· tap a line's name under the curves to show only it").size(11.0).color(theme::text()));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Catalog {
        let j = br#"{"schema":1,"generated":0,"hash":"h","t0":1791990000,
            "boxes":[{"name":"four-box-arm","arch":"aarch64","lane":"x"},{"name":"four-box-intel","arch":"x86_64","lane":"y"}],
            "runs":[{"id":"20261014T150000Z-a","box":0,"itype":"m7g","start":0,"end":0},
                    {"id":"20261014T150000Z-b","box":1,"itype":"m7i","start":0,"end":0}],
            "sets":["r4.6_py3.13-release"],
            "items":[{"k":"R","n":"a","ln":"C/C++","s":["r4.6_py3.13-release","r4.5_py3.12-release"]},{"k":"easybuild","n":"GCC","ln":"C/C++"},{"k":"Python","n":"p"}],
            "steps":{"i":[0,0,1,2,0],"r":[0,0,1,1,1],"t":[10,10,20,30,40],"d":[5,5,50,5,5],"s":"dddfd","m":[0,0,-1,-1,-1]}}"#;
        Catalog::parse(j).unwrap()
    }

    #[test]
    fn steps_code_on_every_dimension() {
        let c = cat();
        let d = Dims::new(&c);
        assert_eq!(d.unique, vec![0, 2, 3, 4], "the inherited copy counts once");
        assert_eq!(d.name(Dim::Machine, d.code(Dim::Machine, 2)), "intel");
        assert_eq!(d.name(Dim::Arch, d.code(Dim::Arch, 0)), "arm");
        assert_eq!(d.name(Dim::Paradigm, d.code(Dim::Paradigm, 2)), "EasyBuild");
        assert_eq!(d.name(Dim::Manifest, d.code(Dim::Manifest, 0)), "r4.6 rel", "the box's record");
        assert_eq!(d.name(Dim::Manifest, d.code(Dim::Manifest, 4)), "unrecorded", "in two manifests, none recorded");
        assert_eq!(d.name(Dim::Manifest, d.code(Dim::Manifest, 2)), "the stack");
        assert_eq!(d.name(Dim::Language, d.code(Dim::Language, 3)), "Python", "an older catalog falls back to the paradigm");
        let r = d.code(Dim::Paradigm, 0);
        assert!(d.under(&[(Dim::Paradigm, r)], 4) && !d.under(&[(Dim::Paradigm, r)], 2));
    }

}
