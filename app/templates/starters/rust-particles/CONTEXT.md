# Particle Universe · __HOST__
An N-body gravity simulation in a plane, computed in Rust and painted full screen. Every
body pulls on every other (direct O(n²) summation, Plummer-softened), the
outer loop split across all cores with rayon, integrated with kick-drift-kick
leapfrog. Three universes: two disc galaxies colliding, one disc around a
central mass, and the cold collapse of a uniform disc at rest. Sliders set G,
softening, time step, steps per frame and body count (500 to 20 000); a click
drops a star, a drag pans, a wheel or pinch zooms. The header reports bodies
x steps per second and pair interactions per second; the sparkline traces
total-energy drift ΔE/E₀, which under a symplectic integrator stays bounded
instead of growing. Rust is here for speed with safe parallelism: the force
loop is plain iterator code over contiguous f64 slices that the compiler
vectorises, and the borrow checker proves the parallel writes are disjoint.

## Files
- Cargo.toml - one binary, `app`; tiny_http, serde_json and rayon pinned to the stack's Rust set
- src/main.rs - `app serve <port>`
- src/sim.rs - the physics: presets, softened forces, leapfrog, energy; `cargo test` checks a Kepler orbit
- src/server.rs - HTTP on 127.0.0.1: the routes, the Float32 frame format
- src/page.html - the page, compiled into the binary: canvas, glass panel, sparkline
- run.sh - serve target/release/app on 127.0.0.1:$APP_PORT
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - `cargo build --release`; start in the
  background and wait for /health (pid and log in .run/); stop; cargo 1.95+
  from PATH or the stack's Rust/1.96.0, crates from the stack's vendored set

## Endpoints
- GET  /health
- POST /reset?preset=galaxies|disc|collapse|kepler&n=4000&seed=7&soft=0.02
- GET  /frame?steps=6&g=1&soft=0.02&dt=0.002 -> advances, then Float32 LE: 16
  header floats (n, stars, t, steps, E, E0, K, U, ms/step, body·steps/s,
  pairs/s, threads, epoch, steps this frame, dt, 0), then x, y per body, stars first
- GET  /state (same query) -> the same header as JSON, with drift and 2K/|U|
- POST /drop?x=0.5&y=0.2&mass=0.5 -> a star at rest there

The universe advances only when a frame is asked for, so an idle app uses no
CPU. All pages share one universe.

## Run
`./build.sh` compiles; `./run.sh` then runs `target/release/app serve
$APP_PORT`, the port app.env gives this project (`PORT=<n> ./run.sh` for one
run); `./up.sh` and `./down.sh` start and stop it in the background. Crates
come offline from the stack's vendor directory,
/opt/ndexr/site-library/<triple>/Rust/1.96.0/rust/vendor (env.sh).

## Built on
- Leapfrog / velocity Verlet: L. Verlet, Phys. Rev. 159, 98 (1967); its
  symplectic, bounded-energy-error property: E. Hairer, C. Lubich, G. Wanner,
  "Geometric numerical integration illustrated by the Störmer-Verlet method",
  Acta Numerica 12, 399 (2003).
- Plummer softening: S. J. Aarseth, Gravitational N-Body Simulations (CUP 2003);
  the potential used for energy is the one the softened force derives from.
- Tidal tails from a disc-galaxy encounter: A. Toomre and J. Toomre, ApJ 178,
  623 (1972). The second disc counter-rotates, so its tails are weaker.
- Cold collapse and violent relaxation: D. Lynden-Bell, MNRAS 136, 101 (1967);
  /state reports 2K/|U|, which settles near 1 once the system is virialised.
- Barnes-Hut (J. Barnes and P. Hut, Nature 324, 446 (1986)) is the next step
  past 20 000 bodies; direct summation is exact and fast enough below that.

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- One shared universe: two pages open at once both advance it.
- Fixed time step: in the collision the two centres pass 0.19 apart at t≈2.4
  and ΔE/E₀ jumps from 2e-6 to 7e-4, then stays bounded (measured 2026-10-26,
  dt 0.002). Individual or adaptive time steps would remove the jump.
