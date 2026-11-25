//! The atlas: what Cove opens on (operator, 2026-10-15: "cove needs to not
//! start looking like an ide but instead a 3d visualization of all of the
//! software that currently exists in its ecosystem ... from a central origin
//! of software from the kernel and outwards through its connections, and the
//! user would see that tree and see them at the very top of it").
//!
//! The order is the stack's own (same day): "Prefix > EasyBuild > Env (which
//! are compositions of easybuilds within easybuilds) which contain a
//! manifest.dcf with a folder name of the same as the env > language
//! packages > users domains".
//!
//! Two atoms (2026-10-16: "I liked the kernel and you as their own kind of
//! individual singularities with more of the circular orbits around them
//! which almost kind of looked like electron level orbits ... with a little
//! bit more distinction of what's there"). The kernel is one nucleus, left;
//! the person the other, right; a line joins them. Round the kernel each
//! layer is an orbit, tilted its own way, from the inside out:
//!
//!   Prefix    Gentoo Prefix                          (catalog k=prefix)
//!   EasyBuild the toolchains' modules                (k=easybuild)
//!   Env       env/<set>: EasyBuilds composed, one per manifest folder
//!   R, Python packages, each in the sets that list it (`s`)
//!
//! Round the person, each level of their domains is an orbit: apexes on the
//! first, their subdomains on the second inside the apex's arc, and so on.
//! On every orbit a group (a toolchain, a set, a family of subdomains) is
//! its own clump of stars, as big as it has members, parted from the next.
//!
//! Every edge is a real one: a catalog `dep`, a package's set, a domain's
//! env, a subdomain's parent. Only two are the stack's shape rather than a
//! record: Prefix packages that need nothing sit on the kernel, and EasyBuild
//! modules that need nothing are bootstrapped from Prefix's compiler.
//!
//! The Tree (2026-10-16: "improve hourglass and call it tree and the levels
//! from bottom to top will go something like kernel, prefix, easybuild (non
//! language), easybuild languages (R, Py, Rust, etc), easybuild envs w/
//! manifest, you w/ domains, subdomains ... the envs are what connect the
//! kernel to the domains") reorders the same stack bottom up: the kernel
//! the root, Prefix and EasyBuild spread wide over it, the languages split out
//! of EasyBuild into their own ring, the packages the manifests list, then the
//! env sets drawn narrow - the trunk everything passes through - and right
//! above them the domains, subdomains over those, the canopy. No "you" node
//! there (same day: "we can remove the you for that one as we are linking
//! envs to domains"): a domain reaches the kernel through its env alone.
//!
//! Drag turns it, the wheel zooms, a click walks in (a layer, a group, one
//! software or domain with everything under it lit down to the kernel), Esc
//! or the breadcrumb walks out. A domain's Enter opens the IDE in it, as it
//! opens on any load; the toolbar's ◎ comes back here.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, UiBuilder, Vec2};

use crate::api::DomainInfo;
use crate::ui::software::{Catalog, Deployment, Kind};
use crate::ui::kit::theme;

mod inspect;
mod serving;
mod owners;
mod provenance;
mod patched;
use owners::Tier;

/// What the visualization is, in its header.
pub const TAGLINE: &str = "science, software, visualized";

/// The layers, origin outwards. The order is the drawing's and the rail's.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Layer {
    Kernel,
    Prefix,
    EasyBuild,
    Env,
    R,
    Python,
    /// Rust crates (2026-10-18), the third language's packages.
    Rust,
    /// Julia packages (2026-10-21), the fourth language's.
    Julia,
    /// The env/<combo> modules (r4.6_py3.13-release): each joins an R set
    /// and a Python set (the kit's combos.txt); applications work in them.
    Combo,
    /// The applications' own stack (serving.rs): the on-prem machine - the
    /// running copy of the kernel - then nginx and what it hands each domain
    /// to, then, over the domains, DNS and the edge router people come in by.
    Host,
    Route,
    Edge,
    Domain,
    You,
}

/// One layer's name and line, the `n`th round the ring: what the restart card
/// tells the person while Cove comes back (app.rs session_ended).
pub fn fact(n: usize) -> (&'static str, &'static str) {
    const ALL: [Layer; 11] = [Layer::Kernel, Layer::Prefix, Layer::EasyBuild, Layer::Env, Layer::R, Layer::Python, Layer::Rust, Layer::Julia, Layer::Combo, Layer::Domain, Layer::You];
    let l = ALL[n % ALL.len()];
    (l.label(), l.what())
}

