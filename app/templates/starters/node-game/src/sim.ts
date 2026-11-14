// The simulation: everything that happens in the game, one fixed step at a
// time, and nothing about drawing. Its state is typed arrays (structure of
// arrays: x, y, vx, vy, ... per drone), so a step allocates nothing, walks
// memory in order, and is the shape a Rust / WebAssembly simulation would
// share with the renderer unchanged (CONTEXT.md). Same seed, same inputs:
// the same game, step for step.
import { rng } from './rng.js';
import type { Input } from './input.js';

/** Seconds per step: the simulation always moves by exactly this much. */
export const STEP = 1 / 60;
/** The world's size in world units; the renderer scales it to the screen. */
export const W = 1600;
export const H = 900;

const CELL = 32;
const COLS = Math.ceil(W / CELL);
const ROWS = Math.ceil(H / CELL);
const SHIP_ACCEL = 2600;
const SHIP_MAX = 420;
const SHIP_DRAG = 3.2;
// The swarm's top speed eases up from DRONE_START to DRONE_MAX over the
// first RAMP seconds, so a new game is not lost before the controls are found.
const DRONE_START = 80;
const DRONE_MAX = 150;
const RAMP = 20;
const DRONE_SEEK = 230;
const SEPARATE = 10;
const SEPARATE_PUSH = 900;
const NEIGHBOURS = 16;
const HIT = 10;
const SAFE = 360;
const ORB = 22;
const PULSE = 260;
const PULSE_PUSH = 4200;

export interface Ship {
  x: number;
  y: number;
  px: number;
  py: number;
  vx: number;
  vy: number;
  angle: number;
  alive: boolean;
}

export interface Sim {
  readonly count: number;
  // Now, and one step before: the renderer draws between the two.
  readonly x: Float32Array;
  readonly y: Float32Array;
  readonly px: Float32Array;
  readonly py: Float32Array;
  readonly vx: Float32Array;
  readonly vy: Float32Array;
  readonly heading: Float32Array;
  readonly ship: Ship;
  readonly orb: { x: number; y: number };
  score: number;
  time: number;
  steps: number;
  /** 1 the step an orb is taken, fading to 0: it pushes the swarm away. */
  pulse: number;
  step(input: Input): void;
}

