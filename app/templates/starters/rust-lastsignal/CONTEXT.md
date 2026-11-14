# Last Signal · __HOST__
A Rust app, started from Cove's template: a survival strategy game. The machines
woke up. Survivors hold a Haven on the west edge of a ruined city; in the east
two AI Cores grow the Grid, a lattice that makes every machine on it faster and
self-repairing. By day you salvage scrap and take in survivors; at nightfall
the Cores send a wave, larger each night. Burn out both Cores to win; lose the
Haven and it is over. A finished game can go on the site's score board.

How it plays, and what is its own:
- Two resources. Scrap, salvaged by Scavengers from wrecks - and every machine
  you destroy leaves one. Survivors, who arrive at the Haven by day while there
  is housing (Shelters add it); every human unit takes one.
- Day and night. Day 95 s, night 50 s. At night the map is dark except what
  your buildings, units and Floodlights light; machines show as eyes.
- The Grid spreads from the Cores a tile at a time (faster at night). Nothing
  can be built on it. A Jammer stops it within 4.5 tiles and erases it there.
- Hackers turn a machine to your side (3 s, a Sentry 4 s, a Titan 7 s); EMP
  troopers stun them; machines in a Floodlight's light take +35% damage.
- The nights get harder: night 1's machines move at 70% speed and hit at 70%;
  each night adds a tenth, up to 140% (`night_power`). And each night power
  cells glow out in the dark - worth more the further east - for whoever
  goes out to grab them; at dawn the rest are gone.
- Pets (from Haven, scrap only, no survivor, six at most): the Dog bites and,
  at night, runs off to fetch power cells; the Cat sees far in the dark and
  purrs the wounded near it back to health.
- Units: Scavenger, Rifleman, EMP trooper, Hacker (Armory), Technical
  (Garage). Machines: Crawler, Sentry, Titan, and the Reclaimer, which eats
  wrecks before you can salvage them.
- Controls: click or drag to select; right-click (or, with units selected, a
  click or tap) to move, attack, salvage or hack; shift+right-click or Fight
  move to fight on the way. 1-7 picks a building to place, Space pauses.

Sound: every sound is made in the browser with Web Audio (web/main.js `sound`),
no audio files - gunfire, the machines' beams, EMP pulses, hack chirps,
explosions, a wind bed at night and short stings for nightfall, dawn, a Core
down, a machine turned and the Haven under fire - panned by where it happens
and quieter further from the Haven. On/off and volume in the panel, M toggles.

Look: a 2D map that reads as 2.5D - ruined blocks with height, south faces,
lit windows and cast shadows (light from the upper left); buildings and units
each with their own silhouette and a ground shadow; water that moves; shots,
blasts and pickups in layers (flash, sparks, smoke, debris, scorch marks
that fade over two minutes); a night that deepens to the end, lit by
coloured light - lamps warm, power cells cyan, the Grid and machine eyes
red, a cat's eyes green. Lights are pre-drawn sprites scaled per light, so a
busy night stays cheap.

Screens: one page for a phone, a laptop and a TV. Sizes are rem and the root
size follows the viewport; wide screens put the map at full height beside the
panel, narrow ones stack. The canvas draws at the shown size times the
screen's density (up to 3x), so a 4K TV gets a sharp map.

Change first: src/game.rs - every number is in `UKind::spec` and
`BKind::spec`, the night's wave in `light` and `machines`, the Grid in
`grow_grid` - then `./build-web.sh` and reload; the server picks up the new
module without a restart. `cargo test` runs the rules: paths, salvage, the
night wave, hacking, EMP and Floodlights, the Grid and Jammers, building and
training, winning and losing, the frame layout. `cargo test -- --ignored
--nocapture hands_off` plays three games with nobody at the controls and
prints how long they last - a quick balance check.

What it shows off: the same Rust compiled twice, to WebAssembly for the page
and natively for `cargo test`, with nothing between them but a dozen
`extern "C"` functions, one array of numbers and one byte per map tile. No
wasm-bindgen, no wasm-pack, no npm: the module imports nothing.

## Files
- src/game.rs - the game: the seeded city (xorshift, so a seed is a city),
  8-way A*, units, buildings, the Grid, day and night, the machines' waves,
  combat, salvage, hacking; `frame` (24 header floats, then 11 per unit, 10
  per building, 3 per wreck, 7 per effect, 2 per unit in training) and
  `tile_bytes`; the tests
- src/server.rs - tiny_http on 127.0.0.1: GET / and /static/<file> (web/, and
  game.wasm from dist/web/, read from disk each request); GET /health;
  GET /api/scores; POST /api/scores (`{"name","score","nights","seconds"}`,
  refused past 100 000) → rank, kept in data/scores.json; GET /api/env
- src/main.rs - `app serve <port>`
- web/index.html, web/style.css, web/main.js - the page: loads game.wasm,
  calls game_update each frame, reads the world out of the module's memory,
  draws it and the night, plays its sounds, and turns clicks into orders
- build-web.sh - src/game.rs for wasm32-unknown-unknown → dist/web/game.wasm;
  build.sh runs it after the server
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - the app contract
  (APP-CONTRACT.md); crates from the stack's vendored Rust set, offline

## Auth
As published at route.ndexr.io.

## Open
- (none logged)