impl Layer {
    /// The stack top down, as the rail lists it.
    fn rail() -> &'static [Layer] {
        &[Layer::You, Layer::Edge, Layer::Domain, Layer::Route, Layer::Host, Layer::Combo, Layer::Julia, Layer::Rust, Layer::Python, Layer::R, Layer::Env, Layer::EasyBuild, Layer::Prefix, Layer::Kernel]
    }

    /// The axis ← and → walk, from Prefix up through the combinations to
    /// the applications (operator, 2026-10-21: "left and right buttons should
    /// be able to traverse from prefix up through the application
    /// combinations").
    fn axis() -> &'static [Layer] {
        &[Layer::Prefix, Layer::EasyBuild, Layer::Env, Layer::R, Layer::Python, Layer::Rust, Layer::Julia, Layer::Combo, Layer::Domain]
    }

    fn label(self) -> &'static str {
        match self {
            Layer::Kernel => "Kernel",
            Layer::Prefix => "Gentoo Prefix",
            Layer::EasyBuild => "EasyBuild",
            Layer::Env => "Env sets",
            Layer::R => "R packages",
            Layer::Python => "Python packages",
            Layer::Rust => "Rust crates",
            Layer::Julia => "Julia packages",
            Layer::Combo => "Env combinations",
            Layer::Host => "On-prem host",
            Layer::Route => "nginx and upstreams",
            Layer::Edge => "DNS and edge router",
            // Applications, now software has domains of its own (2026-10-18).
            Layer::Domain => "Applications",
            Layer::You => "Account",
        }
    }

    /// The layer's place in one line, written beside its ring on the canvas
    /// (operator, 2026-10-21: "focus on improving the visual so that the
    /// layers describe their place"). The layering is the one Boissonneault,
    /// Oldeman and Taylor give for Canada's national systems (PEARC'19): what
    /// is local to a machine at the bottom, a compatibility layer that frees
    /// everything above from the host OS, EasyBuild's toolchains per CPU
    /// architecture as modules, then each language's own packages.
    fn place(self) -> &'static str {
        match self {
            Layer::Kernel => "each machine's own operating system, kept by its admins",
            Layer::Prefix => "our own base system, so no vendor's install changes what runs above",
            Layer::EasyBuild => "compilers and scientific libraries, built for each kind of computer",
            Layer::Env => "frozen toolkits: a study names the one it ran in, and it never changes",
            Layer::R | Layer::Python | Layer::Rust | Layer::Julia => "the add-on packages analyses use, every version recorded",
            Layer::Combo => "an R toolkit and a Python one, paired for the applications",
            Layer::Domain => "the applications people use, each traceable to the kernel",
            Layer::Host => "our own machine: every application runs here",
            Layer::Route => "sends each address to what answers it",
            Layer::Edge => "the names people type, and the public door they arrive at",
            Layer::You => "the account that owns the applications",
        }
    }

    fn what(self) -> &'static str {
        match self {
            Layer::Kernel => "the host's own Linux - kernel, drivers, daemons, anything privileged: local to each machine, kept by its admins, never part of what the stack ships",
            Layer::Prefix => "the compatibility layer: a Gentoo Prefix with its own glibc, coreutils and the rest a distribution gives, so nothing above it calls into the host OS",
            Layer::EasyBuild => "the scientific layer: toolchains (compiler + MPI + maths libraries) and what they build, once per CPU architecture, as hierarchical Lmod modules on Prefix",
            Layer::Env => "env/R/<v>, env/Python/<v> and env/Rust/<v>: one language each, with its own pinned manifest and site library; env/<name>: an R and a Python one, composed",
            Layer::R => "R packages, installed by R's own installer against the stack, into the site library of every set whose manifest lists them",
            Layer::Python => "Python packages, wheels built against the stack's Python and libraries, in the site library of every set whose manifest lists them",
            Layer::Rust => "Rust crates, vendored and built against the stack's Rust, in the site library of every Rust set whose manifest lists them",
            Layer::Julia => "Julia packages, one Pkg resolution of the set's manifest against the stack's Julia, in the set's environment and depot",
            Layer::Combo => "env/<combo> modules (r4.6_py3.13-release ...): each joins one R set and one Python set as the kit's combos.txt names them; a domain's .ndexr-env usually names one",
            Layer::Host => "the on-prem machine every domain is served from: the running copy of the kernel (its own floor in the stack), its CPUs, memory and load, read live",
            Layer::Route => "nginx on the host, and what each vhost hands its requests to: the platform's Shiny router (src/app.r, five console replicas on 9000-9004), a port of its own (a Rust app or another process), the owner's cell, a remote, or a redirect",
            Layer::Edge => "each apex's DNS (its name servers - Route 53 for ndexr.io) and the address it resolves to: the edge router, whose reverse SSH tunnel carries 443 and 80 down to nginx here, or an address of its own",
            Layer::Domain => "the account's applications - its domains, each working in the env its .ndexr-env names (software projects show on their own star)",
            Layer::You => "the account the domains belong to: the top of the tree",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Layer::Kernel => Color32::from_rgb(0xf4, 0xe9, 0xc8),
            Layer::Prefix => Kind::Prefix.color(),
            Layer::EasyBuild => Kind::EasyBuild.color(),
            Layer::Env => Color32::from_rgb(0x5e, 0xc4, 0xc0),
            Layer::R => Kind::R.color(),
            Layer::Python => Kind::Python.color(),
            Layer::Rust => Kind::Rust.color(),
            // Julia's own purple, and a green for the pairings.
            Layer::Julia => Color32::from_rgb(0xb0, 0x7c, 0xd8),
            Layer::Combo => Color32::from_rgb(0x8c, 0xd8, 0x9a),
            // The serving column: the machine in the kernel's cream, routing
            // in a steel blue, the internet's edge in a pale sky.
            Layer::Host => Color32::from_rgb(0xf0, 0xd8, 0x9c),
            Layer::Route => Color32::from_rgb(0x7f, 0xa8, 0xd8),
            Layer::Edge => Color32::from_rgb(0xc8, 0xe0, 0xf8),
            Layer::Domain => Color32::from_rgb(0xe8, 0xa7, 0x5d),
            Layer::You => Color32::from_rgb(0xff, 0xff, 0xff),
        }
    }

    /// World size of a node's dot.
    fn size(self) -> f32 {
        match self {
            Layer::Kernel => 0.16,
            Layer::Prefix | Layer::EasyBuild => 0.03,
            Layer::Env => 0.045,
            Layer::R | Layer::Python | Layer::Rust | Layer::Julia => 0.026,
            Layer::Combo => 0.045,
            Layer::Host => 0.12,
            Layer::Route | Layer::Edge => 0.07,
            Layer::Domain => 0.06,
            Layer::You => 0.11,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct V3 {
    x: f32,
    y: f32,
    z: f32,
}

const fn v3(x: f32, y: f32, z: f32) -> V3 {
    V3 { x, y, z }
}

impl V3 {
    fn add(self, o: V3) -> V3 {
        v3(self.x + o.x, self.y + o.y, self.z + o.z)
    }
    fn sub(self, o: V3) -> V3 {
        v3(self.x - o.x, self.y - o.y, self.z - o.z)
    }
    fn mul(self, k: f32) -> V3 {
        v3(self.x * k, self.y * k, self.z * k)
    }
    fn lerp(self, o: V3, t: f32) -> V3 {
        self.add(o.sub(self).mul(t))
    }
    fn len(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}

const GOLDEN_ANGLE: f32 = 2.399_963;

/// The j-th of m stars in a cluster of radius r, as an offset from its
/// centre: on its surface for a few, through its body for many, much
/// thicker toward the core - a globular cluster, not an even ball.
fn ball(j: usize, m: usize, k: u64, r: f32) -> V3 {
    if m <= 1 {
        return v3(0.0, 0.0, 0.0);
    }
    let y = 1.0 - 2.0 * (j as f32 + 0.5) / m as f32;
    let w = (1.0 - y * y).max(0.0).sqrt();
    let th = j as f32 * GOLDEN_ANGLE;
    let d = if m <= 9 { r } else { r * hash01(k).powf(1.35) };
    v3(w * th.cos() * d, y * d, w * th.sin() * d)
}

/// The Hourglass's geometry (spaced out 2026-10-21, operator: "all of the
/// circles could have increased space between them ... started to feel
/// jumbled up a lot"): rings this far apart vertically, the innermost
/// this wide, each stack layer this much wider than the one under it, and
/// each domain level this much wider than the one below; the applications'
/// first ring this many steps over the stack's top one - the waist.
// Opened up again 2026-10-21 (operator: "all of the major rings need more
// space from the others, open up the viewing to be more expansive and
// immersive"): steps 1.4 → 2.0, lobes 7 → 10 out, domain rings wider and
// further apart. Taller again the same day (operator: "need more vertical
// space between things"): the stack's layers stacked their labels on one
// another, so every level is 2.0 → 3.2 apart and the applications' rings
// 3.2 → 5.0; widths are unchanged. Drawn in again that evening (operator:
// "distance between places seems too much now"): steps 3.2 → 2.4, the
// applications' rings 5.0 → 3.6 apart and 4.0 → 3.4 wider each, the lobes
// 10 → 8.5 out - between the two.
// Spread within, closer between (operator, 2026-10-22: "within a region,
// galaxy, cluster we could have more spread of applications while bringing
// the different regions slightly closer together"): levels 2.4 → 2.0 apart,
// applications' rings 3.6 → 3.0, lobes 8.5 → 7.6 out; inside, wider stack
// rings, thicker group clusters, wider galaxy bands, looser applications.
const HG_STEP: f32 = 2.0;
const HG_R0: f32 = 1.3;
const HG_WIDEN: f32 = 1.15;
/// The applications' rings (operator, 2026-10-21: "i need the applications
/// layer to still grow and be more spead out then even the packages layers
/// so big big circles up towards the top"): the first wider than the
/// language lobes reach, each next this much wider and this far higher.
const HG_DOM_R0: f32 = HG_LOBE + 5.5;
const HG_DOM_WIDEN: f32 = 3.4;
const HG_DOM_STEP: f32 = 3.0;
const HG_WAIST: f32 = 1.5;
/// The language packages: one level (in steps over the kernel), split into a
/// lobe per language this far from the axis, so the env sets at the centre
/// reach out to each and the lines between them read as what each set holds.
const HG_PKG_LEVEL: f32 = 4.2;
const HG_LOBE: f32 = 7.6;
/// A language's galaxy: the gap before its core band and between bands, and
/// how far an owner's arm turns per e-fold of radius.
const GALAXY_GAP: f32 = 0.55;
const GALAXY_TWIST: f32 = 0.9;

/// Which way from the axis a language's lobe lies: R, Python, Rust and
/// Julia a quarter of the way round from each other; None for every other
/// layer.
fn lobe_angle(layer: Layer) -> Option<f32> {
    let j = match layer {
        Layer::R => 0.0,
        Layer::Python => 1.0,
        Layer::Rust => 2.0,
        Layer::Julia => 3.0,
        _ => return None,
    };
    Some(std::f32::consts::FRAC_PI_2 + j * std::f32::consts::TAU / 4.0)
}

/// The combinations' ring: on the axis, this many steps over the kernel -
/// over the language lobes, under the applications.
const HG_COMBO_LEVEL: f32 = HG_PKG_LEVEL + 1.8;

/// The language an EasyBuild module is: its family (Scala and Groovy run on the JVM).
pub(crate) fn language(name: &str) -> Option<&'static str> {
    Some(match name {
        "R" => "R",
        "Python" => "Python",
        "Rust" => "Rust",
        "Julia" => "Julia",
        "Go" => "Go",
        "Java" | "Scala" | "Groovy" => "JVM",
        "Perl" => "Perl",
        "Ruby" => "Ruby",
        "GHC" => "Haskell",
        "nodejs" => "JavaScript",
        "PHP" => "PHP",
        "Octave" => "Octave",
        "Lua" => "Lua",
        _ => return None,
    })
}

/// A level ring round the vertical through `c`.
#[derive(Clone, Copy)]
pub struct Orbit {
    c: V3,
    r: f32,
    layer: Layer,
    /// Named (the person's levels once, on the first).
    named: bool,
    /// A language's ring for one tier (owners::Tier), and how many it holds.
    tier: Option<Tier>,
    n: usize,
}

impl Orbit {
    fn at(&self, th: f32, r: f32) -> V3 {
        self.c.add(v3(r * th.cos(), 0.0, r * th.sin()))
    }
}

/// Arcs for groups of these weights round a whole orbit, starting at `th0`:
/// each its share of 82% of the ring (never less than a sliver, so a group
/// of one still reads as a group), the rest left as gaps between them.
fn arcs(weights: &[f32], th0: f32) -> Vec<(f32, f32)> {
    use std::f32::consts::TAU;
    let total: f32 = weights.iter().sum::<f32>().max(1e-6);
    let w: Vec<f32> = weights.iter().map(|w| (w / total).max(0.012)).collect();
    let wt: f32 = w.iter().sum();
    let gap = TAU * 0.18 / weights.len().max(1) as f32;
    let mut th = th0 + gap / 2.0;
    w.iter()
        .map(|w| {
            let span = TAU * 0.82 * w / wt;
            let a = (th, span);
            th += span + gap;
            a
        })
        .collect()
}

fn hash01(mut x: u64) -> f32 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^= x >> 33;
    (x >> 40) as f32 / (1u64 << 24) as f32
}

pub struct Node {
    pos: V3,
    layer: Layer,
    label: String,
    group: String,
    /// The catalog item, or the domain (an index into Scene::domains).
    sw: Option<u32>,
    dom: Option<usize>,
}

impl Scene {
    /// A node's colour: its layer's, or for a domain its app kind's.
    fn tint(&self, nd: &Node) -> Color32 {
        nd.dom.and_then(|d| self.domains.get(d)).and_then(|d| app_color(&d.app)).unwrap_or_else(|| nd.layer.color())
    }
}

pub struct Edge {
    /// `from` is built on (or works in) `to`: edges point toward the kernel.
    from: u32,
    to: u32,
    /// Drawn when nothing is focused; the rest only when lit (a package's
    /// other sets, R's and Python's own deps - thousands of lines otherwise).
    ambient: bool,
    rel: Rel,
}

/// What an edge says, kept apart so membership never reads as dependency
/// (operator, 2026-10-21: "Environment membership, package dependencies, and
/// build relationships need to remain distinguishable").
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Rel {
    /// The catalog's own `dep`: built on.
    Dep,
    /// A package is in an env set's manifest and site library.
    Member,
    /// An env set's module loads this EasyBuild module (its easyconfig).
    Uses,
    /// The stack's shape: a dependency-free Prefix package on its kernel
    /// floor, a dependency-free or SYSTEM-toolchain module on Prefix's compiler.
    Floor,
    /// A domain works in this env set (its .ndexr-env).
    Works,
    /// A subdomain under its parent.
    Sub,
    /// An env/<combo> joins a language set (the kit's combos.txt).
    Joins,
    /// A domain is answered by this upstream (its nginx target).
    Serves,
    /// nginx, over the domains, hands a request down to what runs it.
    Proxy,
    /// nginx and each upstream run on the host; the host runs a kernel floor.
    Runs,
    /// An apex's DNS is answered here (Route 53 ...).
    Dns,
    /// An apex resolves to this address: the edge router, or another.
    Reach,
    /// The edge router's tunnel brings traffic down to nginx here.
    Tunnel,
}

impl Rel {
    /// Said from the `from` side, then from the `to` side.
    fn words(self) -> (&'static str, &'static str) {
        match self {
            Rel::Dep => ("depends on", "used by"),
            Rel::Member => ("member of env sets", "holds packages"),
            Rel::Uses => ("loads from EasyBuild", "loaded by env sets"),
            Rel::Floor => ("stands on", "stands on it"),
            Rel::Works => ("works in", "applications in it"),
            Rel::Sub => ("under", "subdomains"),
            Rel::Joins => ("joins env sets", "joined in combinations"),
            Rel::Serves => ("served by", "serves domains"),
            Rel::Proxy => ("hands requests to", "gets its requests from"),
            Rel::Runs => ("runs on", "runs"),
            Rel::Dns => ("DNS by", "answers DNS for"),
            Rel::Reach => ("comes in through", "brings in"),
            Rel::Tunnel => ("forwards to", "forwarded from"),
        }
    }
}

/// A domain's star by the application behind it (api::DomainInfo::app):
/// the Shiny module every site starts as keeps the domains' amber; a Rust
/// app, its own process and a redirect each read apart at a glance
/// (operator, 2026-10-18: "visualize ... not only across users and
/// subdomains, but also the type of application that is behind that domain").
fn app_color(kind: &str) -> Option<Color32> {
    match kind {
        "rust" => Some(Color32::from_rgb(0xf0, 0x6a, 0x4a)),
        "process" => Some(Color32::from_rgb(0xc0, 0x8c, 0xf0)),
        "redirect" => Some(Color32::from_rgb(0x8a, 0x90, 0xa2)),
        _ => None,
    }
}

/// The kinds the legend lists, in its order.
const APP_KINDS: [&str; 4] = ["shiny", "rust", "process", "redirect"];

/// The stack and the person's domains, laid out once per catalog and list.
pub struct Scene {
    key: u64,
    /// Per node: known to the stack, never yet built (Catalog::planned).
    planned: Vec<bool>,
    nodes: Vec<Node>,
    edges: Vec<Edge>,
    up: Vec<Vec<u32>>,
    down: Vec<Vec<u32>>,
    counts: HashMap<Layer, usize>,
    /// Each layer's groups, largest first, with their centre.
    groups: HashMap<Layer, Vec<(String, usize, V3)>>,
    domains: Vec<DomainInfo>,
    /// The first kernel star; `kernels` every one (one per floor).
    kernel: u32,
    kernels: Vec<u32>,
    floors: Vec<Floor>,
    /// The person's software projects (names.rs), kept off the canopy, and
    /// the domain they hang from - None in the public view.
    projects: Vec<DomainInfo>,
    owned_root: Option<String>,
    you: Option<u32>,
    /// The orbits the rings are drawn on.
    orbits: Vec<Orbit>,
    /// Each language package's tier (owners.rs); None for everything else.
    tier: Vec<Option<Tier>>,
    /// What each env set holds, by language and group: one line per pair,
    /// as heavy as the packages it stands for.
    bundles: Vec<Bundle>,
    /// An env set's edge to an EasyBuild module it loads, and the version
    /// its easyconfig names (the catalog keeps one star per module name).
    used: HashMap<(u32, u32), String>,
}

/// An env set and one group of packages in it (R: bioconductor, Python:
/// Scientific/Engineering ...): `n` packages, drawn as one curve from the
/// set's star to the group's centre in its language's lobe.
pub struct Bundle {
    env: u32,
    layer: Layer,
    group: String,
    to: V3,
    n: u32,
    /// The packages it stands for: each a real Member edge to `env`, so a
    /// shared route never joins two things that are not related.
    members: Vec<u32>,
}

/// One kernel floor (host.rs `kernel_floors`, /view/kernels.json): the oldest
/// Linux a built stack stands on, and the evidence for it.
#[derive(Clone, Debug, Default, PartialEq, serde::Deserialize)]
#[serde(default)]
pub struct Floor {
    pub triple: String,
    pub arch: String,
    /// What its glibc's ELF ABI note declares: "3.2.0".
    pub min_kernel: String,
    pub glibc: String,
    pub headers: String,
    /// Kernels it has been run on (this host's, so far).
    pub proven_on: Vec<String>,
}

impl Floor {
    /// The star's name: "Linux ≥ 3.2 · x86-64".
    fn name(&self) -> String {
        let v = self.min_kernel.strip_suffix(".0").unwrap_or(&self.min_kernel);
        let arch = match self.arch.as_str() {
            "x86_64" => "x86-64",
            "aarch64" => "arm64",
            a => a,
        };
        format!("Linux ≥ {v} · {arch}")
    }

    /// Whether a catalog item built for `archs` stands on this floor (the GPU
    /// box's "cuda" is x86_64 underneath).
    fn holds(&self, archs: &[String]) -> bool {
        archs.is_empty() || archs.iter().any(|a| *a == self.arch || (a == "cuda" && self.arch == "x86_64"))
    }
}

/// The set an env module is: env/r4.6_py3.13-release → r4.6_py3.13-release.
fn env_set_name(name: &str) -> Option<&str> {
    name.strip_prefix("env/")
}

/// What each env set's module loads from EasyBuild, by module
/// (env/R/4.6.1-bioc3.24 → [(R, 4.6.1-foss-2024a), ...]), from its easyconfig
/// (/view/envdeps.json, host.rs env_uses): the catalog does not carry it.
pub type EnvUses = HashMap<String, Vec<(String, String)>>;

impl Scene {
    #[cfg_attr(not(test), allow(dead_code))]
    fn key(cat: &Catalog, domains: &[DomainInfo]) -> u64 {
        Scene::key_with(cat, domains, &[], &EnvUses::new(), &Default::default())
    }

    fn key_with(cat: &Catalog, domains: &[DomainInfo], floors: &[Floor], uses: &EnvUses, kit: &crate::api::EnvSets) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        cat.hash().hash(&mut h);
        for d in domains {
            (&d.host, &d.env, &d.title).hash(&mut h);
        }
        for f in floors {
            (&f.triple, &f.min_kernel).hash(&mut h);
        }
        let mut u: Vec<_> = uses.iter().collect();
        u.sort();
        u.hash(&mut h);
        for k in &kit.sets {
            (&k.set, k.packages.len()).hash(&mut h);
        }
        for c in &kit.combos {
            (&c.name, &c.sets).hash(&mut h);
        }
        kit.repos.len().hash(&mut h);
        h.finish()
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn build(cat: &Catalog, domains: &[DomainInfo], who: &str) -> Scene {
        Scene::build_with(cat, domains, who, &[], &EnvUses::new(), &Default::default())
    }

    /// The scene, its kernel split into one star per floor the stack is built
    /// for (none known: one star, "Linux kernel").
    ///
    /// One arrangement, the hourglass, since 2026-10-21 (operator: "drop the
    /// space, galaxy, and orbital visual - keep hourglass visual only and dont
    /// need a selector to see it - focus on improving the visual so that the
    /// layers describe their place"). One vertical axis through the kernel:
    /// the stack's layers hang below it in the order Boissonneault et al.
    /// (PEARC'19) give them - compatibility layer, EasyBuild, then what is
    /// built on it - each lower and wider; the person stands above it, their
    /// domains rising level by level over them. The others are in git.
    pub fn build_with(cat: &Catalog, domains: &[DomainInfo], _who: &str, floors: &[Floor], uses: &EnvUses, kit: &crate::api::EnvSets) -> Scene {
        let key = Scene::key_with(cat, domains, floors, uses, kit);
        // A software project's domain (names.rs: data-table.r.<root>) is not an
        // application: it belongs to its star, which shows it, and stays out of
        // the canopy. The root projects hang from: the personal domain in the
        // owner folder of the same name (domains/<owner>/<owner>.ndexr.io).
        let (projects, apps): (Vec<DomainInfo>, Vec<DomainInfo>) = domains.iter().cloned().partition(|d| crate::catalog::names::parse(&d.host).is_some());
        let owned_root = domains
            .iter()
            .find(|d| std::path::Path::new(&d.path).parent().and_then(|p| p.file_name()).and_then(|f| f.to_str()).is_some_and(|o| d.host == format!("{o}.ndexr.io")))
            .map(|d| d.host.clone());
        let domains = &apps[..];
        let n = cat.len();
        let mut nodes: Vec<Node> = Vec::with_capacity(n + domains.len() + 2);
        let mut edges: Vec<Edge> = Vec::new();

        // ── the catalog, each item in its layer and group ──────────────────
        for i in 0..n {
            let s = cat.sw(i);
            let (layer, group) = match s.kind {
                Kind::Prefix => (Layer::Prefix, s.group.to_owned()),
                // Language sets first, each its own manifest and library
                // (env/R/<v>, env/Python/<v>); the compositions of one of each
                // by their R (2026-10-18, operator: "we can compose those into
                // what we currently have").
                Kind::Env => {
                    let set = env_set_name(s.name).unwrap_or(s.name);
                    let group = if set.starts_with("R/") {
                        "R sets".to_owned()
                    } else if set.starts_with("Python/") {
                        "Python sets".to_owned()
                    } else if set.starts_with("Rust/") {
                        "Rust sets".to_owned()
                    } else {
                        set.split('_').next().unwrap_or(set).to_owned()
                    };
                    (Layer::Env, group)
                }
                Kind::EasyBuild => (Layer::EasyBuild, s.group.to_owned()),
                // A language's packages by who looks after them (owners.rs):
                // whose territory each is, and where competitors build.
                Kind::R => (Layer::R, owners::owner(s.home)),
                Kind::Python => (Layer::Python, owners::owner(s.home)),
                Kind::Rust => (Layer::Rust, owners::owner(s.home)),
                // No homepage in the catalog: the repository the set's own
                // registry names (host.rs julia_repos).
                Kind::Julia => (Layer::Julia, owners::owner(if s.home.is_empty() { kit.repos.get(s.name).map_or("", String::as_str) } else { s.home })),
                // A kind this Cove does not know: with EasyBuild's, by group.
                Kind::Other => (Layer::EasyBuild, s.group.to_owned()),
            };
            nodes.push(Node { pos: v3(0.0, 0.0, 0.0), layer, label: s.name.to_owned(), group, sw: Some(i as u32), dom: None });
        }
        // The kernel: a star per floor - a binary for x86-64 and arm64, each
        // named for the oldest Linux it is guaranteed on (operator,
        // 2026-10-18: "those will be different singularities and we will need
        // to prove those - might need a split system").
        let kernel = nodes.len() as u32;
        let mut kernels = Vec::new();
        if floors.is_empty() {
            nodes.push(Node { pos: v3(0.0, 0.0, 0.0), layer: Layer::Kernel, label: "Linux kernel".into(), group: String::new(), sw: None, dom: None });
            kernels.push(kernel);
        }
        for f in floors {
            kernels.push(nodes.len() as u32);
            nodes.push(Node { pos: v3(0.0, 0.0, 0.0), layer: Layer::Kernel, label: f.name(), group: f.triple.clone(), sw: None, dom: None });
        }

        // ── what the stack kit defines beyond the catalog ─────────────────
        // A language set the kit has a recipe and a manifest for that the
        // catalog does not list yet (operator, 2026-10-21: "we need the julia
        // envs in the visualization as well and if we dont have them we need
        // basic ones"): its star, to build, with its manifest's packages. And
        // every env/<combo>, joining the two sets combos.txt names.
        let mut to_build: std::collections::HashSet<usize> = std::collections::HashSet::new();
        let in_cat: std::collections::HashSet<&str> = (0..n).filter(|&i| cat.sw(i).kind == Kind::Env).filter_map(|i| env_set_name(cat.sw(i).name)).collect();
        let mut kit_members: Vec<(u32, u32)> = Vec::new();
        for ks in kit.sets.iter().filter(|k| !in_cat.contains(k.set.as_str())) {
            let env = nodes.len();
            nodes.push(Node { pos: v3(0.0, 0.0, 0.0), layer: Layer::Env, label: format!("env/{}", ks.set), group: format!("{} sets", ks.language), sw: None, dom: None });
            to_build.insert(env);
            let layer = match ks.language.as_str() {
                "R" => Layer::R,
                "Python" => Layer::Python,
                "Rust" => Layer::Rust,
                _ => Layer::Julia,
            };
            for p in &ks.packages {
                let k = nodes.len();
                let group = if p.group.is_empty() { "other".to_owned() } else { p.group.clone() };
                nodes.push(Node { pos: v3(0.0, 0.0, 0.0), layer, label: p.name.clone(), group, sw: None, dom: None });
                to_build.insert(k);
                kit_members.push((k as u32, env as u32));
            }
        }
        let held_sets: std::collections::HashSet<&str> = (0..n).flat_map(|i| cat.sw(i).sets.iter().map(String::as_str)).collect();
        let mut combo_of: HashMap<String, u32> = HashMap::new();
        for c in &kit.combos {
            let k = nodes.len();
            // release or devel: the two kinds of pairing.
            let group = c.name.rsplit('-').next().unwrap_or("").to_owned();
            nodes.push(Node { pos: v3(0.0, 0.0, 0.0), layer: Layer::Combo, label: format!("env/{}", c.name), group, sw: None, dom: None });
            if !held_sets.contains(c.name.as_str()) {
                to_build.insert(k);
            }
            combo_of.insert(c.name.clone(), k as u32);
        }

        // ── each language: its owners, and its three tiers ────────────────
        // An account with only a package or two is one of many small hands:
        // past a language's six largest unknown accounts, "independent". A
        // known steward is always named, however few it has (owners.rs).
        let mut tier: Vec<Option<Tier>> = vec![None; nodes.len()];
        for lang in [Layer::R, Layer::Python, Layer::Rust, Layer::Julia] {
            let mut count: HashMap<String, usize> = HashMap::new();
            for nd in nodes.iter().filter(|nd| nd.layer == lang && nd.sw.is_some()) {
                *count.entry(nd.group.clone()).or_default() += 1;
            }
            let mut unknown: Vec<(&String, &usize)> = count.iter().filter(|(g, _)| owners::steward_name(g).is_none() && g.as_str() != owners::UNLISTED).collect();
            unknown.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
            let named: std::collections::HashSet<String> = unknown.iter().take(6).filter(|(_, &c)| c >= 3).map(|(g, _)| (*g).clone()).collect();
            let members: Vec<usize> = (0..nodes.len()).filter(|&k| nodes[k].layer == lang && nodes[k].sw.is_some()).collect();
            for &k in &members {
                let g = &nodes[k].group;
                if owners::steward_name(g).is_none() && g.as_str() != owners::UNLISTED && !named.contains(g) {
                    nodes[k].group = owners::INDEPENDENT.to_owned();
                }
            }
            let word = match lang {
                Layer::R => "R",
                Layer::Python => "Python",
                Layer::Julia => "Julia",
                _ => "Rust",
            };
            let ts = owners::tiers(
                &members,
                word,
                |k| nodes[k].sw.map_or((nodes[k].label.clone(), String::new()), |i| (cat.sw(i as usize).name.to_owned(), cat.sw(i as usize).purpose.to_owned())),
                |k| nodes[k].sw.map_or(Vec::new(), |i| cat.sw(i as usize).deps.iter().map(|&d| d as usize).collect()),
            );
            for (&k, t) in members.iter().zip(ts) {
                tier[k] = Some(t);
            }
        }

        // ── the stack round the kernel ─────────────────────────────────────
        let kernel_at = v3(0.0, 0.0, 0.0);
        // A binary orbits its middle: each star a little off it, level.
        for (j, &k) in kernels.iter().enumerate() {
            let off = if kernels.len() < 2 { v3(0.0, 0.0, 0.0) } else {
                let th = j as f32 / kernels.len() as f32 * std::f32::consts::TAU;
                v3(0.42 * th.cos(), 0.0, 0.42 * th.sin())
            };
            nodes[k as usize].pos = kernel_at.add(off);
        }
        let mut groups: HashMap<Layer, Vec<(String, usize, V3)>> = HashMap::new();
        let mut orbits: Vec<Orbit> = Vec::new();
        // The stack in the paper's order (Boissonneault et al., PEARC'19),
        // kernel down: compatibility layer, EasyBuild, then what is built
        // on it.
        for (li, layer) in [Layer::Prefix, Layer::EasyBuild, Layer::Env, Layer::R, Layer::Python, Layer::Rust, Layer::Julia, Layer::Combo]
        .into_iter()
        .enumerate()
        {
            let mut by: Vec<(String, Vec<usize>)> = Vec::new();
            {
                let mut at: HashMap<&str, usize> = HashMap::new();
                for (k, nd) in nodes.iter().enumerate().filter(|(_, nd)| nd.layer == layer) {
                    match at.get(nd.group.as_str()) {
                        Some(&g) => by[g].1.push(k),
                        None => {
                            at.insert(&nd.group, by.len());
                            by.push((nd.group.clone(), vec![k]));
                        }
                    }
                }
            }
            if by.is_empty() {
                continue;
            }
            // Named owners first, largest first, so the largest named one
            // faces the sets; the many small hands and the unlisted after.
            let bucket = |g: &str| g == owners::INDEPENDENT || g == owners::UNLISTED;
            by.sort_by(|a, b| bucket(&a.0).cmp(&bucket(&b.0)).then(b.1.len().cmp(&a.1.len())).then(a.0.cmp(&b.0)));
            let weights: Vec<f32> = by.iter().map(|g| g.1.len() as f32).collect();
            // A lobe turns its largest group to face the axis, where the env
            // sets are, so the most-held packages are the nearest to them.
            let spans = match lobe_angle(layer) {
                Some(th) => {
                    let s0 = arcs(&weights, 0.0);
                    let turn = th + std::f32::consts::PI - (s0[0].0 + s0[0].1 / 2.0);
                    s0.into_iter().map(|(a, w)| (a + turn, w)).collect()
                }
                None => arcs(&weights, hash01(layer as u64 + 17) * std::f32::consts::TAU),
            };
            // Where along its arc a member sits: the middle for one, else
            // scattered about the middle, densest there.
            let along = |k: usize, m: usize| if m == 1 { 0.5 } else { 0.5 + (hash01(k as u64 * 131) - 0.5) * (0.35 + 0.65 * hash01(k as u64 * 977)) };
            let mut out = Vec::new();
            // A level ring over the kernel, which is the floor (operator,
            // 2026-10-21: "we need the kernel on the bottom and for the bottom
            // half of the stack to reverse"): each layer one step higher and
            // wider, so the further a group is from the kernel in the stack,
            // the further it sits from it in space.
            //
            // R, Python and Rust packages are not a stack among themselves
            // (operator, 2026-10-21: "those circles need to be on the same
            // layer to some extent but split off so that we can see how the
            // envs connect them"): one level over the env sets, each language
            // its own ring far out from the axis, a third of the way round
            // from the next, sized by what it holds.
            // A language with tiers (owners.rs): three rings over one
            // another at its place, each owner the same slice of all three,
            // so an owner's territory reads straight up the lobe (operator,
            // 2026-10-21: "each language to have at least a three circle
            // split ... clear views of ownership ... where other competitors
            // are building and owning").
            if let (Some(th), true) = (lobe_angle(layer), by.iter().flat_map(|g| &g.1).any(|&k| tier[k].is_some())) {
                // Each language its own galaxy (operator, 2026-10-22: "for the R,
                // Py, Rust, Julia language packages which have three vertical
                // layers, collapse them instead into their own galaxy views ...
                // separation without a ton of vertical messiness"): one flat
                // disc at the language's level, its tiers concentric bands -
                // everyday packages the dense core, workflows the middle,
                // supporting packages the halo - and each owner an arm that
                // winds outward through all three at its own bearing.
                let base = kernel_at.add(v3(HG_LOBE * th.cos(), HG_STEP * HG_PKG_LEVEL, HG_LOBE * th.sin()));
                let mut common_r = 0.8;
                let mut inner = 0.0f32;
                for t in Tier::ALL {
                    let in_tier = |k: &usize| tier[*k].unwrap_or(Tier::Supporting) == t;
                    let cnt: usize = by.iter().map(|g| g.1.iter().filter(|k| in_tier(k)).count()).sum();
                    // A band as wide as its share needs, outside the last.
                    let band = (0.5 + 0.05 * (cnt as f32).sqrt()).min(1.8);
                    let rr = inner + GALAXY_GAP + band / 2.0;
                    inner = rr + band / 2.0;
                    if t == Tier::Everyday {
                        common_r = rr;
                    }
                    // The core's ring first: it is the language's own (named, and
                    // the one its level is read from).
                    let orbit = Orbit { c: base, r: rr, layer, named: t == Tier::Everyday, tier: Some(t), n: cnt };
                    for ((_, members), &(a, span)) in by.iter().zip(&spans) {
                        let here: Vec<usize> = members.iter().copied().filter(|k| in_tier(k)).collect();
                        let m = here.len();
                        for &k in &here {
                            // Across the band, densest in its middle; the arm
                            // turned the further out it reaches.
                            let r = rr + band * (hash01(k as u64 * 313) - 0.5) * (0.4 + 0.6 * hash01(k as u64 * 71));
                            let lift = (hash01(k as u64 * 7) - 0.5) * 0.08;
                            nodes[k].pos = orbit.at(a + span * along(k, m) + GALAXY_TWIST * (r / GALAXY_GAP).ln(), r).add(v3(0.0, lift, 0.0));
                        }
                    }
                    if cnt > 0 || t == Tier::Everyday {
                        orbits.push(orbit);
                    }
                }
                let ring = Orbit { c: base, r: common_r, layer, named: false, tier: None, n: 0 };
                for ((name, members), &(a, span)) in by.into_iter().zip(&spans) {
                    out.push((name, members.len(), ring.at(a + span / 2.0, common_r)));
                }
                groups.insert(layer, out);
                continue;
            }
            let (c, rr) = match lobe_angle(layer) {
                Some(th) => {
                    let n: usize = by.iter().map(|g| g.1.len()).sum();
                    (kernel_at.add(v3(HG_LOBE * th.cos(), HG_STEP * HG_PKG_LEVEL, HG_LOBE * th.sin())), (0.8 + 0.06 * (n as f32).sqrt()).min(2.2))
                }
                // The pairings over the lobes, on the axis: between the
                // language sets they join and the applications in them.
                None if layer == Layer::Combo => (kernel_at.add(v3(0.0, HG_STEP * HG_COMBO_LEVEL, 0.0)), HG_R0 + HG_WIDEN * 2.0),
                None => (kernel_at.add(v3(0.0, HG_STEP * (li as f32 + 1.0), 0.0)), HG_R0 + HG_WIDEN * li as f32),
            };
            let orbit = Orbit { c, r: rr, layer, named: true, tier: None, n: 0 };
            for ((name, members), &(a, span)) in by.into_iter().zip(&spans) {
                let m = members.len();
                let thick = (0.08 + 0.018 * (m as f32).sqrt()).min(0.5);
                for (j, &k) in members.iter().enumerate() {
                    nodes[k].pos = orbit.at(a + span * along(k, m), rr).add(ball(j, m, k as u64 * 7919, thick));
                }
                out.push((name, m, orbit.at(a + span / 2.0, rr)));
            }
            orbits.push(orbit);
            groups.insert(layer, out);
        }

        // ── the catalog's own edges, and the two the stack's shape makes ──
        let bootstrap = ["sys-devel/gcc", "sys-devel/binutils", "sys-libs/glibc"]
            .iter()
            .find_map(|want| (0..n).find(|&i| cat.sw(i).kind == Kind::Prefix && cat.sw(i).name == *want))
            .map_or(kernel, |i| i as u32);
        let mut used: HashMap<(u32, u32), String> = HashMap::new();
        let mut env_of: HashMap<String, u32> = HashMap::new();
        for (k, nd) in nodes.iter().enumerate().filter(|(_, nd)| nd.layer == Layer::Env) {
            if let Some(s) = env_set_name(&nd.label) {
                env_of.insert(s.to_owned(), k as u32);
            }
        }
        for i in 0..n {
            let s = cat.sw(i);
            let layer = nodes[i].layer;
            for &d in s.deps {
                if (d as usize) < n {
                    // Not ambient: EasyBuild's own web hid every other line
                    // (operator, 2026-10-21: "disappear into the dense
                    // EasyBuild graph"); a selection reveals it.
                    edges.push(Edge { from: i as u32, to: d, ambient: false, rel: Rel::Dep });
                }
            }
            match layer {
                // On the floor of each architecture it is built for.
                Layer::Prefix if s.deps.is_empty() => {
                    let on: Vec<u32> = if floors.is_empty() { kernels.clone() } else { floors.iter().zip(&kernels).filter(|(f, _)| f.holds(s.archs)).map(|(_, &k)| k).collect() };
                    for k in if on.is_empty() { vec![kernel] } else { on } {
                        edges.push(Edge { from: i as u32, to: k, ambient: true, rel: Rel::Floor });
                    }
                }
                // An env set stands on what its module loads (operator,
                // 2026-10-21: "env seems to go straight to the kernel but if an
                // env is using R or rust then we need the easybuild connections
                // there too"): each module its easyconfig names, by name and
                // version. Until those are read, on Prefix's compiler.
                Layer::Env if s.deps.is_empty() => {
                    let mut any = false;
                    for (name, version) in uses.get(s.name).into_iter().flatten() {
                        let eb = (0..n).find(|&j| nodes[j].layer == Layer::EasyBuild && cat.sw(j).name == name.as_str() && (cat.sw(j).versions.is_empty() || cat.sw(j).versions.iter().any(|v| v == version)));
                        if let Some(j) = eb {
                            edges.push(Edge { from: i as u32, to: j as u32, ambient: true, rel: Rel::Uses });
                            used.insert((i as u32, j as u32), version.clone());
                            any = true;
                        }
                    }
                    if !any {
                        edges.push(Edge { from: i as u32, to: bootstrap, ambient: true, rel: Rel::Floor });
                    }
                }
                // A module EasyBuild builds at the SYSTEM toolchain is compiled
                // by the compiler under it - Prefix's gcc here - whatever else
                // it lists (operator, 2026-10-21: "where easybuilds are
                // connected to prefix we should see the connections ... back
                // trace to the kernel"). Measured: GCCcore and binutils name
                // each other, so without this not one of the 336 modules
                // reached Prefix; with it every one does.
                Layer::EasyBuild if s.deps.is_empty() || s.group == "SYSTEM" => edges.push(Edge { from: i as u32, to: bootstrap, ambient: true, rel: Rel::Floor }),
                // Julia's too, now its set is built and in the catalog
                // (2026-10-22: its 544 packages had no line to their set).
                Layer::R | Layer::Python | Layer::Rust | Layer::Julia => {
                    // A package sits in the sets that hold it: lit when
                    // looked at; the bundles below draw them all at once.
                    for set in s.sets {
                        if let Some(&e) = env_of.get(set) {
                            edges.push(Edge { from: i as u32, to: e, ambient: false, rel: Rel::Member });
                        }
                    }
                }
                _ => {}
            }
        }

        // ── the person's domains: orbits round them ───────────────────────
        // Operator, 2026-10-16: "we need to be able to see those [sub, sub
        // domains] in circular clusters we can pick out visually". So: one
        // apex per family (ndexr.io, koodle.io - the domain itself where the
        // person has it, else a label standing for it) on the first orbit;
        // each name's subdomains on the next, within its arc.
        let at: HashMap<&str, usize> = domains.iter().enumerate().map(|(i, d)| (d.host.as_str(), i)).collect();
        let apex_of = |h: &str| -> String {
            let parts: Vec<&str> = h.split('.').collect();
            if parts.len() <= 2 { h.to_owned() } else { parts[parts.len() - 2..].join(".") }
        };
        let base = nodes.len();
        for (i, d) in domains.iter().enumerate() {
            nodes.push(Node { pos: v3(0.0, 0.0, 0.0), layer: Layer::Domain, label: d.host.clone(), group: apex_of(&d.host), sw: None, dom: Some(i) });
        }
        // The apexes, largest family first; a label node for one the person
        // does not have as a domain of their own.
        let mut apexes: Vec<(String, usize)> = Vec::new();
        for d in domains {
            let a = apex_of(&d.host);
            match apexes.iter_mut().find(|x| x.0 == a) {
                Some(x) => x.1 += 1,
                None => apexes.push((a, 1)),
            }
        }
        apexes.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
        let mut apex_node: HashMap<String, u32> = HashMap::new();
        for (a, _) in &apexes {
            let k = match at.get(a.as_str()) {
                Some(&i) => (base + i) as u32,
                None => {
                    nodes.push(Node { pos: v3(0.0, 0.0, 0.0), layer: Layer::Domain, label: a.clone(), group: a.clone(), sw: None, dom: None });
                    (nodes.len() - 1) as u32
                }
            };
            apex_node.insert(a.clone(), k);
        }
        // Each domain's parent: the nearest name above it the person has,
        // else its apex.
        let mut parent: HashMap<u32, u32> = HashMap::new();
        for (i, d) in domains.iter().enumerate() {
            let me = (base + i) as u32;
            let a = apex_of(&d.host);
            if d.host == a {
                continue;
            }
            let mut rest = d.host.as_str();
            let mut up_to = apex_node[&a];
            while let Some((_, up)) = rest.split_once('.') {
                if up == a {
                    break;
                }
                if let Some(&p) = at.get(up) {
                    up_to = (base + p) as u32;
                    break;
                }
                rest = up;
            }
            parent.insert(me, up_to);
        }
        let mut kids: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut order: Vec<u32> = parent.keys().copied().collect();
        order.sort_by(|x, y| nodes[*x as usize].label.cmp(&nodes[*y as usize].label));
        for c in order {
            kids.entry(parent[&c]).or_default().push(c);
        }
        // The person's levels: apexes on the first, each level of
        // subdomains one further out inside its parent's arc, so a family
        // stays together as it goes out.
        let roots: Vec<u32> = apexes.iter().map(|(a, _)| apex_node[a]).collect();
        fn weight(n: u32, kids: &HashMap<u32, Vec<u32>>) -> f32 {
            1.0 + kids.get(&n).map_or(0.0, |ks| ks.iter().map(|&c| weight(c, kids)).sum())
        }
        // The applications stand on the stack: their levels rise over its top
        // ring, the first already wider than the language lobes, each next
        // wider and higher again - the canopy over everything below.
        let top = orbits.iter().map(|o| o.c.y).fold(kernel_at.y, f32::max);
        let floor = v3(kernel_at.x, top + HG_STEP * (HG_WAIST - 1.0), kernel_at.z);
        let level = |d: usize| Orbit { c: floor.add(v3(0.0, HG_STEP + HG_DOM_STEP * d as f32, 0.0)), r: HG_DOM_R0 + HG_DOM_WIDEN * d as f32, layer: Layer::Domain, named: d == 0, tier: None, n: 0 };
        let spans = arcs(&roots.iter().map(|&r| weight(r, &kids)).collect::<Vec<_>>(), 0.3);
        let mut deepest = 0usize;
        let mut q: VecDeque<(u32, usize, f32, f32)> = roots.iter().zip(&spans).map(|(&r, &(a, w))| (r, 0, a, w)).collect();
        while let Some((n, d, a, span)) = q.pop_front() {
            deepest = deepest.max(d);
            let o = level(d);
            let th = a + span / 2.0;
            // A little out of line, so a crowded arc reads as stars, not beads.
            let jit = |s: u64| hash01(n as u64 * s) - 0.5;
            // Loose round its ring, in and out and up and down, so a crowded
            // level reads as a spread of applications rather than a line.
            nodes[n as usize].pos = o.at(th, o.r * (1.0 + 0.08 * jit(53))).add(v3(0.0, jit(7) * 0.5, 0.0));
            if let Some(ks) = kids.get(&n) {
                let ws: Vec<f32> = ks.iter().map(|&c| weight(c, &kids)).collect();
                let wt: f32 = ws.iter().sum();
                // Inside the parent's arc, a little narrower, so families part.
                let (pad, inner) = (span * 0.06, span * 0.88);
                let mut th = a + pad;
                for (&c, w) in ks.iter().zip(ws) {
                    let s = inner * w / wt;
                    q.push_back((c, d + 1, th, s));
                    th += s;
                }
            }
        }
        if !roots.is_empty() {
            for d in 0..=deepest {
                orbits.push(level(d));
            }
        }
        // No account star over the applications (operator, 2026-10-21:
        // "without the need for account label at the moment"): they join the
        // stack through the env sets they work in.
        let you: Option<u32> = None;
        for (&c, &p) in &parent {
            edges.push(Edge { from: c, to: p, ambient: true, rel: Rel::Sub });
        }
        // The kit's own: a set to build stands on what its easyconfig loads,
        // holds its manifest's packages; a combination joins its two sets.
        for (k, nd) in nodes.iter().enumerate().filter(|(k, nd)| nd.layer == Layer::Env && *k >= n) {
            for (name, version) in uses.get(&nd.label).into_iter().flatten() {
                if let Some(j) = (0..n).find(|&j| nodes[j].layer == Layer::EasyBuild && cat.sw(j).name == name.as_str() && (cat.sw(j).versions.is_empty() || cat.sw(j).versions.iter().any(|v| v == version))) {
                    edges.push(Edge { from: k as u32, to: j as u32, ambient: true, rel: Rel::Uses });
                    used.insert((k as u32, j as u32), version.clone());
                }
            }
        }
        for &(k, env) in &kit_members {
            edges.push(Edge { from: k, to: env, ambient: false, rel: Rel::Member });
        }
        for c in &kit.combos {
            for set in &c.sets {
                if let (Some(&k), Some(&e)) = (combo_of.get(&c.name), env_of.get(set)) {
                    edges.push(Edge { from: k, to: e, ambient: true, rel: Rel::Joins });
                }
            }
        }
        for (i, d) in domains.iter().enumerate() {
            let env = d.env.trim();
            let set = env_set_name(env).unwrap_or(env);
            if let Some(&e) = env_of.get(set).or_else(|| combo_of.get(set)) {
                edges.push(Edge { from: (base + i) as u32, to: e, ambient: true, rel: Rel::Works });
            }
        }

        let mut up = vec![Vec::new(); nodes.len()];
        let mut down = vec![Vec::new(); nodes.len()];
        for (k, e) in edges.iter().enumerate() {
            up[e.from as usize].push(k as u32);
            down[e.to as usize].push(k as u32);
        }
        let mut counts = HashMap::new();
        for nd in &nodes {
            *counts.entry(nd.layer).or_insert(0usize) += 1;
        }
        // Each env set turned toward the lobes it draws on (operator,
        // 2026-10-21: "can we have the envs close to their package cirlces
        // instead of across to minimize line crossover"): round its ring to
        // the mean of its packages' directions - an R set faces R, a
        // composition sits between R and Python. Sets aiming the same way
        // fan out side by side.
        if let Some(ring) = orbits.iter().find(|o| o.layer == Layer::Env).copied() {
            let mut aim: Vec<(u32, f32)> = Vec::new();
            for (k, _) in nodes.iter().enumerate().filter(|(_, nd)| nd.layer == Layer::Env) {
                let (mut x, mut z) = (0.0f32, 0.0f32);
                for &d in &down[k] {
                    if let Some(th) = lobe_angle(nodes[edges[d as usize].from as usize].layer) {
                        x += th.cos();
                        z += th.sin();
                    }
                }
                if x.hypot(z) > 1e-3 {
                    aim.push((k as u32, z.atan2(x)));
                }
            }
            aim.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let mut i = 0;
            while i < aim.len() {
                let mut j = i + 1;
                while j < aim.len() && aim[j].1 - aim[i].1 < 0.05 {
                    j += 1;
                }
                let m = j - i;
                for (q, &(k, th)) in aim[i..j].iter().enumerate() {
                    let th = th + (q as f32 - (m as f32 - 1.0) / 2.0) * 0.16;
                    nodes[k as usize].pos = ring.at(th, ring.r);
                }
                i = j;
            }
            if let Some(gs) = groups.get_mut(&Layer::Env) {
                for (g, _, c) in gs.iter_mut() {
                    let pts: Vec<V3> = nodes.iter().filter(|n| n.layer == Layer::Env && n.group == *g).map(|n| n.pos).collect();
                    if !pts.is_empty() {
                        *c = pts.iter().fold(v3(0.0, 0.0, 0.0), |a, p| a.add(*p)).mul(1.0 / pts.len() as f32);
                    }
                }
            }
        }
        // Each env set to each group of packages it holds, counted.
        let mut held: HashMap<(u32, Layer, &str), Vec<u32>> = HashMap::new();
        for e in edges.iter().filter(|e| e.rel == Rel::Member) {
            let nd = &nodes[e.from as usize];
            held.entry((e.to, nd.layer, nd.group.as_str())).or_default().push(e.from);
        }
        let mut bundles: Vec<Bundle> = held
            .into_iter()
            .filter_map(|((env, layer, g), members)| {
                let x = groups.get(&layer)?.iter().find(|x| x.0 == g)?;
                Some(Bundle { env, layer, group: g.to_owned(), to: x.2, n: members.len() as u32, members })
            })
            .collect();
        bundles.sort_by(|a, b| a.n.cmp(&b.n).then(a.env.cmp(&b.env)).then(a.group.cmp(&b.group)));
        // Known, but never built anywhere yet: drawn as a ring, not a star.
        tier.resize(nodes.len(), None);
        let planned = nodes.iter().enumerate().map(|(k, n)| n.sw.is_some_and(|i| cat.planned(i as usize)) || to_build.contains(&k)).collect();
        Scene { key, nodes, edges, up, down, counts, groups, domains: domains.to_vec(), kernel, kernels, floors: floors.to_vec(), projects, owned_root, you, orbits, tier, bundles, used, planned }
    }


    /// The language packages a source steward looks after (owners.rs: the
    /// group a lobe draws them in).
    /// A person's domains (`DomainInfo::owner`).
    fn built_by<'a>(&'a self, person: &'a str) -> impl Iterator<Item = u32> + 'a {
        self.nodes.iter().enumerate().filter(move |(_, n)| n.dom.is_some_and(|i| self.domains.get(i).is_some_and(|d| d.owner == person))).map(|(k, _)| k as u32)
    }

    /// The stars a steward or a person focus is about: the steward's
    /// packages, the person's domains.
    fn whose(&self, f: &Focus) -> Vec<u32> {
        match f {
            Focus::Owner(o) => self.owned_by(o).collect(),
            Focus::Person(p) => self.built_by(p).collect(),
            _ => Vec::new(),
        }
    }

    /// Everyone who owns a domain here, most domains first: (username, count).
    fn people(&self) -> Vec<(String, usize)> {
        let mut m: std::collections::BTreeMap<&str, usize> = Default::default();
        for n in &self.nodes {
            if let Some(d) = n.dom.and_then(|i| self.domains.get(i)).filter(|d| !d.owner.is_empty()) {
                *m.entry(d.owner.as_str()).or_default() += 1;
            }
        }
        let mut v: Vec<(String, usize)> = m.into_iter().map(|(k, n)| (k.to_owned(), n)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }

    fn owned_by<'a>(&'a self, owner: &'a str) -> impl Iterator<Item = u32> + 'a {
        self.nodes.iter().enumerate().filter(move |(_, n)| lobe_angle(n.layer).is_some() && n.group == owner).map(|(k, _)| k as u32)
    }

    /// Everything `n` stands on, down to the kernel, and what stands on it
    /// directly: (lit nodes, lit edges).
    fn lineage(&self, n: u32) -> (Vec<bool>, Vec<u32>) {
        let mut lit = vec![false; self.nodes.len()];
        let mut lines = Vec::new();
        let mut q = VecDeque::from([n]);
        lit[n as usize] = true;
        while let Some(k) = q.pop_front() {
            for &e in &self.up[k as usize] {
                lines.push(e);
                let t = self.edges[e as usize].to;
                if !lit[t as usize] {
                    lit[t as usize] = true;
                    q.push_back(t);
                }
            }
        }
        for &e in &self.down[n as usize] {
            lines.push(e);
            lit[self.edges[e as usize].from as usize] = true;
        }
        (lit, lines)
    }
}

/// How much of the picked star's details is shown.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Panel {
    Full,
    Bar,
    Hidden,
}

/// Where the atlas is looking: the breadcrumb is a list of these.
#[derive(Clone, PartialEq, Debug)]
enum Focus {
    All,
    Layer(Layer),
    Group(Layer, String),
    Node(u32),
    /// One env set's members in one group (Scene::bundles).
    Bundle(u32),
    /// Everything one source steward looks after, in every language, with
    /// what it is built on here (operator, 2026-10-21: "clicking on source
    /// steward to see their stack build here with the connections").
    Owner(String),
    /// One person's system: the domains they own and everything each stands
    /// on, down to the kernel (operator, 2026-10-23: "atlas shows the whole
    /// system everyone has worked on, and to see individual user systems as
    /// well"). The username, from domains.json's `owner`.
    Person(String),
}

pub enum Act {
    /// Into the IDE, in this domain.
    Enter(DomainInfo),
    /// The Software tab, on this catalog item.
    Software(usize),
    /// This catalog item's source, in the person's workspace (source.rs).
    Source(usize),
    /// Ask for software the stack does not have: a request, typed or spoken
    /// (signed in; the public view sends the visitor to sign in first).
    AddSoftware,
}

struct Cam {
    target: V3,
    yaw: f32,
    pitch: f32,
    dist: f32,
}

/// Face-on, from above and a touch from the right: the axis reads left to
/// right, and the Tree's level rings open into ellipses rather than lines.
const HOME: Cam = Cam { target: v3(-2.0, 0.0, 0.0), yaw: 0.18, pitch: 0.55, dist: 14.0 };

/// The centre of these points and the distance that fits them in view.
fn frame(pts: impl Iterator<Item = V3>, pad: f32) -> Option<(V3, f32)> {
    let (mut lo, mut hi) = (v3(f32::MAX, f32::MAX, f32::MAX), v3(f32::MIN, f32::MIN, f32::MIN));
    let mut any = false;
    for p in pts {
        any = true;
        lo = v3(lo.x.min(p.x), lo.y.min(p.y), lo.z.min(p.z));
        hi = v3(hi.x.max(p.x), hi.y.max(p.y), hi.z.max(p.z));
    }
    any.then(|| {
        let half = ((hi.x - lo.x).max(hi.y - lo.y) / 2.0 + pad).max(0.6);
        (lo.add(hi).mul(0.5), half * 1.9 + 1.5)
    })
}

pub struct Atlas {
    /// The public view (view.rs): there is no IDE to open.
    pub public: bool,
    /// Nothing but the stack and the tour's messages (operator, 2026-10-18:
    /// "allow the user to hide all of the extra stuff other than the messages
    /// in the auto tour so that they can really soak it in"): no rail, bar,
    /// card or hints; the tour starts at once and a stray mouse does not stop
    /// it. H toggles it, Esc leaves it; the slides go bare too (showcase.rs).
    pub immersive: bool,
    scene: Option<Scene>,
    trail: Vec<Focus>,
    cam: Cam,
    goal: Cam,
    /// What the current focus lights: computed once per focus.
    lit: Option<(Focus, Vec<bool>, Vec<u32>)>,
    hover: Option<u32>,
    search: String,
    /// Who the rail and the tour speak to, and the catalog's numbers for them
    /// (by catalog hash, so a new catalog is counted again).
    /// Seconds since the person last touched it: it turns by itself after a while.
    idle_since: f64,
    /// Left alone long enough, it walks the stack by itself, top down
    /// (operator, 2026-10-16: "should automatically traverse through them for
    /// a screen saver like visualization over time where the groups are all
    /// explored and the user can watch it almost like a meditation").
    tour: Option<Tour>,
    /// The kernel floors (host.rs kernel_floors), once they have come.
    floors: Arc<std::sync::Mutex<Option<Vec<Floor>>>>,
    floors_asked: bool,
    /// What the env sets load from EasyBuild (/view/envdeps.json), asked
    /// once per catalog.
    uses: Arc<std::sync::Mutex<EnvUses>>,
    uses_asked: String,
    /// The stack kit's sets and combinations (/view/envsets.json), and what
    /// was asked: (catalog hash, the sets whose packages were asked for).
    kit: Arc<std::sync::Mutex<crate::api::EnvSets>>,
    kit_asked: (String, String),
    /// builds.json as it last came (the machines, their queues, the package
    /// lanes), asked every 30 s; and what it says, matched to stars.
    live_raw: Arc<std::sync::Mutex<Option<serde_json::Value>>>,
    live_at: f64,
    live: Building,
    /// The build machines sheet (the pill's tap; a phone has no rail).
    machines_open: bool,
    /// The inspection view's open groups (inspect.rs group_key).
    open: std::collections::HashSet<String>,
    /// In the stack, hide what the selection does not touch, not only dim it.
    isolate: bool,
    /// The star the person asked to see laid flat (inspect.rs); otherwise a
    /// pick is shown in the stack itself.
    flat: Option<u32>,
    /// When the focus last changed: the picked star's lines grow from then.
    picked_at: f64,
    /// What was inspected last: marked in the overview, so it is found again.
    last_pick: Option<u32>,
    /// The bundle under the pointer: previewed, not chosen.
    bundle_hover: Option<u32>,
    /// Where each package's machine code came from (provenance.rs).
    prov: provenance::Fetch,
    /// Which modules were built with patches, and which (patched.rs).
    patches: patched::Fetch,
    /// What each module was built from, and the patch texts read (patched.rs).
    built: patched::Records,
    /// The ship's way through space (world units a second): thrust adds to
    /// it, it drifts to a stop.
    vel: V3,
    /// The picked star's details: open, folded to a one-line bar, or hidden.
    /// Apart from the focus: closing them never leaves the star or moves the
    /// camera (operator, 2026-10-21: "I like it to stay focused on that item
    /// instead without blocking the view").
    panel: Panel,
    /// The focus is the person's own (Atlas::go), not a slide's.
    picked: bool,
    /// The rail's "not built yet" list, opened.
    show_planned: bool,
    /// The rail's PATCHED list, folded until asked.
    show_patched: bool,
    /// The binaries' fates opened in the rail ("" = undecided, open by default).
    open_fates: Vec<String>,
    /// A deployment played back over time; None is now.
    replay: Option<Replay>,
    /// How the applications are served (Query::Serving, app.rs every
    /// minute; admins only): the serving column, and its live numbers.
    serving: Option<crate::api::Serving>,
    built_shape: u64,
}

/// What the build machines are doing, matched to the catalog (operator,
/// 2026-10-18: "if we are actively compiling software ... show compiling
/// right now and going to it in the visualization, and if there are
/// environments ... or easybuilds that we haven't yet compiled but are going
/// to ... show those as well ... maybe they have a difference in their
/// visualization if they don't yet exist").
#[derive(Default)]
struct Building {
    /// Compiling now - more than one machine, arch and version at once.
    now: Vec<Compile>,
    /// In a machine's queue, not started: (catalog item, entry, machine).
    queued: Vec<(Option<usize>, String, String)>,
    /// Every build machine with a log here, on ones first: where its run is.
    machines: Vec<BoxNow>,
}

/// One build machine as its lane tells it (host.rs build_lanes): on or off,
/// the `ndexr stack up` stage, the env set and package, its last log lines
/// (operator, 2026-10-24: "if we are running any of the boxes or if they are
/// currently live then we need to include those aspects in the messaging to
/// the user ... like are packages building and where, what stage").
#[derive(Clone, Default, Debug, PartialEq)]
struct BoxNow {
    key: String,
    arch: String,
    on: bool,
    state: String,
    steps: Vec<String>,
    /// 1-based, as the run file counts it.
    step_index: usize,
    started: String,
    finished: String,
    host: String,
    publish_id: String,
    set: String,
    set_n: u64,
    sets: u64,
    now: String,
    kind: String,
    n: u64,
    of: u64,
    quiet_s: i64,
    tail: Vec<String>,
}

/// "2026-10-24T21:36:17Z" as "21:36 UTC".
fn hhmm(t: &str) -> String {
    t.get(11..16).map_or(String::new(), |h| format!("{h} UTC"))
}

fn ago(s: i64) -> String {
    match s {
        s if s < 90 => format!("{s} s"),
        s if s < 90 * 60 => format!("{} min", s / 60),
        s => format!("{} h", s / 3600),
    }
}

impl BoxNow {
    fn read(l: &serde_json::Value, at: i64, arch_of: &dyn Fn(&str) -> String) -> BoxNow {
        let str_of = |k: &str| l[k].as_str().unwrap_or("").to_owned();
        let key = str_of("key");
        let arch = arch_of(&key);
        BoxNow {
            on: l["power"].as_str().map_or(false, |p| p != "off"),
            state: if l["status"].as_str().is_some_and(|s| s != "off") { str_of("status") } else { str_of("state") },
            steps: l["steps"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
            step_index: l["step_index"].as_u64().unwrap_or(0) as usize,
            started: str_of("started"),
            finished: str_of("finished"),
            host: str_of("host"),
            publish_id: str_of("publish_id"),
            set: str_of("set"),
            set_n: l["set_n"].as_u64().unwrap_or(0),
            sets: l["sets"].as_u64().unwrap_or(0),
            now: str_of("now"),
            kind: str_of("kind"),
            n: l["n"].as_u64().unwrap_or(0),
            of: l["of"].as_u64().unwrap_or(0),
            quiet_s: (at - l["mtime"].as_i64().unwrap_or(at)).max(0),
            tail: l["tail"].as_array().into_iter().flatten().filter_map(|x| x.as_str().map(str::to_owned)).collect(),
            key,
            arch,
        }
    }

    /// On, but its log has not moved in 15 minutes: the machine is booting or
    /// waiting to start a run, and the stage on file is the last run's
    /// (2026-10-24: arm, started again, read "stage 9 of 9 ... running since
    /// 21:36" from the run that had finished at 22:00).
    fn starting(&self) -> bool {
        self.on && self.quiet_s > 15 * 60
    }

    fn step(&self) -> &str {
        self.steps.get(self.step_index.saturating_sub(1)).map_or("", String::as_str)
    }

    /// The stage in words: "stage 9 of 9: packages - R/4.6.1-bioc3.24, set 2 of 9".
    fn stage(&self) -> String {
        if self.steps.is_empty() {
            return String::new();
        }
        let mut t = format!("stage {} of {}: {}", self.step_index, self.steps.len(), self.step());
        if self.step() == "packages" && !self.set.is_empty() {
            t.push_str(&format!(" - {}", self.set));
            if self.sets > 0 {
                t.push_str(&format!(", set {} of {}", self.set_n, self.sets));
            }
        }
        t
    }

    /// What the machine is doing, one line each, in plain words.
    fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.starting() {
            out.push(format!("On, starting - its log has not moved for {}", ago(self.quiet_s)));
            if !self.finished.is_empty() {
                out.push(format!("Last run {} at {}", if self.state.is_empty() || self.state == "running" { "ended" } else { &self.state }, hhmm(&self.finished)));
            }
        } else if self.on {
            let stage = self.stage();
            if let Some(first) = stage.chars().next() {
                out.push(format!("{}{}", first.to_uppercase(), &stage[first.len_utf8()..]));
            }
            // A running run's package lane: what it compiles now.
            // Only in the packages stage: the log's last "now installing" line
            // otherwise belongs to the previous run (it read "Installing HTTP
            // (Julia)" while the box built cove, 2026-10-24).
            if !self.now.is_empty() && self.state == "running" && self.step() == "packages" {
                out.push(format!("Installing {} ({}) - {} of {}", self.now, self.kind, self.n, self.of));
            }
            let since = if self.started.is_empty() { String::new() } else { format!(" · running since {}", hhmm(&self.started)) };
            let on = if self.host.is_empty() { String::new() } else { format!(" on {}", self.host) };
            out.push(format!("Log written {} ago{since}{on}", ago(self.quiet_s)));
        } else if !self.finished.is_empty() {
            let published = if self.publish_id.is_empty() { String::new() } else { format!(", published as {}", self.publish_id) };
            out.push(format!("Off · last run {} at {}{published}", if self.state.is_empty() { "ended" } else { &self.state }, hhmm(&self.finished)));
        } else {
            out.push("Off".to_owned());
        }
        out
    }

    /// The pill's words when this machine is on and nothing compiles.
    fn pill(&self) -> String {
        if self.starting() {
            return format!("●  {} ({}) is on · starting", self.key, arch_word(&self.arch));
        }
        let stage = self.stage();
        format!("●  {} ({}) is on{}", self.key, arch_word(&self.arch), if stage.is_empty() { String::new() } else { format!(" · {stage}") })
    }
}

/// One compile in progress: what, which version, on which machine and arch
/// (operator, 2026-10-18: "what box something is compiling on ... can be
/// traced all the way down to which glibc for linking and display because
/// some boxes may be on or not and compiling can be more than one version").
#[derive(Clone, Default, Debug, PartialEq)]
struct Compile {
    item: Option<usize>,
    name: String,
    version: String,
    machine: String,
    /// x86_64 or aarch64: which glibc it links against, and which kernel floor.
    arch: String,
}

/// The arch as people say it.
fn arch_word(a: &str) -> &str {
    match a {
        "x86_64" => "x86-64",
        "aarch64" => "arm64",
        a => a,
    }
}

impl Compile {
    /// "four-box/intel" → "intel".
    fn short_machine(&self) -> &str {
        self.machine.rsplit('/').next().unwrap_or(&self.machine)
    }
}

impl Building {
    /// The star an item is, in this scene.
    fn node_of(sc: &Scene, item: Option<usize>) -> Option<usize> {
        let i = item? as u32;
        sc.nodes.iter().position(|n| n.sw == Some(i))
    }

    /// Per node: 0, 1 queued, 2 compiling now.
    fn marks(&self, sc: &Scene) -> Vec<u8> {
        let mut m = vec![0u8; sc.nodes.len()];
        for (i, ..) in &self.queued {
            if let Some(n) = Self::node_of(sc, *i) {
                m[n] = m[n].max(1);
            }
        }
        for c in &self.now {
            if let Some(n) = Self::node_of(sc, c.item) {
                m[n] = 2;
            }
        }
        m
    }

    /// A compile's line down the stack: its star, what it is built on down to
    /// glibc (the shortest chain of real dependency edges), then the kernel
    /// floor its machine's arch runs on. Empty when the star is not here.
    fn trace(sc: &Scene, c: &Compile) -> Vec<usize> {
        let Some(start) = Self::node_of(sc, c.item) else { return Vec::new() };
        let glibc = sc.nodes.iter().position(|n| n.label == "sys-libs/glibc");
        let floor = sc.kernels.iter().map(|&k| k as usize).find(|&k| !c.arch.is_empty() && sc.nodes[k].group.starts_with(&c.arch)).or_else(|| sc.kernels.first().map(|&k| k as usize));
        // Breadth first along "built on" edges, to glibc - else to any kernel.
        let goal = |n: usize| match glibc {
            Some(g) => n == g,
            None => sc.nodes[n].layer == Layer::Kernel,
        };
        let mut from = vec![usize::MAX; sc.nodes.len()];
        let mut q = VecDeque::from([start]);
        from[start] = start;
        let mut hit = None;
        while let Some(n) = q.pop_front() {
            if goal(n) {
                hit = Some(n);
                break;
            }
            for &e in &sc.up[n] {
                let t = sc.edges[e as usize].to as usize;
                if from[t] == usize::MAX {
                    from[t] = n;
                    q.push_back(t);
                }
            }
        }
        let mut path = Vec::new();
        if let Some(mut n) = hit {
            while n != start {
                path.push(n);
                n = from[n];
            }
        }
        path.push(start);
        path.reverse();
        if let Some(k) = floor {
            if path.last() != Some(&k) {
                path.push(k);
            }
        }
        path
    }

    /// The floor a compile lands on (kernels.json), by its machine's arch.
    fn floor_of<'a>(sc: &'a Scene, c: &Compile) -> Option<&'a Floor> {
        sc.floors.iter().find(|f| f.arch == c.arch)
    }

    /// " · x86-64 → glibc 2.43-r2 → Linux ≥ 3.2", or the arch alone.
    fn chain(sc: &Scene, c: &Compile) -> String {
        match Self::floor_of(sc, c) {
            Some(f) => format!(" · {} → glibc {} → {}", arch_word(&f.arch), f.glibc, f.name().split(" · ").next().unwrap_or("")),
            None if !c.arch.is_empty() => format!(" · {}", arch_word(&c.arch)),
            None => String::new(),
        }
    }

    /// From builds.json: live machines only (heard from in 15 minutes) -
    /// their running step and their queue - and the package lanes running now.
    fn read(v: &serde_json::Value, cat: &Catalog) -> Building {
        let mut b = Building::default();
        let at = v["generated"].as_i64().unwrap_or(0);
        // Each machine's arch, to say which glibc its compiles link against.
        let arch_of = |id: &str| -> String {
            v["eta"]["machines"].as_array().into_iter().flatten().find(|m| m["id"].as_str() == Some(id)).and_then(|m| m["arch"].as_str()).unwrap_or("").to_owned()
        };
        for m in v["eta"]["machines"].as_array().into_iter().flatten() {
            // On by the one answer every view gives (host.rs box_power,
            // carried on the box's lane); a machine with no lane, by its rollup.
            let id_key = m["id"].as_str().unwrap_or("").rsplit('/').next().unwrap_or("");
            let lane_power = v["lanes"].as_array().into_iter().flatten().find(|l| l["key"].as_str() == Some(id_key) && m["id"].as_str().is_some_and(|i| i.starts_with("four-box"))).and_then(|l| l["power"].as_str());
            let live = match lane_power {
                Some(p) => p != "off",
                None => m["active"].as_bool().unwrap_or(false) || m["mirror_age_s"].as_i64().is_some_and(|a| a < 15 * 60),
            };
            if !live {
                continue;
            }
            let id = m["id"].as_str().unwrap_or("").to_owned();
            if let Some(name) = m["running"]["name"].as_str() {
                b.now.push(Compile {
                    item: cat.find_entry(name),
                    name: name.to_owned(),
                    version: m["running"]["version"].as_str().unwrap_or("").to_owned(),
                    machine: id.clone(),
                    arch: m["arch"].as_str().unwrap_or("").to_owned(),
                });
            }
            for p in m["pending"].as_array().into_iter().flatten().filter(|p| p["status"].as_str() != Some("running")) {
                if let Some(e) = p["entry"].as_str() {
                    b.queued.push((cat.find_entry(e), e.to_owned(), id.clone()));
                }
            }
        }
        // Every machine with a log, on ones first. A four-box key's arch comes
        // from its eta machine, else from what the key names.
        let key_arch = |k: &str| -> String {
            let a = arch_of(&format!("four-box/{k}"));
            if !a.is_empty() {
                return a;
            }
            if k == "arm" || k.contains("arm") { "aarch64".into() } else { "x86_64".into() }
        };
        b.machines = v["lanes"].as_array().into_iter().flatten().map(|l| BoxNow::read(l, at, &key_arch)).collect();
        b.machines.sort_by_key(|m| (!m.on, m.key.clone()));
        for l in v["lanes"].as_array().into_iter().flatten() {
            let fresh = at - l["mtime"].as_i64().unwrap_or(0) < 15 * 60;
            let on = l["power"].as_str().map_or(true, |p| p != "off");
            if l["state"].as_str() == Some("running") && fresh && on {
                if let Some(p) = l["now"].as_str() {
                    let lang = l["kind"].as_str().unwrap_or("Python");
                    let i = cat.find(lang, p).or_else(|| cat.find_entry(p));
                    // The lane is a machine's log: four-box/<key>.
                    let key = l["key"].as_str().unwrap_or("");
                    let machine = v["eta"]["machines"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str()).find(|id| id.rsplit('/').next() == Some(key)).unwrap_or(key).to_owned();
                    // The same package on two machines is two compiles.
                    if !b.now.iter().any(|x| x.name == p && x.machine == machine) {
                        let arch = arch_of(&machine);
                        let version = i.and_then(|i| cat.sw(i).versions.last().cloned()).unwrap_or_default();
                        b.now.push(Compile { item: i, name: p.to_owned(), version, machine, arch });
                    }
                }
            }
        }
        b
    }
}

