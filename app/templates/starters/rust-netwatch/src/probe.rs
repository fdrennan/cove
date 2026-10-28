//! Latency probes: the server opens a TCP connection to host:port, times the
//! handshake (SYN out to SYN-ACK back, as connect() returns), and closes it.
//! No byte of payload is sent, so a probe costs the target one accept().
//!
//! Each probe keeps its last WINDOW samples. A lost sample (timeout, refused)
//! is kept as None so loss is a fraction of attempts, not of successes.

use std::collections::VecDeque;
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

pub const WINDOW: usize = 300;
pub const MAX_PROBES: usize = 8;
const TIMEOUT: Duration = Duration::from_secs(2);
/// Histogram bins: 0.2 decade wide from 10 µs to 3.2 s, so loopback (tens of
/// µs), a LAN (sub-ms) and another continent (100+ ms) share one axis.
const BIN_LO: f64 = -2.0; // log10(ms)
const BIN_W: f64 = 0.2;
const BINS: usize = 28;

pub struct Probe {
    pub id: u32,
    pub target: String,
    addr: SocketAddr,
    every: Duration,
    next: Instant,
    inflight: bool,
    samples: VecDeque<Option<f64>>,
    /// RFC 3550 interarrival jitter estimator applied to connect times:
    /// J += (|D| - J) / 16, D the change between consecutive successes.
    jitter: f64,
    last_ok: Option<f64>,
    last_err: Option<String>,
    attempts: u64,
}

#[derive(Default)]
pub struct Probes {
    pub list: Vec<Probe>,
    next_id: u32,
}

/// `host:port` or `[v6]:port`. Only the characters a hostname or address can
/// hold, so nothing odd reaches the resolver.
fn valid(target: &str) -> bool {
    !target.is_empty()
        && target.len() <= 260
        && target.bytes().all(|b| b.is_ascii_alphanumeric() || b".-:[]_".contains(&b))
}

/// `host:port` to one address. Resolved once, when the probe is added: a
/// name that stops resolving later shows as loss on its old address rather
/// than as a moving target.
pub fn resolve(target: &str) -> Result<SocketAddr, String> {
    let target = target.trim();
    if !valid(target) {
        return Err("target is host:port, e.g. example.org:443 or 127.0.0.1:22".into());
    }
    let addr = target
        .to_socket_addrs()
        .map_err(|e| format!("{target}: {e}"))?
        .next()
        .ok_or_else(|| format!("{target}: no address"))?;
    if addr.port() == 0 {
        return Err("port 0 is not a port".into());
    }
    Ok(addr)
}

impl Probes {
    /// Adds a probe to an address `resolve` returned for `target`.
    pub fn add(&mut self, target: &str, addr: SocketAddr, every_s: f64) -> Result<Value, String> {
        let target = target.trim();
        if !(1.0..=60.0).contains(&every_s) {
            return Err("every is 1..60 seconds".into());
        }
        if self.list.len() >= MAX_PROBES {
            return Err(format!("{MAX_PROBES} probes at most; remove one first"));
        }
        if let Some(p) = self.list.iter().find(|p| p.target == target) {
            return Ok(json!({ "id": p.id, "target": p.target, "existing": true }));
        }
        self.next_id += 1;
        let id = self.next_id;
        self.list.push(Probe {
            id,
            target: target.to_owned(),
            addr,
            every: Duration::from_secs_f64(every_s),
            next: Instant::now(),
            inflight: false,
            samples: VecDeque::with_capacity(WINDOW),
            jitter: 0.0,
            last_ok: None,
            last_err: None,
            attempts: 0,
        });
        Ok(json!({ "id": id, "target": target, "family": if addr.is_ipv6() { "v6" } else { "v4" } }))
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.list.len();
        self.list.retain(|p| p.id != id);
        self.list.len() < before
    }

    pub fn json(&self) -> Value {
        Value::Array(self.list.iter().map(Probe::json).collect())
    }
}

impl Probe {
    fn record(&mut self, r: Result<f64, String>) {
        if self.samples.len() == WINDOW {
            self.samples.pop_front();
        }
        self.attempts += 1;
        match r {
            Ok(ms) => {
                if let Some(prev) = self.last_ok {
                    self.jitter += ((ms - prev).abs() - self.jitter) / 16.0;
                }
                self.last_ok = Some(ms);
                self.last_err = None;
                self.samples.push_back(Some(ms));
            }
            Err(e) => {
                self.last_err = Some(e);
                self.samples.push_back(None);
            }
        }
    }

