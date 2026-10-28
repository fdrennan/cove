//! Once a second: read the kernel's network tables, turn counters into
//! rates, and publish one JSON sample to every open page. Each sample
//! carries its own cost - how long the kernel took to produce each file and
//! how long Rust took to parse it - and the process's resident memory.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::proc::{self, Buf, IfCounters, TcpAgg, TCP_STATES};

/// Samples kept for a page that opens late: three minutes at 1 Hz.
pub const HISTORY: usize = 180;
/// Interfaces sent per sample, busiest first: a container host can carry
/// hundreds of veths and the page needs the ones that move.
const MAX_IFACES: usize = 48;
const MAX_LISTENERS: usize = 64;

/// The published stream: a sequence number and the last HISTORY samples,
/// each already serialised once and shared by every reader.
#[derive(Default)]
pub struct Feed {
    pub seq: u64,
    pub history: VecDeque<Arc<String>>,
}

pub struct Shared {
    pub feed: Mutex<Feed>,
    pub tick: Condvar,
}

/// /proc/net/snmp counters turned into per-second rates, and the one gauge.
const SNMP: [(&str, &str); 13] = [
    ("Tcp", "ActiveOpens"),
    ("Tcp", "PassiveOpens"),
    ("Tcp", "AttemptFails"),
    ("Tcp", "EstabResets"),
    ("Tcp", "InSegs"),
    ("Tcp", "OutSegs"),
    ("Tcp", "RetransSegs"),
    ("Tcp", "InErrs"),
    ("Tcp", "OutRsts"),
    ("Udp", "InDatagrams"),
    ("Udp", "OutDatagrams"),
    ("Udp", "InErrors"),
    ("Tcp", "CurrEstab"),
];
const SNMP_RATES: usize = 12;

struct Cost {
    file: &'static str,
    read_us: f64,
    parse_us: f64,
    bytes: usize,
}

pub struct Sampler {
    dev: Buf,
    tcp: Buf,
    tcp6: Buf,
    sock: Buf,
    sock6: Buf,
    snmp: Buf,
    status: Buf,
    sys: Buf,
    ifs: Vec<IfCounters>,
    prev_ifs: Vec<IfCounters>,
    agg: TcpAgg,
    snmp_now: [i64; SNMP.len()],
    snmp_prev: [i64; SNMP.len()],
    prev_at: Option<Instant>,
    page_size: u64,
}

fn us(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e6
}

fn unix_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64)
}

/// Bytes per page, for sockstat's `mem`: KernelPageSize in /proc/self/smaps,
/// 4 KiB when the file says nothing (x86_64's size; arm64 kernels may use 64 KiB).
fn page_size() -> u64 {
    let mut b = Buf::default();
    if b.read("/proc/self/smaps").is_ok() {
        let kb = proc::status_kb(&b.bytes, "KernelPageSize");
        if kb > 0 {
            return kb;
        }
    }
    4096
}

impl Sampler {
    pub fn new() -> Sampler {
        Sampler {
            dev: Buf::default(),
            tcp: Buf::default(),
            tcp6: Buf::default(),
            sock: Buf::default(),
            sock6: Buf::default(),
            snmp: Buf::default(),
            status: Buf::default(),
            sys: Buf::default(),
            ifs: Vec::new(),
            prev_ifs: Vec::new(),
            agg: TcpAgg::default(),
            snmp_now: [0; SNMP.len()],
            snmp_prev: [0; SNMP.len()],
            prev_at: None,
            page_size: page_size(),
        }
    }

    /// A short /sys/class/net/<if>/<attr> value. `speed` fails with EINVAL on
    /// virtual interfaces and reads -1 on a down link: both are None.
    fn sys_attr(&mut self, iface: &str, attr: &str) -> Option<String> {
        self.sys.read(&format!("/sys/class/net/{iface}/{attr}")).ok()?;
        let s = std::str::from_utf8(&self.sys.bytes).ok()?.trim();
        (!s.is_empty()).then(|| s.to_owned())
    }