struct Tour {
    /// Each stop is a whole trail, so the breadcrumb reads as it would by hand.
    stops: Vec<Vec<Focus>>,
    at: usize,
    since: f64,
    /// The trail the person left it on: a touch puts it back.
    before: Vec<Focus>,
}

/// Quiet this long and the tour begins; each stop holds this long.
const TOUR_AFTER: f64 = 30.0;
// Longer on each layer and on the kernel (operator, 2026-10-18: "spend a
// little bit more time on each of the layers as that tour goes through").
const TOUR_LAYER: f64 = 12.0;
const TOUR_GROUP: f64 = 9.0;
const TOUR_KERNEL: f64 = 16.0;

impl Tour {
    /// Every layer from the packages down to the kernel, and every group in
    /// each, then the whole sky again.
    fn stops(sc: &Scene) -> Vec<Vec<Focus>> {
        let mut out = Vec::new();
        for &l in Layer::rail().iter().filter(|l| !matches!(l, Layer::You | Layer::Domain)) {
            if l != Layer::Kernel && sc.counts.get(&l).copied().unwrap_or(0) == 0 {
                continue;
            }
            out.push(vec![Focus::Layer(l)]);
            for (g, _, _) in sc.groups.get(&l).map(Vec::as_slice).unwrap_or(&[]) {
                out.push(vec![Focus::Layer(l), Focus::Group(l, g.clone())]);
            }
        }
        out.push(Vec::new());
        out
    }

    fn hold(&self) -> f64 {
        match self.stops[self.at].last() {
            Some(Focus::Group(..)) => TOUR_GROUP,
            Some(Focus::Layer(Layer::Kernel)) => TOUR_KERNEL,
            Some(_) => TOUR_LAYER,
            None => TOUR_GROUP * 1.5,
        }
    }
}

impl Default for Atlas {
    fn default() -> Self {
        Atlas {
            public: false,
            immersive: false,
            scene: None,
            trail: Vec::new(),
            cam: HOME,
            goal: HOME,
            lit: None,
            hover: None,
            search: String::new(),
            idle_since: 0.0,
            tour: None,
            floors: Default::default(),
            floors_asked: false,
            uses: Default::default(),
            uses_asked: String::new(),
            kit: Default::default(),
            kit_asked: Default::default(),
            live_raw: Default::default(),
            live_at: f64::NEG_INFINITY,
            machines_open: false,
            live: Building::default(),
            vel: v3(0.0, 0.0, 0.0),
            panel: Panel::Full,
            open: Default::default(),
            isolate: false,
            flat: None,
            picked_at: f64::NAN,
            last_pick: None,
            bundle_hover: None,
            prov: Default::default(),
            patches: Default::default(),
            built: Default::default(),
            picked: false,
            serving: None,
            built_shape: 0,
            show_planned: false,
            show_patched: false,
            open_fates: vec![String::new()],
            replay: None,
        }
    }
}

/// A deployment played back (operator, 2026-10-23: "atlas must be able to help
/// us visualize over time a new gentoo deployment ... through the n boxes" -
/// each one a generation of the stack, built from source). At moment `t` a
/// star is built, compiling, or not built yet, by its first finished build in
/// the deployment on the chosen box (or any): drawn as the live view draws
/// them - a ring and "to build", an amber "compiling", a star.
pub struct Replay {
    deps: Vec<Deployment>,
    pick: usize,
    /// One machine's build, or the first build on any of them.
    on: Option<String>,
    t: i64,
    playing: bool,
    /// Per catalog item: its first finished build here, (start, end).
    first: Vec<Option<(i64, i64)>>,
    /// What `first` was made for: catalog hash, deployment, machine.
    made: (String, usize, Option<String>),
}

impl Replay {
    /// A full deployment plays in this many seconds.
    const PLAY_SECS: f64 = 90.0;

    fn new() -> Replay {
        Replay { deps: Vec::new(), pick: usize::MAX, on: None, t: 0, playing: true, first: Vec::new(), made: (String::new(), usize::MAX, None) }
    }

    fn dep(&self) -> Option<&Deployment> {
        self.deps.get(self.pick)
    }

    /// The deployments and first builds, again when anything they depend on changed.
    fn prepare(&mut self, cat: &Catalog) {
        if self.made.0 != cat.hash() {
            self.deps = cat.deployments();
            if self.pick >= self.deps.len() {
                // The newest generation first.
                self.pick = self.deps.len().saturating_sub(1);
                self.t = self.deps.get(self.pick).map_or(0, |d| d.lo);
            }
        }
        let key = (cat.hash().to_owned(), self.pick, self.on.clone());
        if self.made != key {
            if let Some(d) = self.deps.get(self.pick).cloned() {
                self.first = cat.first_builds(d.lo, d.hi, self.on.as_deref());
                self.t = self.t.clamp(d.lo, d.hi);
            }
            self.made = key;
        }
    }

    fn advance(&mut self, dt: f64) {
        let Some(d) = self.dep().cloned() else { return };
        if !self.playing {
            return;
        }
        let step = ((d.hi - d.lo) as f64 / Self::PLAY_SECS * dt).max(1.0) as i64;
        self.t = (self.t + step).min(d.hi);
        if self.t >= d.hi {
            self.playing = false;
        }
    }

    /// Per node, as Building::marks: 0 built, 1 not yet, 2 compiling at `t`.
    /// A star with no build in this deployment was not part of it: not yet.
    fn marks(&self, sc: &Scene) -> Vec<u8> {
        sc.nodes
            .iter()
            .map(|n| match n.sw.and_then(|k| self.first.get(k as usize).copied()) {
                None => 0,
                Some(None) => 1,
                Some(Some((s, e))) => {
                    if self.t < s {
                        1
                    } else if self.t < e {
                        2
                    } else {
                        0
                    }
                }
            })
            .collect()
    }

    /// (built by `t`, built in the whole deployment), over the scene's stars.
    fn progress(&self, sc: &Scene) -> (usize, usize) {
        let mut built = 0;
        let mut all = 0;
        for n in &sc.nodes {
            if let Some(Some((_, e))) = n.sw.and_then(|k| self.first.get(k as usize).copied()) {
                all += 1;
                if e <= self.t {
                    built += 1;
                }
            }
        }
        (built, all)
    }
}

struct Proj {
    centre: Pos2,
    f: f32,
    cy: f32,
    sy: f32,
    cp: f32,
    sp: f32,
    target: V3,
    dist: f32,
}

impl Proj {
    fn new(cam: &Cam, rect: Rect) -> Proj {
        Proj {
            centre: rect.center(),
            // A wide lens: the sky round what is looked at stays in view.
            f: rect.width().min(rect.height()) * 0.72,
            cy: cam.yaw.cos(),
            sy: cam.yaw.sin(),
            cp: cam.pitch.cos(),
            sp: cam.pitch.sin(),
            target: cam.target,
            dist: cam.dist,
        }
    }

    /// Screen point, scale (pixels per world unit) and depth; None behind the eye.
    fn at(&self, p: V3) -> Option<(Pos2, f32, f32)> {
        let v = p.sub(self.target);
        let x1 = v.x * self.cy - v.z * self.sy;
        let z1 = v.x * self.sy + v.z * self.cy;
        let y2 = v.y * self.cp - z1 * self.sp;
        let z2 = v.y * self.sp + z1 * self.cp;
        let depth = self.dist - z2;
        // Right up to the eye: a star can be flown up to (operator,
        // 2026-10-21: "we need to be able to continue to zoom in").
        if depth < 0.03 {
            return None;
        }
        let s = self.f / depth;
        Some((Pos2::new(self.centre.x + x1 * s, self.centre.y - y2 * s), s, depth))
    }
}

impl Atlas {
    /// Look at catalog item `sw` and what it is built on, as a click on its
    /// star would: the front page's slides point the atlas this way
    /// (showcase.rs). False until the scene has it.
    pub fn show_item(&mut self, sw: usize) -> bool {
        // What the person chose is theirs: a slide does not take it away.
        if self.chosen() {
            return true;
        }
        let Some(n) = self.scene.as_ref().and_then(|sc| sc.nodes.iter().position(|n| n.sw == Some(sw as u32))) else { return false };
        self.tour = None;
        self.trail = vec![Focus::Node(n as u32)];
        self.picked = false;
        self.aim();
        true
    }

    /// Look at kernel floor `j` (wrapping), as a slide asks.
    pub fn show_kernel(&mut self, j: usize) -> bool {
        if self.chosen() {
            return true;
        }
        let Some(k) = self.scene.as_ref().and_then(|sc| (!sc.kernels.is_empty()).then(|| sc.kernels[j % sc.kernels.len()])) else { return false };
        self.tour = None;
        self.trail = vec![Focus::Node(k)];
        self.picked = false;
        self.aim();
        true
    }

    /// The serving path as the session last read it (app.rs asks every
    /// minute): the column is built when its shape changes; the numbers are
    /// read each frame.
    pub fn set_serving(&mut self, s: Option<crate::api::Serving>) {
        if s.is_some() {
            self.serving = s;
        }
    }

    /// The person picked a star or a bundle themselves (not a slide): the
    /// front page's slides hold, and do not point the atlas elsewhere.
    pub fn chosen(&self) -> bool {
        self.picked && matches!(self.focus(), Focus::Node(_) | Focus::Bundle(_))
    }

    /// Back to the whole sky, if a slide had pointed it somewhere.
    pub fn release(&mut self) {
        if !self.chosen() && matches!(self.focus(), Focus::Node(_)) {
            self.trail.clear();
            self.aim();
        }
    }

    fn focus(&self) -> Focus {
        self.trail.last().cloned().unwrap_or(Focus::All)
    }

    fn go(&mut self, f: Focus) {
        self.picked = true;
        // A pick shows its details - again, for the star already picked,
        // with no camera move (go returns before aim for the same focus).
        if matches!(f, Focus::Node(_) | Focus::Bundle(_)) {
            self.panel = Panel::Full;
        }
        if self.focus() == f {
            return;
        }
        if f == Focus::All {
            self.trail.clear();
        } else {
            // Walking back to something already on the trail cuts it there.
            if let Some(k) = self.trail.iter().position(|t| *t == f) {
                self.trail.truncate(k);
            }
            self.trail.push(f);
            if self.trail.len() > 12 {
                self.trail.remove(0);
            }
        }
        self.aim();
    }

    /// A lens from the rail: the trail starts again from it.
    fn lens(&mut self, f: Focus) {
        let keep = match (&f, self.trail.first()) {
            (Focus::Group(l, _), Some(Focus::Layer(x))) if l == x => 1,
            _ => 0,
        };
        self.trail.truncate(keep);
        if keep == 0 {
            if let Focus::Group(l, _) = &f {
                self.trail.push(Focus::Layer(*l));
            }
        }
        self.go(f);
    }

    fn back(&mut self) {
        if let Some(Focus::Node(n)) = self.trail.pop() {
            self.last_pick = Some(n);
        }
        self.aim();
    }

    /// The camera's goal for the focus.
    fn aim(&mut self) {
        // A new goal: the ship's own drift stops.
        self.vel = v3(0.0, 0.0, 0.0);
        let Some(sc) = &self.scene else { return };
        let (yaw, pitch) = (self.goal.yaw, self.goal.pitch);
        self.goal = match self.focus() {
            // The stack fills the view, from above enough that its rings
            // open and part; the applications' wide rings run off the top
            // rather than shrinking the stack to a corner (operator's
            // screenshot, 2026-10-21: "please improve viewableness").
            Focus::All => match frame(sc.nodes.iter().filter(|n| !matches!(n.layer, Layer::Domain | Layer::You)).map(|n| n.pos), 0.3) {
                Some((target, dist)) => Cam { target: target.add(v3(0.0, HG_STEP * 0.6, 0.0)), yaw, pitch: pitch.max(0.55), dist: (dist * 0.85).clamp(6.0, 90.0) },
                None => Cam { yaw, ..HOME },
            },
            Focus::Layer(Layer::Kernel) => Cam { target: sc.nodes[sc.kernel as usize].pos.add(v3(1.2, 0.0, 0.0)), yaw, pitch, dist: 5.5 },
            Focus::Layer(l) => {
                let of = |n: &&Node| n.layer == l || (l == Layer::Domain && n.layer == Layer::You);
                match frame(sc.nodes.iter().filter(of).map(|n| n.pos), 0.4) {
                    Some((target, dist)) => Cam { target, yaw, pitch, dist },
                    None => Cam { yaw, ..HOME },
                }
            }
            Focus::Group(l, g) => match frame(sc.nodes.iter().filter(|n| n.layer == l && n.group == g).map(|n| n.pos), 0.5) {
                Some((target, dist)) => Cam { target, yaw, pitch, dist: dist.max(2.6) },
                None => Cam { yaw, ..HOME },
            },
            // Flown to and circled (operator, 2026-10-21: "click on something
            // and then get closer to it visually with its dependency
            // connections visible instead of a menu which hides the view"):
            // it in the middle, far enough back that everything it touches
            // directly is in view, from a little above so its lines part.
            Focus::Node(n) => {
                let nd = &sc.nodes[n as usize];
                let near = sc.up[n as usize].iter().map(|&e| sc.edges[e as usize].to).chain(sc.down[n as usize].iter().map(|&e| sc.edges[e as usize].from));
                let reach = near.map(|k| sc.nodes[k as usize].pos.sub(nd.pos).len()).fold(0.0f32, f32::max);
                let floor = match nd.layer {
                    Layer::You | Layer::Kernel => 5.5,
                    Layer::Domain => 4.0,
                    _ => 3.2,
                };
                Cam { target: nd.pos, yaw, pitch: pitch.max(0.55), dist: (reach * 2.1 + 1.0).clamp(floor, 26.0) }
            }
            Focus::Bundle(b) => match sc.bundles.get(b as usize).and_then(|b| frame(std::iter::once(sc.nodes[b.env as usize].pos).chain(b.members.iter().map(|&m| sc.nodes[m as usize].pos)), 0.5)) {
                Some((target, dist)) => Cam { target, yaw, pitch, dist: dist.max(3.0) },
                None => Cam { yaw, ..HOME },
            },
            f @ (Focus::Owner(_) | Focus::Person(_)) => match frame(sc.whose(&f).into_iter().map(|k| sc.nodes[k as usize].pos), 0.6) {
                Some((target, dist)) => Cam { target, yaw, pitch: pitch.max(0.5), dist: dist.max(4.0) },
                None => Cam { yaw, ..HOME },
            },
        };
    }

