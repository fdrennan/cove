# Jellyfish Drift · __HOST__
A Rust app, started from Cove's template: an underwater arcade game. Pulse the
jellyfish up through gaps in the coral and catch the glowing pearl in each gap;
the floor and the coral cost a heart, three end the run, and the reef speeds up
and narrows as the score climbs. Space, ↑ or a tap pulses. A finished run can go
on the site's score board, shared by every player of this site.
Change first: src/game.rs - the swell and drag in `update`, the gap in the
spawn, what a pearl is worth - then `./build-web.sh` and reload; the server
picks up the new module without a restart.

What it shows off: the same Rust compiled twice, to WebAssembly for the page
and natively for `cargo test`, with nothing between them but five
`extern "C"` functions and one array of numbers. No wasm-bindgen, no
wasm-pack, no npm: the stack's Rust has the wasm32-unknown-unknown target, and
the module imports nothing.

## Files
- src/game.rs - the game: physics, the seeded reef (xorshift, so a seed is a
  reef), scoring, the frame layout (`frame`: 11 header floats, then x, center,
  gap per gate, then x, y per pearl) and its tests
- src/server.rs - tiny_http on 127.0.0.1: GET / and /static/<file> (web/, and
  game.wasm from dist/web/, read from disk each request); GET /health;
  GET /api/scores; POST /api/scores (`{"name","score","pearls","seconds"}`,
  refused past 100 000) → rank, kept in data/scores.json; GET /api/env
- src/main.rs - `app serve <port>`
- web/index.html, web/style.css, web/main.js - the page: loads game.wasm,
  calls game_update each frame, reads the world out of the module's memory,
  draws it
- build-web.sh - src/game.rs for wasm32-unknown-unknown → dist/web/game.wasm;
  build.sh runs it after the server
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - the app contract
  (APP-CONTRACT.md); crates from the stack's vendored Rust set, offline

## Logging
Logging: src/log.rs (`log::info!`), one format across languages; LOG_LEVEL=DEBUG for more.

## Auth
As published at route.ndexr.io.

## Open
- (none logged)
