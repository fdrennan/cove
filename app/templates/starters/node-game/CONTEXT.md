# __HOST__ · a high-performance browser game, TypeScript + PixiJS

Swarm: thousands of drones flock after your ship; take the orbs, and each
one pushes them back. It is a template for any fast 2D browser game: the
parts below are the architecture to keep, and the game is the example.

## How it fits together

| file | what it is | runs |
|---|---|---|
| `src/sim.ts` | the simulation: every rule of the game, one fixed step (1/60 s) at a time, nothing about drawing | in fixed steps |
| `src/loop.ts` | the frame loop: steps from an accumulator, draws between steps, caps catch-up | every frame |
| `src/render.ts` | PixiJS 8: reads the simulation, places sprites between its last two steps, draws | every frame |
| `src/input.ts` | keys and pointer, read once per step | per step |
| `src/hud.ts` | menus and the heads-up display, in React (htm, no JSX compile) | ~4 times a second |
| `src/main.ts` | puts them together; game modes; best score in localStorage | once |
| `server.js` | serves `static/` and `/health` (Node's standard library only) | - |

## The rules that keep it fast

- **Simulation apart from drawing.** `sim.step()` advances exactly `STEP`;
  the display can run at 60, 120 or 144 Hz and the game plays the same. The
  renderer draws between the last two steps (`alpha`), so motion is smooth.
- **No allocation in the loop.** The simulation's state is typed arrays (x,
  y, vx, vy, heading per drone) made once; the renderer keeps one sprite per
  drone and only writes numbers into them.
- **Neighbours through a grid.** Each step sorts drones into 32-unit cells
  (a counting sort, no allocation); a drone looks only at the nine cells
  around it, and at no more than 16 neighbours.
- **One draw for the swarm.** The drones are a PixiJS `ParticleContainer`:
  position and rotation are the only things sent to the GPU per frame.
- **React is not in the frame.** The HUD reads a small store the game writes
  about four times a second (`useSyncExternalStore`); a menu never redraws
  per frame.
- **Deterministic.** All randomness is the seeded `rng` (`rng.ts`): the same
  seed and inputs give the same game, step for step - what replays, tests
  and networked play are built on.
- **A slow frame cannot spiral.** At most 5 steps per frame, at most 0.25 s
  of time taken from one frame; a hidden tab pauses the game.

The HUD shows what to watch: frames per second, milliseconds per frame in
the simulation and in drawing, and steps per frame.

## Measured (2026-11-05, Node 22.22.2, this host, one core)

| drones | ms per step | budget at 60 Hz |
|---|---|---|
| 3,000 | 0.8 - 1.8 | 16.7 |
| 30,000 | 6 - 15 | 16.7 |

(The range is the host's load.) Phones are several times slower.

## When to move the simulation to Rust

When the HUD's "sim" time eats a frame on the devices that matter, move
`sim.ts` to Rust compiled to WebAssembly (the stack has Rust; see the
`rust-blank-wasm` starter). Keep the same shape: the Rust side owns the
typed arrays in WebAssembly memory and the renderer reads `Float32Array`
views of them - so `render.ts`, `hud.ts` and the loop do not change.
Rendering stays in PixiJS either way.

## Logging
log.js (`const log = require('./log')`) in server.js only, never the browser code in src/; one format across languages; LOG_LEVEL=DEBUG for more.

## Building

`.cove/build.sh` turns `src/*.ts` into `static/js/*.js` with the stack's own
Node (22.22: `node:module` `stripTypeScriptTypes`), then checks every script
and the vendored libraries' sha256. `.cove/node/run.sh` strips again on every
save in `src/`, and the open page reloads itself. No npm, no bundler, no
network.

- **Types are stripped, not checked:** there is no TypeScript compiler in the
  ndexr stack. Write TypeScript that erases (no `enum`, `namespace` or
  constructor parameter properties) - the build stops with the line if not.
- `src/globals.d.ts` names the libraries the page loads as globals (`PIXI`,
  `React`, `ReactDOM`, `htm`).
- Libraries: `static/vendor/README.md` (PixiJS 8.22.0, React 18.3.1, htm 3.1.1;
  sources, licences and hashes).