    /// Which nodes and edges the focus lights (None: everything, ambient).
    fn lighting(&mut self) {
        let f = self.focus();
        if self.lit.as_ref().is_some_and(|l| l.0 == f) {
            return;
        }
        let Some(sc) = &self.scene else { return };
        let (lit, lines) = match &f {
            Focus::All => (Vec::new(), Vec::new()),
            Focus::Layer(l) => (sc.nodes.iter().map(|n| n.layer == *l).collect(), Vec::new()),
            Focus::Group(l, g) => {
                let lit: Vec<bool> = sc.nodes.iter().map(|n| n.layer == *l && n.group == *g).collect();
                let lines = sc.edges.iter().enumerate().filter(|(_, e)| lit[e.from as usize] && lit[e.to as usize]).map(|(k, _)| k as u32).collect();
                (lit, lines)
            }
            Focus::Node(n) => sc.lineage(*n),
            // The steward's packages, or the person's domains, and everything
            // each stands on, down to the kernel: their whole build here.
            Focus::Owner(_) | Focus::Person(_) => {
                let mut lit = vec![false; sc.nodes.len()];
                let mut lines: Vec<u32> = Vec::new();
                for k in sc.whose(&f) {
                    let (l, e) = sc.lineage(k);
                    for (a, b) in lit.iter_mut().zip(l) {
                        *a |= b;
                    }
                    lines.extend(e);
                }
                lines.sort_unstable();
                lines.dedup();
                (lit, lines)
            }
            // The set, the members, the membership edges between: nothing else.
            Focus::Bundle(b) => match sc.bundles.get(*b as usize) {
                Some(b) => {
                    let mut lit = vec![false; sc.nodes.len()];
                    lit[b.env as usize] = true;
                    for &m in &b.members {
                        lit[m as usize] = true;
                    }
                    let lines = sc.down[b.env as usize].iter().copied().filter(|&e| sc.edges[e as usize].rel == Rel::Member && lit[sc.edges[e as usize].from as usize]).collect();
                    (lit, lines)
                }
                None => (Vec::new(), Vec::new()),
            },
        };
        self.lit = Some((f, lit, lines));
        self.picked_at = f64::NAN;
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, cat: Option<Arc<Catalog>>, domains: Option<Result<Vec<DomainInfo>, String>>, who: &str, current: Option<&str>) -> Option<Act> {
        let rect = ui.max_rect();
        let now = ctx.input(|i| i.time);
        // A deployment being played back moves on with the clock.
        if let (Some(r), Some(c)) = (self.replay.as_mut(), cat.as_deref()) {
            r.prepare(c);
            r.advance(ctx.input(|i| i.stable_dt) as f64);
            if r.playing {
                ctx.request_repaint();
            }
        }
        // ── the scene, built again when the catalog or the domains change ──
        let doms = match &domains {
            Some(Ok(v)) => v.as_slice(),
            _ => &[],
        };
        // The kernel floors, asked once (/view/kernels.json): until they come,
        // one star; then a star per floor.
        if !self.floors_asked {
            self.floors_asked = true;
            let (slot, ctx2) = (self.floors.clone(), ctx.clone());
            ehttp::fetch(ehttp::Request::get(format!("{}/kernels.json", crate::ui::front::showcase::base())), move |r| {
                if let Some(v) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice::<Vec<Floor>>(&r.bytes).ok()) {
                    *slot.lock().unwrap() = Some(v);
                    ctx2.request_repaint();
                }
            });
        }
        let floors = self.floors.lock().unwrap().clone().unwrap_or_default();
        // What the machines are compiling and have queued, every 30 s
        // (/view/builds.json, the same for every viewer).
        if now - self.live_at > 30.0 {
            self.live_at = now;
            let (slot, ctx2) = (self.live_raw.clone(), ctx.clone());
            ehttp::fetch(ehttp::Request::get(format!("{}/builds.json", crate::ui::front::showcase::base())), move |r| {
                if let Some(v) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice::<serde_json::Value>(&r.bytes).ok()) {
                    *slot.lock().unwrap() = Some(v);
                    ctx2.request_repaint();
                }
            });
        }
        if let (Some(cat), Some(v)) = (&cat, self.live_raw.lock().unwrap().take()) {
            self.live = Building::read(&v, cat);
        }
        // The kit's sets and combinations, once per catalog; then the
        // packages of the sets the catalog does not have yet.
        if let Some(cat) = cat.as_ref().filter(|c| self.kit_asked.0 != c.hash()) {
            self.kit_asked = (cat.hash().to_owned(), String::new());
            fetch_kit(&self.kit, ctx, "");
        }
        let in_cat: Vec<&str> = cat.as_ref().map_or(Vec::new(), |cat| (0..cat.len()).map(|i| cat.sw(i)).filter(|s| s.kind == Kind::Env).map(|s| s.name).collect());
        let missing: Vec<String> = self.kit.lock().unwrap().sets.iter().map(|k| format!("env/{}", k.set)).filter(|m| !in_cat.contains(&m.as_str())).collect();
        let want = missing.iter().map(|m| m.trim_start_matches("env/")).collect::<Vec<_>>().join(",");
        if !want.is_empty() && self.kit_asked.1 != want {
            self.kit_asked.1 = want.clone();
            fetch_kit(&self.kit, ctx, &want);
        }
        let kit = self.kit.lock().unwrap().clone();
        // What each env set loads from EasyBuild - the kit's sets to build
        // too - asked again whenever that list changes.
        let mods: Vec<&str> = in_cat.iter().copied().chain(missing.iter().map(String::as_str)).collect();
        if cat.is_some() && self.uses_asked != mods.join(",") {
            self.uses_asked = mods.join(",");
            let (slot, ctx2) = (self.uses.clone(), ctx.clone());
            ehttp::fetch(ehttp::Request::get(format!("{}/envdeps.json?m={}", crate::ui::front::showcase::base(), mods.join(","))), move |r| {
                if let Some(v) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice::<Vec<crate::api::EnvUses>>(&r.bytes).ok()) {
                    let got: EnvUses = v.into_iter().map(|e| (e.module, e.uses.into_iter().map(|u| (u.name, u.version)).collect())).collect();
                    *slot.lock().unwrap() = got;
                    ctx2.request_repaint();
                }
            });
        }
        if let Some(c) = &cat {
            self.prov.ask(c.hash(), ctx);
            self.patches.ask(c.hash(), ctx);
        }
        let uses = self.uses.lock().unwrap().clone();
        if let Some(cat) = &cat {
            let shape = self.serving.as_ref().map_or(0, serving::shape);
            if self.scene.as_ref().is_none_or(|s| s.key != Scene::key_with(cat, doms, &floors, &uses, &kit)) || self.built_shape != shape {
                let mut sc = Scene::build_with(cat, doms, who, &floors, &uses, &kit);
                // The applications' own stack, when this session may see it.
                if let Some(s) = &self.serving {
                    sc.add_serving(s);
                }
                self.built_shape = shape;
                self.scene = Some(sc);
                self.lit = None;
                // A rebuilt scene renumbers its nodes: a node focus would point elsewhere.
                self.trail.retain(|f| !matches!(f, Focus::Node(_) | Focus::Bundle(_)));
                self.last_pick = None;
                self.flat = None;
                self.aim();
            }
        }
        let Some(_) = &self.scene else {
            ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
                ui.centered_and_justified(|ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(rect.height() * 0.4);
                        ui.spinner();
                        ui.label(RichText::new("reading the stack's catalog…").color(theme::muted()));
                    });
                });
            });
            return None;
        };
        let compact = rect.width() < 760.0;

        // ── input: drag turns, right-drag/shift-drag/two fingers/WASD move
        // through space, the wheel zooms, Esc walks out ─────────────────────
        // (operator, 2026-10-16: "user need be able to take their view and
        // move it through space so not always looking at the middle forever")
        let resp = ui.interact(rect, ui.id().with("atlas-scene"), Sense::click_and_drag());
        let typing = ctx.memory(|m| m.focused().is_some());
        let (shift, touch_pan) = ctx.input(|i| (i.modifiers.shift, i.multi_touch().map(|t| t.translation_delta)));
        // Flown like a ship (operator, 2026-10-21: "improve user controls to
        // be able to experience the views of scientific software space to
        // feel more space like ... like a spaceship view"): W/S thrust along
        // where it looks, A/D slide, E/Q rise and sink, Shift boosts; it keeps
        // its way and drifts to a stop. Dragging turns the view where it is;
        // right-drag (or two fingers) circles what is ahead; the wheel closes
        // in or pulls back. Speed goes with how far out it is.
        let (cy, sy, cp, sp) = (self.cam.yaw.cos(), self.cam.yaw.sin(), self.cam.pitch.cos(), self.cam.pitch.sin());
        let right = v3(cy, 0.0, -sy);
        let up = v3(-sp * sy, cp, -sp * cy);
        let back = v3(sy * cp, sp, cy * cp);
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        let thrust = if typing {
            v3(0.0, 0.0, 0.0)
        } else {
            ctx.input(|i| {
                let k = |a: egui::Key| i.key_down(a) as i32 as f32;
                back.mul(k(egui::Key::S) - k(egui::Key::W)).add(right.mul(k(egui::Key::D) - k(egui::Key::A))).add(v3(0.0, k(egui::Key::E) - k(egui::Key::Q), 0.0))
            })
        };
        let top = self.cam.dist.max(0.3) * if shift { 3.5 } else { 0.9 };
        self.vel = self.vel.add(thrust.mul(top * 3.0 * dt)).mul((-dt * 3.0).exp());
        if thrust.len() > 0.0 || self.vel.len() > top * 0.01 {
            let mv = self.vel.mul(dt);
            self.goal.target = self.goal.target.add(mv);
            self.cam.target = self.cam.target.add(mv);
            self.idle_since = now;
            ctx.request_repaint();
        } else {
            self.vel = v3(0.0, 0.0, 0.0);
        }
        let pan = if shift && resp.dragged_by(egui::PointerButton::Primary) { resp.drag_delta() } else { touch_pan.unwrap_or(Vec2::ZERO) };
        let orbit = resp.dragged_by(egui::PointerButton::Secondary) || resp.dragged_by(egui::PointerButton::Middle);
        if pan != Vec2::ZERO {
            // The screen's right and up in the world, and how far a pixel
            // reaches at the target's depth.
            let per_px = self.cam.dist / (rect.width().min(rect.height()) * 0.72);
            let mv = right.mul(-pan.x * per_px).add(up.mul(pan.y * per_px));
            self.goal.target = self.goal.target.add(mv);
            self.cam.target = self.cam.target.add(mv);
            self.idle_since = now;
            ctx.request_repaint();
        } else if resp.dragged() {
            let d = resp.drag_delta();
            // Where the eye is, before it turns.
            let eye = self.cam.target.add(back.mul(self.cam.dist));
            self.goal.yaw -= d.x * 0.005;
            self.goal.pitch = (self.goal.pitch + d.y * 0.004).clamp(-1.45, 1.45);
            self.cam.yaw = self.goal.yaw;
            self.cam.pitch = self.goal.pitch;
            if !orbit {
                // Turning the ship: the eye stays, what it looks at swings.
                let b = v3(self.cam.yaw.sin() * self.cam.pitch.cos(), self.cam.pitch.sin(), self.cam.yaw.cos() * self.cam.pitch.cos());
                let t = eye.sub(b.mul(self.cam.dist));
                self.cam.target = t;
                self.goal.target = t;
            }
            self.idle_since = now;
        }
        if resp.hovered() {
            let (scroll, zoom) = ctx.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            if scroll != 0.0 || zoom != 1.0 {
                self.goal.dist = (self.goal.dist * (-scroll * 0.0018).exp() / zoom).clamp(0.15, 120.0);
                self.idle_since = now;
            }
        }
        if !typing && ctx.input(|i| i.key_pressed(egui::Key::H)) {
            self.immersive = !self.immersive;
            self.idle_since = now - if self.immersive { TOUR_AFTER } else { 0.0 };
        }
        if !typing && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.immersive {
                self.immersive = false;
            } else if !self.trail.is_empty() {
                self.back();
            }
        }
        // ← and → walk the axis a layer at a time, kernel to the person.
        // Only layers with something in them.
        let step = ctx.input(|i| {
            let k = |a| i.key_pressed(a) as i32;
            k(egui::Key::ArrowRight) - k(egui::Key::ArrowLeft)
        });
        if !typing && step != 0 {
            self.step_layer(step);
            self.idle_since = now;
        }
        // Any touch anywhere, over the rail too, counts: the tour must stop
        // the moment the person reaches for it. Immersed, only a real touch
        // does - a mouse drifting across the screen is not taking over.
        let moved = !self.immersive && ctx.input(|i| i.pointer.delta() != Vec2::ZERO);
        if moved || ctx.input(|i| i.pointer.any_down() || i.smooth_scroll_delta != Vec2::ZERO || (!i.keys_down.is_empty() && !i.key_down(egui::Key::H))) {
            self.idle_since = now;
        }

        // ── the tour: idle, it walks the stack top down, group by group ────
        if let Some(t) = &self.tour {
            if self.idle_since > t.since {
                // Touched: back to where they left it, and theirs again.
                self.trail = self.tour.take().unwrap().before;
                self.aim();
            } else if now - t.since > t.hold() {
                let t = self.tour.as_mut().unwrap();
                t.at = (t.at + 1) % t.stops.len();
                t.since = now;
                self.trail = t.stops[t.at].clone();
                self.aim();
            }
        } else if now - self.idle_since > if self.immersive { 2.0 } else { TOUR_AFTER } && !typing && !matches!(self.focus(), Focus::Node(_) | Focus::Bundle(_)) {
            let stops = Tour::stops(self.scene.as_ref().unwrap());
            self.tour = Some(Tour { stops, at: 0, since: now, before: self.trail.clone() });
            self.trail = self.tour.as_ref().unwrap().stops[0].clone();
            self.aim();
        }

        // ── the camera eases toward its goal; idle, the stack turns ───────
        let dt = ctx.input(|i| i.stable_dt).min(0.1);
        // Touring, it glides: a stop is reached slowly, and watched arriving.
        let touring = self.tour.is_some();
        let k = 1.0 - (-dt * if touring { 1.1 } else { 5.0 }).exp();
        self.cam.target = self.cam.target.lerp(self.goal.target, k);
        self.cam.dist += (self.goal.dist - self.cam.dist) * k;
        self.cam.yaw += (self.goal.yaw - self.cam.yaw) * k;
        self.cam.pitch += (self.goal.pitch - self.cam.pitch) * k;
        let moving = self.cam.target.sub(self.goal.target).len() > 0.002 || (self.cam.dist - self.goal.dist).abs() > 0.002;
        // A picked star is circled once the person lets go (operator,
        // 2026-10-21: "i really like ... the circular obsercvation of the
        // thing clicked"); the rest of the sky only after a longer rest.
        let picked_star = matches!(self.focus(), Focus::Node(_) | Focus::Bundle(_));
        let rest = if picked_star { 1.5 } else { 8.0 };
        let turning = touring || (now - self.idle_since > rest && ctx.input(|i| i.focused));
        if turning {
            // A slow orbit round whatever it is looking at (operator,
            // 2026-10-16: "I liked how the circle orbit views looked");
            // touring, slower still - a breath.
            self.goal.yaw += dt * if touring { 0.035 } else if picked_star { 0.09 } else { 0.06 };
            self.cam.yaw = self.goal.yaw;
        }
        if moving || turning {
            ctx.request_repaint();
        }

        self.lighting();
        if self.picked_at.is_nan() {
            self.picked_at = now;
        }
        let sc = self.scene.as_ref().unwrap();
        // A canopy of hundreds of domains takes smaller dots than a handful.
        let many = sc.counts.get(&Layer::Domain).copied().unwrap_or(0);
        let scale = |l: Layer| if l == Layer::Domain && many > 40 { 0.55 } else { 1.0 };
        // The night photograph behind it again (operator, 2026-10-18: "put the
        // camping image back as the background on the visual side, can
        // slightly dim it so the network is better seen"): backdrop.rs paints
        // it under everything; here a veil, heavier at the top where the
        // stars are busiest, so the stack's own points read over it.
        if !theme::is_light() {
            let mut m = egui::Mesh::default();
            // Darker than the photograph wants: the data reads first.
            let (top, low) = (Color32::from_black_alpha(175), Color32::from_black_alpha(115));
            m.colored_vertex(rect.left_top(), top);
            m.colored_vertex(rect.right_top(), top);
            m.colored_vertex(rect.right_bottom(), low);
            m.colored_vertex(rect.left_bottom(), low);
            m.add_triangle(0, 1, 2);
            m.add_triangle(0, 2, 3);
            ui.painter().add(m);
        }
        let pr = Proj::new(&self.cam, rect);
        let painter = ui.painter_at(rect);
        let (lit, lines) = match &self.lit {
            Some((_, l, e)) => (l.as_slice(), e.as_slice()),
            None => (&[][..], &[][..]),
        };
        let focused = !lit.is_empty();
        let light = theme::is_light();
        let ink = if light { Color32::from_rgb(0x1c, 0x1f, 0x2b) } else { Color32::WHITE };

        // ── the orbits, and the two nuclei's glow ──────────────────────────
        let focus = self.focus();
        for o in &sc.orbits {
            let mine = matches!(focus, Focus::Layer(x) | Focus::Group(x, _) if x == o.layer);
            let a = if mine { 70 } else if focused { 10 } else if light { 40 } else { 30 };
            let pts: Vec<Pos2> = (0..=120).filter_map(|j| pr.at(o.at(j as f32 / 120.0 * std::f32::consts::TAU, o.r)).map(|p| p.0)).collect();
            if pts.len() > 2 {
                let lc = o.layer.color();
                painter.add(egui::Shape::line(pts, Stroke::new(if mine { 1.3 } else { 0.9 }, Color32::from_rgba_unmultiplied(lc.r(), lc.g(), lc.b(), a))));
            }
        }
        let suns = sc.kernels.iter().map(|&k| (Some(k), (0xf4, 0xe9, 0xc8))).chain([(sc.you, (0xcf, 0xe3, 0xff))]);
        for (n, rgb) in suns {
            let Some((p, s, _)) = n.and_then(|n| pr.at(sc.nodes[n as usize].pos)) else { continue };
            for (m, a) in [(9.0, 6u8), (5.0, 12), (2.6, 26)] {
                // Capped: up close the glow had grown into discs over the stack.
                painter.circle_filled(p, (0.16 * s * m).min(14.0 * m), Color32::from_rgba_unmultiplied(rgb.0, rgb.1, rgb.2, if light { a / 2 } else { a }));
            }
        }

        // The inspection view (inspect.rs) covers the middle of the canvas:
        // what is under it is not under the pointer.
        let inspecting = self.inspect_area(rect, compact);
        let pointer = resp.hover_pos().filter(|p| inspecting.is_none_or(|(a, _)| !a.contains(*p)));
        // Isolated, what the selection does not touch is not drawn at all.
        let hide = self.isolate && focused;

        // ── edges: ambient when nothing is focused, else the lit ones ──────
        let proj: Vec<Option<(Pos2, f32, f32)>> = sc.nodes.iter().map(|n| pr.at(n.pos)).collect();
        let tone = |c: Color32, a: u8| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a);
        if !focused || matches!(self.focus(), Focus::Layer(_)) {
            let a = if focused { 6 } else if light { 22 } else { 16 };
            for e in sc.edges.iter().filter(|e| e.ambient) {
                if let (Some(p), Some(q)) = (proj[e.from as usize], proj[e.to as usize]) {
                    painter.line_segment([p.0, q.0], Stroke::new(0.7, tone(sc.nodes[e.from as usize].layer.color(), a)));
                }
            }
        } else if !hide {
            for e in sc.edges.iter().filter(|e| e.ambient) {
                if let (Some(p), Some(q)) = (proj[e.from as usize], proj[e.to as usize]) {
                    painter.line_segment([p.0, q.0], Stroke::new(0.6, tone(ink, 5)));
                }
            }
        }
        // ── what each env set holds: a curve to each group of packages ────
        // One route per (set, group) - every package it stands for a real
        // Member edge - leaving the set toward its language, up over the
        // rings and down onto the group from above, so a line to the far side
        // of a ring goes round it, not through it (operator, 2026-10-21:
        // "dedicated routes between groups"). As heavy as its count, in the
        // language's colour. Hovered, it is previewed; clicked, it is chosen
        // and parts into its packages where it arrives.
        let env_view = matches!(self.focus(), Focus::Layer(Layer::Env | Layer::R | Layer::Python | Layer::Rust | Layer::Julia) | Focus::Group(Layer::Env | Layer::R | Layer::Python | Layer::Rust | Layer::Julia, _));
        let most = sc.bundles.iter().map(|b| b.n).max().unwrap_or(1).max(1) as f32;
        let chosen = match self.focus() {
            Focus::Bundle(b) => Some(b),
            _ => None,
        };
        let mut bundle_hit: Option<(u32, f32)> = None;
        let mut bundle_counts: Vec<(Pos2, String, Color32, bool)> = Vec::new();
        for (k, b) in sc.bundles.iter().enumerate() {
            let k = k as u32;
            let from = sc.nodes[b.env as usize].pos;
            let out = v3(b.to.x - from.x, 0.0, b.to.z - from.z);
            let reach = out.len().max(1e-3);
            let dir = out.mul(1.0 / reach);
            let up = v3(0.0, HG_STEP * 0.8, 0.0);
            let ctl = [from, from.add(dir.mul(reach * 0.25)).add(up), b.to.add(up.mul(1.3)).sub(dir.mul(reach * 0.1)), b.to];
            let (Some(p0), Some(p1), Some(p2), Some(p3)) = (pr.at(ctl[0]), pr.at(ctl[1]), pr.at(ctl[2]), pr.at(ctl[3])) else { continue };
            let picked = chosen == Some(k) || matches!(self.focus(), Focus::Node(n) if n == b.env);
            let hov = self.bundle_hover == Some(k);
            if hide && !picked {
                continue;
            }
            let a = if hov || picked { 230.0 } else if !focused || env_view { 110.0 } else { 14.0 };
            let w = (b.n as f32 / most).sqrt();
            let st = Stroke::new(0.4 + 2.6 * w + if hov || picked { 1.0 } else { 0.0 }, tone(b.layer.color(), (a * (0.25 + 0.75 * w)) as u8));
            let curve = egui::epaint::CubicBezierShape::from_points_stroke([p0.0, p1.0, p2.0, p3.0], false, Color32::TRANSPARENT, st);
            let pts = curve.flatten(Some(1.0));
            if let Some(m) = pointer {
                let d = pts.windows(2).map(|s| dist_to_segment(m, s[0], s[1])).fold(f32::INFINITY, f32::min);
                if d < 6.0 && bundle_hit.is_none_or(|h| d < h.1) {
                    bundle_hit = Some((k, d));
                }
            }
            painter.add(curve);
            if chosen == Some(k) {
                // Parted where it arrives: one dashed line per package.
                let split = egui::epaint::CubicBezierShape::from_points_stroke([p0.0, p1.0, p2.0, p3.0], false, Color32::TRANSPARENT, st).sample(0.8);
                for &m in &b.members {
                    if let Some((q, _, _)) = proj[m as usize] {
                        inspect::stroke_rel(&painter, &[split, q], Rel::Member, Stroke::new(0.8, tone(b.layer.color(), 170)));
                    }
                }
            }
            if (hov || picked || (!focused && w > 0.55)) && b.n > 0 {
                bundle_counts.push((pts[pts.len() / 2], b.n.to_string(), b.layer.color(), hov || picked));
            }
        }
        let heavy = lines.len() > 1500;
        // Picked, its lines grow out of it over most of a second: what it
        // touches directly first and brightest, what lies further down after.
        let sel_now = match self.focus() {
            Focus::Node(n) => Some(n),
            _ => None,
        };
        let grown = |delay: f64| (((now - self.picked_at - delay) / 0.7).clamp(0.0, 1.0)) as f32;
        let ease = |t: f32| 1.0 - (1.0 - t).powi(3);
        if sel_now.is_some() && grown(0.5) < 1.0 {
            ctx.request_repaint();
        }
        for &k in lines.iter().filter(|_| chosen.is_none()) {
            let e = &sc.edges[k as usize];
            if let (Some(p), Some(q)) = (proj[e.from as usize], proj[e.to as usize]) {
                let c = sc.nodes[e.from as usize].layer.color();
                let direct = sel_now.is_some_and(|s| e.from == s || e.to == s);
                // What lies further down stays behind what it touches directly
                // (screenshot, 2026-10-21: tidyverse's lineage to glibc buried
                // its own thirty dependencies).
                let width = if direct { 1.8 } else if sel_now.is_some() { 0.8 } else if heavy { 0.8 } else { 1.3 };
                let alpha = if direct { 210 } else if sel_now.is_some() { 45 } else if heavy { 70 } else { 140 };
                // From the picked end outward.
                let (a, b) = if sel_now == Some(e.to) { (q.0, p.0) } else { (p.0, q.0) };
                let t = if sel_now.is_some() { ease(grown(if direct { 0.0 } else { 0.5 })) } else { 1.0 };
                if t <= 0.0 {
                    continue;
                }
                // Membership dashed, everything built-on solid.
                inspect::stroke_rel(&painter, &[a, a + (b - a) * t], e.rel, Stroke::new(width, tone(c, alpha)));
            }
        }
        // The hovered node's own edges, whatever the focus.
        if let Some(h) = self.hover.filter(|&h| (h as usize) < sc.nodes.len()) {
            for &k in sc.up[h as usize].iter().chain(&sc.down[h as usize]) {
                let e = &sc.edges[k as usize];
                if let (Some(p), Some(q)) = (proj[e.from as usize], proj[e.to as usize]) {
                    painter.line_segment([p.0, q.0], Stroke::new(1.4, tone(ink, 120)));
                }
            }
        }

        // ── each compile, traced down: its star, the chain of what it is
        // built on to glibc, then its machine's kernel floor - so which box,
        // and so which glibc and kernel, is never a guess ───────────────────
        for c in &self.live.now {
            let path = Building::trace(sc, c);
            let pts: Vec<Pos2> = path.iter().filter_map(|&n| proj[n].map(|p| p.0)).collect();
            if pts.len() >= 2 {
                painter.add(egui::Shape::line(pts.clone(), Stroke::new(3.0, tone(theme::amber(), 40))));
                painter.add(egui::Shape::line(pts, Stroke::new(1.4, tone(theme::amber(), 190))));
            }
            // Where it links: glibc, named with its version for this arch.
            if let (Some(f), Some(g)) = (Building::floor_of(sc, c), path.iter().copied().find(|&n| sc.nodes[n].label == "sys-libs/glibc")) {
                if let Some((p, _, _)) = proj[g] {
                    let t = format!("{} links glibc {} ({})", c.name, f.glibc, arch_word(&f.arch));
                    painter.text(p + egui::vec2(0.0, -14.0), Align2::CENTER_BOTTOM, t, FontId::proportional(11.0), tone(theme::amber(), 230));
                }
            }
        }

        // ── nodes, far to near ─────────────────────────────────────────────
        let mut order: Vec<usize> = (0..sc.nodes.len()).filter(|&i| proj[i].is_some() && !(hide && !lit[i])).collect();
        order.sort_by(|a, b| proj[*b].unwrap().2.total_cmp(&proj[*a].unwrap().2));
        let mut hit: Option<(u32, f32)> = None;
        let sel = match self.focus() {
            Focus::Node(n) => Some(n),
            _ => None,
        };
        // Per node: 1 queued on a machine, 2 compiling now.
        let mark = match &self.replay {
            Some(r) => r.marks(sc),
            None => self.live.marks(sc),
        };
        // A package that came as a binary built elsewhere (provenance.rs).
        let foreign = self.foreign(sc);
        // Built with patches applied: Some(kit's own among them).
        let patched = self.patched(sc);
        let pulse = ((now * 3.0).sin() * 0.5 + 0.5) as f32;
        if mark.contains(&2) {
            ctx.request_repaint();
        }
        for &i in &order {
            let nd = &sc.nodes[i];
            let (p, s, depth) = proj[i].unwrap();
            let raw = nd.layer.size() * scale(nd.layer) * s;
            let r = star_radius(nd.layer, raw);
            let fog = (1.25 - (depth - self.cam.dist) / 7.0).clamp(0.35, 1.0);
            let on = !focused || lit[i];
            let a = if on { fog } else { fog * 0.13 };
            // Unlit, a near dot stays a speck rather than a blur across the view.
            let r = if on { r } else { r.min(5.0) };
            let c = sc.tint(nd);
            // Not built yet (known, or queued): a ring where the star will be.
            if (sc.planned[i] || mark[i] == 1) && mark[i] != 2 {
                painter.circle_stroke(p, r.max(2.4), Stroke::new(1.2, tone(c, (a * 210.0) as u8)));
            } else {
                painter.circle_filled(p, r, tone(c, (a * 235.0) as u8));
            }
            // Downloaded as a binary built elsewhere: a dashed amber ring,
            // always, whatever is lit - what a validation has to explain.
            if foreign[i] {
                let ring: Vec<Pos2> = (0..=24).map(|j| p + egui::Vec2::angled(j as f32 / 24.0 * std::f32::consts::TAU) * (r + 3.5)).collect();
                painter.extend(egui::Shape::dashed_line(&ring, Stroke::new(1.4, tone(theme::amber(), (if on { 230.0 } else { 120.0 }) as u8)), 2.5, 2.0));
            }
            // Built with patches: a small cross at its shoulder - solid where
            // the stack's own kit patched it, open where EasyBuild's did.
            if let Some(ours) = patched[i] {
                let at = p + egui::vec2(r + 3.0, -(r + 3.0));
                let c = tone(PATCH, (if on { 240.0 } else { 120.0 }) as u8);
                let w = if ours { 2.2 } else { 1.3 };
                painter.line_segment([at - egui::vec2(3.5, 0.0), at + egui::vec2(3.5, 0.0)], Stroke::new(w, c));
                painter.line_segment([at - egui::vec2(0.0, 3.5), at + egui::vec2(0.0, 3.5)], Stroke::new(w, c));
            }
            // Compiling now: a slow amber pulse round it.
            if mark[i] == 2 {
                painter.circle_stroke(p, r + 4.0 + 6.0 * pulse, Stroke::new(1.8, tone(theme::amber(), (230.0 * (1.0 - 0.6 * pulse)) as u8)));
            }
            if matches!(nd.layer, Layer::Domain | Layer::You | Layer::Env) && on {
                painter.circle_stroke(p, r + 2.5, Stroke::new(1.0, tone(c, (a * 90.0) as u8)));
            }
            if sel == Some(i as u32) || self.hover == Some(i as u32) {
                painter.circle_stroke(p, r + 5.0, Stroke::new(1.6, tone(ink, 220)));
            }
            // Picked: a ring goes out from it once, as its lines start to grow.
            if sel == Some(i as u32) {
                let t = (((now - self.picked_at) / 0.9).clamp(0.0, 1.0)) as f32;
                if t < 1.0 {
                    painter.circle_stroke(p, r + 5.0 + 46.0 * (1.0 - (1.0 - t).powi(2)), Stroke::new(2.0, tone(c, (200.0 * (1.0 - t)) as u8)));
                    ctx.request_repaint();
                }
            }
            // What was inspected last, found again in the overview.
            if !focused && self.last_pick == Some(i as u32) {
                painter.circle_stroke(p, r + 8.0 + 3.0 * pulse, Stroke::new(1.6, tone(ink, 200)));
            }
            if let Some(m) = pointer {
                // Every star drawn can be picked, lit or not: up close among
                // something's traces, the next thing is a click away rather
                // than back out through Esc (operator, 2026-10-22). A lit star
                // wins over an unlit one under the same point.
                let d = m.distance(p);
                let reach = if on { r.max(6.0) + 3.0 } else { r.max(5.0) + 1.0 };
                let score = if on { d - 4.0 } else { d };
                if d <= reach && hit.is_none_or(|h| score <= h.1 + 2.0) {
                    hit = Some((i as u32, score));
                }
            }
        }

        // ── labels: layers at the left rim, the canopy, the focus ──────────
        let font = |s: f32| FontId::proportional(s);
        let label = |p: Pos2, text: &str, size: f32, c: Color32, anchor: Align2| {
            let g = painter.layout_no_wrap(text.to_owned(), font(size), c);
            let r = anchor.anchor_size(p, g.size());
            painter.rect_filled(r.expand2(egui::vec2(4.0, 1.0)), 3, if light { Color32::from_white_alpha(150) } else { Color32::from_black_alpha(120) });
            painter.galley(r.min, g, c);
        };
        // Labels that would land on one already drawn are left out, nearest
        // and biggest first: a stack of names was unreadable (screenshot,
        // 2026-10-18: the env sets' "r4.2 · 14 r4.3 · 14 …" in one clump).
        let placed = std::cell::RefCell::new(Vec::<Rect>::new());
        let label_free = |p: Pos2, text: &str, size: f32, c: Color32, anchor: Align2| {
            let g = painter.layout_no_wrap(text.to_owned(), font(size), c);
            let r = anchor.anchor_size(p, g.size()).expand2(egui::vec2(5.0, 2.0));
            if placed.borrow().iter().any(|o| o.intersects(r)) {
                return;
            }
            placed.borrow_mut().push(r);
            label(p, text, size, c, anchor);
        };
        // The stewards' names on the canvas, where they landed: a click opens
        // everything that steward looks after.
        let mut owner_hits: Vec<(Rect, String)> = Vec::new();
        if !focused {
            // Each ring says what it is and where it sits, at its right
            // edge - level rings keep their edges as the sky turns. Where
            // the right has no room, the left; a line that would land on
            // another keeps only its name.
            let side = |o: &Orbit| {
                let pts: Vec<Pos2> = (0..72).filter_map(|j| pr.at(o.at(j as f32 / 72.0 * std::f32::consts::TAU, o.r)).map(|p| p.0)).collect();
                let l = pts.iter().copied().min_by(|a, b| a.x.total_cmp(&b.x))?;
                let r = pts.iter().copied().max_by(|a, b| a.x.total_cmp(&b.x))?;
                Some((l, r))
            };
            // Name and count on one line, away from the middle: the
            // two-line descriptions crossed each other once the rings sat
            // close (operator's screenshots, 2026-10-21). What a layer is,
            // the rail and the hover say.
            let described = |l: Pos2, r: Pos2, layer: Layer| {
                let c = tone(layer.color(), 240);
                let n = sc.counts.get(&layer).copied().unwrap_or(0);
                let text = if matches!(layer, Layer::Kernel) || n == 0 { layer.label().to_owned() } else { format!("{} · {n}", layer.label()) };
                let right = r.x - rect.center().x >= rect.center().x - l.x;
                let (at, anchor) = if right { (r + egui::vec2(12.0, 0.0), Align2::LEFT_CENTER) } else { (l - egui::vec2(12.0, 0.0), Align2::RIGHT_CENTER) };
                label_free(at, &text, 12.5, c, anchor);
            };
            // The kernel first: the waist everything meets at.
            let ks: Vec<Pos2> = sc.kernels.iter().filter_map(|&k| proj[k as usize].map(|p| p.0)).collect();
            if let (Some(l), Some(r)) = (ks.iter().copied().min_by(|a, b| a.x.total_cmp(&b.x)), ks.iter().copied().max_by(|a, b| a.x.total_cmp(&b.x))) {
                let s = sc.kernels.first().and_then(|&k| proj[k as usize]).map_or(0.0, |p| p.1);
                let pad = egui::vec2((0.16 * s).min(14.0) + 10.0, 0.0);
                described(l - pad, r + pad, Layer::Kernel);
            }
            // A language's tiers, each at its ring's outer edge, quieter than
            // the ring names; and its largest owners at their slices.
            for o in sc.orbits.iter().filter(|o| o.tier.is_some() && o.n > 0) {
                let t = o.tier.unwrap();
                if let Some((l, r)) = side(o) {
                    let out = if r.x - rect.center().x >= rect.center().x - l.x { (r + egui::vec2(10.0, 0.0), Align2::LEFT_CENTER) } else { (l - egui::vec2(10.0, 0.0), Align2::RIGHT_CENTER) };
                    label_free(out.0, &format!("{} · {}", t.label(), o.n), 11.5, tone(theme::text(), 235), out.1);
                }
            }
            for (layer, gs) in sc.groups.iter().filter(|(l, _)| lobe_angle(**l).is_some()) {
                for (name, m, c) in gs.iter().filter(|g| g.0 != owners::INDEPENDENT && g.0 != owners::UNLISTED).take(6) {
                    if let Some((p, _, _)) = pr.at(*c) {
                        let text = format!("{name} {m}");
                        label_free(p, &text, 11.0, tone(layer.color(), 235), Align2::CENTER_CENTER);
                        let r = Align2::CENTER_CENTER.anchor_size(p, painter.layout_no_wrap(text, font(11.0), Color32::WHITE).size()).expand2(egui::vec2(5.0, 2.0));
                        if placed.borrow().contains(&r) {
                            owner_hits.push((r, name.clone()));
                        }
                    }
                }
            }
            let mut stack: Vec<(Pos2, Pos2)> = Vec::new();
            for o in sc.orbits.iter().filter(|o| o.named) {
                if let Some((l, r)) = side(o) {
                    described(l, r, o.layer);
                    if o.layer != Layer::Domain {
                        stack.push((l, r));
                    }
                }
            }
            // Each star by its name: the floor it guarantees.
            for &k in &sc.kernels {
                if let Some((p, s, _)) = proj[k as usize] {
                    let name = if sc.floors.is_empty() { "kernel" } else { sc.nodes[k as usize].label.as_str() };
                    label(p + egui::vec2(0.0, 0.16 * s + 8.0), name, 11.0, tone(Layer::Kernel.color(), 230), Align2::CENTER_TOP);
                }
            }
        }
        if let Focus::Layer(l) | Focus::Group(l, _) = self.focus() {
            if let Some(gs) = sc.groups.get(&l) {
                // Largest groups first (gs is), so the big ones keep their names.
                for (name, m, c) in gs.iter().take(24) {
                    if let Some((p, _, _)) = pr.at(*c) {
                        label_free(p, &format!("{name} · {m}"), 11.0, tone(l.color(), 235), Align2::CENTER_CENTER);
                    }
                }
            }
        }
        let canopy = sc.counts.get(&Layer::Domain).copied().unwrap_or(0) <= 40 || matches!(self.focus(), Focus::Layer(Layer::Domain) | Focus::Node(_));
        // Nearest first, so a near star keeps its name over a far one; the
        // one picked and the ones compiling are always named.
        let many_lit = lit.iter().filter(|x| **x).count() <= 40;
        // What the picked star touches directly is named, whatever the count:
        // where things are, in the picture (operator, 2026-10-21: "i didnt
        // exactly know where things were because i was expecting a visual").
        let mut near = vec![false; sc.nodes.len()];
        // A steward's view names every package it looks after.
        for k in sc.whose(&self.focus()) {
            near[k as usize] = true;
        }
        if let Some(s) = sel {
            for &e in sc.up[s as usize].iter().chain(&sc.down[s as usize]) {
                let e = &sc.edges[e as usize];
                near[e.from as usize] = true;
                near[e.to as usize] = true;
            }
        }
        for &i in order.iter().rev() {
            let nd = &sc.nodes[i];
            let (p, s, _) = proj[i].unwrap();
            let raw = nd.layer.size() * scale(nd.layer) * s;
            let r = star_radius(nd.layer, raw);
            let on = !focused || lit[i];
            let show = match nd.layer {
                Layer::You => true,
                // The connection layer: every set named (there are few).
                Layer::Env => on,
                // The serving column: a handful of stars, each named.
                Layer::Host | Layer::Route | Layer::Edge => on,
                Layer::Domain => on && (canopy || sel == Some(i as u32)),
                // Close enough, every star says its name (operator,
                // 2026-10-21: "see the individual names of things when close
                // enough") - nearest first, so a crowd keeps the near ones.
                _ => sel == Some(i as u32) || near[i] || (focused && on && many_lit) || (on && raw >= CLOSE),
            };
            let last = !focused && self.last_pick == Some(i as u32);
            // Env sets were always named, and a dozen of them stacked into one
            // unreadable column near the axis: now they give way like the rest.
            let must = mark[i] == 2 || sel == Some(i as u32) || nd.layer == Layer::You || last;
            let (text, size, c) = if mark[i] == 2 {
                (format!("compiling · {}", nd.label), 11.5, theme::amber())
            } else if show {
                let kind = nd.dom.and_then(|d| sc.domains.get(d)).map_or("", |d| d.app.as_str());
                let name = if last {
                    format!("{} · inspected last", nd.label)
                } else if nd.layer == Layer::Env {
                    nd.label.trim_start_matches("env/").to_owned()
                } else if sc.planned[i] || mark[i] == 1 {
                    format!("{} · to build", nd.label)
                } else if matches!(kind, "rust" | "process") {
                    format!("{} · {}", nd.label, crate::api::app_kind_word(kind))
                } else {
                    nd.label.clone()
                };
                (name, if nd.layer == Layer::You { 14.0 } else { 11.0 }, if nd.layer == Layer::You { ink } else { tone(sc.tint(nd), 240) })
            } else {
                continue;
            };
            let at = p + egui::vec2(r + 6.0, 0.0);
            if must {
                // Always shown - but stepped down past another must-show
                // label rather than over it (two compiles side by side read
                // "compiling · ccompiling", 2026-10-18).
                let g = painter.layout_no_wrap(text.clone(), font(size), c);
                let mut at = at;
                for _ in 0..4 {
                    let r = Align2::LEFT_CENTER.anchor_size(at, g.size()).expand2(egui::vec2(5.0, 2.0));
                    if !placed.borrow().iter().any(|o| o.intersects(r)) {
                        break;
                    }
                    at.y += g.size().y + 4.0;
                }
                label(at, &text, size, c, Align2::LEFT_CENTER);
                placed.borrow_mut().push(Align2::LEFT_CENTER.anchor_size(at, g.size()).expand2(egui::vec2(5.0, 2.0)));
            } else {
                label_free(at, &text, size, c, Align2::LEFT_CENTER);
            }
        }

        // The bundles' counts: always for the chosen or previewed one, and in
        // the overview for the heaviest.
        for (p, t, c, must) in &bundle_counts {
            if *must {
                label(*p, t, 11.5, tone(*c, 245), Align2::CENTER_CENTER);
            } else {
                label_free(*p, t, 10.5, tone(*c, 220), Align2::CENTER_CENTER);
            }
        }

        // ── hover and click ────────────────────────────────────────────────
        self.hover = hit.map(|h| h.0);
        // A star under the pointer wins over a line.
        self.bundle_hover = if self.hover.is_none() { bundle_hit.map(|h| h.0) } else { None };
        let mut act = None;
        if let (None, None, Some(m)) = (self.hover, self.bundle_hover, pointer) {
            if let Some((_, o)) = owner_hits.iter().find(|(r, _)| r.contains(m)) {
                label(m + egui::vec2(14.0, 14.0), &format!("{o} - click for everything it looks after, and what it is built on here"), 12.0, ink, Align2::LEFT_TOP);
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
        if let (Some(k), Some(m)) = (self.bundle_hover, pointer) {
            let b = &sc.bundles[k as usize];
            let t = format!("{} holds {} {} · {}   (membership) - click to see them", sc.nodes[b.env as usize].label, b.n, b.layer.label(), b.group);
            label(m + egui::vec2(14.0, 14.0), &t, 12.0, ink, Align2::LEFT_TOP);
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if let Some(h) = self.hover {
            let nd = &sc.nodes[h as usize];
            let what = match nd.layer {
                Layer::Domain | Layer::You | Layer::Kernel => nd.layer.label().to_owned(),
                _ => format!("{} · {}", nd.layer.label(), nd.group),
            };
            let what = match mark[h as usize] {
                2 => format!("{what} · compiling now"),
                1 => format!("{what} · queued to build"),
                _ if sc.planned[h as usize] => format!("{what} · not built yet"),
                _ => what,
            };
            if let Some(m) = pointer {
                let how = match (nd.sw.is_some(), nd.dom.is_some(), sel == Some(h)) {
                    (_, true, _) => "   ·   double-click to enter",
                    (true, _, true) => "   ·   double-click for its project",
                    (true, _, false) => "   ·   click to fly to it",
                    _ => "",
                };
                label(m + egui::vec2(14.0, 14.0), &format!("{}   {what}{how}", nd.label), 12.0, ink, Align2::LEFT_TOP);
            }
            ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        if resp.double_clicked() {
            // A domain: into the IDE in it. A piece of software: its project
            // (operator, 2026-10-21: "clicking on it twice can take the user
            // to its domain project").
            if let Some(h) = self.hover {
                let nd = &sc.nodes[h as usize];
                act = match (nd.dom.and_then(|d| sc.domains.get(d)), nd.sw, &cat) {
                    (Some(d), _, _) => Some(Act::Enter(d.clone())),
                    (None, Some(i), Some(cat)) => Some(project_act(sc, cat, i as usize)),
                    _ => None,
                };
            }
        } else if let (true, None, None, Some(o)) = (resp.clicked(), self.hover, self.bundle_hover, pointer.and_then(|m| owner_hits.iter().find(|(r, _)| r.contains(m))).map(|(_, o)| o.clone())) {
            self.go(Focus::Owner(o));
        } else if resp.clicked() {
            match (self.hover, self.bundle_hover) {
                (Some(h), _) => {
                    self.flat = None;
                    self.go(Focus::Node(h));
                }
                (None, Some(k)) => self.choose_bundle(k),
                // A tap on empty sky closes the card that is open.
                // A tap on empty sky folds the details away; the star stays
                // picked and the camera where it is.
                (None, None) if matches!(self.focus(), Focus::Node(_) | Focus::Bundle(_)) => {
                    if self.panel == Panel::Full {
                        self.panel = Panel::Bar;
                    }
                }
                (None, None) => {}
            }
        }

        // ── the rail, the breadcrumb, the card ─────────────────────────────
        let pad = 14.0;
        if self.immersive {
            // Only the way back out, faint in the corner, and the tour's own line.
            let b = Rect::from_min_size(Pos2::new(rect.right() - pad - 34.0, rect.top() + pad), egui::vec2(34.0, 30.0));
            let r = ui.put(b, egui::Button::new(RichText::new("◑").size(15.0).color(tone(theme::muted(), 150))).frame(false));
            if r.on_hover_text("show the controls again · H or Esc").clicked() {
                self.immersive = false;
            }
            if self.tour.is_some() {
                let what = match self.focus() {
                    Focus::Group(l, g) => format!("{} · {g}", l.label()),
                    Focus::Layer(l) => l.label().to_owned(),
                    _ => "the whole stack".to_owned(),
                };
                painter.text(Pos2::new(rect.left() + pad + 6.0, rect.top() + pad + 14.0), Align2::LEFT_CENTER, what, font(15.0), tone(theme::text(), 200));
            }
            return act;
        }
        let top = Rect::from_min_size(rect.min + egui::vec2(pad, pad), egui::vec2(rect.width() - 2.0 * pad, 34.0));
        // On a phone the bar sits on a band of the ground, fading out below:
        // the stack's labels ("no vhost", "login", "nginx and upstreams")
        // read through Back and the title (IMG_8271, 2026-10-24).
        if compact {
            let band = Rect::from_min_max(rect.min, Pos2::new(rect.right(), top.bottom() + 6.0));
            ui.painter().rect_filled(band, 0, theme::ground().gamma_multiply(0.88));
            let fade = Rect::from_min_max(Pos2::new(rect.left(), band.bottom()), Pos2::new(rect.right(), band.bottom() + 18.0));
            let mut mesh = egui::Mesh::default();
            let (c0, c1) = (theme::ground().gamma_multiply(0.88), Color32::TRANSPARENT);
            mesh.colored_vertex(fade.left_top(), c0);
            mesh.colored_vertex(fade.right_top(), c0);
            mesh.colored_vertex(fade.right_bottom(), c1);
            mesh.colored_vertex(fade.left_bottom(), c1);
            mesh.add_triangle(0, 1, 2);
            mesh.add_triangle(0, 2, 3);
            ui.painter().add(mesh);
        }
        if let Some(a) = self.top_bar(ui, top, current, compact) {
            act = Some(a);
        }
        // A machine on and nothing compiling (a Prefix, the suite's
        // EasyBuild, a publish): say which machine and which stage.
        let idle_on = if self.live.now.is_empty() { self.live.machines.iter().find(|m| m.on).cloned() } else { None };
        if let Some(m) = idle_on {
            let text = m.pill();
            let room = rect.width() - 2.0 * pad - 24.0;
            let mut job = egui::text::LayoutJob::simple_singleline(text, FontId::proportional(12.0), theme::amber());
            job.wrap = egui::text::TextWrapping::truncate_at_width(room);
            let g = ui.painter().layout_job(job);
            let y = rect.top() + pad + if compact { 42.0 } else { 44.0 };
            let h = if compact { 32.0 } else { 24.0 };
            let pill = Rect::from_center_size(Pos2::new(rect.center().x, y + h / 2.0), egui::vec2(g.size().x + 24.0, h));
            let r = ui.interact(pill, ui.id().with("atlas-machine-on"), Sense::click());
            ui.painter().rect_filled(pill, 12, theme::bg().gamma_multiply(if r.hovered() { 0.95 } else { 0.8 }));
            ui.painter().rect_stroke(pill, 12, Stroke::new(1.0, tone(theme::amber(), 150)), egui::StrokeKind::Inside);
            ui.painter().galley(pill.center() - g.size() / 2.0, g, theme::amber());
            if r.on_hover_text("the build machines: stage, set, package and their logs").clicked() {
                self.machines_open = !self.machines_open;
            }
        }
        // What is compiling right now, under the bar: a click flies there
        // (on a phone, where there is no rail, it opens the machines).
        if let Some(c) = self.live.now.first().cloned() {
            let item = c.item;
            let more = self.live.now.len() - 1;
            let queued = self.live.queued.len();
            // Which machine, which arch - and so which glibc it links and
            // which kernel floor it will run on.
            let chain = self.scene.as_ref().map_or(String::new(), |sc| Building::chain(sc, &c));
            let ver = if c.version.is_empty() { String::new() } else { format!(" {}", c.version) };
            let mut text = format!("●  compiling {}{ver} on {}{chain}", c.name, c.short_machine());
            if more > 0 {
                text.push_str(&format!("  +{more}"));
            }
            if queued > 0 {
                text.push_str(&format!("  ·  {queued} queued"));
            }
            let target = self.scene.as_ref().and_then(|sc| Building::node_of(sc, item));
            let y = rect.top() + pad + if compact { 42.0 } else { 44.0 };
            // Never wider than the screen: on a phone the chain ran off both
            // edges (screenshot, 2026-10-18) - the short form, then an ellipsis.
            let room = rect.width() - 2.0 * pad - 24.0;
            let mut g = ui.painter().layout_no_wrap(text.clone(), FontId::proportional(12.0), theme::amber());
            if g.size().x > room {
                let short = format!("●  compiling {} · {}", c.name, c.short_machine());
                let mut job = egui::text::LayoutJob::simple_singleline(short, FontId::proportional(12.0), theme::amber());
                job.wrap = egui::text::TextWrapping::truncate_at_width(room);
                g = ui.painter().layout_job(job);
            }
            let h = if compact { 32.0 } else { 24.0 };
            let pill = Rect::from_center_size(Pos2::new(rect.center().x, y + h / 2.0), egui::vec2(g.size().x + 24.0, h));
            let r = ui.interact(pill, ui.id().with("atlas-compiling"), Sense::click());
            ui.painter().rect_filled(pill, 12, theme::bg().gamma_multiply(if r.hovered() { 0.95 } else { 0.8 }));
            ui.painter().rect_stroke(pill, 12, Stroke::new(1.0, tone(theme::amber(), 150)), egui::StrokeKind::Inside);
            ui.painter().galley(pill.center() - g.size() / 2.0, g, theme::amber());
            let r = r.on_hover_text(if target.is_some() { "fly to it in the stack" } else { "not in the catalog yet - Builds › Queue has it" });
            if r.clicked() {
                if compact {
                    self.machines_open = !self.machines_open;
                } else if let Some(n) = target {
                    self.go(Focus::Node(n as u32));
                }
            }
            if r.hovered() && target.is_some() {
                ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        }
        if self.machines_open {
            let mut open = true;
            let w = (rect.width() - 2.0 * pad).min(460.0);
            egui::Window::new("Build machines")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .default_width(w)
                .max_width(w)
                .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, pad + 84.0))
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().max_height(rect.height() * 0.7).show(ui, |ui| machines_panel(ui, &self.live.machines, true));
                });
            self.machines_open = open;
        }
        // One box to ask for what the stack lacks (operator, 2026-10-18: "a
        // single box which allows the user to push an add software button
        // which sends a request"): under the rail on a desk, at the bottom on a
        // phone unless a card is open there.
        // Title, line, gap, button, and the window's own 12 px all round.
        let add_h = 104.0;
        if !compact {
            let rail = Rect::from_min_size(rect.min + egui::vec2(pad, pad + 46.0), egui::vec2(250.0, (rect.height() - 2.0 * pad - 46.0 - add_h - 30.0).clamp(120.0, 660.0)));
            self.rail(ui, rail);
        }
        // On a phone it is the bar's "+ Software" (top_bar_compact).
        if !compact {
            let w = 250.0;
            let at = Pos2::new(rect.left() + pad, rect.bottom() - pad - add_h - 22.0);
            if self.add_box(ui, Rect::from_min_size(at, egui::vec2(w, add_h))) {
                act = Some(Act::AddSoftware);
            }
        }
        // A chosen bundle's card is its env set's.
        let carded = match self.focus() {
            Focus::Node(n) => Some(n),
            Focus::Bundle(b) => self.scene.as_ref().and_then(|sc| sc.bundles.get(b as usize)).map(|b| b.env),
            _ => None,
        };
        if let Some((area, n)) = self.inspect_area(rect, compact) {
            self.inspect(ui, area, n, cat.as_deref());
        }
        match (carded, self.panel) {
            (Some(n), Panel::Full) => {
                let w = if compact { rect.width() - 2.0 * pad } else { 360.0 };
                // On a phone, under half the screen: the star stays in view.
                let h = if compact { rect.height() * 0.38 } else { (rect.height() - 2.0 * pad - 46.0).min(620.0) };
                let at = if compact { Pos2::new(rect.left() + pad, rect.bottom() - pad - h) } else { Pos2::new(rect.right() - pad - w, rect.top() + pad + 46.0) };
                if let Some(a) = self.card(ui, Rect::from_min_size(at, egui::vec2(w, h)), n, cat.as_deref()) {
                    act = Some(a);
                }
            }
            (Some(n), Panel::Bar) => {
                let w = if compact { rect.width() - 2.0 * pad } else { 360.0 };
                let at = if compact { Pos2::new(rect.left() + pad, rect.bottom() - pad - 48.0) } else { Pos2::new(rect.right() - pad - w, rect.top() + pad + 46.0) };
                self.bar(ui, Rect::from_min_size(at, egui::vec2(w, 48.0)), n);
            }
            _ => {}
        }
        // The tour card (showcase.rs, an Area anchored bottom right) sits
        // over the scene: what the atlas draws along the bottom keeps clear
        // of it. Under it, the key and the caption could not be read
        // (request #34, 2026-10-18: "hard to see things are behind").
        let card = ui.ctx().memory(|m| {
            let id = egui::Id::new("showcase");
            if m.areas().visible_last_frame(&egui::LayerId::new(egui::Order::Middle, id)) { m.area_rect(id) } else { None }
        })
        .filter(|c| c.intersects(rect));
        // The bottom line's centre and width: the whole width, or the part
        // left of the card.
        let (mid, span) = match card {
            Some(c) if c.left() - rect.left() > 320.0 => {
                let (l, r) = (rect.left() + pad, c.left() - pad);
                ((l + r) / 2.0, r - l)
            }
            _ => (rect.center().x, rect.width() - 2.0 * pad),
        };
        let bottom = Pos2::new(mid, rect.bottom() - 12.0);
        // What is behind the domains, counted: a key to the tint (app_color).
        if !compact {
            let doms = self.scene.as_ref().map_or(&[][..], |sc| sc.domains.as_slice());
            let counts: Vec<(&str, usize)> = APP_KINDS.iter().map(|k| (*k, doms.iter().filter(|d| d.app == *k).count())).filter(|x| x.1 > 0).collect();
            if !counts.is_empty() {
                let lp = ui.painter_at(rect);
                let floor = card.map_or(rect.bottom(), |c| c.top().min(rect.bottom()));
                let mut y = floor - pad - 18.0 * counts.len() as f32;
                lp.text(Pos2::new(rect.right() - pad, y - 18.0), Align2::RIGHT_TOP, "applications", FontId::proportional(11.0), tone(theme::muted(), 220));
                for (k, n) in counts {
                    let c = app_color(k).unwrap_or(Layer::Domain.color());
                    let t = format!("{}  {n}", crate::api::app_kind_word(k));
                    let g = lp.text(Pos2::new(rect.right() - pad, y), Align2::RIGHT_TOP, t, FontId::proportional(12.0), tone(c, 235));
                    lp.circle_filled(Pos2::new(g.left() - 9.0, g.center().y), 4.0, c);
                    y += 18.0;
                }
            }
        }
        if let Some(t) = &self.tour {
            // What the stop is and what that layer does - no "n of m"
            // (operator, 2026-10-18: "we don't need it saying something like
            // tour 12 ... you're just going through and explaining").
            let _ = t;
            // The stop's name only: no line of small grey text under it
            // (operator, 2026-10-21: "all of that text is invisible to me").
            let what = match self.focus() {
                Focus::Group(l, g) => format!("{} · {g}", l.label()),
                Focus::Layer(l) => l.label().to_owned(),
                _ => "The whole stack".to_owned(),
            };
            centred_wrap(&painter, bottom, what, font(13.0), tone(theme::text_strong(), 230), span);
        }
        if let Some(Err(e)) = &domains {
            centred_wrap(&painter, bottom - egui::vec2(0.0, 18.0), format!("the domains did not list: {e}"), font(11.0), theme::warn(), span);
        }
        act
    }

    /// A translucent window over the scene, like the IDE's panes; it takes
    /// the pointer, so a click on it never falls through to the stack.
    fn window<R>(ui: &mut egui::Ui, rect: Rect, id: &str, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        ui.interact(rect, ui.id().with(id), Sense::click_and_drag());
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
        egui::Frame::new()
            .fill(theme::bg().gamma_multiply(0.8))
            .stroke(Stroke::new(1.0, theme::pane_brd()))
            .corner_radius(egui::CornerRadius::same(theme::WIN_RADIUS))
            .inner_margin(egui::Margin::same(12))
            .show(&mut child, |ui| {
                ui.set_min_size(rect.size() - egui::vec2(24.0, 24.0));
                ui.set_max_size(rect.size() - egui::vec2(24.0, 24.0));
                add(ui)
            })
            .inner
    }

    /// The Add software box; true when its button is pressed.
    fn add_box(&mut self, ui: &mut egui::Ui, rect: Rect) -> bool {
        let public = self.public;
        // The question, then one full-width button under it: side by side the
        // button sat over the words (operator screenshot, 2026-10-18: "issing
        // something section looks ugly").
        // Centred and evenly spaced: the title, one quiet line under it, then
        // the button the full width, with the same air above and below
        // (operator, 2026-10-18: "seems too close together ... should feel
        // symmetric and clear and easy to ask for").
        Self::window(ui, rect, "atlas-add", |ui| {
            ui.vertical_centered(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("Missing software?").size(13.5).strong().color(theme::text_strong()));
                ui.add_space(8.0);
                let label = if public { "Sign in to request software" } else { "+  Add software" };
                let tip = if public { "requests need an account: sign in, then ask from here" } else { "a request to the ndexr team for a package, a library or a tool the stack does not have" };
                ui.add_sized([ui.available_width(), 30.0], theme::primary_button(label)).on_hover_text(tip).clicked()
            })
            .inner
        })
    }

    /// One layer along the axis (← and →, or the phone's ‹ › buttons).
    fn step_layer(&mut self, step: i32) {
        let Some(sc) = self.scene.as_ref() else { return };
        let axis: Vec<Layer> = Layer::axis().iter().copied().filter(|l| matches!(l, Layer::Kernel | Layer::Domain) || sc.counts.get(l).copied().unwrap_or(0) > 0).collect();
        if axis.is_empty() {
            return;
        }
        let here = match self.focus() {
            Focus::Layer(l) | Focus::Group(l, _) => axis.iter().position(|x| *x == l),
            Focus::Node(n) => axis.iter().position(|x| *x == sc.nodes[n as usize].layer),
            Focus::Bundle(_) => axis.iter().position(|x| *x == Layer::Env),
            Focus::All | Focus::Owner(_) | Focus::Person(_) => None,
        };
        // From the whole stack, → starts at Prefix and ← at the top.
        let to = match here {
            Some(k) => (k as i32 + step).clamp(0, axis.len() as i32 - 1) as usize,
            None if step > 0 => 0,
            None => axis.len() - 1,
        };
        self.lens(Focus::Layer(axis[to]));
    }

    /// The phone's top bar: one row (operator, 2026-10-18: "I can't really go
    /// next or back or anything like that in mobile"; 2026-10-23: "people need
    /// to get around mobile much more easily than desktop"). Back, where you
    /// are (a tap goes home), the layer steps, and More for the rest: Home, add
    /// software, immersive. The two rows it replaces took 80 px of a phone's
    /// stack for buttons used once a visit.
    fn top_bar_compact(&mut self, ui: &mut egui::Ui, rect: Rect) -> Option<Act> {
        let mut act = None;
        let thumb = egui::vec2(38.0, 34.0);
        let mut row = ui.new_child(UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        row.spacing_mut().item_spacing.x = 6.0;
        let back = !self.trail.is_empty();
        if row.add_enabled(back, egui::Button::new(RichText::new("‹ Back").size(14.0)).min_size(thumb)).clicked() {
            self.back();
        }
        let here = match (self.focus(), &self.scene) {
            (Focus::All, _) | (_, None) => TAGLINE.to_owned(),
            (Focus::Layer(l), _) => l.label().to_owned(),
            (Focus::Group(_, g), _) => g,
            (Focus::Node(n), Some(sc)) => sc.nodes.get(n as usize).map_or(String::new(), |n| n.label.clone()),
            (Focus::Bundle(b), Some(sc)) => sc.bundles.get(b as usize).map_or(String::new(), |b| format!("{} · {}", b.layer.label(), b.group)),
            (Focus::Owner(o), _) => format!("{o} · everything it looks after"),
            (Focus::Person(p), _) => format!("{p} · their system"),
        };
        row.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // A plain button with a popup, so More is as tall as its
            // neighbours: egui's menu_button sizes itself to its text.
            let more = ui.add(egui::Button::new(RichText::new("More").size(14.0)).min_size(egui::vec2(52.0, 34.0)));
            egui::Popup::menu(&more).show(|ui| {
                let big = |t: &str| RichText::new(t).size(15.0);
                if ui.add_enabled(back, egui::Button::new(big("Home · the whole stack")).min_size(egui::vec2(220.0, 38.0))).clicked() {
                    self.go(Focus::All);
                    ui.close();
                }
                let label = if self.public { "Sign in" } else { "+ Software · ask for what the stack lacks" };
                if ui.add(egui::Button::new(big(label)).min_size(egui::vec2(220.0, 38.0))).clicked() {
                    act = Some(Act::AddSoftware);
                    ui.close();
                }
                if ui.add(egui::Button::new(big("◐ Only the stack")).min_size(egui::vec2(220.0, 38.0))).clicked() {
                    self.immersive = true;
                    self.idle_since = f64::NEG_INFINITY;
                    ui.close();
                }
            });
            if ui.add(egui::Button::new(RichText::new("›").size(17.0)).min_size(thumb)).on_hover_text("the next layer").clicked() {
                self.step_layer(1);
            }
            if ui.add(egui::Button::new(RichText::new("‹").size(17.0)).min_size(thumb)).on_hover_text("the layer before").clicked() {
                self.step_layer(-1);
            }
            let t = RichText::new(here).size(13.5).color(theme::text_strong());
            if ui.add(egui::Label::new(t).truncate().sense(Sense::click())).on_hover_text("the whole stack").clicked() {
                self.go(Focus::All);
            }
        });
        act
    }

    fn top_bar(&mut self, ui: &mut egui::Ui, rect: Rect, current: Option<&str>, compact: bool) -> Option<Act> {
        if compact {
            return self.top_bar_compact(ui, rect);
        }
        let act = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(egui::Layout::left_to_right(egui::Align::Center)));
        child.set_clip_rect(rect.intersect(ui.clip_rect()));
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 6.0;
        // The name is the bar's above (app.rs mode_bar, view.rs's own): here
        // only what this view is, and a click back to the whole stack.
        // The concept first (operator, 2026-10-18: "science, software,
        // visualized"), then how far this view reaches.
        let tag = RichText::new(TAGLINE).size(13.0).color(theme::text_strong());
        if ui.add(egui::Label::new(tag).sense(Sense::click())).on_hover_text("the whole stack").clicked() {
            self.go(Focus::All);
        }
        ui.label(RichText::new(if self.public { "· kernel to every domain" } else { "· kernel to application" }).size(11.5).color(theme::muted()));
        ui.add_space(10.0);
        // The layers one at a time, Prefix up to the applications (← → too).
        if ui.add(egui::Button::new(RichText::new("‹").size(15.0)).min_size(egui::vec2(26.0, 24.0))).on_hover_text("the layer below (←)").clicked() {
            self.step_layer(-1);
        }
        if ui.add(egui::Button::new(RichText::new("›").size(15.0)).min_size(egui::vec2(26.0, 24.0))).on_hover_text("the layer above (→) - Prefix up to the applications").clicked() {
            self.step_layer(1);
        }
        // Under 1100 points (a laptop zoomed in with Ctrl +, operator
        // 2026-10-18) the breadcrumb keeps only its last two steps.
        let tight = rect.width() < 1100.0;
        ui.add_space(10.0);
        let Some(sc) = &self.scene else { return None };
        let crumbs: Vec<(Focus, String)> = self
            .trail
            .iter()
            .map(|f| {
                let name = match f {
                    Focus::All => "all".to_owned(),
                    Focus::Layer(l) => l.label().to_owned(),
                    Focus::Group(_, g) => g.clone(),
                    Focus::Node(n) => sc.nodes.get(*n as usize).map_or(String::new(), |n| n.label.clone()),
                    Focus::Bundle(b) => sc.bundles.get(*b as usize).map_or(String::new(), |b| format!("{} · {}", b.layer.label(), b.group)),
                    Focus::Owner(o) => format!("{o} · its packages"),
                    Focus::Person(p) => format!("{p} · their system"),
                };
                (f.clone(), name)
            })
            .collect();
        let skip = if compact || tight { crumbs.len().saturating_sub(2) } else { 0 };
        let mut to = None;
        for (k, (f, name)) in crumbs.iter().enumerate().skip(skip) {
            ui.label(RichText::new("›").color(theme::hush()));
            let last = k + 1 == crumbs.len();
            let t = RichText::new(name).size(13.0).color(if last { theme::text_strong() } else { theme::muted() });
            if ui.add(egui::Label::new(t).sense(Sense::click())).clicked() && !last {
                to = Some(f.clone());
            }
        }
        if let Some(f) = to {
            self.go(f);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // The way into the IDE is the mode bar over every screen (app.rs).
            let _ = current;
            if !self.trail.is_empty() && ui.button("← Back").on_hover_text("Esc").clicked() {
                self.back();
            }
            ui.button("Controls").on_hover_text("W S  forward, back\nA D  slide left, right\nE Q  rise, sink\nShift  faster\ndrag  turn the view\nright-drag  circle what is ahead\nwheel  closer, further\nclick a star  fly to it\nEsc  back · H  immersive");
            if ui.button("◐ Immersive").on_hover_text("hide everything but the stack and the tour's messages, and let it walk itself · H").clicked() {
                self.immersive = true;
                self.idle_since = f64::NEG_INFINITY;
            }
        });
        act
    }

    /// Per node: its package came as a binary built elsewhere.
    fn foreign(&self, sc: &Scene) -> Vec<bool> {
        let book = self.prov.slot.lock().unwrap();
        let Some(book) = book.as_ref() else { return vec![false; sc.nodes.len()] };
        sc.nodes
            .iter()
            .map(|n| match n.layer {
                Layer::R => book.external("R", &n.label),
                Layer::Python => book.external("Python", &n.label),
                Layer::Julia => book.external("Julia", &n.label),
                Layer::Prefix => book.external("Prefix", &n.label),
                _ => false,
            })
            .collect()
    }

    /// The downloaded binaries, each with what the next release does about it.
    fn binary_fates(&self, sc: &Scene, foreign: &[u32]) -> Vec<(u32, String)> {
        let book = self.prov.slot.lock().unwrap();
        let Some(book) = book.as_ref() else { return Vec::new() };
        foreign.iter().map(|&k| {
            let n = &sc.nodes[k as usize];
            (k, book.fate(prov_lang(n.layer), &n.label).0)
        }).collect()
    }

    /// Per node: built with patches (EasyBuild's install record), and
    /// whether the stack's own kit carries one of them.
    fn patched(&self, sc: &Scene) -> Vec<Option<bool>> {
        let book = self.patches.slot.lock().unwrap();
        let Some(book) = book.as_ref() else { return vec![None; sc.nodes.len()] };
        sc.nodes.iter().map(|n| if n.layer == Layer::EasyBuild { book.mark(&n.label) } else { None }).collect()
    }

    fn rail(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let mut to = None;
        let focus = self.focus();
        let foreign: Vec<u32> = self.scene.as_ref().map(|sc| self.foreign(sc).iter().enumerate().filter(|x| *x.1).map(|x| x.0 as u32).collect()).unwrap_or_default();
        let fates: Vec<(u32, String)> = self.scene.as_ref().map(|sc| self.binary_fates(sc, &foreign)).unwrap_or_default();
        // Patched modules, the kit's own first (operator, 2026-10-22).
        let mut patched: Vec<(u32, bool)> = self.scene.as_ref().map(|sc| self.patched(sc).iter().enumerate().filter_map(|(k, p)| p.map(|o| (k as u32, o))).collect()).unwrap_or_default();
        patched.sort_by_key(|&(_, o)| !o);
        Self::window(ui, rect, "atlas-rail", |ui| {
            let Some(sc) = &self.scene else { return };
            // A deployment over time: the generation the build machines made,
            // star by star, as they finished.
            let mut close = false;
            match self.replay.as_mut() {
                None => {
                    if ui.add(egui::Button::new(RichText::new("▶ Replay a deployment").size(13.0).strong()).min_size(egui::vec2(ui.available_width(), 30.0)))
                        .on_hover_text("play back a whole generation of the stack - Gentoo Prefix, EasyBuild, the package sets - as the build machines made it")
                        .clicked()
                    {
                        self.replay = Some(Replay::new());
                    }
                }
                Some(r) => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("DEPLOYMENT").size(11.0).strong().color(theme::amber()));
                        let picks: Vec<(usize, String, i64)> = r.deps.iter().enumerate().map(|(k, d)| (k, d.label.clone(), d.lo)).collect();
                        for (k, label, lo) in picks {
                            if ui.selectable_label(k == r.pick, RichText::new(label).size(13.0)).clicked() && k != r.pick {
                                r.pick = k;
                                r.t = lo;
                                r.playing = true;
                            }
                        }
                    });
                    if let Some(d) = r.dep().cloned() {
                        ui.label(RichText::new(format!("{} runs on {} machines", d.runs, d.boxes.len())).size(12.5).color(theme::text()));
                        ui.horizontal_wrapped(|ui| {
                            if ui.selectable_label(r.on.is_none(), RichText::new("first on any").size(12.5)).on_hover_text("each star at its first finished build on any machine").clicked() {
                                r.on = None;
                            }
                            for b in &d.boxes {
                                let short = b.trim_start_matches("four-box-").to_owned();
                                if ui.selectable_label(r.on.as_deref() == Some(b.as_str()), RichText::new(short).size(12.5)).on_hover_text(format!("{b}'s own build, star by star")).clicked() {
                                    r.on = Some(b.clone());
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            let play = if r.playing { "⏸ Pause" } else if r.t >= d.hi { "⟲ Again" } else { "▶ Play" };
                            if ui.button(RichText::new(play).size(13.0)).clicked() {
                                if r.t >= d.hi {
                                    r.t = d.lo;
                                }
                                r.playing = !r.playing || r.t == d.lo;
                            }
                            if ui.button(RichText::new("✕ Now").size(13.0)).on_hover_text("back to the stack as it is").clicked() {
                                close = true;
                            }
                        });
                        let mut t = r.t as f64;
                        ui.spacing_mut().slider_width = ui.available_width() - 8.0;
                        if ui.add(egui::Slider::new(&mut t, d.lo as f64..=d.hi as f64).show_value(false)).changed() {
                            r.t = t as i64;
                            r.playing = false;
                        }
                        let (built, all) = r.progress(sc);
                        ui.label(RichText::new(crate::ui::software::stamp(r.t)).size(14.0).strong().color(theme::text_strong()));
                        ui.label(RichText::new(format!("{built} of {all} built")).size(13.0).color(theme::text()));
                    }
                    ui.add_space(6.0);
                }
            }
            if close {
                self.replay = None;
            }
            ui.add(egui::TextEdit::singleline(&mut self.search).hint_text("find software or a domain").desired_width(f32::INFINITY));
            // No audience tabs and no paragraph of numbers under them
            // (operator, 2026-10-21: "all of that text is invisible to me ...
            // dont do that and dont do tabs of text"): the rail is search and
            // rows to click.
            ui.add_space(4.0);
            let q = self.search.trim().to_lowercase();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                if !q.is_empty() {
                    let mut hits: Vec<(usize, i32)> = sc
                        .nodes
                        .iter()
                        .enumerate()
                        .filter_map(|(i, n)| {
                            let l = n.label.to_lowercase();
                            let base = l.rsplit('/').next().unwrap_or(&l);
                            let s = if base == q { 0 } else if base.starts_with(&q) { 1 } else if l.contains(&q) { 2 } else { return None };
                            Some((i, s))
                        })
                        .collect();
                    hits.sort_by_key(|h| (h.1, sc.nodes[h.0].label.len()));
                    let who: Vec<(String, usize)> = sc.people().into_iter().filter(|(p, _)| p.contains(&q)).collect();
                    for (p, n) in &who {
                        if row(ui, Layer::Domain.color(), p, &format!("{n} domain{}", if *n == 1 { "" } else { "s" }), focus == Focus::Person(p.clone())).clicked() {
                            to = Some(Focus::Person(p.clone()));
                        }
                    }
                    if hits.is_empty() && who.is_empty() {
                        ui.label(RichText::new("nothing by that name").color(theme::muted()));
                    }
                    for (i, _) in hits.into_iter().take(60) {
                        let n = &sc.nodes[i];
                        if row(ui, n.layer.color(), &n.label, n.layer.label(), focus == Focus::Node(i as u32)).clicked() {
                            to = Some(Focus::Node(i as u32));
                        }
                    }
                    return;
                }
                // The stack first, its groups under the open layer (operator,
                // 2026-10-23: "the left bar needs to start with the user seeing
                // the groups and moving from them, not the binaries"): what is
                // building, people, patches and binaries come after, folded.
                // A machine that is on is the news: before the stack.
                let live: Vec<BoxNow> = self.live.machines.iter().filter(|m| m.on).cloned().collect();
                if !live.is_empty() {
                    machines_panel(ui, &live, true);
                    ui.add_space(8.0);
                }
                ui.label(RichText::new("THE STACK, TOP DOWN").size(10.0).color(theme::hush()));
                for &l in Layer::rail() {
                    let n = sc.counts.get(&l).copied().unwrap_or(0);
                    if n == 0 && l != Layer::Domain {
                        continue;
                    }
                    let on = matches!(&focus, Focus::Layer(x) | Focus::Group(x, _) if *x == l) || (l == Layer::You && sc.you.is_some_and(|y| focus == Focus::Node(y)));
                    let count = match l {
                        Layer::You => sc.you.map_or(String::new(), |y| sc.nodes[y as usize].label.clone()),
                        Layer::Kernel => String::new(),
                        _ => n.to_string(),
                    };
                    if row(ui, l.color(), l.label(), &count, on).on_hover_text(l.place()).clicked() {
                        to = Some(match (l, sc.you) {
                            (Layer::You, Some(y)) => Focus::Node(y),
                            (Layer::Kernel, _) => Focus::Node(sc.kernel),
                            _ => Focus::Layer(l),
                        });
                    }
                    // The open layer's groups, under it.
                    if let (true, Some(gs)) = (on && l != Layer::You, sc.groups.get(&l)) {
                        for (g, m, _) in gs {
                            let here = matches!(&focus, Focus::Group(_, x) if x == g);
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                if row(ui, l.color().gamma_multiply(0.6), g, &m.to_string(), here).clicked() {
                                    to = Some(Focus::Group(l, g.clone()));
                                }
                            });
                        }
                    }
                    if on && l == Layer::Domain {
                        for (i, n) in sc.nodes.iter().enumerate().filter(|(_, n)| n.layer == Layer::Domain) {
                            ui.horizontal(|ui| {
                                ui.add_space(14.0);
                                if row(ui, l.color().gamma_multiply(0.6), &n.label, "", false).clicked() {
                                    to = Some(Focus::Node(i as u32));
                                }
                            });
                        }
                    }
                }
                ui.add_space(8.0);
                // Building now, queued, and known but never built: where they
                // are in the stack, a click away.
                let now: Vec<Compile> = self.live.now.clone();
                let queued: Vec<(Option<usize>, String, String)> = self.live.queued.clone();
                if !now.is_empty() {
                    ui.label(RichText::new("COMPILING NOW").size(10.0).color(theme::amber()));
                    for c in &now {
                        let n = Building::node_of(sc, c.item);
                        let name = if c.version.is_empty() { c.name.clone() } else { format!("{} {}", c.name, c.version) };
                        let note = format!("{}{}", c.short_machine(), if c.arch.is_empty() { String::new() } else { format!(" · {}", arch_word(&c.arch)) });
                        let tip = format!("on {}{} - every step below it, down to the glibc it links, is lit in the stack", c.machine, Building::chain(sc, c));
                        if row(ui, theme::amber(), &name, &note, n.is_some_and(|n| focus == Focus::Node(n as u32))).on_hover_text(tip).clicked() {
                            if let Some(n) = n {
                                to = Some(Focus::Node(n as u32));
                            }
                        }
                    }
                    ui.add_space(6.0);
                }
                // The machines that are off: how their last run ended.
                let off: Vec<BoxNow> = self.live.machines.iter().filter(|m| !m.on).cloned().collect();
                if !off.is_empty() {
                    machines_panel(ui, &off, false);
                    ui.add_space(6.0);
                }
                if !queued.is_empty() {
                    ui.label(RichText::new(format!("QUEUED · {}", queued.len())).size(10.0).color(theme::hush()));
                    for (item, entry, machine) in queued.iter().take(12) {
                        let n = Building::node_of(sc, *item);
                        let dot = n.map_or(theme::hush(), |n| sc.nodes[n].layer.color().gamma_multiply(0.5));
                        if row(ui, dot, entry, machine, false).on_hover_text(if n.is_some() { "fly to where it will be" } else { "new to the stack - it appears once built" }).clicked() {
                            if let Some(n) = n {
                                to = Some(Focus::Node(n as u32));
                            }
                        }
                    }
                    ui.add_space(6.0);
                }
                // Each person's system: their domains and all they stand on
                // (operator, 2026-10-23: "to see individual user systems as
                // well"). Names come only to a signed-in viewer.
                let people = sc.people();
                if !people.is_empty() {
                    ui.label(RichText::new(format!("PEOPLE · {}", people.len())).size(10.0).color(Layer::Domain.color()));
                    for (p, n) in &people {
                        let on = matches!(&focus, Focus::Person(x) if x == p);
                        let tip = format!("{p}'s system: their domains and everything each stands on, down to the kernel");
                        if row(ui, Layer::Domain.color(), p, &format!("{n} domain{}", if *n == 1 { "" } else { "s" }), on).on_hover_text(tip).clicked() {
                            to = Some(Focus::Person(p.clone()));
                        }
                    }
                    ui.add_space(6.0);
                }
                if !patched.is_empty() {
                    let t = format!("{} PATCHED · {}", if self.show_patched { "▾" } else { "▸" }, patched.len());
                    if ui.add(egui::Label::new(RichText::new(t).size(10.0).color(PATCH)).sense(Sense::click()))
                        .on_hover_text("built with patches applied to upstream source, as each install recorded them; a solid cross: the stack's own kit carries one")
                        .clicked()
                    {
                        self.show_patched = !self.show_patched;
                    }
                    for &(k, ours) in patched.iter().take(if self.show_patched { usize::MAX } else { 0 }) {
                        let n = &sc.nodes[k as usize];
                        if row(ui, PATCH, &n.label, if ours { "kit's patch" } else { "EasyBuild's" }, focus == Focus::Node(k)).clicked() {
                            to = Some(Focus::Node(k));
                        }
                    }
                    ui.add_space(6.0);
                }
                let planned: Vec<usize> = (0..sc.nodes.len()).filter(|&i| sc.planned[i]).collect();
                if !planned.is_empty() {
                    // Folded: a dozen rows of it pushed the stack's own list
                    // out of the rail (operator's screenshot, 2026-10-21).
                    let t = format!("{} NOT BUILT YET · {}", if self.show_planned { "▾" } else { "▸" }, planned.len());
                    if ui.add(egui::Label::new(RichText::new(t).size(10.0).color(theme::hush())).sense(Sense::click())).on_hover_text("known to the stack, never built yet").clicked() {
                        self.show_planned = !self.show_planned;
                    }
                    for &i in planned.iter().take(if self.show_planned { 12 } else { 0 }) {
                        let n = &sc.nodes[i];
                        if row(ui, n.layer.color().gamma_multiply(0.5), &n.label, n.layer.label(), focus == Focus::Node(i as u32)).on_hover_text("known to the stack, never built yet: a ring where its star will be").clicked() {
                            to = Some(Focus::Node(i as u32));
                        }
                    }
                    ui.add_space(6.0);
                }
                // The downloaded binaries, by what the next release does about
                // them (operator, 2026-10-23: "where we can replace binaries
                // with source do or label for removal in next release"; the
                // kit's binary-plan.tsv), toward a binaryless deployment:
                // each is built from source or removed. After the stack: a
                // count per fate, each folded; the undecided ones open, since
                // each is a decision this release has not made yet.
                if !fates.is_empty() {
                    ui.add_space(8.0);
                    let n = |f: &str| fates.iter().filter(|x| x.1 == f).count();
                    ui.label(RichText::new(format!("DOWNLOADED BINARIES · NEXT RELEASE · {}", fates.len())).size(10.0).color(theme::hush()))
                        .on_hover_text("machine code built somewhere else and downloaded, not compiled in this stack - and what the next release of the stack does about each (ndexr.ndexr.io's stack kit, manifest/binary-plan.tsv)");
                    for (f, title, color, tip) in [
                        ("", "undecided", theme::amber(), "downloaded, and no release has decided yet: build it from source, remove it, or keep it with a reason"),
                        ("source", "to source", theme::ok(), "the next release compiles it; its card says the step it takes"),
                        ("remove", "to remove", theme::alt(), "the next release drops it: no source exists to build it from"),
                    ] {
                        let ks: Vec<u32> = fates.iter().filter(|x| x.1 == f).map(|x| x.0).collect();
                        if ks.is_empty() {
                            continue;
                        }
                        let open = self.open_fates.iter().any(|o| o == f);
                        let label = format!("{} {title}", if open { "▾" } else { "▸" });
                        if row(ui, color, &label, &n(f).to_string(), false).on_hover_text(tip).clicked() {
                            if open {
                                self.open_fates.retain(|o| o != f);
                            } else {
                                self.open_fates.push(f.to_owned());
                            }
                        }
                        if open {
                            for &k in &ks {
                                let nd = &sc.nodes[k as usize];
                                ui.horizontal(|ui| {
                                    ui.add_space(14.0);
                                    if row(ui, color.gamma_multiply(0.7), &nd.label, nd.layer.label(), focus == Focus::Node(k)).clicked() {
                                        to = Some(Focus::Node(k));
                                    }
                                });
                            }
                        }
                    }
                }
            });
        });
        if let Some(f) = to {
            if matches!(f, Focus::Node(_)) {
                self.go(f);
            } else {
                self.lens(f);
            }
        }
    }

    fn card(&mut self, ui: &mut egui::Ui, rect: Rect, n: u32, cat: Option<&Catalog>) -> Option<Act> {
        let mut act = None;
        let mut to = None;
        let sc = self.scene.as_ref()?;
        let nd = sc.nodes.get(n as usize)?;
        let (mut close, mut hide) = (false, false);
        let secs = inspect::sections(sc, n, &self.open, cat);
        // How its machine code came, set by set, and what says so.
        let came: Vec<crate::api::Provenance> = {
            self.prov.slot.lock().unwrap().as_ref().map(|b| b.of(prov_lang(nd.layer), &nd.label).to_vec()).unwrap_or_default()
        };
        // The patches its install recorded, version by version.
        let patched_with: Vec<crate::api::Patched> = if nd.layer == Layer::EasyBuild { self.patches.slot.lock().unwrap().as_ref().map(|b| b.of(&nd.label).to_vec()).unwrap_or_default() } else { Vec::new() };
        // What each of its installs was built from, file by file (patched.rs).
        let built = self.built.clone();
        let records = if nd.layer == Layer::EasyBuild { built.of(&nd.label, ui.ctx()).unwrap_or_default() } else { Vec::new() };
        let (mut toggle, mut reinspect, mut flip_isolate) = (None::<String>, None::<u32>, false);
        let (flat, isolate) = (self.flat == Some(n), self.isolate);
        Self::window(ui, rect, "atlas-card", |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 5.0, nd.layer.color());
                ui.label(RichText::new(nd.layer.label()).size(11.0).color(theme::muted()));
                if !nd.group.is_empty() {
                    ui.add(egui::Label::new(RichText::new(format!("· {}", nd.group)).size(11.0).color(theme::muted())).truncate());
                }
                // Operator, 2026-10-18: "I will click on something and see the
                // software, but then I can't close out of it."
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Thumb-sized: × hides the details altogether, ▾ folds
                    // them to a one-line bar; the star stays picked either way.
                    if ui.add(egui::Button::new(RichText::new("×").size(20.0)).frame(false).min_size(egui::vec2(44.0, 36.0))).on_hover_text("hide the details - the star stays picked").clicked() {
                        hide = true;
                    }
                    if ui.add(egui::Button::new(RichText::new("▾").size(20.0)).frame(false).min_size(egui::vec2(44.0, 36.0))).on_hover_text("fold to one line").clicked() {
                        close = true;
                    }
                });
            });
            ui.label(RichText::new(&nd.label).font(theme::display(19.0)).color(theme::text_strong()));
            // The flat view and the stack, either way round, for what has one.
            if matches!(nd.layer, Layer::Env | Layer::R | Layer::Python | Layer::Rust | Layer::Julia | Layer::Combo | Layer::EasyBuild | Layer::Prefix) {
                ui.horizontal(|ui| {
                    let (t, tip) = if flat { ("Show in the stack", "back to it in the stack, its lines lit") } else { ("Lay it flat", "every relation as a list: what it stands on left, what stands on it right, by kind") };
                    if ui.button(t).on_hover_text(tip).clicked() {
                        reinspect = Some(n);
                    }
                    if ui.selectable_label(isolate, "Isolate").on_hover_text("in the stack, hide everything it does not touch").clicked() {
                        flip_isolate = true;
                    }
                });
            }
            ui.add_space(4.0);
            let ups: Vec<u32> = sc.up[n as usize].iter().map(|&e| sc.edges[e as usize].to).collect();
            // A domain's "below" is its subdomains; the person above it is not one.
            let downs: Vec<u32> = sc.down[n as usize].iter().map(|&e| sc.edges[e as usize].from).filter(|&k| !(nd.layer == Layer::Domain && sc.nodes[k as usize].layer == Layer::You)).collect();
            match (nd.layer, nd.sw, nd.dom) {
                (_, _, Some(d)) => {
                    let d = &sc.domains[d];
                    if !d.title.is_empty() {
                        ui.label(RichText::new(&d.title).color(theme::text()));
                    }
                    ui.label(RichText::new(&d.path).monospace().size(11.0).color(theme::muted()));
                    let env = if d.env.is_empty() { "no .ndexr-env - the session's own env".to_owned() } else { d.env.clone() };
                    ui.label(RichText::new(format!("env  {env}")).size(12.0).color(theme::text()));
                    let word = crate::api::app_kind_word(&d.app);
                    if !word.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("app").size(12.0).color(theme::muted()));
                            ui.label(RichText::new(word).size(12.0).strong().color(app_color(&d.app).unwrap_or(Layer::Domain.color())));
                        });
                    }
                    if !d.target.is_empty() {
                        ui.label(RichText::new(format!("served by  {}", d.target)).size(12.0).color(theme::text()));
                    }
                    // Whose it is, and a way into their whole system.
                    if !d.owner.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("owner").size(12.0).color(theme::muted()));
                            let r = ui.add(egui::Label::new(RichText::new(format!("{} ›", d.owner)).size(12.5).underline().color(Layer::Domain.color())).sense(Sense::click()));
                            if r.on_hover_text(format!("{}'s system: their domains and everything each stands on", d.owner)).clicked() {
                                to = Some(Focus::Person(d.owner.clone()));
                            }
                        });
                    }
                    ui.add_space(8.0);
                    if ui.add(theme::primary_button(&format!("Enter {}  ⏎", d.host))).on_hover_text("the IDE, in this domain").clicked() {
                        act = Some(Act::Enter(d.clone()));
                    }
                    ui.add_space(6.0);
                }
                (_, Some(i), _) => {
                    if let Some(cat) = cat {
                        let s = cat.sw(i as usize);
                        if !s.purpose.is_empty() && s.purpose != s.group {
                            ui.label(RichText::new(s.purpose).color(theme::text()));
                        }
                        let mut desc: String = s.desc.chars().take(420).collect();
                        if s.desc.chars().count() > 420 {
                            desc.push('…');
                        }
                        ui.label(RichText::new(desc).size(12.0).color(theme::muted()));
                        if !s.versions.is_empty() {
                            ui.label(RichText::new(format!("versions  {}", s.versions.join(" · "))).size(11.5).color(theme::text()));
                        }
                        // Who controls what, each said apart (operator, 2026-10-21:
                        // "Grouping, provenance, and ownership ... answer
                        // different questions"): who keeps its source, whether
                        // the build is ours, and the role it is drawn in.
                        if let Some(t) = sc.tier.get(n as usize).copied().flatten() {
                            let fact = |ui: &mut egui::Ui, k: &str, v: &str, c: Color32, tip: &str| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new(k).size(11.0).color(theme::hush()));
                                    ui.label(RichText::new(v).size(12.0).color(c)).on_hover_text(tip);
                                });
                            };
                            let from = if s.home.is_empty() { "no homepage in its metadata".to_owned() } else { format!("read from its homepage, {}", s.home) };
                            // The steward is a way in: everything it looks after.
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("source steward").size(11.0).color(theme::hush()));
                                let r = ui.add(egui::Label::new(RichText::new(format!("{} ›", nd.group)).size(12.0).underline().color(nd.layer.color())).sense(Sense::click()));
                                if r.on_hover_text(format!("{from}\nclick: everything {} looks after, and what it is built on here", nd.group)).clicked() {
                                    to = Some(Focus::Owner(nd.group.clone()));
                                }
                            });
                            // What its own install record says (provenance.rs),
                            // never assumed: a downloaded binary is said as one.
                            let said = |r: &crate::api::Provenance| format!("{} {} in {} - {}", nd.label, r.version, r.sets.join(", "), r.evidence);
                            let tip_all = came.iter().map(said).collect::<Vec<_>>().join("\n");
                            if let Some(r) = came.iter().find(|r| r.how == "external") {
                                fact(ui, "build", "a binary built elsewhere, downloaded", theme::amber(), &tip_all);
                                ui.label(RichText::new(&r.evidence).monospace().size(10.5).color(theme::muted()));
                            if r.how == "external" {
                                // Why it was taken, from the kit's list - or that
                                // nobody recorded why.
                                let (t, c) = if r.reason.is_empty() { ("not on the stack's list of accepted binaries".to_owned(), theme::warn()) } else { (format!("accepted: {}", r.reason), theme::text()) };
                                ui.label(RichText::new(t).size(11.5).color(c));
                                // What the next release does about it (binary-plan.tsv).
                                let (t, c) = match r.fate.as_str() {
                                    "source" => (format!("next release: built from source - {}", r.plan), theme::ok()),
                                    "remove" => (format!("next release: removed - {}", r.plan), theme::alt()),
                                    _ => ("next release: undecided - no line in the kit's binary-plan.tsv".to_owned(), theme::warn()),
                                };
                                ui.add(egui::Label::new(RichText::new(t).size(12.0).color(c)).wrap());
                            }
                            } else if came.iter().any(|r| r.how == "built") {
                                fact(ui, "build", "compiled here", theme::text(), &tip_all);
                            } else if !came.is_empty() {
                                fact(ui, "build", "installed here - no compiled code", theme::text(), &tip_all);
                            } else if sc.planned[n as usize] {
                                fact(ui, "build", "not built here yet", theme::text(), "known to the stack; no build of it has succeeded on our machines");
                            } else {
                                fact(ui, "build", "no install record read on this machine", theme::muted(), "its site library on this machine's architecture does not list it, so how it came is not known here");
                            }
                            fact(ui, "role", t.label(), theme::text(), &format!("{} - a curated starting point, until observed use can say it", t.what()));
                        }
                        // One star per package, shared by every set that holds
                        // it; which of these each set has is not in the catalog.
                        if s.versions.len() > 1 && matches!(nd.layer, Layer::R | Layer::Python | Layer::Rust) {
                            ui.label(RichText::new("one record for every set that holds it - the catalog does not say which version each set has").size(10.5).color(theme::hush()));
                        }
                        if !s.module.is_empty() {
                            ui.label(RichText::new(format!("ml {}", s.module)).monospace().size(11.5).color(theme::primary()));
                        }
                        // Built from: the upstream source and its checksum, the
                        // recipe, every patch (read it here, or its public copy),
                        // the build log - one block per installed version.
                        for r in &records {
                            built_from(ui, r, patched_with.iter().find(|p| p.version == r.version), &built);
                        }
                        // Inlet runs as a session of its own (inlet.ndexr.io, 2026-10-29; until 2026-10-28
                        // its module was pimp-my-rstudio, which the stack may still carry.
                        if matches!(nd.label.as_str(), "Inlet" | "pimp-my-rstudio") {
                            ui.hyperlink_to(RichText::new("Open Inlet  ↗").size(12.5).strong(), "https://inlet.ndexr.io/")
                                .on_hover_text("start or reopen an Inlet session - ndexr's modified RStudio Server, built from the files above");
                        }
                        // Patched: every patch, whose, its size; why on hover.
                        for pv in patched_with.iter().filter(|pv| !records.iter().any(|r| r.version == pv.version)) {
                            ui.label(RichText::new(format!("patched · {} · {} patch{}", pv.version, pv.patches.len(), if pv.patches.len() == 1 { "" } else { "es" })).size(12.0).strong().color(PATCH));
                            for p in &pv.patches {
                                let whose = if p.ours { "stack kit" } else { "EasyBuild" };
                                let tip = if p.why.is_empty() { "the patch says nothing about why".to_owned() } else { p.why.clone() };
                                ui.label(RichText::new(format!("{}  ·  {whose}  ·  +{} −{}", p.file, p.added, p.removed)).monospace().size(10.5).color(theme::text())).on_hover_text(tip);
                            }
                        }
                        if nd.layer == Layer::Env {
                            let set = env_set_name(&nd.label).unwrap_or(&nd.label);
                            let pkgs = ups.iter().chain(&downs).filter(|&&k| matches!(sc.nodes[k as usize].layer, Layer::R | Layer::Python | Layer::Rust | Layer::Julia)).count();
                            ui.label(RichText::new(format!("manifests/{set}/manifest.dcf · {pkgs} packages")).monospace().size(11.0).color(theme::muted()));
                        }
                        ui.add_space(6.0);
                        // Its project: the domain its source is - the one the
                        // person has, or the name opening it would make (names.rs;
                        // 2026-10-18: "selecting a software node should reveal its
                        // project address"). The button says where it goes.
                        let kind = s.kind.key();
                        let hosts = sc.owned_root.as_ref().zip(crate::catalog::names::Identity::of(kind, s.name)).map(|(root, id)| id.hosts(root)).unwrap_or_default();
                        let have = sc.projects.iter().find(|d| hosts.contains(&d.host)).cloned();
                        let address = have.as_ref().map(|d| d.host.clone()).or_else(|| hosts.first().cloned());
                        if let Some(a) = &address {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(RichText::new("project").size(11.0).color(theme::hush()));
                                ui.label(RichText::new(a).monospace().size(11.5).color(theme::text()));
                                if have.is_none() {
                                    ui.label(RichText::new("- made when its source is opened").size(11.0).color(theme::hush()));
                                }
                                if ui.small_button("copy").on_hover_text("copy the project's address").clicked() {
                                    ui.ctx().copy_text(a.clone());
                                }
                            });
                        }
                        ui.horizontal(|ui| {
                            // Every star is a place its source can be read and
                            // changed (operator, 2026-10-17).
                            match &have {
                                Some(d) => {
                                    if ui.add(theme::primary_button("Open project  </>")).on_hover_text(format!("{} in the IDE - its source, its versions, your changes", d.host)).clicked() {
                                        act = Some(Act::Enter(d.clone()));
                                    }
                                }
                                None => {
                                    let tip = match &address {
                                        Some(a) => format!("fetch its exact source into {a} - a git project of yours: upstream as it came, your changes on work"),
                                        None => "its exact source, as git: upstream as it came, your changes on work".to_owned(),
                                    };
                                    if ui.add(theme::primary_button("Open source  </>")).on_hover_text(tip).clicked() {
                                        act = Some(Act::Source(i as usize));
                                    }
                                }
                            }
                            if ui.button("Open in Builds ↗").on_hover_text("its builds, versions and both directions of its tree, in Builds").clicked() {
                                act = Some(Act::Software(i as usize));
                            }
                        });
                    }
                }
                (Layer::Host | Layer::Route | Layer::Edge, _, _) => {
                    // What it is doing now, from the last serving answer.
                    if let Some(s) = &self.serving {
                        for line in serving::live(s, nd) {
                            ui.label(RichText::new(line).size(12.0).color(theme::text()));
                        }
                    }
                    ui.label(RichText::new(nd.layer.what()).size(11.0).color(theme::muted()));
                }
                (Layer::Kernel, _, _) => {
                    match sc.floors.iter().find(|f| f.triple == nd.group) {
                        // What this star guarantees, and how far that is proven.
                        Some(f) => {
                            ui.label(RichText::new(format!("The {} stack runs on any Linux from {} up: the oldest kernel it assumes, wherever it is cloned.", f.triple, f.min_kernel)).size(12.0).color(theme::text()));
                            ui.add_space(4.0);
                            let fact = |ui: &mut egui::Ui, k: &str, v: &str| {
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(RichText::new(k).size(11.0).color(theme::hush()));
                                    ui.label(RichText::new(v).monospace().size(11.5).color(theme::text()));
                                });
                            };
                            fact(ui, "declared by", &format!("glibc {} - its ELF ABI note: Linux ≥ {}", f.glibc, f.min_kernel));
                            fact(ui, "built against", &format!("linux-headers {}", f.headers));
                            let proven = if f.proven_on.is_empty() { "not yet run on any kernel Cove has recorded".to_owned() } else { format!("ran on {}", f.proven_on.join(", ")) };
                            fact(ui, "proven", &proven);
                            let gap = f.proven_on.iter().all(|k| !k.starts_with(&format!("{}.", f.min_kernel.trim_end_matches(".0"))));
                            if gap {
                                ui.label(RichText::new(format!("Declared, not proven at the floor: a run on Linux {} itself is what would prove it.", f.min_kernel)).size(11.0).color(theme::amber()));
                            }
                        }
                        None => {
                            ui.label(RichText::new("Every build stands on the host's Linux through Gentoo Prefix: these are the Prefix packages that need nothing else.").size(12.0).color(theme::muted()));
                        }
                    }
                }
                (Layer::You, _, _) => {
                    ui.label(RichText::new(format!("{} domains. Lit below: every env they work in, down through EasyBuild and Prefix to the kernel.", sc.domains.len())).size(12.0).color(theme::muted()));
                }
                _ => {}
            }
            ui.add_space(4.0);
            // Every relation by its kind, the same rows the inspection view
            // draws: membership apart from dependency, long lists by group.
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                for s in &secs {
                    ui.label(RichText::new(format!("{} · {}", s.title.to_uppercase(), s.count)).size(10.0).color(theme::hush()));
                    for r in &s.rows {
                        ui.horizontal(|ui| {
                            if r.depth == 1 {
                                ui.add_space(14.0);
                            }
                            if row(ui, r.color, &r.label, &r.note, false).on_hover_text(&r.tip).clicked() {
                                match &r.go {
                                    inspect::Go::Node(k) => to = Some(Focus::Node(*k)),
                                    inspect::Go::Toggle(key) => toggle = Some(key.clone()),
                                    inspect::Go::Nothing => {}
                                }
                            }
                        });
                    }
                    ui.add_space(6.0);
                }
            });
        });
        // As in the inspection: only the pass egui keeps acts.
        if ui.ctx().will_discard() {
            (toggle, to, reinspect, flip_isolate, close) = (None, None, None, false, false);
        }
        if let Some(key) = toggle {
            if !self.open.remove(&key) {
                self.open.insert(key);
            }
        }
        if let Some(f) = to {
            self.flat = None;
            self.go(f);
        }
        if let Some(n) = reinspect {
            if self.flat == Some(n) {
                self.flat = None;
                self.aim();
            } else {
                self.flat = Some(n);
            }
        }
        if flip_isolate {
            self.isolate = !self.isolate;
        }
        // Only the details fold or hide: the star stays picked, lit, framed.
        if close {
            self.panel = Panel::Bar;
        }
        if hide {
            self.panel = Panel::Hidden;
        }
        act
    }

    /// The picked star folded to one line: its name and kind, a way back to
    /// its details, and a way to hide even this - the star stays picked.
    fn bar(&mut self, ui: &mut egui::Ui, rect: Rect, n: u32) {
        let Some(nd) = self.scene.as_ref().and_then(|sc| sc.nodes.get(n as usize)) else { return };
        let (dot, name, kind) = (nd.layer.color(), nd.label.clone(), nd.layer.label());
        let mut to = None;
        Self::window(ui, rect, "atlas-bar", |ui| {
            ui.horizontal_centered(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 5.0, dot);
                ui.add(egui::Label::new(RichText::new(&name).size(14.0).strong().color(theme::text_strong())).truncate());
                ui.label(RichText::new(kind).size(12.0).color(theme::text()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(RichText::new("×").size(20.0)).frame(false).min_size(egui::vec2(44.0, 36.0))).on_hover_text("hide this bar - the star stays picked").clicked() {
                        to = Some(Panel::Hidden);
                    }
                    if ui.add(egui::Button::new(RichText::new("▴ Details").size(14.0)).min_size(egui::vec2(0.0, 36.0))).on_hover_text("its details again - or tap the star").clicked() {
                        to = Some(Panel::Full);
                    }
                });
            });
        });
        if let Some(p) = to {
            self.panel = p;
        }
    }

    /// A bundle chosen: its env set's inspection, with that group open.
    fn choose_bundle(&mut self, k: u32) {
        let Some((env, layer, group)) = self.scene.as_ref().and_then(|sc| sc.bundles.get(k as usize)).map(|b| (b.env, b.layer, b.group.clone())) else { return };
        self.open.insert(inspect::group_key(env, false, Rel::Member, layer, &group));
        self.flat = None;
        self.go(Focus::Bundle(k));
    }
}

