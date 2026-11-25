//! The applications' own stack: how a request reaches a domain and comes
//! back (operator, 2026-10-21: "the applications layer needs circles or their
//! own easybuild, prefix, kernel, Rust, R equivalents (maybe nginx, servers
//! (router, on prem), and the module routing the shiny application managers
//! and which decision is passed, especially in the different application (non
//! shiny module) types, are, and the resources required").
//!
//! Read live (host.rs `serving`, admins only), bottom up: the on-prem machine
//! over the software stack - the running copy of the kernel, its CPUs and
//! memory - then the ring of what runs each domain (the Shiny router's
//! replicas, a port, a cell, a remote, a redirect), then the domains on
//! those. nginx is over the domains, not under them: it is where requests
//! arrive from outside, and a module domain runs nearer the machine than the
//! front door does (operator, 2026-10-21: "nginx needs to be above the
//! domains as its receiving from the outside world and many of the domains
//! are modules which run closer to the ubuntu side"). Over nginx, where
//! people come in: the edge router whose tunnel brings traffic down to it,
//! and each apex's DNS (Route 53).

use super::*;
use crate::api::Serving;

/// How far the domains move up to make room for the host and the upstream
/// ring under them, in steps.
const ROOM: f32 = 2.6;

/// A domain's target as a person reads it (api::DomainInfo::target).
pub(super) fn target_word(t: &str) -> String {
    match t {
        "module" => "Shiny router · app.r".to_owned(),
        "cell" => "the owner's cell".to_owned(),
        "redirect" => "a redirect".to_owned(),
        "" => "no vhost".to_owned(),
        t => match t.split_once(':') {
            Some(("port", p)) => format!("port {p}"),
            Some(("remote", r)) => format!("remote · {r}"),
            _ => t.to_owned(),
        },
    }
}