export function createSim(count: number, seed: number): Sim {
  const r = rng(seed);
  const x = new Float32Array(count);
  const y = new Float32Array(count);
  const px = new Float32Array(count);
  const py = new Float32Array(count);
  const vx = new Float32Array(count);
  const vy = new Float32Array(count);
  const heading = new Float32Array(count);
  // The spatial grid: drones sorted by cell each step (a counting sort), so a
  // drone looks only at the drones in the nine cells around it.
  const cells = COLS * ROWS;
  const start = new Int32Array(cells + 1);
  const cursor = new Int32Array(cells);
  const items = new Int32Array(count);
  const cellOf = new Int32Array(count);

  // Anywhere in the arena but within SAFE of the ship's start, so the first
  // seconds are for finding the controls.
  for (let i = 0; i < count; i++) {
    do {
      x[i] = 4 + r() * (W - 8);
      y[i] = 4 + r() * (H - 8);
    } while (Math.hypot(x[i] - W / 2, y[i] - H / 2) < SAFE);
    heading[i] = Math.atan2(H / 2 - y[i], W / 2 - x[i]);
  }
  px.set(x);
  py.set(y);
  const ship: Ship = { x: W / 2, y: H / 2, px: W / 2, py: H / 2, vx: 0, vy: 0, angle: -Math.PI / 2, alive: true };
  const orb = { x: 0, y: 0 };
  const placeOrb = () => {
    orb.x = 80 + r() * (W - 160);
    orb.y = 80 + r() * (H - 160);
  };
  placeOrb();

  const sim: Sim = { count, x, y, px, py, vx, vy, heading, ship, orb, score: 0, time: 0, steps: 0, pulse: 0, step };

  function grid(): void {
    start.fill(0);
    for (let i = 0; i < count; i++) {
      const c = cellAt(x[i], y[i]);
      cellOf[i] = c;
      start[c + 1]++;
    }
    for (let c = 0; c < cells; c++) start[c + 1] += start[c];
    cursor.set(start.subarray(0, cells));
    for (let i = 0; i < count; i++) items[cursor[cellOf[i]]++] = i;
  }

  function step(input: Input): void {
    const dt = STEP;
    sim.steps++;
    px.set(x);
    py.set(y);
    ship.px = ship.x;
    ship.py = ship.y;

    if (ship.alive) {
      sim.time += dt;
      // Steer: toward a held pointer, else by the keys.
      let ax = input.x;
      let ay = input.y;
      if (input.pointing) {
        const dx = input.tx - ship.x;
        const dy = input.ty - ship.y;
        const d = Math.hypot(dx, dy);
        ax = d > 6 ? dx / d : 0;
        ay = d > 6 ? dy / d : 0;
      } else {
        const l = Math.hypot(ax, ay);
        if (l > 0) {
          ax /= l;
          ay /= l;
        }
      }
      ship.vx += (ax * SHIP_ACCEL - ship.vx * SHIP_DRAG) * dt;
      ship.vy += (ay * SHIP_ACCEL - ship.vy * SHIP_DRAG) * dt;
      const s = Math.hypot(ship.vx, ship.vy);
      if (s > SHIP_MAX) {
        ship.vx *= SHIP_MAX / s;
        ship.vy *= SHIP_MAX / s;
      }
      if (s > 20) ship.angle = Math.atan2(ship.vy, ship.vx);
      ship.x += ship.vx * dt;
      ship.y += ship.vy * dt;
      if (ship.x < 8 || ship.x > W - 8) ship.vx = -ship.vx * 0.5;
      if (ship.y < 8 || ship.y > H - 8) ship.vy = -ship.vy * 0.5;
      ship.x = clamp(ship.x, 8, W - 8);
      ship.y = clamp(ship.y, 8, H - 8);
      // An orb taken: a point, a new orb, and a pulse that pushes the swarm back.
      if (Math.hypot(orb.x - ship.x, orb.y - ship.y) < ORB) {
        sim.score++;
        sim.pulse = 1;
        placeOrb();
      }
    }
    sim.pulse = Math.max(0, sim.pulse - dt * 1.5);

    grid();
    const top = DRONE_START + (DRONE_MAX - DRONE_START) * Math.min(1, sim.time / RAMP);
    for (let i = 0; i < count; i++) {
      // Toward the ship, and away from it while a pulse lasts.
      const dx = ship.x - x[i];
      const dy = ship.y - y[i];
      const d = Math.hypot(dx, dy) + 1e-6;
      let fx = (dx / d) * DRONE_SEEK;
      let fy = (dy / d) * DRONE_SEEK;
      if (sim.pulse > 0 && d < PULSE) {
        const k = sim.pulse * PULSE_PUSH * (1 - d / PULSE);
        fx -= (dx / d) * k;
        fy -= (dy / d) * k;
      }
      // Apart from the drones nearest it, at most NEIGHBOURS of them.
      const cx = Math.floor(x[i] / CELL);
      const cy = Math.floor(y[i] / CELL);
      let seen = 0;
      for (let oy = -1; oy <= 1 && seen < NEIGHBOURS; oy++) {
        const gy = cy + oy;
        if (gy < 0 || gy >= ROWS) continue;
        for (let ox = -1; ox <= 1 && seen < NEIGHBOURS; ox++) {
          const gx = cx + ox;
          if (gx < 0 || gx >= COLS) continue;
          const c = gy * COLS + gx;
          for (let k = start[c]; k < start[c + 1] && seen < NEIGHBOURS; k++) {
            const j = items[k];
            if (j === i) continue;
            const ex = x[i] - x[j];
            const ey = y[i] - y[j];
            const e2 = ex * ex + ey * ey;
            if (e2 > 1e-6 && e2 < SEPARATE * SEPARATE) {
              const e = Math.sqrt(e2);
              const push = ((SEPARATE - e) / SEPARATE) * SEPARATE_PUSH;
              fx += (ex / e) * push;
              fy += (ey / e) * push;
            }
            seen++;
          }
        }
      }
      let nvx = vx[i] + fx * dt;
      let nvy = vy[i] + fy * dt;
      const s = Math.hypot(nvx, nvy);
      if (s > top) {
        nvx *= top / s;
        nvy *= top / s;
      }
      let nx = x[i] + nvx * dt;
      let ny = y[i] + nvy * dt;
      if (nx < 2 || nx > W - 2) {
        nvx = -nvx;
        nx = clamp(nx, 2, W - 2);
      }
      if (ny < 2 || ny > H - 2) {
        nvy = -nvy;
        ny = clamp(ny, 2, H - 2);
      }
      vx[i] = nvx;
      vy[i] = nvy;
      x[i] = nx;
      y[i] = ny;
      if (s > 1) heading[i] = Math.atan2(nvy, nvx);
      if (ship.alive && Math.abs(dx) < HIT && Math.abs(dy) < HIT && dx * dx + dy * dy < HIT * HIT) ship.alive = false;
    }
  }

  return sim;
}

function cellAt(x: number, y: number): number {
  const cx = Math.min(COLS - 1, Math.max(0, Math.floor(x / CELL)));
  const cy = Math.min(ROWS - 1, Math.max(0, Math.floor(y / CELL)));
  return cy * COLS + cx;
}

function clamp(v: number, lo: number, hi: number): number {
  return v < lo ? lo : v > hi ? hi : v;
}