/// The kit's sets and combinations into `slot`, with the packages of the
/// sets in `with` (comma-separated).
fn fetch_kit(slot: &Arc<std::sync::Mutex<crate::api::EnvSets>>, ctx: &egui::Context, with: &str) {
    let (slot, ctx) = (slot.clone(), ctx.clone());
    ehttp::fetch(ehttp::Request::get(format!("{}/envsets.json?with={with}", crate::ui::front::showcase::base())), move |r| {
        if let Some(v) = r.ok().filter(|r| r.ok).and_then(|r| serde_json::from_slice::<crate::api::EnvSets>(&r.bytes).ok()) {
            *slot.lock().unwrap() = v;
            ctx.request_repaint();
        }
    });
}

/// A piece of software's project: the person's domain for it when they have
/// one, else its source fetched into a new one (names.rs) - as the card's
/// "Open project" / "Open source" buttons do.
fn project_act(sc: &Scene, cat: &Catalog, i: usize) -> Act {
    let s = cat.sw(i);
    let hosts = sc.owned_root.as_ref().zip(crate::catalog::names::Identity::of(s.kind.key(), s.name)).map(|(root, id)| id.hosts(root)).unwrap_or_default();
    match sc.projects.iter().find(|d| hosts.contains(&d.host)) {
        Some(d) => Act::Enter(d.clone()),
        None => Act::Source(i),
    }
}

