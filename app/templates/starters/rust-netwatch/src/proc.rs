//! Parsers for the kernel's network tables: /proc/net/dev, /proc/net/tcp and
//! tcp6, /proc/net/sockstat and sockstat6, /proc/net/snmp.
//!
//! Every parser walks a byte slice the caller read into a reused buffer and
//! writes into reused containers: no line is copied into a String and no
//! field is split into a Vec, so a second of sampling allocates nothing once
//! the tables have grown to size. /proc/net/tcp on a busy host is thousands of
//! lines; the parse cost is reported beside the kernel's own read cost.

use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, Read};

/// One reusable read buffer per file: `read` clears it and fills it again,
/// so its capacity settles at the file's largest size.
#[derive(Default)]
pub struct Buf {
    pub bytes: Vec<u8>,
}

impl Buf {
    pub fn read(&mut self, path: &str) -> io::Result<usize> {
        self.bytes.clear();
        File::open(path)?.read_to_end(&mut self.bytes)
    }
}

/// Whitespace-separated fields of one line, borrowed from the buffer.
fn fields(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.split(|b| b.is_ascii_whitespace()).filter(|f| !f.is_empty())
}

fn lines(buf: &[u8]) -> impl Iterator<Item = &[u8]> {
    buf.split(|&b| b == b'\n').filter(|l| !l.is_empty())
}

fn dec(f: &[u8]) -> Option<u64> {
    if f.is_empty() || f.len() > 20 {
        return None;
    }
    f.iter().try_fold(0u64, |acc, &b| match b {
        b'0'..=b'9' => acc.checked_mul(10)?.checked_add(u64::from(b - b'0')),
        _ => None,
    })
}

fn hex(f: &[u8]) -> Option<u64> {
    if f.is_empty() || f.len() > 16 {
        return None;
    }
    f.iter().try_fold(0u64, |acc, &b| {
        let d = match b {
            b'0'..=b'9' => b - b'0',
            b'a'..=b'f' => b - b'a' + 10,
            b'A'..=b'F' => b - b'A' + 10,
            _ => return None,
        };
        Some(acc << 4 | u64::from(d))
    })
}

// ---------------------------------------------------------------- /proc/net/dev

/// Cumulative counters of one interface since it came up.
#[derive(Clone, Default, Debug, PartialEq)]
pub struct IfCounters {
    pub name: String,
    pub rx_bytes: u64,
    pub rx_packets: u64,
    pub rx_errs: u64,
    pub rx_drop: u64,
    pub tx_bytes: u64,
    pub tx_packets: u64,
    pub tx_errs: u64,
    pub tx_drop: u64,
}

/// /proc/net/dev: two header lines, then `name: 8 receive fields 8 transmit
/// fields`. Older kernels print `eth0:123` with no space, so the name ends at
/// the colon, not at whitespace. Entries in `out` are overwritten in place.
pub fn parse_dev(buf: &[u8], out: &mut Vec<IfCounters>) -> usize {
    let mut n = 0;
    for line in lines(buf).skip(2) {
        let Some(colon) = line.iter().position(|&b| b == b':') else { continue };
        let name = line[..colon].trim_ascii();
        let mut v = [0u64; 16];
        let mut k = 0;
        for f in fields(&line[colon + 1..]).take(16) {
            v[k] = dec(f).unwrap_or(0);
            k += 1;
        }
        if k < 16 {
            continue;
        }
        if out.len() <= n {
            out.push(IfCounters::default());
        }
        let e = &mut out[n];
        e.name.clear();
        e.name.push_str(std::str::from_utf8(name).unwrap_or("?"));
        (e.rx_bytes, e.rx_packets, e.rx_errs, e.rx_drop) = (v[0], v[1], v[2], v[3]);
        (e.tx_bytes, e.tx_packets, e.tx_errs, e.tx_drop) = (v[8], v[9], v[10], v[11]);
        n += 1;
    }
    out.truncate(n);
    n
}

// ------------------------------------------------------------ /proc/net/tcp(6)

/// Names of the kernel's TCP states, indexed by the `st` column
/// (include/net/tcp_states.h). 0 is unused.
pub const TCP_STATES: [&str; 13] = [
    "", "ESTABLISHED", "SYN_SENT", "SYN_RECV", "FIN_WAIT1", "FIN_WAIT2", "TIME_WAIT", "CLOSE",
    "CLOSE_WAIT", "LAST_ACK", "LISTEN", "CLOSING", "NEW_SYN_RECV",
];
const ST_ESTABLISHED: usize = 1;
const ST_LISTEN: usize = 10;

