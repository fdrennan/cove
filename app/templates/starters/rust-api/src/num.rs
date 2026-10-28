//! The numerics. Plain functions over slices: no allocation beyond the answer,
//! no dependencies beyond PNG encoding.

use serde_json::{json, Value};

/// Linear-interpolated quantile of sorted data (R's type 7, numpy's default).
pub fn quantile(sorted: &[f64], p: f64) -> f64 {
    let h = (sorted.len() - 1) as f64 * p;
    let lo = h.floor() as usize;
    let hi = (lo + 1).min(sorted.len() - 1);
    sorted[lo] + (h - lo as f64) * (sorted[hi] - sorted[lo])
}

pub fn stats(xs: &[f64], probs: &[f64]) -> Result<Value, String> {
    if xs.is_empty() {
        return Err("no numbers".into());
    }
    if let Some(p) = probs.iter().find(|p| !(0.0..=1.0).contains(*p)) {
        return Err(format!("probability outside [0, 1]: {p}"));
    }
    let n = xs.len() as f64;
    let sum: f64 = xs.iter().sum();
    let mean = sum / n;
    // Two passes: the centred sum of squares keeps precision for large means.
    let ss: f64 = xs.iter().map(|x| (x - mean).powi(2)).sum();
    let var = (xs.len() > 1).then(|| ss / (n - 1.0));
    let sd = var.map(f64::sqrt);
    let skew = sd.filter(|s| *s > 0.0).map(|s| xs.iter().map(|x| ((x - mean) / s).powi(3)).sum::<f64>() / n);
    let mut sorted = xs.to_vec();
    sorted.sort_by(f64::total_cmp);
    let quantiles: Vec<Value> = probs.iter().map(|&p| json!({ "p": p, "q": quantile(&sorted, p) })).collect();
    Ok(json!({
        "n": xs.len(),
        "sum": sum,
        "mean": mean,
        "var": var,
        "sd": sd,
        "se": sd.map(|s| s / n.sqrt()),
        "skewness": skew,
        "min": sorted[0],
        "max": sorted[sorted.len() - 1],
        "median": quantile(&sorted, 0.5),
        "iqr": quantile(&sorted, 0.75) - quantile(&sorted, 0.25),
        "quantiles": quantiles,
    }))
}

/// Ordinary least squares, y = intercept + slope * x.
pub fn fit_linear(x: &[f64], y: &[f64]) -> Result<Value, String> {
    if x.len() != y.len() {
        return Err(format!("x has {} values, y has {}", x.len(), y.len()));
    }
    if x.len() < 2 {
        return Err("a line needs at least two points".into());
    }
    let n = x.len() as f64;
    let mx = x.iter().sum::<f64>() / n;
    let my = y.iter().sum::<f64>() / n;
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for (a, b) in x.iter().zip(y) {
        sxx += (a - mx) * (a - mx);
        sxy += (a - mx) * (b - my);
        syy += (b - my) * (b - my);
    }
    if sxx == 0.0 {
        return Err("every x is the same: the slope is undefined".into());
    }
    let slope = sxy / sxx;
    let intercept = my - slope * mx;
    let ss_res: f64 = x.iter().zip(y).map(|(a, b)| (b - intercept - slope * a).powi(2)).sum();
    let r2 = if syy == 0.0 { 1.0 } else { 1.0 - ss_res / syy };
    // Standard errors need a residual degree of freedom.
    let s = (x.len() > 2).then(|| (ss_res / (n - 2.0)).sqrt());
    Ok(json!({
        "n": x.len(),
        "slope": slope,
        "intercept": intercept,
        "r2": r2,
        "r": if syy == 0.0 { Value::Null } else { json!(sxy / (sxx * syy).sqrt()) },
        "residual_se": s,
        "se_slope": s.map(|s| s / sxx.sqrt()),
        "se_intercept": s.map(|s| s * (1.0 / n + mx * mx / sxx).sqrt()),
    }))
}

pub const PRIMES_MAX: u64 = 200_000_000;

/// Sieve of Eratosthenes over the odd numbers: index i stands for 2i + 1.
pub fn primes(n: u64) -> Value {
    if n < 2 {
        return json!({ "n": n, "count": 0, "largest": null, "first": [], "last": [] });
    }
    let n = n as usize;
    let m = (n - 1) / 2;
    let mut odd = vec![true; m + 1];
    odd[0] = false; // 1
    let mut i = 1;
    while (2 * i + 1) * (2 * i + 1) <= n {
        if odd[i] {
            let p = 2 * i + 1;
            let mut j = (p * p - 1) / 2;
            while j <= m {
                odd[j] = false;
                j += p;
            }
        }
        i += 1;
    }
    let count = 1 + odd.iter().filter(|b| **b).count();
    let all = std::iter::once(2).chain(odd.iter().enumerate().filter(|(_, b)| **b).map(|(i, _)| 2 * i + 1));
    let first: Vec<usize> = all.take(10).collect();
    let mut last: Vec<usize> = odd.iter().enumerate().rev().filter(|(_, b)| **b).map(|(i, _)| 2 * i + 1).take(10).collect();
    if last.len() < 10 {
        last.push(2);
    }
    last.reverse();
    json!({ "n": n, "count": count, "largest": last.last(), "first": first, "last": last })
}