/// How big a star's dot is drawn, given its size at this distance (`raw`, in
/// points): it grows as it nears, to about 8 points, then shrinks again the
/// closer it comes, so up close a dot never covers its own name (operator,
/// 2026-10-21: "they should shrink when close close ish"). The kernels keep
/// their size: they are the floor.
fn star_radius(layer: Layer, raw: f32) -> f32 {
    if layer == Layer::Kernel {
        return raw.clamp(1.3, 26.0);
    }
    const PEAK: f32 = 8.0;
    if raw <= PEAK { raw.max(1.3) } else { (PEAK * (PEAK / raw).sqrt()).max(1.5) }
}

/// The colour patched software is marked in: violet, apart from the amber of
/// downloaded binaries and compiles.
const PATCH: Color32 = Color32::from_rgb(0xc6, 0x8f, 0xf5);

/// From this drawn size up (points, before star_radius), a star is near
/// enough to be named.
const CLOSE: f32 = 4.5;

/// How far `p` is from the segment a-b, in points.
fn dist_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// One line of the rail or the card: a dot, a name, a quiet right-hand note.
/// Painted text wrapped to `wrap` points, its bottom centre at `at`, each
/// line centred. Returns where it landed, so the next line can sit above it.
fn centred_wrap(painter: &egui::Painter, at: Pos2, text: String, font: FontId, color: Color32, wrap: f32) -> Rect {
    let mut job = egui::text::LayoutJob::simple(text, font, color, wrap.max(40.0));
    job.halign = egui::Align::Center;
    let g = painter.layout_job(job);
    // Centred, a galley's x = 0 is the middle of its lines.
    let r = Rect::from_min_size(Pos2::new(at.x - g.size().x / 2.0, at.y - g.size().y), g.size());
    painter.galley(Pos2::new(at.x, r.top()), g, color);
    r
}