    fn json(&self) -> Value {
        let mut ok: Vec<f64> = self.samples.iter().flatten().copied().collect();
        ok.sort_by(|a, b| a.total_cmp(b));
        let n = self.samples.len();
        let q = |p: f64| if ok.is_empty() { Value::Null } else { json!(ok[((ok.len() - 1) as f64 * p).round() as usize]) };
        let mean = if ok.is_empty() { Value::Null } else { json!(ok.iter().sum::<f64>() / ok.len() as f64) };
        let mut hist = [0u32; BINS];
        for &ms in &ok {
            let b = ((ms.max(1e-6).log10() - BIN_LO) / BIN_W).floor();
            hist[b.clamp(0.0, (BINS - 1) as f64) as usize] += 1;
        }
        let recent: Vec<Value> = self.samples.iter().rev().take(120).rev().map(|s| s.map_or(Value::Null, |v| json!(v))).collect();
        json!({
            "id": self.id, "target": self.target, "every_s": self.every.as_secs_f64(),
            "attempts": self.attempts, "window": n, "ok": ok.len(),
            "loss": if n > 0 { 1.0 - ok.len() as f64 / n as f64 } else { 0.0 },
            "min": q(0.0), "p50": q(0.5), "p95": q(0.95), "max": q(1.0), "mean": mean,
            "jitter": self.jitter, "last": self.samples.back().copied().flatten(),
            "error": self.last_err,
            "hist": { "lo_log10_ms": BIN_LO, "width": BIN_W, "counts": hist.to_vec() },
            "recent": recent,
        })
    }
}

/// The probe loop: wakes ten times a second, starts every due probe on its
/// own short thread so a 2 s timeout to one target never delays another.
pub fn run(probes: Arc<Mutex<Probes>>) {
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let now = Instant::now();
        let due: Vec<(u32, SocketAddr)> = {
            let mut ps = probes.lock().unwrap_or_else(|e| e.into_inner());
            ps.list
                .iter_mut()
                .filter(|p| !p.inflight && p.next <= now)
                .map(|p| {
                    p.inflight = true;
                    p.next = now + p.every;
                    (p.id, p.addr)
                })
                .collect()
        };
        for (id, addr) in due {
            let probes = Arc::clone(&probes);
            std::thread::spawn(move || {
                let t = Instant::now();
                let r = TcpStream::connect_timeout(&addr, TIMEOUT)
                    .map(|_| t.elapsed().as_secs_f64() * 1e3)
                    .map_err(|e| e.kind().to_string());
                let mut ps = probes.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(p) = ps.list.iter_mut().find(|p| p.id == id) {
                    p.inflight = false;
                    p.record(r);
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jitter_and_quantiles() {
        let mut ps = Probes::default();
        ps.add("127.0.0.1:9", resolve("127.0.0.1:9").unwrap(), 1.0).unwrap();
        let p = &mut ps.list[0];
        for ms in [10.5, 12.5, 10.5, 12.5] {
            p.record(Ok(ms));
        }
        p.record(Err("timed out".into()));
        // |D| = 2 three times: J = 2(1 - (15/16)^3).
        assert!((p.jitter - 2.0 * (1.0 - (15.0f64 / 16.0).powi(3))).abs() < 1e-12);
        let j = p.json();
        assert_eq!(j["ok"], 4);
        assert!((j["loss"].as_f64().unwrap() - 0.2).abs() < 1e-12);
        assert_eq!(j["min"], 10.5);
        assert_eq!(j["max"], 12.5);
        assert_eq!(j["mean"], 11.5);
        // 10.5 and 12.5 ms: log10 = 1.02 and 1.10, both in bin floor((log10 + 2) / 0.2) = 15.
        assert_eq!(j["hist"]["counts"][15], 4);
    }

    #[test]
    fn rejects_bad_targets() {
        assert!(resolve("example.org").is_err());
        assert!(resolve("a b:80").is_err());
        assert!(resolve("127.0.0.1:0").is_err());
        let mut ps = Probes::default();
        assert!(ps.add("127.0.0.1:80", resolve("127.0.0.1:80").unwrap(), 0.1).is_err());
    }
}
