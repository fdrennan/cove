# Network Observatory · __HOST__
Live network metrics read straight from the Linux kernel's own tables by a
small Rust binary, no external tools: per-interface throughput, packets,
errors and drops from /proc/net/dev (link state, MTU and speed from
/sys/class/net); a histogram of TCP connection states, the listening ports
and where established peers sit (loopback, private, public) from
/proc/net/tcp and tcp6; socket and buffer totals from /proc/net/sockstat;
segment, retransmission, open and reset rates from /proc/net/snmp. The
server samples once a second and streams each sample to the page over
Server-Sent Events (polling when a proxy buffers the stream); the page draws
received above and sent below a three-minute axis, full screen. Latency
probes are added from the page: the server times a TCP handshake to
host:port every few seconds and reports p50, p95, RFC 3550 jitter, loss and
a log-axis histogram. A download button pulls zeros from the server to watch
the counters move. Remote addresses never leave the server: the page sees
counts, classes and ports. Rust is here for cheap, exact parsing: every
table is walked as bytes in a reused buffer, with no per-line String or
Vec, and each sample reports its own parse time beside the kernel's read
time and the process's resident memory (about 8 MB, a 1.3 MB binary).

## Files
- Cargo.toml - one binary, `app`; tiny_http and serde_json pinned to the stack's Rust set, all else std
- build.rs - records rustc, target, profile, the stack modules loaded at build, and each dependency's version from Cargo.lock, for /api/env
- src/main.rs - `app serve <port>`
- src/proc.rs - the /proc parsers, allocation-free; `cargo test` checks them on fixed tables
- src/sampler.rs - the 1 Hz loop: counters to rates, TCP aggregates, cost and memory per sample
- src/probe.rs - TCP connect probes: timing, quantiles, jitter, loss, histogram
- src/server.rs - HTTP on 127.0.0.1: the routes, the event stream, /api/env
- src/page.html - the page, compiled into the binary: canvas charts, glass panels, the Environment panel
- run.sh - serve target/release/app on 127.0.0.1:$APP_PORT
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - `cargo build --release`; start in the
  background and wait for /health (pid and log in .run/); stop; cargo 1.95+
  from PATH or the stack's Rust/1.96.0, crates from the stack's vendored set

## Endpoints
- GET  /health
- GET  /api/env -> rustc, target, crates and versions, stack modules at build and at run, start time, pid, host CPUs, RSS
- GET  /api/snapshot?n=180 -> the last n one-second samples (three minutes kept) and the probes
- GET  /api/stream -> Server-Sent Events: `sample` every second, `probes` beside it
- GET  /api/probes; POST /api/probes?target=host:port&every=2; POST /api/probes/delete?id=1
- GET  /api/blob?mb=64 -> that many MiB of zeros, for load

A sample holds `ifaces` (rx/tx bytes and packets per second, error and drop
rates, cumulative counters), `tcp` (states, listeners with their inbound
counts, peers by class, top remote service ports, queued bytes), `snmp`
(rates per second), `sockstat` (`mem` converted from pages to bytes), `cost`
(read and parse microseconds per file) and `self` (RSS, threads). The page
opens with one probe, this server's own port over loopback: the floor every
other probe is measured against.

## Run
`./build.sh` compiles; `./run.sh` then runs `target/release/app serve
$APP_PORT`, the port app.env gives this project (`PORT=<n> ./run.sh` for one
run); `./up.sh` and `./down.sh` start and stop it in the background. Crates
come offline from the stack's vendor directory,
/opt/ndexr/site-library/<triple>/Rust/1.96.0/rust/vendor (env.sh).

## Built on
- The kernel's tables: Documentation/networking/proc_net_tcp.rst (the
  /proc/net/tcp columns), include/net/tcp_states.h (state numbers),
  Documentation/networking/statistics.rst (/proc/net/dev counters).
- TCP states: RFC 9293 (2022), section 3.3.2. The kernel's CurrEstab, which
  `ss -s` prints as "estab", counts ESTABLISHED and CLOSE_WAIT together, as
  RFC 1213's tcpCurrEstab defines it; the page shows both sides of that sum.
- Jitter: the interarrival jitter estimator of RFC 3550 (RTP), section 6.4.1,
  J += (|D| - J)/16, applied to consecutive connect times.
- Address classes: RFC 1918 (private IPv4), RFC 6598 (shared 100.64/10),
  RFC 3927 (link-local), RFC 4193 (IPv6 unique local), RFC 4291 (IPv6
  loopback, link-local, v4-mapped).
- Handshake timing as a latency measure: the connect() return marks the
  SYN-ACK, one round trip; see S. Savage, "Sting: a TCP-based Network
  Measurement Tool", USENIX USITS (1999).

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it. Probes let a signed-in visitor make the server open TCP
connections to a host:port of their choice (no payload, at most 8 probes,
once a second at most each).

## Open
- What the page sees is the network namespace the server runs in: a
  container sees its own interfaces and sockets, not the host's.
- /proc/net/tcp costs the kernel tens of milliseconds on a host with
  thousands of sockets (measured 2026-10-26, median of 71 samples at load
  average 9: 6 784 sockets, 46 ms kernel read, 3.1 ms parse); netlink
  sock_diag (what `ss` uses) would cut the read.
- Probes and history are in memory and shared by every page; a restart
  clears them.