impl Scene {
    /// The serving column, laid into a built scene: appended, so every index
    /// already handed out stays good; the domains move up to make room.
    pub(super) fn add_serving(&mut self, s: &Serving) {
        let stack_top = self.orbits.iter().filter(|o| o.layer != Layer::Domain).map(|o| o.c.y).fold(0.0f32, f32::max);
        let lift = v3(0.0, HG_STEP * ROOM, 0.0);
        for nd in self.nodes.iter_mut().filter(|n| n.layer == Layer::Domain) {
            nd.pos = nd.pos.add(lift);
        }
        for o in self.orbits.iter_mut().filter(|o| o.layer == Layer::Domain) {
            o.c = o.c.add(lift);
        }
        if let Some(gs) = self.groups.get_mut(&Layer::Domain) {
            for g in gs.iter_mut() {
                g.2 = g.2.add(lift);
            }
        }
        let node = |nodes: &mut Vec<Node>, pos: V3, layer: Layer, label: String, group: String| {
            nodes.push(Node { pos, layer, label, group, sw: None, dom: None });
            (nodes.len() - 1) as u32
        };
        let mut edges: Vec<Edge> = Vec::new();
        let y = |k: f32| stack_top + HG_STEP * k;

        // ── the machine: the running copy of the kernel, and what it has ──
        // Named for where it is, not its hostname (operator, 2026-10-21:
        // "ubuntu system product name in the view should just be on prem").
        let mem = |kb: u64| format!("{:.0} GB", kb as f64 / 1_048_576.0);
        let host = node(&mut self.nodes, v3(0.0, y(1.1), 0.0), Layer::Host, "on prem".into(), format!("Linux {} · {} CPUs · {}", s.kernel, s.cpus, mem(s.mem_total_kb)));
        // It runs the floor built for its own architecture.
        for &k in &self.kernels {
            let nd = &self.nodes[k as usize];
            if self.floors.is_empty() || nd.group.starts_with(&s.arch) {
                edges.push(Edge { from: host, to: k, ambient: true, rel: Rel::Runs });
            }
        }
        // ── what runs each domain, on the machine ────────────────────────
        let mut targets: Vec<String> = Vec::new();
        for d in &self.domains {
            if !targets.contains(&d.target) {
                targets.push(d.target.clone());
            }
        }
        // The Shiny router first, then by how many domains each serves.
        let serves = |t: &str| self.domains.iter().filter(|d| d.target == t).count();
        targets.sort_by_key(|t| (t != "module", std::cmp::Reverse(serves(t)), t.clone()));
        let ring = Orbit { c: v3(0.0, y(2.0), 0.0), r: 2.4, layer: Layer::Route, named: true, tier: None, n: targets.len() };
        let weights: Vec<f32> = targets.iter().map(|t| (serves(t) as f32).sqrt().max(1.0)).collect();
        let spans = arcs(&weights, 0.0);
        let mut upstream: HashMap<String, u32> = HashMap::new();
        let mut out = Vec::new();
        for (t, &(a, w)) in targets.iter().zip(&spans) {
            let at = ring.at(a + w / 2.0, ring.r);
            let k = node(&mut self.nodes, at, Layer::Route, target_word(t), t.clone());
            // It runs on the machine.
            edges.push(Edge { from: k, to: host, ambient: true, rel: Rel::Runs });
            upstream.insert(t.clone(), k);
            out.push((target_word(t), serves(t), at));
        }
        self.orbits.push(Orbit { c: v3(0.0, y(1.1), 0.0), r: 0.7, layer: Layer::Host, named: true, tier: None, n: 1 });
        self.orbits.push(ring);
        self.groups.insert(Layer::Route, out);
        self.groups.insert(Layer::Host, vec![(self.nodes[host as usize].group.clone(), 1, v3(0.0, y(1.1), 0.0))]);
        // Each domain stands on what serves it.
        for (k, nd) in self.nodes.iter().enumerate() {
            if let Some(&u) = nd.dom.and_then(|d| self.domains.get(d)).and_then(|d| upstream.get(&d.target)) {
                edges.push(Edge { from: k as u32, to: u, ambient: false, rel: Rel::Serves });
            }
        }

        // ── nginx over the domains: the front door, where requests arrive ─
        let top = self.nodes.iter().filter(|n| n.layer == Layer::Domain).map(|n| n.pos.y).fold(y(3.0), f32::max);
        let nginx = node(&mut self.nodes, v3(0.0, top + HG_STEP * 1.4, 0.0), Layer::Route, "nginx".into(), "front".into());
        edges.push(Edge { from: nginx, to: host, ambient: true, rel: Rel::Runs });
        // It hands each request down to what runs the domain.
        for &u in upstream.values() {
            edges.push(Edge { from: nginx, to: u, ambient: true, rel: Rel::Proxy });
        }

        // ── where people come in: the edge router and DNS, over nginx ────
        let sky = top + HG_STEP * 2.6;
        let router = (!s.router.is_empty()).then(|| node(&mut self.nodes, v3(0.0, sky, 0.0), Layer::Edge, "edge router".into(), s.router.clone()));
        if let Some(r) = router {
            // The tunnel brings what reaches it down to nginx here.
            edges.push(Edge { from: r, to: nginx, ambient: true, rel: Rel::Tunnel });
        }
        let mut dns: HashMap<String, u32> = HashMap::new();
        let mut direct: HashMap<String, u32> = HashMap::new();
        let providers: Vec<String> = {
            let mut p: Vec<String> = s.zones.iter().map(|z| z.dns.clone()).filter(|d| !d.is_empty()).collect();
            p.sort();
            p.dedup();
            p
        };
        let edge_ring = Orbit { c: v3(0.0, sky + HG_STEP * 0.9, 0.0), r: 1.8, layer: Layer::Edge, named: true, tier: None, n: providers.len() };
        for (j, p) in providers.iter().enumerate() {
            let th = std::f32::consts::TAU * j as f32 / providers.len().max(1) as f32;
            let k = node(&mut self.nodes, edge_ring.at(th, edge_ring.r), Layer::Edge, p.clone(), "DNS".into());
            dns.insert(p.clone(), k);
        }
        let mut edge_groups = vec![("DNS".to_owned(), providers.len(), edge_ring.c)];
        if let Some(r) = router {
            edge_groups.push(("edge router".to_owned(), 1, self.nodes[r as usize].pos));
        }
        for z in &s.zones {
            // The apex's star: the domain itself, or the label standing for it.
            let Some(apex) = self.nodes.iter().position(|n| n.layer == Layer::Domain && n.label == z.apex) else { continue };
            let apex = apex as u32;
            if let Some(&d) = dns.get(&z.dns) {
                edges.push(Edge { from: apex, to: d, ambient: true, rel: Rel::Dns });
            }
            match router {
                Some(r) if !z.address.is_empty() && z.address == s.router => edges.push(Edge { from: apex, to: r, ambient: true, rel: Rel::Reach }),
                _ if !z.address.is_empty() => {
                    // Not through the router: straight to some other address.
                    let n = self.nodes.len() as u32;
                    let k = *direct.entry(z.address.clone()).or_insert(n);
                    if k == n {
                        let th = 0.6 + direct.len() as f32 * 0.7;
                        node(&mut self.nodes, v3(3.2 * th.cos(), sky, 3.2 * th.sin()), Layer::Edge, format!("direct · {}", z.address), "direct".into());
                    }
                    edges.push(Edge { from: apex, to: k, ambient: true, rel: Rel::Reach });
                }
                _ => {}
            }
        }
        if !direct.is_empty() {
            edge_groups.push(("direct".to_owned(), direct.len(), v3(0.0, sky, 0.0)));
        }
        self.orbits.push(edge_ring);
        self.groups.insert(Layer::Edge, edge_groups);

        // ── fold it in: edges, both directions, counts ───────────────────
        for e in edges {
            self.edges.push(e);
        }
        self.up = vec![Vec::new(); self.nodes.len()];
        self.down = vec![Vec::new(); self.nodes.len()];
        for (k, e) in self.edges.iter().enumerate() {
            self.up[e.from as usize].push(k as u32);
            self.down[e.to as usize].push(k as u32);
        }
        self.counts.clear();
        for nd in &self.nodes {
            *self.counts.entry(nd.layer).or_insert(0usize) += 1;
        }
        self.tier.resize(self.nodes.len(), None);
        self.planned.resize(self.nodes.len(), false);
    }
}