/// Where an address points. The page sees these classes and counts, never an
/// address: a remote peer's IP is the visitor's business, not the page's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Class {
    Any,
    Loopback,
    Private,
    Public,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::Any => "any",
            Class::Loopback => "loopback",
            Class::Private => "private",
            Class::Public => "public",
        }
    }
}

fn class_v4(a: [u8; 4]) -> Class {
    match a {
        [0, 0, 0, 0] => Class::Any,
        [127, ..] => Class::Loopback,
        [10, ..] | [192, 168, ..] | [169, 254, ..] => Class::Private,
        [172, b, ..] if (16..32).contains(&b) => Class::Private,
        [100, b, ..] if (64..128).contains(&b) => Class::Private, // carrier-grade NAT, RFC 6598
        _ => Class::Public,
    }
}

fn class_v6(a: [u8; 16]) -> Class {
    if a == [0; 16] {
        return Class::Any;
    }
    if a[..15] == [0; 15] && a[15] == 1 {
        return Class::Loopback;
    }
    // ::ffff:a.b.c.d - an IPv4 peer on a dual-stack socket.
    if a[..10] == [0; 10] && a[10] == 0xff && a[11] == 0xff {
        return class_v4([a[12], a[13], a[14], a[15]]);
    }
    if a[0] & 0xfe == 0xfc || (a[0] == 0xfe && a[1] & 0xc0 == 0x80) {
        return Class::Private; // ULA fc00::/7, link-local fe80::/10
    }
    Class::Public
}

/// `0100007F:1F90` -> (address bytes, port). The kernel prints each 32-bit
/// word of the address in host byte order, so `to_ne_bytes` restores network
/// order on any machine; the port is printed big-endian.
fn addr_port(f: &[u8]) -> Option<([u8; 16], usize, u16)> {
    let colon = f.iter().position(|&b| b == b':')?;
    let (a, p) = (&f[..colon], &f[colon + 1..]);
    let port = hex(p)? as u16;
    let mut out = [0u8; 16];
    match a.len() {
        8 => {
            out[..4].copy_from_slice(&(hex(a)? as u32).to_ne_bytes());
            Some((out, 4, port))
        }
        32 => {
            for w in 0..4 {
                out[4 * w..4 * w + 4].copy_from_slice(&(hex(&a[8 * w..8 * w + 8])? as u32).to_ne_bytes());
            }
            Some((out, 16, port))
        }
        _ => None,
    }
}

fn classify(a: &[u8; 16], len: usize) -> Class {
    if len == 4 {
        class_v4([a[0], a[1], a[2], a[3]])
    } else {
        class_v6(*a)
    }
}

/// One listening socket, as the page shows it: port, family, bind class.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Listener {
    pub port: u16,
    pub v6: bool,
    pub bind: Class,
}

/// Everything the TCP tables add up to. Containers are cleared, not dropped,
/// between samples.
#[derive(Default)]
pub struct TcpAgg {
    pub states: [u64; 13],
    pub v4: u64,
    pub v6: u64,
    pub listeners: Vec<Listener>,
    /// Established connections by local port: the inbound count of a listener.
    pub local_ports: HashMap<u16, u64>,
    /// Established connections by remote port: what this host talks to.
    pub remote_ports: HashMap<u16, u64>,
    /// Established connections by where the peer is.
    pub remote_class: [u64; 4],
    /// Distinct peers, hashed: counted, never stored as text or sent.
    pub peers: HashSet<u128>,
    pub tx_queue: u64,
    pub rx_queue: u64,
    pub retransmitting: u64,
    pub lines: u64,
}

impl TcpAgg {
    pub fn clear(&mut self) {
        self.states = [0; 13];
        (self.v4, self.v6, self.tx_queue, self.rx_queue, self.retransmitting, self.lines) = (0, 0, 0, 0, 0, 0);
        self.remote_class = [0; 4];
        self.listeners.clear();
        self.local_ports.clear();
        self.remote_ports.clear();
        self.peers.clear();
    }