pub struct Lorenz {
    pub sigma: f64,
    pub rho: f64,
    pub beta: f64,
}

impl Lorenz {
    fn f(&self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        [self.sigma * (y - x), x * (self.rho - z) - y, x * y - self.beta * z]
    }

    /// Classical fourth-order Runge-Kutta, `steps` steps of `dt` from `start`.
    pub fn rk4(&self, start: [f64; 3], dt: f64, steps: usize) -> Vec<[f64; 3]> {
        let add = |a: [f64; 3], b: [f64; 3], h: f64| [a[0] + h * b[0], a[1] + h * b[1], a[2] + h * b[2]];
        let mut out = Vec::with_capacity(steps + 1);
        let mut s = start;
        out.push(s);
        for _ in 0..steps {
            let k1 = self.f(s);
            let k2 = self.f(add(s, k1, dt / 2.0));
            let k3 = self.f(add(s, k2, dt / 2.0));
            let k4 = self.f(add(s, k3, dt));
            for d in 0..3 {
                s[d] += dt / 6.0 * (k1[d] + 2.0 * k2[d] + 2.0 * k3[d] + k4[d]);
            }
            out.push(s);
        }
        out
    }
}

pub struct View {
    pub w: u32,
    pub h: u32,
    pub cx: f64,
    pub cy: f64,
    pub scale: f64,
    pub iter: u32,
}

/// The Mandelbrot set as RGB pixels, smooth-coloured, rows split across cores.
pub fn mandelbrot(v: &View) -> Vec<u8> {
    let (w, h) = (v.w as usize, v.h as usize);
    let mut px = vec![0u8; w * h * 3];
    let step = v.scale / w as f64;
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let rows_per = h.div_ceil(threads).max(1);
    std::thread::scope(|sc| {
        for (chunk, band) in px.chunks_mut(rows_per * w * 3).enumerate() {
            sc.spawn(move || {
                for (r, row) in band.chunks_mut(w * 3).enumerate() {
                    let ci = v.cy + ((chunk * rows_per + r) as f64 - h as f64 / 2.0) * step;
                    for (c, rgb) in row.chunks_mut(3).enumerate() {
                        let cr = v.cx + (c as f64 - w as f64 / 2.0) * step;
                        rgb.copy_from_slice(&colour(escape(cr, ci, v.iter), v.iter));
                    }
                }
            });
        }
    });
    px
}

/// Smooth escape count, or None inside the set.
fn escape(cr: f64, ci: f64, max: u32) -> Option<f64> {
    // Main cardioid and period-2 bulb: inside, skip the loop.
    let q = (cr - 0.25).powi(2) + ci * ci;
    if q * (q + cr - 0.25) <= 0.25 * ci * ci || (cr + 1.0).powi(2) + ci * ci <= 0.0625 {
        return None;
    }
    let (mut x, mut y) = (0.0f64, 0.0f64);
    for i in 0..max {
        let (x2, y2) = (x * x, y * y);
        if x2 + y2 > 256.0 {
            return Some(i as f64 + 1.0 - ((x2 + y2).ln() / 2.0).ln() / std::f64::consts::LN_2);
        }
        y = 2.0 * x * y + ci;
        x = x2 - y2 + cr;
    }
    None
}

fn colour(e: Option<f64>, max: u32) -> [u8; 3] {
    let Some(n) = e else { return [8, 10, 16] };
    let t = (n / max as f64).sqrt();
    // Cosine palette: deep blue through teal to a warm white edge.
    let ch = |a: f64, b: f64, c: f64, d: f64| {
        let v = a + b * (std::f64::consts::TAU * (c * t * 4.0 + d)).cos();
        // Far from the set fades into the background.
        (v.clamp(0.0, 1.0) * 255.0 * (t * 2.5).min(1.0)) as u8
    };
    [ch(0.5, 0.5, 1.0, 0.0), ch(0.5, 0.5, 1.0, 0.10), ch(0.5, 0.5, 1.0, 0.20)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_answers() {
        let s = stats(&[1.0, 2.0, 3.0, 4.0], &[0.25]).unwrap();
        assert_eq!(s["mean"], 2.5);
        assert_eq!(s["median"], 2.5);
        assert_eq!(s["quantiles"][0]["q"], 1.75);
        let f = fit_linear(&[0.0, 1.0, 2.0], &[1.0, 3.0, 5.0]).unwrap();
        assert_eq!(f["slope"], 2.0);
        assert_eq!(f["intercept"], 1.0);
        assert_eq!(primes(100)["count"], 25);
        assert_eq!(primes(1_000_000)["count"], 78_498);
        assert_eq!(primes(2)["largest"], 2);
    }
}