/// The build machines: each one's name and arch, its stages as a strip
/// (done, now, to come), what it is doing in words and, when `logs`, the
/// last lines of its log - the same masked text the Logs view shows.
fn machines_panel(ui: &mut egui::Ui, ms: &[BoxNow], logs: bool) {
    let on_n = ms.iter().filter(|m| m.on).count();
    let head = if on_n > 0 { format!("BUILD MACHINES · {on_n} ON") } else { "BUILD MACHINES · ALL OFF".to_owned() };
    ui.label(RichText::new(head).size(10.0).color(if on_n > 0 { theme::amber() } else { theme::hush() }));
    for m in ms {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 14.0), Sense::hover());
            ui.painter().circle_filled(dot.center(), 4.0, if m.on { theme::amber() } else { theme::hush() });
            ui.label(RichText::new(&m.key).size(13.0).strong().color(theme::text_strong()));
            ui.label(RichText::new(arch_word(&m.arch)).size(12.0).color(theme::text()));
        });
        if m.on && !m.starting() && !m.steps.is_empty() {
            // The run's stages, left to right: done, the one it is on, to come.
            let w = ui.available_width();
            let (r, resp) = ui.allocate_exact_size(egui::vec2(w, 8.0), Sense::hover());
            let k = m.steps.len() as f32;
            for (i, name) in m.steps.iter().enumerate() {
                let x0 = r.left() + w * i as f32 / k;
                let cell = Rect::from_min_max(Pos2::new(x0 + 1.0, r.top()), Pos2::new(x0 + w / k - 1.0, r.bottom()));
                let c = match (i + 1).cmp(&m.step_index) {
                    std::cmp::Ordering::Less => theme::text().gamma_multiply(0.7),
                    std::cmp::Ordering::Equal => theme::amber(),
                    std::cmp::Ordering::Greater => theme::hush().gamma_multiply(0.5),
                };
                ui.painter().rect_filled(cell, 2, c);
                let _ = name;
            }
            resp.on_hover_text(m.steps.iter().enumerate().map(|(i, s)| format!("{} {s}", i + 1)).collect::<Vec<_>>().join(" · "));
        }
        for l in m.lines() {
            ui.label(RichText::new(l).size(12.0).color(theme::text()));
        }
        if logs && m.on && !m.starting() && !m.tail.is_empty() {
            egui::Frame::new().fill(theme::bg().gamma_multiply(0.9)).inner_margin(egui::Margin::same(6)).corner_radius(4).show(ui, |ui| {
                for l in &m.tail {
                    ui.add(egui::Label::new(RichText::new(l).monospace().size(10.5).color(theme::text())).truncate());
                }
            });
        }
    }
}