    /// Adds one /proc/net/tcp or tcp6 table. Columns: sl, local, remote, st,
    /// tx_queue:rx_queue, tr:when, retrnsmt, uid, timeout, inode, ...
    pub fn add(&mut self, buf: &[u8], v6: bool) {
        for line in lines(buf).skip(1) {
            let mut f = fields(line).skip(1);
            let (Some(local), Some(remote), Some(st), Some(queues), _, Some(retr)) =
                (f.next(), f.next(), f.next(), f.next(), f.next(), f.next())
            else {
                continue;
            };
            let (Some((la, ll, lport)), Some((ra, rl, rport)), Some(st)) = (addr_port(local), addr_port(remote), hex(st))
            else {
                continue;
            };
            let st = st as usize;
            if st >= self.states.len() {
                continue;
            }
            self.lines += 1;
            self.states[st] += 1;
            if v6 { self.v6 += 1 } else { self.v4 += 1 }
            if let Some(c) = queues.iter().position(|&b| b == b':') {
                self.tx_queue += hex(&queues[..c]).unwrap_or(0);
                self.rx_queue += hex(&queues[c + 1..]).unwrap_or(0);
            }
            if hex(retr).unwrap_or(0) > 0 {
                self.retransmitting += 1;
            }
            match st {
                ST_LISTEN => self.listeners.push(Listener { port: lport, v6, bind: classify(&la, ll) }),
                ST_ESTABLISHED => {
                    *self.local_ports.entry(lport).or_default() += 1;
                    *self.remote_ports.entry(rport).or_default() += 1;
                    self.remote_class[classify(&ra, rl) as usize] += 1;
                    self.peers.insert(u128::from_be_bytes(ra) ^ (rl as u128) << 120);
                }
                _ => {}
            }
        }
    }

    /// Listeners sorted and deduplicated: the kernel lists one socket per
    /// SO_REUSEPORT worker, the page wants one row per service.
    pub fn finish(&mut self) {
        self.listeners.sort_unstable();
        self.listeners.dedup();
    }
}

// ------------------------------------------------------- sockstat, snmp, self

/// `TCP: inuse 6432 orphan 0 tw 259 alloc 7103 mem 1676` lines into
/// (protocol, key, value) triples.
pub fn parse_sockstat<'a>(buf: &'a [u8], out: &mut Vec<(&'a str, &'a str, u64)>) {
    for line in lines(buf) {
        let Some(colon) = line.iter().position(|&b| b == b':') else { continue };
        let proto = std::str::from_utf8(&line[..colon]).unwrap_or("?");
        let mut f = fields(&line[colon + 1..]);
        while let (Some(k), Some(v)) = (f.next(), f.next()) {
            if let (Ok(k), Some(v)) = (std::str::from_utf8(k), dec(v)) {
                out.push((proto, k, v));
            }
        }
    }
}

/// /proc/net/snmp prints each protocol as a header line of names and a line
/// of values: `Tcp: ActiveOpens ...` then `Tcp: 1 200 ...`. `want` lists the
/// (protocol, name) pairs to pick; values may be negative (MaxConn is -1).
pub fn parse_snmp(buf: &[u8], want: &[(&str, &str)], out: &mut [i64]) {
    let mut it = lines(buf);
    while let (Some(names), Some(values)) = (it.next(), it.next()) {
        let colon = names.iter().position(|&b| b == b':').unwrap_or(0);
        let proto = &names[..colon];
        for (n, v) in fields(&names[colon + 1..]).zip(fields(&values[colon + 1..])) {
            for (i, (wp, wn)) in want.iter().enumerate() {
                if wp.as_bytes() == proto && wn.as_bytes() == n {
                    let (neg, digits) = if v.first() == Some(&b'-') { (true, &v[1..]) } else { (false, v) };
                    let x = dec(digits).unwrap_or(0) as i64;
                    out[i] = if neg { -x } else { x };
                }
            }
        }
    }
}