/// What a serving star is doing now, as card lines.
pub(super) fn live(s: &Serving, nd: &Node) -> Vec<String> {
    let gb = |kb: u64| kb as f64 / 1_048_576.0;
    match nd.layer {
        Layer::Host => vec![
            format!("Linux {} · {}", s.kernel, s.arch),
            format!("{} CPUs · load {:.1} ({:.0}% of them)", s.cpus, s.load1, 100.0 * s.load1 / s.cpus.max(1) as f32),
            format!("memory {:.1} of {:.0} GB free", gb(s.mem_avail_kb), gb(s.mem_total_kb)),
        ],
        Layer::Route if nd.label == "nginx" => vec![format!("{} sites enabled · {}", s.vhosts, if s.nginx_up { "answering on 443" } else { "not answering on 443" })],
        Layer::Route => match s.upstreams.iter().find(|u| u.target == nd.group) {
            Some(u) if u.of > 0 => vec![format!("{} of {} listening here", u.up, u.of), format!("target `{}`", u.target)],
            Some(u) => vec![format!("target `{}` - nothing to call on this machine", u.target)],
            None => Vec::new(),
        },
        Layer::Edge if nd.label == "edge router" => vec![format!("{} · tunnel {}", s.router, if s.tunnel.is_empty() { "unknown" } else { s.tunnel.as_str() })],
        Layer::Edge => {
            let zs: Vec<&str> = s.zones.iter().filter(|z| z.dns == nd.label || nd.label.ends_with(&z.address)).map(|z| z.apex.as_str()).collect();
            vec![format!("{}: {}", zs.len(), zs.join(", "))]
        }
        _ => Vec::new(),
    }
}