fn row(ui: &mut egui::Ui, dot: Color32, name: &str, note: &str, on: bool) -> egui::Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 21.0), Sense::click());
    let p = ui.painter();
    if on || resp.hovered() {
        p.rect_filled(rect, 4, theme::surface_hi().gamma_multiply(if on { 0.9 } else { 0.6 }));
    }
    p.circle_filled(Pos2::new(rect.left() + 8.0, rect.center().y), 4.0, dot);
    let note_g = p.layout_no_wrap(note.to_owned(), FontId::proportional(11.0), theme::muted());
    let room = (rect.width() - 22.0 - note_g.size().x - 8.0).max(20.0);
    let mut job = egui::text::LayoutJob::simple_singleline(name.to_owned(), FontId::proportional(12.5), if on { theme::text_strong() } else { theme::text() });
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let g = p.layout_job(job);
    p.galley(Pos2::new(rect.left() + 18.0, rect.center().y - g.size().y / 2.0), g, theme::text());
    p.galley(Pos2::new(rect.right() - 4.0 - note_g.size().x, rect.center().y - note_g.size().y / 2.0), note_g, theme::muted());
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_box_on_with_a_quiet_log_reads_as_starting_not_as_its_last_run() {
        let lane = serde_json::json!({"key": "arm", "power": "on", "status": "running", "state": "running", "mtime": 1000,
            "steps": ["kit", "prefix", "packages"], "step_index": 3, "started": "2026-10-24T21:36:17Z", "finished": "2026-10-24T22:00:27Z",
            "set": "Julia/1.11.7", "set_n": 9, "sets": 9, "tail": ["done"]});
        let quiet = BoxNow::read(&lane, 1000 + 89 * 60, &|_| "aarch64".into());
        assert!(quiet.starting());
        assert!(quiet.pill().contains("is on · starting"), "{}", quiet.pill());
        assert!(quiet.lines()[0].starts_with("On, starting"), "{:?}", quiet.lines());
        let busy = BoxNow::read(&lane, 1000 + 30, &|_| "aarch64".into());
        assert!(!busy.starting());
        assert_eq!(busy.lines()[0], "Stage 3 of 3: packages - Julia/1.11.7, set 9 of 9");
        let mut cove = lane.clone();
        cove["step_index"] = serde_json::json!(2);
        cove["now"] = serde_json::json!("HTTP");
        let cove = BoxNow::read(&cove, 1000 + 30, &|_| "aarch64".into());
        assert!(!cove.lines().iter().any(|l| l.contains("HTTP")), "{:?}", cove.lines());
        let off = BoxNow::read(&serde_json::json!({"key": "gpu", "power": "off", "state": "done", "finished": "2026-10-24T00:54:31Z"}), 0, &|_| "x86_64".into());
        assert_eq!(off.lines(), vec!["Off · last run done at 00:54 UTC".to_string()]);
    }

    #[test]
    fn the_tour_walks_every_group_top_down() {
        let cat = Catalog::parse(br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"items":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""}}"#).unwrap();
        let sc = Scene::build(&cat, &[], "me");
        let stops = Tour::stops(&sc);
        // Nothing built yet: the kernel, then the whole sky again.
        assert_eq!(stops.first(), Some(&vec![Focus::Layer(Layer::Kernel)]));
        assert_eq!(stops.last(), Some(&Vec::new()));
        let groups: usize = sc.groups.iter().filter(|(l, _)| **l != Layer::Domain && **l != Layer::You).map(|(_, v)| v.len()).sum();
        assert_eq!(stops.iter().filter(|t| matches!(t.last(), Some(Focus::Group(..)))).count(), groups);
    }

    #[test]
    fn domains_orbit_in_levels() {
        let cat = Catalog::parse(br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"items":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""}}"#).unwrap();
        let doms: Vec<DomainInfo> = ["a.ndexr.io", "b.ndexr.io", "x.b.ndexr.io", "y.x.b.ndexr.io", "koodle.io"]
            .iter()
            .map(|h| serde_json::from_value(serde_json::json!({ "host": h, "path": format!("/d/{h}") })).unwrap())
            .collect();
        let sc = Scene::build(&cat, &doms, "me");
        let node = |l: &str| sc.nodes.iter().position(|n| n.label == l).unwrap();
        let parent_of = |l: &str| sc.up[node(l)].iter().map(|&e| sc.nodes[sc.edges[e as usize].to as usize].label.clone()).find(|t| t.contains('.')).unwrap_or_default();
        // ndexr.io is not one of these domains: a node stands for it.
        assert!(sc.nodes[node("ndexr.io")].dom.is_none());
        assert_eq!(parent_of("b.ndexr.io"), "ndexr.io");
        assert_eq!(parent_of("x.b.ndexr.io"), "b.ndexr.io");
        assert_eq!(parent_of("y.x.b.ndexr.io"), "x.b.ndexr.io");
        // Over the kernel; each level a ring higher and wider.
        let k = sc.nodes[sc.kernel as usize].pos;
        let r = |l: &str| { let p = sc.nodes[node(l)].pos; v3(p.x - k.x, 0.0, p.z - k.z).len() };
        assert!(k.y < sc.nodes[node("ndexr.io")].pos.y);
        assert!(r("ndexr.io") < r("b.ndexr.io") && r("b.ndexr.io") < r("x.b.ndexr.io") && r("x.b.ndexr.io") < r("y.x.b.ndexr.io"));
        // Four levels, four rings.
        assert_eq!(sc.orbits.iter().filter(|o| o.layer == Layer::Domain).count(), 4);
    }

    /// The Hourglass traces an application down its env set, the packages
    /// in it and the toolchain under them, to the kernel.
    #[test]
    fn the_hourglass_traces_a_domain_to_the_kernel() {
        let cat = Catalog::parse(
            br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""},"items":[
                {"k":"prefix","n":"sys-devel/gcc","g":"sys-devel"},
                {"k":"easybuild","n":"GCCcore","g":"SYSTEM","dep":[0]},
                {"k":"easybuild","n":"R","g":"foss-2024a","dep":[1]},
                {"k":"easybuild","n":"Python","g":"GCCcore-13.3.0","dep":[1]},
                {"k":"R","n":"dplyr","s":["r4.6_py3.13-release"]},
                {"k":"python","n":"numpy","s":["r4.6_py3.13-release"]},
                {"k":"easybuild","n":"env/r4.6_py3.13-release","g":"env sets","dep":[2,3]}]}"#,
        )
        .unwrap();
        let doms: Vec<DomainInfo> = ["b.ndexr.io", "x.b.ndexr.io"]
            .iter()
            .map(|h| serde_json::from_value(serde_json::json!({ "host": h, "path": format!("/d/{h}"), "env": "env/r4.6_py3.13-release" })).unwrap())
            .collect();
        let sc = Scene::build(&cat, &doms, "me");
        let node = |l: &str| sc.nodes.iter().position(|n| n.label == l).unwrap();
        assert_eq!(sc.nodes[node("R")].layer, Layer::EasyBuild);
        let to = |a: &str, b: &str| sc.up[node(a)].iter().any(|&e| sc.edges[e as usize].to as usize == node(b));
        assert!(to("dplyr", "env/r4.6_py3.13-release") && to("b.ndexr.io", "env/r4.6_py3.13-release"));
        assert!(sc.lineage(node("x.b.ndexr.io") as u32).0[sc.kernel as usize]);
        // The languages' packages: one level, each far out on its own side,
        // and the env set reaches each through a bundle.
        let (r, py) = (sc.nodes[node("dplyr")].pos, sc.nodes[node("numpy")].pos);
        assert!((r.y - py.y).abs() < 0.5);
        assert!(v3(r.x, 0.0, r.z).len() > 3.0 && v3(py.x, 0.0, py.z).len() > 3.0 && r.sub(py).len() > 5.0);
        let env = node("env/r4.6_py3.13-release") as u32;
        assert_eq!(sc.bundles.iter().filter(|b| b.env == env).map(|b| b.layer).collect::<std::collections::HashSet<_>>().len(), 2);
        // It faces them: nearer both packages than the far side of its ring.
        let e = sc.nodes[env as usize].pos;
        let k = sc.nodes[sc.kernel as usize].pos;
        let far = v3(2.0 * k.x - e.x, e.y, 2.0 * k.z - e.z);
        assert!(e.sub(r).len() < far.sub(r).len() && e.sub(py).len() < far.sub(py).len());
        // Membership and dependency stay apart; a bundle is only real members.
        let dplyr = node("dplyr") as u32;
        assert!(sc.up[dplyr as usize].iter().any(|&e| sc.edges[e as usize].rel == Rel::Member && sc.edges[e as usize].to == env));
        assert!(sc.edges.iter().filter(|e| e.rel == Rel::Member).all(|e| sc.nodes[e.to as usize].layer == Layer::Env));
        for b in &sc.bundles {
            assert!(b.members.iter().all(|&m| sc.up[m as usize].iter().any(|&e| sc.edges[e as usize].rel == Rel::Member && sc.edges[e as usize].to == b.env)));
        }
        // Inspected, the set holds its packages on the right; a package names
        // its set on the left, as membership.
        let secs = inspect::sections(&sc, env, &Default::default(), None);
        assert!(secs.iter().any(|s| !s.left && s.rel == Rel::Member && s.count == 2));
        let secs = inspect::sections(&sc, dplyr, &Default::default(), None);
        assert!(secs.iter().any(|s| s.left && s.rel == Rel::Member && s.rows.iter().any(|r| r.go == inspect::Go::Node(env))));
        // The applications' first ring is wider than the language lobes reach.
        let lobes = sc.orbits.iter().filter(|o| lobe_angle(o.layer).is_some()).map(|o| v3(o.c.x, 0.0, o.c.z).len() + o.r).fold(0.0, f32::max);
        assert!(sc.orbits.iter().filter(|o| o.layer == Layer::Domain).all(|o| o.r > lobes));
    }

    /// The Hourglass: the kernel the floor, the stack's rings rising over it,
    /// the domains rising level by level over those; no account star.
    #[test]
    fn the_hourglass_rises_through_your_domains() {
        let cat = Catalog::parse(br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"items":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""}}"#).unwrap();
        let doms: Vec<DomainInfo> = ["b.ndexr.io", "x.b.ndexr.io", "y.x.b.ndexr.io"]
            .iter()
            .map(|h| serde_json::from_value(serde_json::json!({ "host": h, "path": format!("/d/{h}") })).unwrap())
            .collect();
        let sc = Scene::build(&cat, &doms, "me");
        let y = |l: &str| sc.nodes.iter().find(|n| n.label == l).unwrap().pos.y;
        let kernel = sc.nodes[sc.kernel as usize].pos;
        assert!(sc.you.is_none() && !sc.nodes.iter().any(|n| n.layer == Layer::You));
        assert!(kernel.y < y("ndexr.io") && y("ndexr.io") < y("b.ndexr.io") && y("b.ndexr.io") < y("x.b.ndexr.io") && y("x.b.ndexr.io") < y("y.x.b.ndexr.io"));
        // Every ring over the kernel, every domain ring over every stack ring.
        let stack_top = sc.orbits.iter().filter(|o| o.layer != Layer::Domain).map(|o| o.c.y).fold(kernel.y, f32::max);
        for o in &sc.orbits {
            assert!(o.c.y > kernel.y);
            if o.layer == Layer::Domain { assert!(o.c.y > stack_top) }
        }
    }

    /// Relations stay what they are: membership apart from dependency and
    /// from what an env set loads; a bundle is only real Member edges; the
    /// inspection lists an env set's EasyBuild on one side and its package
    /// groups on the other, and a package every set that holds it.
    #[test]
    fn environments_organise_the_inspection() {
        let cat = Catalog::parse(
            br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""},"items":[
                {"k":"prefix","n":"sys-devel/gcc","g":"sys-devel"},
                {"k":"easybuild","n":"R","g":"foss-2024a","v":["4.6.1-foss-2024a"],"dep":[0]},
                {"k":"easybuild","n":"env/R/4.6.1-bioc3.24","g":"env sets"},
                {"k":"easybuild","n":"env/R/4.5.3-bioc3.22","g":"env sets"},
                {"k":"R","n":"rlang","s":["R/4.6.1-bioc3.24","R/4.5.3-bioc3.22","r4.6_py3.13-release"]},
                {"k":"R","n":"dplyr","s":["R/4.6.1-bioc3.24"],"dep":[4]}]}"#,
        )
        .unwrap();
        let uses: EnvUses = [("env/R/4.6.1-bioc3.24".to_owned(), vec![("R".to_owned(), "4.6.1-foss-2024a".to_owned())])].into_iter().collect();
        let sc = Scene::build_with(&cat, &[], "me", &[], &uses, &Default::default());
        let node = |l: &str| sc.nodes.iter().position(|n| n.label == l).unwrap() as u32;
        let rel = |a: &str, b: &str| sc.up[node(a) as usize].iter().map(|&e| &sc.edges[e as usize]).find(|e| e.to == node(b)).map(|e| e.rel);
        assert_eq!(rel("dplyr", "rlang"), Some(Rel::Dep));
        assert_eq!(rel("dplyr", "env/R/4.6.1-bioc3.24"), Some(Rel::Member));
        assert_eq!(rel("env/R/4.6.1-bioc3.24", "R"), Some(Rel::Uses));
        for b in &sc.bundles {
            assert!(b.members.iter().all(|&m| rel(&sc.nodes[m as usize].label, &sc.nodes[b.env as usize].label) == Some(Rel::Member)));
            assert_eq!(b.n as usize, b.members.len());
        }
        let open = std::collections::HashSet::new();
        let env = inspect::sections(&sc, node("env/R/4.6.1-bioc3.24"), &open, Some(&cat));
        assert!(env.iter().any(|s| s.left && s.rel == Rel::Uses && s.rows[0].note == "4.6.1-foss-2024a"));
        assert!(env.iter().any(|s| !s.left && s.rel == Rel::Member && s.count == 2));
        // The shared package: both sets, and the composition it is listed in
        // that has no module record, named apart.
        let pkg = inspect::sections(&sc, node("rlang"), &open, Some(&cat));
        let member: Vec<&str> = pkg.iter().filter(|s| s.left && s.rel == Rel::Member).flat_map(|s| s.rows.iter().map(|r| r.label.as_str())).collect();
        assert!(member.contains(&"env/R/4.6.1-bioc3.24") && member.contains(&"env/R/4.5.3-bioc3.22") && member.contains(&"r4.6_py3.13-release"));
        assert!(pkg.iter().any(|s| !s.left && s.rel == Rel::Dep && s.rows.iter().any(|r| r.label == "dplyr")));
    }

    /// ← → walk Prefix up through the combinations to the applications; a
    /// kit set the catalog lacks (Julia) is a star to build with its
    /// manifest's packages; a combination joins its two sets and carries the
    /// domains that name it.
    #[test]
    fn julia_and_the_combinations_are_on_the_walk() {
        let cat = Catalog::parse(
            br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""},"items":[
                {"k":"prefix","n":"sys-devel/gcc","g":"sys-devel"},
                {"k":"easybuild","n":"Julia","g":"SYSTEM","v":["1.11.7"]},
                {"k":"easybuild","n":"env/R/4.6.1-bioc3.23","g":"env sets"},
                {"k":"easybuild","n":"env/Python/3.13.15","g":"env sets"},
                {"k":"R","n":"rlang","s":["R/4.6.1-bioc3.23","r4.6_py3.13-release"]},
                {"k":"python","n":"numpy","s":["Python/3.13.15","r4.6_py3.13-release"]}]}"#,
        )
        .unwrap();
        let kit = crate::api::EnvSets {
            sets: vec![
                crate::api::KitSet { set: "R/4.6.1-bioc3.23".into(), language: "R".into(), records: 1, packages: vec![] },
                crate::api::KitSet { set: "Julia/1.11.7".into(), language: "Julia".into(), records: 2, packages: vec![crate::api::KitPackage { name: "DataFrames".into(), version: "1.8.2".into(), group: "Data frames and IO".into() }, crate::api::KitPackage { name: "Turing".into(), version: "0.40".into(), group: "Statistics".into() }] },
            ],
            combos: vec![crate::api::KitCombo { name: "r4.6_py3.13-release".into(), sets: vec!["R/4.6.1-bioc3.23".into(), "Python/3.13.15".into()] }],
            ..Default::default()
        };
        let uses: EnvUses = [("env/Julia/1.11.7".to_owned(), vec![("Julia".to_owned(), "1.11.7".to_owned())])].into_iter().collect();
        let doms: Vec<DomainInfo> = vec![serde_json::from_value(serde_json::json!({ "host": "b.ndexr.io", "path": "/d/b", "env": "env/r4.6_py3.13-release" })).unwrap()];
        let sc = Scene::build_with(&cat, &doms, "me", &[], &uses, &kit);
        let node = |l: &str| sc.nodes.iter().position(|n| n.label == l).unwrap();
        let rel = |a: &str, b: &str| sc.up[node(a)].iter().map(|&e| &sc.edges[e as usize]).find(|e| e.to as usize == node(b)).map(|e| e.rel);
        // Julia: a set to build, its packages in it, on the Julia module.
        assert!(sc.planned[node("env/Julia/1.11.7")] && sc.planned[node("DataFrames")]);
        assert_eq!(sc.nodes[node("DataFrames")].layer, Layer::Julia);
        assert_eq!(rel("DataFrames", "env/Julia/1.11.7"), Some(Rel::Member));
        assert_eq!(rel("env/Julia/1.11.7", "Julia"), Some(Rel::Uses));
        assert!(sc.bundles.iter().any(|b| b.layer == Layer::Julia && b.env as usize == node("env/Julia/1.11.7")));
        // The combination: built (packages list it), joins both sets, and the
        // domain naming it works in it; it stands over the lobes, under the
        // applications.
        let combo = node("env/r4.6_py3.13-release");
        assert_eq!(sc.nodes[combo].layer, Layer::Combo);
        assert!(!sc.planned[combo]);
        assert_eq!(rel("env/r4.6_py3.13-release", "env/R/4.6.1-bioc3.23"), Some(Rel::Joins));
        assert_eq!(rel("env/r4.6_py3.13-release", "env/Python/3.13.15"), Some(Rel::Joins));
        assert_eq!(rel("b.ndexr.io", "env/r4.6_py3.13-release"), Some(Rel::Works));
        assert!(sc.nodes[combo].pos.y > sc.nodes[node("rlang")].pos.y && sc.nodes[combo].pos.y < sc.nodes[node("b.ndexr.io")].pos.y);
        // The walk: from the whole stack → is Prefix, then up, layer by layer.
        let mut a = Atlas { scene: Some(sc), ..Default::default() };
        let mut seen = Vec::new();
        for _ in 0..10 {
            a.step_layer(1);
            if let Focus::Layer(l) = a.focus() {
                if seen.last() != Some(&l) {
                    seen.push(l);
                }
            }
        }
        assert_eq!(seen, [Layer::Prefix, Layer::EasyBuild, Layer::Env, Layer::R, Layer::Python, Layer::Julia, Layer::Combo, Layer::Domain]);
        a.step_layer(-1);
        assert_eq!(a.focus(), Focus::Layer(Layer::Combo));
    }

    /// The atlas's shape, as the operator built it up on 2026-10-21, kept
    /// ("focus on making sure were not reverting back on our visual
    /// improvements"; onprem/run.sh holds a release that fails this): the
    /// kernel at the bottom and the stack rising from it; R, Python and Rust
    /// packages on one level, each far out on its own side; each language set
    /// turned toward its packages; the applications over everything, every
    /// ring of theirs wider than the package lobes reach and wider again
    /// level by level.
    #[test]
    fn the_atlas_keeps_its_shape() {
        let cat = Catalog::parse(
            br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""},"items":[
                {"k":"prefix","n":"sys-devel/gcc","g":"sys-devel"},
                {"k":"easybuild","n":"GCCcore","g":"SYSTEM","dep":[0]},
                {"k":"easybuild","n":"R","g":"foss-2024a","dep":[1]},
                {"k":"easybuild","n":"env/R/4.6.1-bioc3.24","g":"env sets"},
                {"k":"easybuild","n":"env/Python/3.13.15","g":"env sets"},
                {"k":"easybuild","n":"env/Rust/1.96.0","g":"env sets"},
                {"k":"R","n":"rlang","s":["R/4.6.1-bioc3.24"]},
                {"k":"R","n":"dplyr","s":["R/4.6.1-bioc3.24"],"dep":[6]},
                {"k":"python","n":"numpy","s":["Python/3.13.15"]},
                {"k":"python","n":"pandas","s":["Python/3.13.15"],"dep":[8]},
                {"k":"Rust","n":"serde","s":["Rust/1.96.0"]}]}"#,
        )
        .unwrap();
        let doms: Vec<DomainInfo> = ["a.ndexr.io", "x.a.ndexr.io", "y.x.a.ndexr.io", "koodle.io"]
            .iter()
            .map(|h| serde_json::from_value(serde_json::json!({ "host": h, "path": format!("/d/{h}"), "env": "env/R/4.6.1-bioc3.24" })).unwrap())
            .collect();
        let sc = Scene::build(&cat, &doms, "me");
        let ring = |l: Layer| sc.orbits.iter().find(|o| o.layer == l).copied().unwrap_or_else(|| panic!("{l:?} has a ring"));
        let kernel = sc.nodes[sc.kernel as usize].pos;
        // The kernel is the floor: nothing under it.
        assert!(sc.nodes.iter().all(|n| n.pos.y >= kernel.y - 0.5), "something hangs under the kernel");
        // The stack rises from it on the axis: Prefix, EasyBuild, the sets.
        let (pre, eb, env) = (ring(Layer::Prefix), ring(Layer::EasyBuild), ring(Layer::Env));
        assert!(kernel.y < pre.c.y && pre.c.y < eb.c.y && eb.c.y < env.c.y, "the stack rises from the kernel");
        for o in [pre, eb, env] {
            assert!(v3(o.c.x, 0.0, o.c.z).len() < 1e-3, "{:?} on the axis", o.layer);
        }
        // The languages' packages: one level, over the sets, each far out on
        // its own side and well apart from the next.
        let lobes: Vec<Orbit> = [Layer::R, Layer::Python, Layer::Rust].into_iter().map(ring).collect();
        for o in &lobes {
            assert!((o.c.y - lobes[0].c.y).abs() < 1e-3, "{:?} on the packages' level", o.layer);
            assert!(o.c.y > env.c.y, "{:?} over the sets", o.layer);
            assert!(v3(o.c.x, 0.0, o.c.z).len() > env.r + o.r + 2.0, "{:?} far from the axis", o.layer);
        }
        for (i, a) in lobes.iter().enumerate() {
            for b in &lobes[i + 1..] {
                assert!(a.c.sub(b.c).len() > a.r + b.r + 2.0, "{:?} and {:?} apart", a.layer, b.layer);
            }
        }
        // Each language set faces its own packages, not across the axis.
        let angle = |p: V3| p.z.atan2(p.x);
        let off = |a: f32, b: f32| ((a - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI).abs();
        for (set, lobe) in [("env/R/4.6.1-bioc3.24", Layer::R), ("env/Python/3.13.15", Layer::Python), ("env/Rust/1.96.0", Layer::Rust)] {
            let p = sc.nodes.iter().find(|n| n.label == set).unwrap().pos;
            let toward = off(angle(p), angle(ring(lobe).c));
            assert!(toward < 0.7, "{set} faces its {lobe:?} packages ({toward:.2} rad off)");
        }
        // The applications over everything: every ring of theirs higher than
        // any ring of the stack, wider than the lobes reach, and wider again
        // level by level.
        let doms_rings: Vec<Orbit> = sc.orbits.iter().filter(|o| o.layer == Layer::Domain).copied().collect();
        assert!(doms_rings.len() >= 3, "a ring per level of domains");
        let stack_top = sc.orbits.iter().filter(|o| o.layer != Layer::Domain).map(|o| o.c.y).fold(f32::MIN, f32::max);
        let reach = lobes.iter().map(|o| v3(o.c.x, 0.0, o.c.z).len() + o.r).fold(0.0, f32::max);
        for w in doms_rings.windows(2) {
            assert!(w[1].c.y > w[0].c.y && w[1].r > w[0].r, "each level of domains higher and wider");
        }
        for o in &doms_rings {
            assert!(o.c.y > stack_top, "applications over the stack");
            assert!(o.r > reach, "applications wider than the package lobes reach ({:.1} vs {reach:.1})", o.r);
        }
    }

    /// A steward's view: every package it looks after, in every language,
    /// lit with everything each stands on - and nobody else's packages.
    #[test]
    fn a_steward_lights_its_own_build() {
        let cat = Catalog::parse(
            br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""},"items":[
                {"k":"prefix","n":"sys-libs/glibc","g":"sys-libs"},
                {"k":"R","n":"rlang","h":"https://rlang.r-lib.org"},
                {"k":"R","n":"ggplot2","h":"https://ggplot2.tidyverse.org","dep":[1]},
                {"k":"R","n":"admiral","h":"https://pharmaverse.github.io/admiral/","dep":[2]},
                {"k":"python","n":"shiny-py","h":"https://github.com/posit-dev/py-shiny"}]}"#,
        )
        .unwrap();
        let sc = Scene::build(&cat, &[], "me");
        let node = |l: &str| sc.nodes.iter().position(|n| n.label == l).unwrap() as u32;
        let mut posit: Vec<u32> = sc.owned_by("Posit").collect();
        posit.sort();
        let mut want = vec![node("rlang"), node("ggplot2"), node("shiny-py")];
        want.sort();
        assert_eq!(posit, want, "Posit's packages, in R and Python");
        let mut a = Atlas { scene: Some(sc), ..Default::default() };
        a.trail = vec![Focus::Owner("Posit".into())];
        a.lighting();
        let (_, lit, lines) = a.lit.as_ref().unwrap();
        let sc = a.scene.as_ref().unwrap();
        let node = |l: &str| sc.nodes.iter().position(|n| n.label == l).unwrap();
        assert!(lit[node("ggplot2")] && lit[node("rlang")] && lit[node("shiny-py")]);
        // Its connections: ggplot2's dependency on rlang is drawn.
        assert!(lines.iter().any(|&e| sc.edges[e as usize].from as usize == node("ggplot2") && sc.edges[e as usize].to as usize == node("rlang")));
    }

    /// A person's system: their domains lit, someone else's not, and the
    /// rail's PEOPLE rows counted most domains first.
    #[test]
    fn a_person_lights_their_own_system() {
        let cat = Catalog::parse(br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""},"items":[{"k":"prefix","n":"sys-libs/glibc","g":"sys-libs"}]}"#).unwrap();
        let d = |h: &str, o: &str| DomainInfo { host: h.into(), owner: o.into(), ..Default::default() };
        let doms = [d("a.ndexr.io", "ann"), d("b.ndexr.io", "ann"), d("c.ndexr.io", "bo"), d("d.ndexr.io", "")];
        let sc = Scene::build(&cat, &doms, "ann");
        assert_eq!(sc.people(), vec![("ann".to_owned(), 2), ("bo".to_owned(), 1)]);
        let mut a = Atlas { scene: Some(sc), ..Default::default() };
        a.trail = vec![Focus::Person("ann".into())];
        a.lighting();
        let (_, lit, _) = a.lit.as_ref().unwrap();
        let sc = a.scene.as_ref().unwrap();
        let node = |l: &str| sc.nodes.iter().position(|n| n.label == l).unwrap();
        assert!(lit[node("a.ndexr.io")] && lit[node("b.ndexr.io")]);
        assert!(!lit[node("c.ndexr.io")] && !lit[node("d.ndexr.io")], "not someone else's");
    }

    /// A star grows as it nears, then shrinks again, so up close its name
    /// is readable beside it; it is named from CLOSE on.
    #[test]
    fn stars_shrink_up_close() {
        let r = |raw| star_radius(Layer::R, raw);
        assert!(r(2.0) < r(8.0), "nearer, bigger");
        assert!(r(80.0) < r(8.0) && r(800.0) < r(80.0), "then smaller the closer it comes");
        assert!(r(800.0) >= 1.5, "never gone");
        assert!(CLOSE < 8.0);
        assert_eq!(star_radius(Layer::Kernel, 20.0), 20.0);
    }

    /// Closing a star's details is not leaving it: the star stays picked
    /// and the camera stays put; picking it again reopens them where it is;
    /// only Back leaves (operator, 2026-10-21, with screenshots: "I like it
    /// to stay focused on that item instead without blocking the view").
    #[test]
    fn closing_details_keeps_the_star() {
        let cat = Catalog::parse(br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""},"items":[{"k":"R","n":"rlang"},{"k":"R","n":"car","dep":[0]}]}"#).unwrap();
        let sc = Scene::build(&cat, &[], "me");
        let car = sc.nodes.iter().position(|n| n.label == "car").unwrap() as u32;
        let mut a = Atlas { scene: Some(sc), ..Default::default() };
        a.go(Focus::Node(car));
        assert_eq!(a.panel, Panel::Full);
        let at = a.goal.target;
        // × on the details (card): folded, nothing else changes.
        a.panel = Panel::Bar;
        assert_eq!(a.focus(), Focus::Node(car));
        assert_eq!(a.goal.target, at);
        // The star again: its details, no camera move.
        a.go(Focus::Node(car));
        assert_eq!(a.panel, Panel::Full);
        assert_eq!(a.goal.target, at);
        // Back is what leaves - and the star is remembered.
        a.back();
        assert_eq!(a.focus(), Focus::All);
        assert_eq!(a.last_pick, Some(car));
    }

    #[test]
    fn the_hourglass_places_every_node() {
        let cat = Catalog::parse(br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"items":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""}}"#).unwrap();
        let doms: Vec<DomainInfo> = ["a.ndexr.io", "b.ndexr.io", "x.b.ndexr.io", "koodle.io"]
            .iter()
            .map(|h| serde_json::from_value(serde_json::json!({ "host": h, "path": format!("/d/{h}") })).unwrap())
            .collect();
        let sc = Scene::build(&cat, &doms, "me");
        assert!(sc.nodes.iter().all(|n| n.pos.x.is_finite() && n.pos.y.is_finite() && n.pos.z.is_finite()));
        let k = sc.nodes[sc.kernel as usize].pos;
        assert!(sc.nodes.iter().all(|n| n.pos.y >= k.y - 0.5), "nothing hangs under the kernel");
    }

    #[test]
    fn the_kernel_is_a_star_per_floor() {
        let f = |arch: &str, min: &str| Floor { triple: format!("{arch}-x-linux-gnu"), arch: arch.into(), min_kernel: min.into(), ..Default::default() };
        let floors = [f("x86_64", "3.2.0"), f("aarch64", "3.7.0")];
        assert_eq!(floors[0].name(), "Linux ≥ 3.2 · x86-64");
        assert_eq!(floors[1].name(), "Linux ≥ 3.7 · arm64");
        assert!(floors[0].holds(&["cuda".into()]) && !floors[1].holds(&["cuda".into()]) && floors[1].holds(&[]));
        let cat = Catalog::parse(br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"items":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""}}"#).unwrap();
        let sc = Scene::build_with(&cat, &[], "me", &floors, &EnvUses::new(), &Default::default());
        assert_eq!(sc.kernels.len(), 2);
        let names: Vec<&str> = sc.kernels.iter().map(|&k| sc.nodes[k as usize].label.as_str()).collect();
        assert_eq!(names, ["Linux ≥ 3.2 · x86-64", "Linux ≥ 3.7 · arm64"]);
        // A binary: two stars, apart.
        let (a, b) = (sc.nodes[sc.kernels[0] as usize].pos, sc.nodes[sc.kernels[1] as usize].pos);
        assert!(a.sub(b).len() > 0.5);
        assert_ne!(Scene::key_with(&cat, &[], &floors, &EnvUses::new(), &Default::default()), Scene::key(&cat, &[]));
    }

    #[test]
    fn the_machines_mark_what_compiles_and_what_is_coming() {
        let cat = Catalog::parse(
            br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""},"items":[
            {"k":"easybuild","n":"GDAL","g":"foss-2024a","v":["3.10.0-foss-2024a"],"m":"GDAL/3.10.0-foss-2024a","st":{"ok":3,"fail":0}},
            {"k":"easybuild","n":"env/R/4.6.1-bioc3.24","g":"env sets","v":["x"],"st":{"ok":0,"fail":1}},
            {"k":"Python","n":"statsmodels","g":"Python packages","v":["0.14.1"]}]}"#,
        )
        .unwrap();
        let live = serde_json::json!({
            "generated": 10_000,
            "eta": {"machines": [
                {"id": "four-box/intel", "active": true, "running": {"name": "GDAL", "version": "3.10.0"},
                 "pending": [{"entry": "env-R-4.6.1-bioc3.24.eb", "status": "queued"}, {"entry": "Brand-New-1.0.eb", "status": "queued"}]},
                {"id": "laptop/containers", "active": false, "mirror_age_s": 460_000, "running": {"name": "old"}, "pending": [{"entry": "GDAL-3.10.0.eb"}]}
            ]},
            "lanes": [{"key": "intel", "state": "running", "kind": "Python", "now": "statsmodels", "mtime": 9_900},
                      {"key": "arm", "state": "off", "kind": "Python", "now": "statsmodels", "mtime": 1}]
        });
        let b = Building::read(&live, &cat);
        // The switched-off laptop says nothing; the live machine and lane do.
        assert_eq!(b.now.iter().map(|x| x.name.as_str()).collect::<Vec<_>>(), ["GDAL", "statsmodels"]);
        assert_eq!(b.now[0].item, Some(0));
        assert_eq!((b.now[0].version.as_str(), b.now[0].machine.as_str()), ("3.10.0", "four-box/intel"));
        assert_eq!(b.now[1].item, Some(2));
        assert_eq!(b.now[1].machine, "four-box/intel", "the lane is its machine's log");
        assert_eq!(b.queued.len(), 2);
        assert!(b.queued.iter().any(|q| q.1 == "Brand-New-1.0.eb" && q.0.is_none()), "unknown to the catalog, still listed");
        let sc = Scene::build(&cat, &[], "me");
        let m = b.marks(&sc);
        let at = |name: &str| sc.nodes.iter().position(|n| n.label == name).unwrap();
        assert_eq!(m[at("GDAL")], 2);
        assert_eq!(m[at("statsmodels")], 2);
        // Never built: a ring, whatever the machines say.
        assert!(sc.planned[at("env/R/4.6.1-bioc3.24")] && !sc.planned[at("GDAL")]);
    }

    #[test]
    fn a_compile_traces_down_to_its_archs_glibc_and_floor() {
        let cat = Catalog::parse(
            br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""},"items":[
            {"k":"prefix","n":"sys-libs/glibc","g":"sys-libs","v":["2.43"]},
            {"k":"easybuild","n":"zlib","g":"GCCcore-13.3.0","v":["1.3.1"],"m":"zlib/1.3.1","dep":[0]},
            {"k":"easybuild","n":"GDAL","g":"foss-2024a","v":["3.10.0"],"m":"GDAL/3.10.0","dep":[1]}]}"#,
        )
        .unwrap();
        let floor = |arch: &str, triple: &str, k: &str, g: &str| Floor { triple: triple.into(), arch: arch.into(), min_kernel: k.into(), glibc: g.into(), headers: String::new(), proven_on: vec![] };
        let floors = vec![floor("aarch64", "aarch64-unknown-linux-gnu", "3.7.0", "2.43-r3"), floor("x86_64", "x86_64-pc-linux-gnu", "3.2.0", "2.43-r2")];
        let sc = Scene::build_with(&cat, &[], "me", &floors, &EnvUses::new(), &Default::default());
        let c = Compile { item: Some(2), name: "GDAL".into(), version: "3.10.0".into(), machine: "four-box/arm".into(), arch: "aarch64".into() };
        let path: Vec<String> = Building::trace(&sc, &c).iter().map(|&n| sc.nodes[n].label.clone()).collect();
        assert_eq!(path, ["GDAL", "zlib", "sys-libs/glibc", "Linux ≥ 3.7 · arm64"]);
        assert_eq!(Building::chain(&sc, &c), " · arm64 → glibc 2.43-r3 → Linux ≥ 3.7");
        // The same package on an x86-64 box: the other glibc, the other floor.
        let x = Compile { arch: "x86_64".into(), machine: "four-box/intel".into(), ..c };
        assert_eq!(Building::trace(&sc, &x).last().map(|&n| sc.nodes[n].label.as_str()), Some("Linux ≥ 3.2 · x86-64"));
        assert!(Building::chain(&sc, &x).contains("glibc 2.43-r2"));
    }

    #[test]
    fn every_module_traces_through_prefix_to_the_kernel() {
        // GCCcore and binutils name each other, as the real catalog does: the
        // SYSTEM one still stands on Prefix's gcc, and gcc on the kernel.
        let cat = Catalog::parse(
            br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""},"items":[
            {"k":"prefix","n":"sys-libs/glibc","g":"sys-libs","v":["2.43"]},
            {"k":"prefix","n":"sys-devel/gcc","g":"sys-devel","v":["14"],"dep":[0]},
            {"k":"easybuild","n":"GCCcore","g":"SYSTEM","v":["13.3.0"],"m":"GCCcore/13.3.0","dep":[3]},
            {"k":"easybuild","n":"binutils","g":"GCCcore-13.3.0","v":["2.42"],"m":"binutils/2.42","dep":[2]},
            {"k":"easybuild","n":"libiconv","g":"GCCcore-13.3.0","v":["1.17"],"m":"libiconv/1.17","dep":[2,3]}]}"#,
        )
        .unwrap();
        let sc = Scene::build(&cat, &[], "me");
        let at = |name: &str| sc.nodes.iter().position(|n| n.label == name).unwrap() as u32;
        let (lit, _) = sc.lineage(at("libiconv"));
        assert!(lit[at("sys-devel/gcc") as usize] && lit[at("sys-libs/glibc") as usize]);
        assert!(lit[sc.kernel as usize], "down to the kernel");
        assert!(sc.edges.iter().any(|e| e.from == at("GCCcore") && e.to == at("sys-devel/gcc") && e.ambient && e.rel == Rel::Floor));
        assert!(!sc.edges.iter().any(|e| e.from == at("binutils") && e.rel == Rel::Floor), "only SYSTEM modules stand on Prefix directly");
    }

    #[test]
    fn sets_name_their_env() {
        assert_eq!(env_set_name("env/r4.6_py3.13-release"), Some("r4.6_py3.13-release"));
        assert_eq!(env_set_name("R"), None);
    }

    #[test]
    fn the_eye_sees_what_is_in_front() {
        let r = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let p = Proj::new(&Cam { target: v3(0.0, 0.0, 0.0), yaw: 0.0, pitch: 0.0, dist: 10.0 }, r);
        let (c, _, d) = p.at(v3(0.0, 0.0, 0.0)).unwrap();
        assert_eq!(c, r.center());
        assert!((d - 10.0).abs() < 1e-4);
        // Up is up on screen; behind the eye is not drawn.
        assert!(p.at(v3(0.0, 1.0, 0.0)).unwrap().0.y < r.center().y);
        assert!(p.at(v3(0.0, 0.0, 20.0)).is_none());
    }

    /// The live catalog with the live kit: Julia's set and packages come in
    /// to build, and every combination joins two built sets
    /// (`cargo test julia_on_the_real_kit -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn julia_on_the_real_kit() {
        let Ok(bytes) = std::fs::read("/srv/ndexr/software/catalog.json") else { return };
        let cat = Catalog::parse(&bytes).unwrap();
        let kit = crate::host::env_sets(&["Julia/1.11.7"]);
        assert!(kit.repos.len() >= 400, "the registry names the installed packages' repos ({})", kit.repos.len());
        let sc = Scene::build_with(&cat, &[], "me", &[], &EnvUses::new(), &kit);
        let julia = sc.nodes.iter().position(|n| n.label == "env/Julia/1.11.7").expect("Julia's set");
        let pkgs = sc.down[julia].iter().filter(|&&e| sc.edges[e as usize].rel == Rel::Member).count();
        let combos: Vec<usize> = (0..sc.nodes.len()).filter(|&k| sc.nodes[k].layer == Layer::Combo).collect();
        // To build when the kit alone named it; built since 2026-10-22, when
        // the catalog lists the set and its packages itself.
        println!("Julia: {pkgs} packages ({}); {} combinations", if sc.planned[julia] { "to build" } else { "built" }, combos.len());
        assert!(pkgs >= 30);
        // Its players: owners read from the set's own registry, named.
        let mut by: HashMap<&str, usize> = HashMap::new();
        for nd in sc.nodes.iter().filter(|n| n.layer == Layer::Julia) {
            *by.entry(nd.group.as_str()).or_default() += 1;
        }
        let mut top: Vec<(&str, usize)> = by.into_iter().collect();
        top.sort_by(|a, b| b.1.cmp(&a.1));
        println!("Julia owners: {:?}", &top[..top.len().min(10)]);
        assert!(top.iter().any(|(g, n)| *g == "SciML" && *n >= 10), "SciML is a player");
        assert!(top.iter().find(|(g, _)| *g == owners::UNLISTED).map_or(0, |x| x.1) < pkgs / 4, "most have an owner");
        // And its tiers, like the other languages'.
        let tiers = sc.orbits.iter().filter(|o| o.layer == Layer::Julia && o.tier.is_some() && o.n > 0).count();
        assert!(tiers >= 2, "Julia in bands");
        assert!(!combos.is_empty() && combos.iter().all(|&c| sc.up[c].iter().filter(|&&e| sc.edges[e as usize].rel == Rel::Joins).count() == 2));
    }

    /// The Hourglass on the live catalog: how long it takes to build
    /// (`cargo test --release hourglass_on_the_real_catalog -- --ignored --nocapture`).
    #[test]
    #[ignore]
    fn hourglass_on_the_real_catalog() {
        let Ok(bytes) = std::fs::read("/srv/ndexr/software/catalog.json") else { return };
        let cat = Catalog::parse(&bytes).unwrap();
        let t = std::time::Instant::now();
        let sc = Scene::build(&cat, &[], "me");
        println!("{} nodes, {} edges, {} bundles in {:?}", sc.nodes.len(), sc.edges.len(), sc.bundles.len(), t.elapsed());
        // Each language's roles, counted: three rings each, none empty.
        for l in [Layer::R, Layer::Python, Layer::Rust] {
            let of = |t: Tier| (0..sc.nodes.len()).filter(|&k| sc.nodes[k].layer == l && sc.tier[k] == Some(t)).count();
            println!("{l:?}: {} workflows, {} everyday, {} supporting", of(Tier::Workflow), of(Tier::Everyday), of(Tier::Supporting));
            assert!(of(Tier::Workflow) > 0 && of(Tier::Everyday) > 0 && of(Tier::Supporting) > 0, "{l:?} has all three rings");
        }
        // Every membership edge is in exactly one bundle; the largest set's
        // inspection groups its packages and opens one group alone.
        let members = sc.edges.iter().filter(|e| e.rel == Rel::Member).count();
        assert_eq!(sc.bundles.iter().map(|b| b.members.len()).sum::<usize>(), members);
        let env = (0..sc.nodes.len() as u32).filter(|&k| sc.nodes[k as usize].layer == Layer::Env).max_by_key(|&k| sc.down[k as usize].len()).unwrap();
        let shut = inspect::sections(&sc, env, &Default::default(), Some(&cat));
        let held = shut.iter().find(|s| !s.left && s.rel == Rel::Member).unwrap();
        println!("{}: {} packages in {} groups", sc.nodes[env as usize].label, held.count, held.rows.len());
        assert!(held.rows.len() < held.count && held.rows.iter().all(|r| r.depth == 0));
        let b = sc.bundles.iter().filter(|b| b.env == env).max_by_key(|b| b.n).unwrap();
        let open: std::collections::HashSet<String> = [inspect::group_key(env, false, Rel::Member, b.layer, &b.group)].into_iter().collect();
        let opened = inspect::sections(&sc, env, &open, Some(&cat));
        let held2 = opened.iter().find(|s| !s.left && s.rel == Rel::Member).unwrap();
        assert_eq!(held2.rows.len(), held.rows.len() + b.members.len());
        // A package shared by the most sets lists every one of them.
        let shared = (0..sc.nodes.len() as u32).filter(|&k| matches!(sc.nodes[k as usize].layer, Layer::R | Layer::Python)).max_by_key(|&k| sc.up[k as usize].iter().filter(|&&e| sc.edges[e as usize].rel == Rel::Member).count()).unwrap();
        let n_sets = sc.up[shared as usize].iter().filter(|&&e| sc.edges[e as usize].rel == Rel::Member).count();
        let rows = inspect::sections(&sc, shared, &Default::default(), Some(&cat));
        let listed = rows.iter().filter(|s| s.left && s.rel == Rel::Member && s.title == "member of env sets").map(|s| s.count).sum::<usize>();
        println!("{} is in {n_sets} sets with a module", sc.nodes[shared as usize].label);
        assert_eq!(listed, n_sets);
        // Every env set loads from EasyBuild what its easyconfig names, and
        // a package in it traces through that to the kernel.
        let mods: Vec<&str> = (0..cat.len()).map(|i| cat.sw(i)).filter(|s| s.kind == Kind::Env).map(|s| s.name).collect();
        let uses: EnvUses = crate::host::env_uses(&mods).into_iter().map(|e| (e.module, e.uses.into_iter().map(|u| (u.name, u.version)).collect())).collect();
        let sc = Scene::build_with(&cat, &[], "me", &[], &uses, &Default::default());
        for (k, nd) in sc.nodes.iter().enumerate().filter(|(_, nd)| nd.layer == Layer::Env) {
            let eb: Vec<String> = sc.up[k].iter().map(|&e| sc.edges[e as usize].to).filter(|&t| sc.nodes[t as usize].layer == Layer::EasyBuild).map(|t| format!("{} {}", sc.nodes[t as usize].label, sc.used.get(&(k as u32, t)).cloned().unwrap_or_default())).collect();
            println!("{} loads {}", nd.label, eb.join(", "));
            assert!(!eb.is_empty(), "{} loads nothing from EasyBuild", nd.label);
            let lang = nd.label.split('/').nth(1).unwrap_or("");
            assert!(eb.iter().any(|e| e.starts_with(&format!("{lang} "))), "{} does not load {lang}", nd.label);
        }
        let reach: f32 = sc.bundles.iter().map(|b| b.n as f32 * sc.nodes[b.env as usize].pos.sub(b.to).len()).sum::<f32>() / sc.bundles.iter().map(|b| b.n as f32).sum::<f32>().max(1.0);
        println!("a package's line to its set: {reach:.2} long on average");
        assert!(sc.nodes.iter().all(|n| n.pos.x.is_finite() && n.pos.y.is_finite() && n.pos.z.is_finite()));
        // The galaxies, spread out inside, still clear of one another.
        let reach = |l: Layer| sc.orbits.iter().filter(|o| o.layer == l).map(|o| o.r).fold(0.0f32, f32::max);
        let centre = |l: Layer| sc.orbits.iter().find(|o| o.layer == l).map(|o| o.c);
        let langs: Vec<Layer> = [Layer::R, Layer::Python, Layer::Rust, Layer::Julia].into_iter().filter(|&l| centre(l).is_some()).collect();
        for (i, &a) in langs.iter().enumerate() {
            for &b in &langs[i + 1..] {
                let gap = centre(a).unwrap().sub(centre(b).unwrap()).len() - reach(a) - reach(b);
                println!("{a:?}-{b:?}: {gap:.2} between their outer bands");
                assert!(gap > 0.5, "{a:?} and {b:?} overlap");
            }
        }
        // Each language a galaxy of three bands, every owner one arm through
        // all three: its bearing, unwound by the arm's turn, the same in each.
        for lang in [Layer::R, Layer::Python, Layer::Rust] {
            let rings: Vec<&Orbit> = sc.orbits.iter().filter(|o| o.layer == lang && o.tier.is_some()).collect();
            println!("{lang:?}: {}", rings.iter().map(|o| format!("{} {}", o.tier.unwrap().label(), o.n)).collect::<Vec<_>>().join(" · "));
            assert_eq!(rings.iter().filter(|o| o.n > 0).count(), 3, "{lang:?} in three tiers");
            let owners: Vec<String> = sc.groups[&lang].iter().take(8).map(|g| format!("{} {}", g.0, g.1)).collect();
            println!("  owners: {}", owners.join(", "));
            let centre = rings[0].c;
            let angle = |p: V3| {
                let r = v3(p.x - centre.x, 0.0, p.z - centre.z).len().max(1e-3);
                (p.z - centre.z).atan2(p.x - centre.x) - GALAXY_TWIST * (r / GALAXY_GAP).ln()
            };
            // Flat: every band at the language's own level.
            assert!(rings.iter().all(|o| (o.c.y - centre.y).abs() < 1e-3), "{lang:?} one disc");
            for (g, _, _) in sc.groups[&lang].iter().take(4) {
                let mut by_tier: HashMap<Tier, Vec<f32>> = HashMap::new();
                for (k, nd) in sc.nodes.iter().enumerate().filter(|(_, nd)| nd.layer == lang && &nd.group == g) {
                    by_tier.entry(sc.tier[k].unwrap()).or_default().push(angle(nd.pos));
                }
                let mean = |v: &Vec<f32>| { let (x, z) = v.iter().fold((0.0f32, 0.0f32), |a, t| (a.0 + t.cos(), a.1 + t.sin())); z.atan2(x) };
                let ms: Vec<f32> = by_tier.values().filter(|v| v.len() >= 3).map(mean).collect();
                for w in ms.windows(2) {
                    let d = ((w[0] - w[1] + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI).abs();
                    assert!(d < 0.6, "{lang:?} {g}: one arm through every band ({d:.2} rad apart)");
                }
            }
        }
    }
}


/// The provenance language of a layer's packages (host.rs `provenance`).
fn prov_lang(l: Layer) -> &'static str {
    match l {
        Layer::R => "R",
        Layer::Python => "Python",
        Layer::Julia => "Julia",
        Layer::Rust => "Rust",
        Layer::Prefix => "Prefix",
        _ => "",
    }
}

/// One install's provenance on its card: where its upstream source came from
/// and the sha256 the build checked, the recipe as built, every patch - whose
/// it is, how big, why, its public copy, and its text on demand - and the
/// build log. Every link is a public file anyone can fetch (2026-10-28).
fn built_from(ui: &mut egui::Ui, r: &crate::api::BuildRecord, patched: Option<&crate::api::Patched>, store: &patched::Records) {
    let row = |ui: &mut egui::Ui, k: &str, f: &crate::api::RecordFile| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(k).size(11.0).color(theme::hush()));
            if f.url.is_empty() {
                ui.label(RichText::new(&f.file).monospace().size(11.0).color(theme::text()));
            } else {
                ui.hyperlink_to(RichText::new(&f.file).monospace().size(11.0), &f.url);
            }
            if !f.sha256.is_empty() {
                ui.label(RichText::new(format!("sha256 {}…", &f.sha256[..12.min(f.sha256.len())])).monospace().size(10.5).color(theme::muted())).on_hover_text(&f.sha256);
            }
            if !f.note.is_empty() {
                ui.label(RichText::new(format!("not mirrored: {}", f.note)).size(10.5).color(theme::warn()));
            }
        });
    };
    ui.add_space(4.0);
    ui.label(RichText::new(format!("built from · {} · {} · build {}", r.version, r.triple, r.build)).size(12.0).strong().color(theme::text_strong()));
    if !r.homepage.is_empty() {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("upstream").size(11.0).color(theme::hush()));
            ui.hyperlink_to(RichText::new(&r.homepage).size(11.0), &r.homepage);
        });
    }
    for f in &r.sources {
        row(ui, "source", f);
    }
    row(ui, "recipe", &r.recipe);
    for p in &r.patches {
        let key = format!("{}/{}/{}", r.name, r.version, p.file);
        let about = patched.and_then(|pv| pv.patches.iter().find(|x| x.file == p.file));
        ui.horizontal_wrapped(|ui| {
            row(ui, "patch", p);
            if let Some(a) = about {
                let whose = if a.ours { "ndexr" } else { "EasyBuild" };
                let why = if a.why.is_empty() { "the patch says nothing about why".to_owned() } else { a.why.clone() };
                ui.label(RichText::new(format!("{whose} · +{} −{}", a.added, a.removed)).size(10.5).color(PATCH)).on_hover_text(why);
            }
            let open = store.open.lock().unwrap().as_deref() == Some(key.as_str());
            if ui.small_button(if open { "hide" } else { "read" }).on_hover_text("show what this patch changes, line by line").clicked() {
                *store.open.lock().unwrap() = if open { None } else { Some(key.clone()) };
            }
        });
        if store.open.lock().unwrap().as_deref() == Some(key.as_str()) {
            match store.text(&r.name, &r.version, &p.file, ui.ctx()) {
                None => {
                    ui.label(RichText::new("reading the patch…").size(11.0).color(theme::muted()));
                }
                Some(t) => {
                    egui::ScrollArea::both().id_salt(&key).max_height(320.0).show(ui, |ui| {
                        for l in t.lines().take(4000) {
                            let c = if l.starts_with("+++") || l.starts_with("---") || l.starts_with("diff ") {
                                theme::text_strong()
                            } else if l.starts_with('+') {
                                theme::ok()
                            } else if l.starts_with('-') {
                                theme::alt()
                            } else if l.starts_with("@@") {
                                theme::primary()
                            } else {
                                theme::muted()
                            };
                            ui.label(RichText::new(l).monospace().size(10.5).color(c));
                        }
                    });
                }
            }
        }
    }
    row(ui, "build log", &r.log);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("EasyBuild {}", r.easybuild)).size(11.0).color(theme::hush()));
        ui.hyperlink_to(RichText::new("every build of this version").size(11.0), &r.records);
    });
}