    /// One sample, as JSON. Rates need a previous sample, so the first is all
    /// zeros.
    pub fn sample(&mut self, seq: u64, started: Instant) -> Value {
        let t0 = Instant::now();
        let now = Instant::now();
        let dt = self.prev_at.map_or(0.0, |p| now.duration_since(p).as_secs_f64());
        self.prev_at = Some(now);
        let mut costs: Vec<Cost> = Vec::with_capacity(6);

        // /proc/net/dev
        let t = Instant::now();
        let bytes = self.dev.read("/proc/net/dev").unwrap_or(0);
        let read_us = us(t);
        let t = Instant::now();
        std::mem::swap(&mut self.ifs, &mut self.prev_ifs);
        proc::parse_dev(&self.dev.bytes, &mut self.ifs);
        costs.push(Cost { file: "net/dev", read_us, parse_us: us(t), bytes });

        // /proc/net/tcp and tcp6 into one aggregate
        self.agg.clear();
        for (path, v6) in [("/proc/net/tcp", false), ("/proc/net/tcp6", true)] {
            let buf = if v6 { &mut self.tcp6 } else { &mut self.tcp };
            let t = Instant::now();
            let bytes = buf.read(path).unwrap_or(0);
            let read_us = us(t);
            let t = Instant::now();
            self.agg.add(&buf.bytes, v6);
            costs.push(Cost { file: if v6 { "net/tcp6" } else { "net/tcp" }, read_us, parse_us: us(t), bytes });
        }
        self.agg.finish();

        // sockstat, sockstat6
        let mut sockstat = Map::new();
        for (path, file) in [("/proc/net/sockstat", "net/sockstat"), ("/proc/net/sockstat6", "net/sockstat6")] {
            let buf = if file.ends_with('6') { &mut self.sock6 } else { &mut self.sock };
            let t = Instant::now();
            let bytes = buf.read(path).unwrap_or(0);
            let read_us = us(t);
            let t = Instant::now();
            let mut kv = Vec::new();
            proc::parse_sockstat(&buf.bytes, &mut kv);
            let parse_us = us(t);
            for (p, k, v) in kv {
                let e = sockstat.entry(p.to_owned()).or_insert_with(|| Value::Object(Map::new()));
                // `mem` is in pages; the page wants bytes.
                let v = if k == "mem" { v * self.page_size } else { v };
                e[if k == "mem" { "mem_bytes" } else { k }] = json!(v);
            }
            costs.push(Cost { file, read_us, parse_us, bytes });
        }

        // /proc/net/snmp
        let t = Instant::now();
        let bytes = self.snmp.read("/proc/net/snmp").unwrap_or(0);
        let read_us = us(t);
        let t = Instant::now();
        self.snmp_prev = self.snmp_now;
        proc::parse_snmp(&self.snmp.bytes, &SNMP, &mut self.snmp_now);
        costs.push(Cost { file: "net/snmp", read_us, parse_us: us(t), bytes });

        let rate = |now: u64, prev: u64| if dt > 0.0 { now.saturating_sub(prev) as f64 / dt } else { 0.0 };

        // Interfaces: rates against the same name last second. A counter that
        // went backwards (interface re-created) reads as 0, not as 2^64.
        let mut rows: Vec<(f64, Value)> = Vec::with_capacity(self.ifs.len());
        for i in 0..self.ifs.len() {
            let c = self.ifs[i].clone();
            let p = self.prev_ifs.get(i).filter(|p| p.name == c.name).or_else(|| self.prev_ifs.iter().find(|p| p.name == c.name)).cloned();
            let p = p.unwrap_or_else(|| c.clone());
            let (rx, tx) = (rate(c.rx_bytes, p.rx_bytes), rate(c.tx_bytes, p.tx_bytes));
            let oper = self.sys_attr(&c.name, "operstate");
            let mtu = self.sys_attr(&c.name, "mtu").and_then(|s| s.parse::<u64>().ok());
            let speed = self.sys_attr(&c.name, "speed").and_then(|s| s.parse::<i64>().ok()).filter(|&s| s > 0);
            let busy = rx + tx + (c.rx_bytes + c.tx_bytes) as f64 * 1e-9;
            rows.push((busy, json!({
                "name": c.name, "oper": oper, "mtu": mtu, "speed_mbps": speed,
                "rx_Bps": rx, "tx_Bps": tx,
                "rx_pps": rate(c.rx_packets, p.rx_packets), "tx_pps": rate(c.tx_packets, p.tx_packets),
                "err_ps": rate(c.rx_errs + c.tx_errs, p.rx_errs + p.tx_errs),
                "drop_ps": rate(c.rx_drop + c.tx_drop, p.rx_drop + p.tx_drop),
                "rx_bytes": c.rx_bytes, "tx_bytes": c.tx_bytes,
                "rx_errs": c.rx_errs, "tx_errs": c.tx_errs, "rx_drop": c.rx_drop, "tx_drop": c.tx_drop,
            })));
        }
        let n_ifaces = rows.len();
        rows.sort_by(|a, b| b.0.total_cmp(&a.0));
        let ifaces: Vec<Value> = rows.into_iter().take(MAX_IFACES).map(|r| r.1).collect();

        // TCP: states, listeners with their inbound counts, peers by class.
        let a = &self.agg;
        let mut states = Map::new();
        for (i, name) in TCP_STATES.iter().enumerate().skip(1) {
            states.insert((*name).to_owned(), json!(a.states[i]));
        }
        let listeners: Vec<Value> = a.listeners.iter().take(MAX_LISTENERS).map(|l| json!({
            "port": l.port, "family": if l.v6 { "v6" } else { "v4" }, "bind": l.bind.name(),
            "inbound": a.local_ports.get(&l.port).copied().unwrap_or(0),
        })).collect();
        let listen_ports: std::collections::HashSet<u16> = a.listeners.iter().map(|l| l.port).collect();
        let inbound: u64 = listen_ports.iter().filter_map(|p| a.local_ports.get(p)).sum();
        let mut remote: Vec<(u16, u64)> = a.remote_ports.iter().map(|(&p, &n)| (p, n)).collect();
        // Remote ports above 32768 are the far end's ephemeral ports (inbound
        // connections): they say nothing about a service, so they are pooled.
        let ephemeral: u64 = remote.iter().filter(|r| r.0 >= 32768).map(|r| r.1).sum();
        remote.retain(|r| r.0 < 32768);
        remote.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
        remote.truncate(12);
        let tcp = json!({
            "total": a.lines, "v4": a.v4, "v6": a.v6, "states": states,
            "listeners": listeners, "listeners_total": a.listeners.len(),
            "established_inbound": inbound,
            "established_outbound": a.states[1].saturating_sub(inbound),
            "remote_class": { "loopback": a.remote_class[1], "private": a.remote_class[2], "public": a.remote_class[3] },
            "distinct_peers": a.peers.len(),
            "remote_ports": remote.iter().map(|r| json!([r.0, r.1])).collect::<Vec<_>>(),
            "remote_ephemeral": ephemeral,
            "tx_queue_bytes": a.tx_queue, "rx_queue_bytes": a.rx_queue, "retransmitting": a.retransmitting,
        });

        let names = ["active_opens", "passive_opens", "attempt_fails", "estab_resets", "in_segs", "out_segs",
                     "retrans_segs", "in_errs", "out_rsts", "udp_in", "udp_out", "udp_in_errors"];
        let mut snmp = Map::new();
        for i in 0..SNMP_RATES {
            snmp.insert(names[i].to_owned(), json!(rate(self.snmp_now[i].max(0) as u64, self.snmp_prev[i].max(0) as u64)));
        }
        let out = rate(self.snmp_now[5] as u64, self.snmp_prev[5] as u64);
        snmp.insert("retrans_pct".into(), json!(if out > 0.0 { 100.0 * rate(self.snmp_now[6] as u64, self.snmp_prev[6] as u64) / out } else { 0.0 }));
        snmp.insert("curr_estab".into(), json!(self.snmp_now[12]));

        let t = Instant::now();
        let _ = self.status.read("/proc/self/status");
        let rss = proc::status_kb(&self.status.bytes, "VmRSS");
        let hwm = proc::status_kb(&self.status.bytes, "VmHWM");
        let threads = proc::status_kb(&self.status.bytes, "Threads") / 1024;
        let self_us = us(t);

        let read: f64 = costs.iter().map(|c| c.read_us).sum();
        let parse: f64 = costs.iter().map(|c| c.parse_us).sum();
        let bytes: usize = costs.iter().map(|c| c.bytes).sum();
        json!({
            "seq": seq, "t_ms": unix_ms(), "dt_s": dt, "uptime_s": started.elapsed().as_secs(),
            "ifaces": ifaces, "ifaces_total": n_ifaces,
            "tcp": tcp, "snmp": snmp, "sockstat": sockstat,
            "cost": {
                "read_us": read, "parse_us": parse, "bytes": bytes, "lines": self.agg.lines,
                "parse_MBps": if parse > 0.0 { bytes as f64 / parse } else { 0.0 },
                "self_us": self_us, "sample_us": us(t0),
                "files": costs.iter().map(|c| json!({ "file": c.file, "read_us": c.read_us, "parse_us": c.parse_us, "bytes": c.bytes })).collect::<Vec<_>>(),
            },
            "self": { "rss_bytes": rss, "rss_peak_bytes": hwm, "threads": threads },
        })
    }
}

/// The sampling loop, aligned to whole seconds of the monotonic clock so a
/// slow tick does not drift every later one.
pub fn run(shared: Arc<Shared>, started: Instant) {
    let mut s = Sampler::new();
    let mut seq = 0u64;
    let mut next = Instant::now();
    loop {
        seq += 1;
        let text = Arc::new(s.sample(seq, started).to_string());
        {
            let mut f = shared.feed.lock().unwrap_or_else(|e| e.into_inner());
            if f.history.len() == HISTORY {
                f.history.pop_front();
            }
            f.history.push_back(text);
            f.seq = seq;
        }
        shared.tick.notify_all();
        next += Duration::from_secs(1);
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
}