/// The structure of a serving answer, for the scene's key: what is drawn,
/// not how it is doing (liveness changes every minute; the scene need not).
pub(super) fn shape(s: &Serving) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&s.host, &s.kernel, &s.arch, &s.router).hash(&mut h);
    for z in &s.zones {
        (&z.apex, &z.dns, &z.address).hash(&mut h);
    }
    for u in &s.upstreams {
        u.target.hash(&mut h);
    }
    h.finish()
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::api::{Serving, Upstream, Zone};

    /// Bottom up: the host over the stack, what runs each domain over it,
    /// the domains, then nginx over them where requests arrive, then the
    /// router and DNS; each domain reaches the kernel through what runs it,
    /// and each apex comes in through the router.
    #[test]
    fn the_applications_have_their_own_stack() {
        let cat = Catalog::parse(br#"{"schema":1,"generated":10,"hash":"h","t0":100,"boxes":[],"runs":[],"items":[{"k":"prefix","n":"sys-libs/glibc","g":"sys-libs"}],"steps":{"i":[],"r":[],"t":[],"d":[],"s":""}}"#).unwrap();
        let doms: Vec<DomainInfo> = [("a.ndexr.io", "module"), ("x.a.ndexr.io", "port:8001"), ("koodle.io", "module")]
            .iter()
            .map(|(h, t)| serde_json::from_value(serde_json::json!({ "host": h, "path": format!("/d/{h}"), "target": t })).unwrap())
            .collect();
        let mut sc = Scene::build(&cat, &doms, "me");
        let before = sc.nodes.len();
        let s = Serving {
            host: "box".into(),
            kernel: "7.0.0".into(),
            arch: "x86_64".into(),
            router: "3.1.4.1".into(),
            tunnel: "active".into(),
            zones: vec![Zone { apex: "ndexr.io".into(), dns: "Route 53".into(), address: "3.1.4.1".into() }, Zone { apex: "koodle.io".into(), dns: "Route 53".into(), address: "8.8.8.8".into() }],
            upstreams: vec![Upstream { target: "module".into(), up: 5, of: 5 }, Upstream { target: "port:8001".into(), up: 1, of: 1 }],
            ..Default::default()
        };
        sc.add_serving(&s);
        assert!(sc.nodes.len() > before);
        let node = |l: &str| sc.nodes.iter().position(|n| n.label == l).unwrap_or_else(|| panic!("{l}")) as u32;
        let y = |l: &str| sc.nodes[node(l) as usize].pos.y;
        let rel = |a: &str, b: &str| sc.up[node(a) as usize].iter().map(|&e| &sc.edges[e as usize]).find(|e| e.to == node(b)).map(|e| e.rel);
        let stack_top = sc.orbits.iter().filter(|o| !matches!(o.layer, Layer::Domain | Layer::Host | Layer::Route | Layer::Edge)).map(|o| o.c.y).fold(0.0, f32::max);
        assert!(stack_top < y("on prem") && y("on prem") < y("Shiny router · app.r"));
        let lowest_domain = sc.nodes.iter().filter(|n| n.layer == Layer::Domain).map(|n| n.pos.y).fold(f32::MAX, f32::min);
        let highest_domain = sc.nodes.iter().filter(|n| n.layer == Layer::Domain).map(|n| n.pos.y).fold(0.0, f32::max);
        assert!(y("Shiny router · app.r") < lowest_domain && y("port 8001") < lowest_domain);
        assert!(highest_domain < y("nginx") && y("nginx") < y("edge router"), "nginx over the domains, the router over nginx");
        assert_eq!(rel("a.ndexr.io", "Shiny router · app.r"), Some(Rel::Serves));
        assert_eq!(rel("x.a.ndexr.io", "port 8001"), Some(Rel::Serves));
        assert_eq!(rel("nginx", "Shiny router · app.r"), Some(Rel::Proxy));
        assert_eq!(rel("Shiny router · app.r", "on prem"), Some(Rel::Runs));
        assert_eq!(rel("nginx", "on prem"), Some(Rel::Runs));
        assert_eq!(rel("edge router", "nginx"), Some(Rel::Tunnel));
        assert_eq!(rel("ndexr.io", "Route 53"), Some(Rel::Dns));
        assert_eq!(rel("ndexr.io", "edge router"), Some(Rel::Reach));
        assert_eq!(rel("koodle.io", "direct · 8.8.8.8"), Some(Rel::Reach));
        // A domain's line runs through what serves it to the running kernel.
        assert!(sc.lineage(node("x.a.ndexr.io")).0[node("on prem") as usize]);
        assert!(sc.lineage(node("on prem")).0[sc.kernel as usize]);
        // Liveness is not shape: a minute's change does not rebuild the scene.
        let mut later = s.clone();
        later.load1 = 9.0;
        later.upstreams[0].up = 4;
        assert_eq!(super::shape(&s), super::shape(&later));
    }
}