/// `VmRSS:     2048 kB` from /proc/self/status, in bytes.
pub fn status_kb(buf: &[u8], key: &str) -> u64 {
    lines(buf)
        .find(|l| l.starts_with(key.as_bytes()) && l.get(key.len()) == Some(&b':'))
        .and_then(|l| fields(&l[key.len() + 1..]).next().and_then(dec))
        .map_or(0, |kb| kb * 1024)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DEV: &str = "Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 1000 10 0 0 0 0 0 0 1000 10 0 0 0 0 0 0
  eth0:987654321 1234 2 3 0 0 0 7 123456 999 4 5 0 0 0 0
";

    #[test]
    fn dev_counters() {
        let mut out = vec![];
        assert_eq!(parse_dev(DEV.as_bytes(), &mut out), 2);
        assert_eq!(out[1].name, "eth0");
        assert_eq!((out[1].rx_bytes, out[1].rx_packets, out[1].rx_errs, out[1].rx_drop), (987654321, 1234, 2, 3));
        assert_eq!((out[1].tx_bytes, out[1].tx_packets, out[1].tx_errs, out[1].tx_drop), (123456, 999, 4, 5));
        // Reuse shrinks to what the file holds.
        parse_dev(&DEV.as_bytes()[..DEV.find("  eth0").unwrap()], &mut out);
        assert_eq!(out.len(), 1);
    }

    // 127.0.0.1:8080 listening; 10.0.0.5:5432 <- 10.0.0.9:41000 established;
    // a TIME_WAIT to 8.8.8.8:443. Written as the kernel prints on little-endian.
    const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 1 1 0 100 0 0 10 0
   1: 0500000A:1538 0900000A:A028 01 00000010:00000020 00:00000000 00000002  1000        0 2 1 0 20 4 30 10 -1
   2: 0500000A:B000 08080808:01BB 06 00000000:00000000 03:00000ABC 00000000     0        0 0 3 0
";
    const TCP6: &str = "  sl  local_address                         remote_address                        st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 00000000000000000000000000000000:0016 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 3 1 0 100 0 0 10 0
   1: 0000000000000000FFFF00000100007F:1F90 0000000000000000FFFF00000100007F:C350 01 00000000:00000000 00:00000000 00000000  1000        0 4 1 0 20 4 30 10 -1
";

    #[test]
    fn tcp_states_and_classes() {
        let mut a = TcpAgg::default();
        a.add(TCP.as_bytes(), false);
        a.add(TCP6.as_bytes(), true);
        a.finish();
        if cfg!(target_endian = "little") {
            assert_eq!(a.listeners, vec![
                Listener { port: 22, v6: true, bind: Class::Any },
                Listener { port: 8080, v6: false, bind: Class::Loopback },
            ]);
            assert_eq!(a.remote_class, [0, 1, 1, 0]); // the v4-mapped loopback peer, the 10/8 peer
        }
        assert_eq!(a.states[1], 2);
        assert_eq!(a.states[6], 1);
        assert_eq!(a.states[10], 2);
        assert_eq!((a.v4, a.v6, a.lines), (3, 2, 5));
        assert_eq!(a.local_ports[&5432], 1);
        assert_eq!(a.remote_ports[&41000], 1);
        assert_eq!((a.tx_queue, a.rx_queue, a.retransmitting), (0x10, 0x20, 1));
        assert_eq!(a.peers.len(), 2);
    }

    #[test]
    fn classes() {
        assert_eq!(class_v4([172, 20, 1, 1]), Class::Private);
        assert_eq!(class_v4([172, 32, 1, 1]), Class::Public);
        assert_eq!(class_v4([100, 64, 0, 1]), Class::Private);
        let mut ula = [0u8; 16];
        ula[0] = 0xfd;
        assert_eq!(class_v6(ula), Class::Private);
        let mut g = [0u8; 16];
        g[0] = 0x26;
        assert_eq!(class_v6(g), Class::Public);
    }

    #[test]
    fn sockstat_and_snmp() {
        let s = b"sockets: used 11168\nTCP: inuse 6432 orphan 0 tw 259 alloc 7103 mem 1676\n";
        let mut out = vec![];
        parse_sockstat(s, &mut out);
        assert_eq!(out[0], ("sockets", "used", 11168));
        assert_eq!(out[3], ("TCP", "tw", 259));
        let snmp = b"Tcp: RtoAlgorithm MaxConn ActiveOpens\nTcp: 1 -1 6261244\nUdp: InDatagrams\nUdp: 42\n";
        let mut v = [0i64; 3];
        parse_snmp(snmp, &[("Tcp", "ActiveOpens"), ("Tcp", "MaxConn"), ("Udp", "InDatagrams")], &mut v);
        assert_eq!(v, [6261244, -1, 42]);
        assert_eq!(status_kb(b"Name:\tapp\nVmRSS:\t    2048 kB\n", "VmRSS"), 2048 * 1024);
    }
}
