//! The physics. Newtonian gravity in a plane with Plummer softening, summed
//! directly over every pair (O(n²), exact up to rounding) and integrated with
//! kick-drift-kick leapfrog.
//!
//! Leapfrog is symplectic: it conserves a shadow Hamiltonian close to the true
//! one, so the total energy oscillates around its start value and does not
//! wander off the way it does under Euler or RK4. `energy()` is there to show
//! it.
//!
//! State is struct-of-arrays (px, py, vx, vy, m) so the inner force loop reads
//! contiguous f64 slices the compiler can vectorise; rayon splits the outer
//! loop over bodies across cores. Each body writes only its own acceleration,
//! so the parallel loop needs no locks: the borrow checker proves it.

use rayon::prelude::*;

/// Bodies at or above this mass are drawn as stars, and kept at the front.
pub const HEAVY: f64 = 0.05;
pub const N_MAX: usize = 20_000;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Preset {
    Galaxies,
    Disc,
    Collapse,
    /// One planet on a circular orbit of radius 1 around a unit mass: period
    /// 2π, energy -m/2. The check the integrator is measured against.
    Kepler,
}

impl Preset {
    pub fn parse(s: &str) -> Option<Preset> {
        Some(match s {
            "galaxies" => Preset::Galaxies,
            "disc" => Preset::Disc,
            "collapse" => Preset::Collapse,
            "kepler" => Preset::Kepler,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Preset::Galaxies => "galaxies",
            Preset::Disc => "disc",
            Preset::Collapse => "collapse",
            Preset::Kepler => "kepler",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Params {
    pub g: f64,
    pub soft: f64,
    pub dt: f64,
}

pub struct Sim {
    pub preset: Preset,
    pub px: Vec<f64>,
    pub py: Vec<f64>,
    pub vx: Vec<f64>,
    pub vy: Vec<f64>,
    pub m: Vec<f64>,
    ax: Vec<f64>,
    ay: Vec<f64>,
    /// The (G, softening) the cached accelerations were computed with.
    acc_for: Option<(f64, f64)>,
    pub t: f64,
    pub steps: u64,
    /// Energy at the start, or at the last change of the Hamiltonian (G,
    /// softening, a dropped body). Drift is measured from here.
    pub e0: Option<f64>,
    /// Bumped on every reset or drop, so the page knows to restart its trace.
    pub epoch: u64,
}

/// splitmix64: small, fast, and the same stream on every machine, so a seed
/// reproduces a run.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((z ^ (z >> 31)) >> 11) as f64 / (1u64 << 53) as f64
    }
    fn gauss(&mut self) -> f64 {
        let u = self.next().max(1e-300);
        (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * self.next()).cos()
    }
}

impl Sim {
    pub fn new(preset: Preset, n: usize, seed: u64, soft: f64) -> Sim {
        let mut s = Sim {
            preset,
            px: vec![],
            py: vec![],
            vx: vec![],
            vy: vec![],
            m: vec![],
            ax: vec![],
            ay: vec![],
            acc_for: None,
            t: 0.0,
            steps: 0,
            e0: None,
            epoch: 0,
        };
        let mut rng = Rng(seed);
        let n = n.clamp(2, N_MAX);
        match preset {
            Preset::Disc => s.disc(&mut rng, n - 1, [0.0, 0.0], [0.0, 0.0], 1.0, 0.08, 1.0, 1.0, soft),
            Preset::Galaxies => {
                // Two discs on a bound, near-parabolic approach: they swing
                // past, throw out tidal tails, and fall back to merge. The
                // second spins the other way, so one tail is long and one short.
                let half = (n - 2) / 2;
                s.disc(&mut rng, half, [-1.1, -0.35], [0.22, 0.0], 0.5, 0.05, 0.55, 1.0, soft);
                s.disc(&mut rng, n - 2 - half, [1.1, 0.35], [-0.22, 0.0], 0.5, 0.05, 0.55, -1.0, soft);
            }
            Preset::Collapse => {
                // A cold, uniform disc of total mass 1 at rest: no pressure
                // holds it up, so it falls in on itself and rebounds into a
                // hot, virialised blob.
                let mi = 1.0 / n as f64;
                for _ in 0..n {
                    let r = rng.next().sqrt();
                    let th = std::f64::consts::TAU * rng.next();
                    s.push(r * th.cos(), r * th.sin(), 0.01 * rng.gauss(), 0.01 * rng.gauss(), mi);
                }
            }
            Preset::Kepler => {
                s.push(0.0, 0.0, 0.0, 0.0, 1.0);
                s.push(1.0, 0.0, 0.0, 1.0, 1e-6);
            }
        }
        s.sort_heavy_first();
        s
    }

    fn push(&mut self, x: f64, y: f64, vx: f64, vy: f64, m: f64) {
        self.px.push(x);
        self.py.push(y);
        self.vx.push(vx);
        self.vy.push(vy);
        self.m.push(m);
    }

    /// A central mass `mc` with `n` light bodies on circular orbits between
    /// `r0` and `r1`, uniform in radius (surface density ∝ 1/r, a Mestel-like
    /// disc). The disc weighs a tenth of the centre. `spin` is ±1.
    #[allow(clippy::too_many_arguments)]
    fn disc(&mut self, rng: &mut Rng, n: usize, c: [f64; 2], v: [f64; 2], mc: f64, r0: f64, r1: f64, spin: f64, soft: f64) {
        let md = 0.1 * mc;
        let mi = md / n.max(1) as f64;
        self.push(c[0], c[1], v[0], v[1], mc);
        for _ in 0..n {
            let r = r0 + (r1 - r0) * rng.next();
            let th = std::f64::consts::TAU * rng.next();
            // Mass inside r: the centre plus the disc so far. The softened
            // point-mass circular speed is v² = G M r² / (r² + ε²)^{3/2}.
            let menc = mc + md * (r - r0) / (r1 - r0);
            let vc = (menc * r * r / (r * r + soft * soft).powf(1.5)).sqrt();
            // 3% random motion keeps the disc from ringing in lockstep.
            let (dvx, dvy) = (0.03 * vc * rng.gauss(), 0.03 * vc * rng.gauss());
            self.push(
                c[0] + r * th.cos(),
                c[1] + r * th.sin(),
                v[0] - spin * vc * th.sin() + dvx,
                v[1] + spin * vc * th.cos() + dvy,
                mi,
            );
        }
    }

    fn sort_heavy_first(&mut self) {
        let mut idx: Vec<usize> = (0..self.m.len()).collect();
        idx.sort_by_key(|&i| self.m[i] < HEAVY);
        let take = |v: &Vec<f64>| idx.iter().map(|&i| v[i]).collect::<Vec<_>>();
        self.px = take(&self.px);
        self.py = take(&self.py);
        self.vx = take(&self.vx);
        self.vy = take(&self.vy);
        self.m = take(&self.m);
        self.acc_for = None;
    }

    pub fn n(&self) -> usize {
        self.m.len()
    }
    pub fn n_heavy(&self) -> usize {
        self.m.iter().take_while(|&&m| m >= HEAVY).count()
    }

    /// Drops a body of mass `mass` at rest at (x, y). The Hamiltonian changes,
    /// so the energy reference restarts.
    pub fn drop_body(&mut self, x: f64, y: f64, mass: f64) {
        self.push(x, y, 0.0, 0.0, mass);
        self.sort_heavy_first();
        self.e0 = None;
        self.epoch += 1;
    }

    /// a_i = G Σ_j m_j (x_j - x_i) / (|x_j - x_i|² + ε²)^{3/2}. The j = i term
    /// is zero because ε > 0, so the loop has no branch for it.
    fn accel(&mut self, g: f64, soft: f64) {
        let n = self.n();
        let (px, py, m) = (&self.px, &self.py, &self.m);
        let e2 = soft * soft;
        self.ax.resize(n, 0.0);
        self.ay.resize(n, 0.0);
        self.ax
            .par_iter_mut()
            .zip(self.ay.par_iter_mut())
            .enumerate()
            .with_min_len(32)
            .for_each(|(i, (ax, ay))| {
                let (xi, yi) = (px[i], py[i]);
                let (mut sx, mut sy) = (0.0, 0.0);
                for ((&xj, &yj), &mj) in px.iter().zip(py).zip(m) {
                    let (dx, dy) = (xj - xi, yj - yi);
                    let r2 = dx * dx + dy * dy + e2;
                    let s = mj / (r2 * r2.sqrt());
                    sx += dx * s;
                    sy += dy * s;
                }
                *ax = g * sx;
                *ay = g * sy;
            });
        self.acc_for = Some((g, soft));
    }

    /// (kinetic, potential), with the same softened potential the force is
    /// the gradient of: U = -G Σ_{i<j} m_i m_j / sqrt(r² + ε²).
    pub fn energy(&self, g: f64, soft: f64) -> (f64, f64) {
        let e2 = soft * soft;
        let (px, py, m) = (&self.px, &self.py, &self.m);
        let ke: f64 = (0..self.n()).map(|i| 0.5 * m[i] * (self.vx[i].powi(2) + self.vy[i].powi(2))).sum();
        let pe: f64 = (0..self.n())
            .into_par_iter()
            .map(|i| {
                let mut u = 0.0;
                for j in i + 1..px.len() {
                    let (dx, dy) = (px[j] - px[i], py[j] - py[i]);
                    u += m[j] / (dx * dx + dy * dy + e2).sqrt();
                }
                -g * m[i] * u
            })
            .sum();
        (ke, pe)
    }

    /// `k` kick-drift-kick leapfrog steps. The accelerations carry over from
    /// one step to the next, so a step costs one force evaluation.
    pub fn step(&mut self, p: Params, k: usize) {
        if self.acc_for != Some((p.g, p.soft)) {
            // New G or ε: a new Hamiltonian, so a new energy reference too.
            if self.acc_for.is_some() {
                self.e0 = None;
                self.epoch += 1;
            }
            self.accel(p.g, p.soft);
        }
        if self.e0.is_none() {
            let (ke, pe) = self.energy(p.g, p.soft);
            self.e0 = Some(ke + pe);
        }
        let h = 0.5 * p.dt;
        for _ in 0..k {
            for i in 0..self.n() {
                self.vx[i] += h * self.ax[i];
                self.vy[i] += h * self.ay[i];
                self.px[i] += p.dt * self.vx[i];
                self.py[i] += p.dt * self.vy[i];
            }
            self.accel(p.g, p.soft);
            for i in 0..self.n() {
                self.vx[i] += h * self.ax[i];
                self.vy[i] += h * self.ay[i];
            }
            self.t += p.dt;
            self.steps += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One period of a circular Kepler orbit brings the planet back to (1, 0).
    #[test]
    fn kepler_period() {
        let mut s = Sim::new(Preset::Kepler, 2, 1, 1e-4);
        let p = Params { g: 1.0, soft: 1e-4, dt: 1e-3 };
        let k = (std::f64::consts::TAU / p.dt).round() as usize;
        s.step(p, k);
        let (x, y) = (s.px[1] - s.px[0], s.py[1] - s.py[0]);
        assert!((x - 1.0).abs() < 1e-3 && y.abs() < 1e-2, "planet at ({x}, {y})");
        let (ke, pe) = s.energy(p.g, p.soft);
        assert!(((ke + pe) / s.e0.unwrap() - 1.0).abs() < 1e-6);
    }
}
