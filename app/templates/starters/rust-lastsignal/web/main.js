// Last Signal, the page: draws what the Rust game (src/game.rs, compiled to
// /static/game.wasm by build-web.sh) says the world is, and hands it clicks.
//
// No wasm-bindgen and no bundler. Each frame the page calls game_update, then
// reads the world as one Float64Array straight out of the module's memory
// (game_frame, layout in game.rs `frame`) and the map as one byte per tile
// (game_tiles). Every rule lives in Rust; nothing here decides anything.
"use strict";

const MW = 48, MH = 30, T = 24, W = MW * T, H = MH * T, TAU = Math.PI * 2;
const HEAD = 26, UNIT_F = 11, BLD_F = 10, WRECK_F = 3, FX_F = 7, CACHE_F = 3;
const PLAYING = 0, WON = 1, LOST = 2;
const HUMAN = 0, MACHINE = 1;
// game.rs UKind and BKind, in order.
const UNITS = ["Scavenger", "Rifleman", "EMP trooper", "Hacker", "Technical", "Crawler", "Sentry", "Titan", "Reclaimer", "Dog", "Cat"];
const BUILDINGS = ["Haven", "Shelter", "Armory", "Garage", "Barricade", "Turret", "Floodlight", "Jammer", "Core"];
// What a person may place (BKind::PLACEABLE) and train (UKind::HUMAN, `i`),
// with what each costs and does - the same numbers as game.rs `spec`.
const PLACE = [
  { b: 1, cost: 90, w: 2, h: 2, tip: "+4 housing; scavengers unload here" },
  { b: 2, cost: 150, w: 2, h: 2, tip: "trains Riflemen, EMP troopers, Hackers" },
  { b: 3, cost: 220, w: 3, h: 2, tip: "builds Technicals" },
  { b: 4, cost: 15, w: 1, h: 1, tip: "a wall" },
  { b: 5, cost: 120, w: 1, h: 1, tip: "an auto gun" },
  { b: 6, cost: 60, w: 1, h: 1, tip: "machines in its light take +35%" },
  { b: 7, cost: 140, w: 1, h: 1, tip: "stops the Grid and erases it nearby" },
];
const TRAIN = [
  { i: 0, u: 0, cost: 50, from: 0, tip: "salvages wrecks into scrap" },
  { i: 1, u: 1, cost: 60, from: 2, tip: "steady rifle fire" },
  { i: 2, u: 2, cost: 90, from: 2, tip: "pulse that stuns machines" },
  { i: 3, u: 3, cost: 140, from: 2, tip: "turns a machine to your side" },
  { i: 4, u: 4, cost: 170, from: 3, tip: "fast armed truck" },
  { i: 5, u: 9, cost: 70, from: 0, pet: true, tip: "a pet: bites machines, and at night runs off to fetch power cells" },
  { i: 6, u: 10, cost: 60, from: 0, pet: true, tip: "a pet: sees in the dark, and purrs the hurt near it back to health" },
];
const NOTES = [null, ["Nightfall. The Cores are sending a wave - and power cells glow out in the dark.", 1], ["Dawn. You held the night.", 0], ["Not enough scrap.", 1],
  ["No survivors waiting - build Shelters, they arrive by day.", 1], ["No room - build a Shelter.", 1], ["Build that first.", 1],
  ["Can't build there: near your base, off the Grid, on open ground.", 1], ["A machine is ours now.", 0], ["A Core is down.", 0],
  ["The Haven is under attack.", 1], ["A survivor found the Haven.", 0], ["Power cells found.", 0], ["A new pet joined you.", 0], ["Six pets is the most you can keep.", 1]];

const $ = (id) => document.getElementById(id);
const canvas = $("map"), ctx = canvas.getContext("2d");
const dark = document.createElement("canvas"); dark.width = W; dark.height = H;
// Pixels per map unit on the backing store: the canvas is drawn at the size it
// is shown times the screen's density (capped), so a 4K TV gets a sharp map,
// not a 1152 px image stretched. Every draw is in map units under this scale.
let SCALE = 1;
const dctx = dark.getContext("2d");
let wasm, game, world = null, tiles = null, terrain = null, terrainKey = "";
let placing = null, fight = false, drag = null, hover = null, noteTimer = 0;
// From the terrain pass: water tiles (shimmered each frame) and lit windows
// (they glow at night).
let waterTiles = [], windows = [];
// Short-lived sparks, smoke and debris, and long-lived scorch marks: the
// page's own, made when the world reports a shot or a blast (listen).
const particles = [], decals = [];

// ---- reading the module ----------------------------------------------------------
function read() {
  const ptr = wasm.game_frame(game), len = wasm.game_frame_len(game);
  // A fresh view every frame: a grown memory detaches the old views.
  const f = new Float64Array(wasm.memory.buffer, ptr, len);
  const w = {
    state: f[0], time: f[1], night: f[2] === 1, dayT: f[3], until: f[4], scrap: f[5], waiting: f[6], people: f[7],
    housing: f[8], nights: f[9], score: f[10], kills: f[11], turned: f[12], coresDown: f[13], cores: f[14],
    note: f[20], paused: f[21] === 1, wave: f[23], power: f[25], units: [], buildings: [], wrecks: [], fx: [], queue: [], caches: [],
  };
  let i = HEAD;
  for (let k = 0; k < f[15]; k++, i += UNIT_F) w.units.push({ id: f[i], kind: f[i + 1], side: f[i + 2], x: f[i + 3], y: f[i + 4], hp: f[i + 5], sel: f[i + 6] === 1, facing: f[i + 7], stun: f[i + 8] === 1, work: f[i + 9], turned: f[i + 10] === 1 });
  for (let k = 0; k < f[16]; k++, i += BLD_F) w.buildings.push({ id: f[i], kind: f[i + 1], side: f[i + 2], tx: f[i + 3], ty: f[i + 4], w: f[i + 5], h: f[i + 6], hp: f[i + 7], progress: f[i + 8], sel: f[i + 9] === 1 });
  for (let k = 0; k < f[17]; k++, i += WRECK_F) w.wrecks.push({ x: f[i], y: f[i + 1], scrap: f[i + 2] });
  for (let k = 0; k < f[18]; k++, i += FX_F) w.fx.push({ kind: f[i], x0: f[i + 1], y0: f[i + 2], x1: f[i + 3], y1: f[i + 4], t: f[i + 5] });
  for (let k = 0; k < f[19]; k++, i += 2) w.queue.push({ kind: f[i], left: f[i + 1] });
  for (let k = 0; k < f[24]; k++, i += CACHE_F) w.caches.push({ x: f[i], y: f[i + 1], scrap: f[i + 2] });
  tiles = new Uint8Array(wasm.memory.buffer, wasm.game_tiles(game), MW * MH).slice();
  return w;
}

// ---- drawing -------------------------------------------------------------------------
// The look (2026-11-01): a 2D map that reads as 2.5D - ruined blocks with
// height, faces and cast shadows; buildings and units that each have a
// silhouette of their own; lighting that makes the night; effects in layers
// (flash, sparks, smoke, debris, a scorch mark that stays a while). The light
// comes from the upper left, so shadows fall down and to the right.
function hash(x, y) { let h = (x * 374761393 + y * 668265263) >>> 0; h = (h ^ (h >>> 13)) * 1274126177 >>> 0; return (h & 1023) / 1023; }
const SHADOW = "rgba(0,0,0,0.38)";
const open = (x, y) => x >= 0 && y >= 0 && x < MW && y < MH && (tiles[y * MW + x] & 7) <= 1;

// Terrain is drawn once into its own canvas, again only when a tile changes
// kind (the Grid and the water's shimmer are drawn on top each frame).
function drawTerrain() {
  const key = Array.from(tiles, (b) => b & 7).join("");
  if (key === terrainKey) return;
  terrainKey = key;
  terrain = terrain || document.createElement("canvas");
  terrain.width = Math.round(W * SCALE); terrain.height = Math.round(H * SCALE);
  const c = terrain.getContext("2d");
  c.setTransform(SCALE, 0, 0, SCALE, 0, 0);
  waterTiles = []; windows = [];
  const kind = (x, y) => (x < 0 || y < 0 || x >= MW || y >= MH) ? 0 : tiles[y * MW + x] & 7;
  // Ground, road and water first, everywhere.
  for (let y = 0; y < MH; y++) for (let x = 0; x < MW; x++) {
    const t = kind(x, y), r = hash(x, y), px = x * T, py = y * T;
    if (t === 4) {
      const g = c.createLinearGradient(px, py, px, py + T); g.addColorStop(0, "#0c2230"); g.addColorStop(1, "#091821");
      c.fillStyle = g; c.fillRect(px, py, T, T);
      // The bank: a lip of earth where water meets land.
      c.fillStyle = "#2b2a22";
      if (kind(x, y - 1) !== 4) c.fillRect(px, py, T, 3);
      if (kind(x - 1, y) !== 4) c.fillRect(px, py, 2, T);
      if (kind(x + 1, y) !== 4) c.fillRect(px + T - 2, py, 2, T);
      waterTiles.push([px, py]);
      continue;
    }
    if (t === 1) {
      c.fillStyle = `rgb(${26 + r * 5},${28 + r * 5},${32 + r * 5})`; c.fillRect(px, py, T, T);
      c.fillStyle = "#3c3f45"; // kerbs where the road meets the blocks
      if (!open(x, y - 1) || kind(x, y - 1) !== 1 && (y % 7 !== 2)) {}
      const vertical = x % 8 === 3, horizontal = y % 7 === 2;
      c.fillStyle = "#4a4a3f";
      if (vertical && !horizontal) { for (let k = 0; k < 2; k++) c.fillRect(px + T / 2 - 1, py + 2 + k * 12, 2, 6); }
      if (horizontal && !vertical) { for (let k = 0; k < 2; k++) c.fillRect(px + 2 + k * 12, py + T / 2 - 1, 6, 2); }
      if (r > 0.75) { c.strokeStyle = "#16181b"; c.lineWidth = 1; c.beginPath(); c.moveTo(px + r * 20, py + 3); c.lineTo(px + 10, py + 12); c.lineTo(px + 14 + r * 6, py + 21); c.stroke(); }
      continue;
    }
    // Open ground: dusty earth with tufts of weed through it.
    c.fillStyle = `rgb(${33 + r * 9},${34 + r * 8},${29 + r * 5})`; c.fillRect(px, py, T, T);
    for (let k = 0; k < 3; k++) { const a = hash(x * 3 + k, y * 7 - k); c.fillStyle = a > 0.5 ? "rgba(255,255,255,0.025)" : "rgba(0,0,0,0.06)"; c.fillRect(px + a * 20, py + hash(y + k, x - k) * 20, 3, 3); }
    if (r > 0.8) { c.fillStyle = "#34472c"; for (let k = 0; k < 4; k++) c.fillRect(px + 6 + r * 8 + k * 1.6, py + 10 + (k % 2) * 2, 1.2, 4 - (k % 2)); }
  }
  // Rubble heaps: chunks with a lit top and a dark side.
  for (let y = 0; y < MH; y++) for (let x = 0; x < MW; x++) {
    if (kind(x, y) !== 2) continue;
    const px = x * T, py = y * T;
    for (let k = 0; k < 5; k++) {
      const a = hash(x + k * 5, y * 3 + k), b = hash(y + k, x * 2 + k), s = 4 + a * 5;
      const cx = px + 3 + a * 15, cy = py + 4 + b * 14;
      c.fillStyle = SHADOW; c.fillRect(cx + 2, cy + 2, s, s * 0.8);
      c.fillStyle = k % 2 ? "#3e4046" : "#4b4d52"; c.fillRect(cx, cy, s, s * 0.8);
      c.fillStyle = "#5d6066"; c.fillRect(cx, cy, s, 1.4);
    }
  }
  // Ruins: blocks of wall with height. Drawn top row first, so a block's
  // south face lies over the ground beneath it.
  for (let y = 0; y < MH; y++) for (let x = 0; x < MW; x++) {
    if (kind(x, y) !== 3) continue;
    const px = x * T, py = y * T, r = hash(x, y), h = 7 + Math.round(r * 10);
    // Shadow: down and right, onto whatever is there.
    c.fillStyle = SHADOW;
    c.beginPath(); c.moveTo(px + T, py - h + 2); c.lineTo(px + T + h * 0.7, py + h * 0.4); c.lineTo(px + T + h * 0.7, py + T + h * 0.4); c.lineTo(px + h * 0.6, py + T + h * 0.4); c.lineTo(px, py + T); c.closePath(); c.fill();
    // South face, with windows; a few still lit.
    c.fillStyle = "#1c1e23"; c.fillRect(px, py + T - h, T, h);
    for (let k = 0; k < 3; k++) {
      const wx = px + 3 + k * 7, wy = py + T - h + 3;
      if (h > 9) {
        const lit = hash(x * 13 + k, y * 5) > 0.88;
        c.fillStyle = lit ? "#c9a25a" : "#0d0e11"; c.fillRect(wx, wy, 3, 3);
        if (lit) windows.push([wx + 1.5, wy + 1.5]);
      }
    }
    // Top: a broken roof, lighter where the light falls on it.
    const top = py - h + 3;
    c.fillStyle = `rgb(${46 + r * 10},${48 + r * 10},${54 + r * 10})`; c.fillRect(px, top, T, T - 3);
    c.fillStyle = "rgba(255,255,255,0.07)"; c.fillRect(px, top, T, 2); c.fillRect(px, top, 2, T - 3);
    c.fillStyle = "rgba(0,0,0,0.25)"; for (let k = 0; k < 2; k++) c.fillRect(px + 4 + hash(x + k, y) * 12, top + 4 + hash(y + k, x) * 10, 4, 3);
  }
}

// Water moves: a few lines of light drift along each water tile.
function water(t) {
  ctx.save();
  ctx.strokeStyle = "rgba(120,190,220,0.18)"; ctx.lineWidth = 1;
  for (const [px, py] of waterTiles) {
    for (let k = 0; k < 2; k++) {
      const o = ((t * 6 + px * 0.7 + k * 11) % T), yy = py + 7 + k * 9 + Math.sin(t * 1.4 + px * 0.1 + k) * 1.5;
      ctx.beginPath(); ctx.moveTo(px + o - 5, yy); ctx.lineTo(px + o + 2, yy); ctx.stroke();
    }
  }
  ctx.restore();
}

function grid(t) {
  ctx.save();
  for (let y = 0; y < MH; y++) for (let x = 0; x < MW; x++) {
    if (!(tiles[y * MW + x] & 8)) continue;
    const px = x * T, py = y * T, glow = 0.35 + 0.25 * Math.sin(t * 2 + x * 0.6 + y * 0.4);
    ctx.fillStyle = `rgba(110,14,30,${0.22 + glow * 0.1})`; ctx.fillRect(px, py, T, T);
    const r = hash(x, y);
    ctx.strokeStyle = `rgba(255,70,90,${0.3 + glow * 0.35})`; ctx.lineWidth = 1;
    ctx.beginPath();
    ctx.moveTo(px, py + T * r); ctx.lineTo(px + T * 0.5, py + T * r); ctx.lineTo(px + T * 0.5, py + T);
    ctx.moveTo(px + T * 0.5, py + T * r); ctx.lineTo(px + T, py + T * (0.2 + r * 0.6));
    ctx.stroke();
    // A pulse runs along the lines.
    const run = (t * 0.8 + r) % 1;
    ctx.fillStyle = `rgba(255,170,175,${0.8 * glow})`; ctx.fillRect(px + T * 0.5 * run - 1, py + T * r - 1, 2.4, 2.4);
  }
  ctx.restore();
}

const SIDE = (side, turned) => turned ? "#4cd6e0" : side === HUMAN ? "#e8b04b" : "#e0465a";

// A block with height: shadow, south face, roof. Returns the roof's rect.
function block(x, y, w, h, height, roof, face) {
  ctx.fillStyle = SHADOW;
  ctx.beginPath(); ctx.moveTo(x + w, y - height + 3); ctx.lineTo(x + w + height * 0.7, y + height * 0.4); ctx.lineTo(x + w + height * 0.7, y + h + height * 0.4 - 2); ctx.lineTo(x + height * 0.6, y + h + height * 0.4 - 2); ctx.lineTo(x + 2, y + h - 2); ctx.closePath(); ctx.fill();
  ctx.fillStyle = face; ctx.fillRect(x + 2, y + h - height - 2, w - 4, height);
  const r = { x: x + 2, y: y - height + 2, w: w - 4, h: h - 4 };
  ctx.fillStyle = roof; ctx.fillRect(r.x, r.y, r.w, r.h);
  ctx.fillStyle = "rgba(255,255,255,0.08)"; ctx.fillRect(r.x, r.y, r.w, 2); ctx.fillRect(r.x, r.y, 2, r.h);
  return r;
}

function building(b, t) {
  const x = b.tx * T, y = b.ty * T, w = b.w * T, h = b.h * T, c = SIDE(b.side, false);
  ctx.save();
  if (b.progress < 1) ctx.globalAlpha = 0.4 + 0.5 * b.progress;
  const lift = b.progress < 1 ? b.progress : 1;
  switch (b.kind) {
    case 8: { // Core: a black obelisk, a red heart behind its slots, rings that pulse
      const r = block(x, y, w, h, 26 * lift, "#120709", "#0b0405");
      const cx = r.x + r.w / 2, cy = r.y + r.h / 2, beat = 0.5 + 0.5 * Math.sin(t * 3);
      for (let k = 0; k < 3; k++) { ctx.strokeStyle = `rgba(255,60,80,${0.15 + 0.2 * beat - k * 0.05})`; ctx.lineWidth = 1.5; ctx.strokeRect(r.x + 4 + k * 5, r.y + 4 + k * 5, r.w - 8 - k * 10, r.h - 8 - k * 10); }
      const g = ctx.createRadialGradient(cx, cy, 0, cx, cy, 16 + beat * 6); g.addColorStop(0, "#ffd0d6"); g.addColorStop(0.25, "#ff3a52"); g.addColorStop(1, "#ff3a5200");
      ctx.fillStyle = g; ctx.beginPath(); ctx.arc(cx, cy, 16 + beat * 6, 0, TAU); ctx.fill();
      ctx.fillStyle = "#ff3a52"; for (let k = 0; k < 4; k++) ctx.fillRect(r.x + 6 + k * (r.w - 12) / 3 - 1, y + h - 22 * lift, 2, 8);
      ctx.restore();
      if (b.hp < 0.999) bar(x + 3, y - 30, w - 6, b.hp, "#e0465a");
      return;
    }
    case 0: { // Haven: a fortified house, a beacon on a mast, a flag
      const r = block(x, y, w, h, 16 * lift, "#3a3227", "#211c16");
      ctx.fillStyle = "#4d4232"; ctx.fillRect(r.x + 6, r.y + 6, r.w - 12, r.h - 12);
      ctx.fillStyle = c; ctx.fillRect(r.x + r.w / 2 - 10, r.y + r.h / 2 - 2, 20, 4); ctx.fillRect(r.x + r.w / 2 - 2, r.y + r.h / 2 - 10, 4, 20);
      const mx = r.x + r.w - 8, my = r.y + 6;
      ctx.strokeStyle = "#8a8f98"; ctx.lineWidth = 1.5; ctx.beginPath(); ctx.moveTo(mx, my); ctx.lineTo(mx, my - 18); ctx.stroke();
      ctx.fillStyle = c; ctx.beginPath(); ctx.moveTo(mx, my - 18); ctx.lineTo(mx + 10 + Math.sin(t * 4) * 1.5, my - 15); ctx.lineTo(mx, my - 12); ctx.fill();
      break;
    }
    case 1: { // Shelter: a tent of patched canvas
      ctx.fillStyle = SHADOW; ctx.beginPath(); ctx.moveTo(x + 4, y + h - 3); ctx.lineTo(x + w / 2 + 6, y + 6); ctx.lineTo(x + w + 6, y + h + 2); ctx.closePath(); ctx.fill();
      ctx.fillStyle = "#5d5038"; ctx.beginPath(); ctx.moveTo(x + 3, y + h - 4); ctx.lineTo(x + w / 2, y + 4); ctx.lineTo(x + w / 2, y + h - 4); ctx.closePath(); ctx.fill();
      ctx.fillStyle = "#7a6a49"; ctx.beginPath(); ctx.moveTo(x + w / 2, y + 4); ctx.lineTo(x + w - 3, y + h - 4); ctx.lineTo(x + w / 2, y + h - 4); ctx.closePath(); ctx.fill();
      ctx.fillStyle = "#1a160f"; ctx.beginPath(); ctx.moveTo(x + w / 2 - 5, y + h - 4); ctx.lineTo(x + w / 2, y + h - 16); ctx.lineTo(x + w / 2 + 5, y + h - 4); ctx.fill();
      ctx.fillStyle = c; ctx.fillRect(x + w / 2 - 1, y + 2, 2, 4);
      break;
    }
    case 2: { // Armory: a bunker of crates
      const r = block(x, y, w, h, 12 * lift, "#3b3a2b", "#23221a");
      for (let k = 0; k < 4; k++) { ctx.fillStyle = k % 2 ? "#5b5235" : "#6a5f3c"; ctx.fillRect(r.x + 4 + (k % 2) * (r.w / 2 - 2), r.y + 4 + Math.floor(k / 2) * (r.h / 2 - 2), r.w / 2 - 6, r.h / 2 - 6); }
      ctx.fillStyle = c; ctx.fillRect(r.x + r.w / 2 - 6, r.y + r.h / 2 - 1, 12, 2);
      break;
    }
    case 3: { // Garage: a long shed with a roll-up door
      const r = block(x, y, w, h, 14 * lift, "#30343a", "#1d2024");
      for (let k = 0; k < 5; k++) { ctx.fillStyle = "rgba(255,255,255,0.05)"; ctx.fillRect(r.x, r.y + 3 + k * (r.h / 5), r.w, 1); }
      ctx.fillStyle = "#4a4f57"; ctx.fillRect(r.x + r.w / 2 - 12, y + h - 14 * lift - 2, 24, 14 * lift);
      ctx.fillStyle = c; ctx.fillRect(r.x + 4, r.y + 4, 8, 3);
      break;
    }
    case 4: { // Barricade: concrete blocks with hazard paint
      const r = block(x, y, w, h, 8 * lift, "#5a5d63", "#3a3d42");
      ctx.fillStyle = "#c9932f"; for (let k = 0; k < 3; k++) ctx.fillRect(r.x + 2 + k * 7, y + h - 9, 3, 6);
      break;
    }
    case 5: { // Turret: a sandbagged gun that sweeps
      ctx.fillStyle = SHADOW; ctx.beginPath(); ctx.ellipse(x + w / 2 + 3, y + h / 2 + 4, 10, 7, 0, 0, TAU); ctx.fill();
      ctx.fillStyle = "#6b5f45"; ctx.beginPath(); ctx.arc(x + w / 2, y + h / 2, 10, 0, TAU); ctx.fill();
      ctx.fillStyle = "#3a3f47"; ctx.beginPath(); ctx.arc(x + w / 2, y + h / 2, 5.5, 0, TAU); ctx.fill();
      const a = Math.sin(t * 0.8 + b.id) * 1.2; ctx.strokeStyle = "#9aa1ab"; ctx.lineWidth = 2.4;
      ctx.beginPath(); ctx.moveTo(x + w / 2, y + h / 2); ctx.lineTo(x + w / 2 + Math.cos(a) * 12, y + h / 2 + Math.sin(a) * 12); ctx.stroke();
      ctx.fillStyle = c; ctx.fillRect(x + w / 2 - 1.5, y + h / 2 - 1.5, 3, 3);
      break;
    }
    case 6: { // Floodlight: a lamp on a pole
      ctx.strokeStyle = SHADOW; ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(x + w / 2, y + h - 4); ctx.lineTo(x + w / 2 + 12, y + h); ctx.stroke();
      ctx.strokeStyle = "#7d838c"; ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(x + w / 2, y + h - 3); ctx.lineTo(x + w / 2, y + 4); ctx.stroke();
      ctx.fillStyle = "#2a2e35"; ctx.fillRect(x + w / 2 - 5, y + 1, 10, 6);
      ctx.fillStyle = "#fff2c8"; ctx.fillRect(x + w / 2 - 4, y + 5, 8, 2);
      break;
    }
    case 7: { // Jammer: a lattice mast with a blinking light
      ctx.strokeStyle = SHADOW; ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(x + w / 2, y + h - 3); ctx.lineTo(x + w / 2 + 16, y + h + 2); ctx.stroke();
      ctx.strokeStyle = "#8a9099"; ctx.lineWidth = 1.2; ctx.beginPath();
      ctx.moveTo(x + 6, y + h - 3); ctx.lineTo(x + w / 2, y + 1); ctx.lineTo(x + w - 6, y + h - 3);
      for (let k = 1; k < 4; k++) { const yy = y + h - 3 - k * 5; ctx.moveTo(x + 6 + k * 2, yy); ctx.lineTo(x + w - 6 - k * 2, yy); }
      ctx.stroke();
      if (Math.sin(t * 5 + b.id) > 0) { ctx.fillStyle = c; ctx.beginPath(); ctx.arc(x + w / 2, y + 2, 2, 0, TAU); ctx.fill(); }
      break;
    }
  }
  if (b.sel) { ctx.strokeStyle = "#ffffffcc"; ctx.lineWidth = 1.5; ctx.strokeRect(x + 1, y + 1, w - 2, h - 2); }
  ctx.restore();
  if (b.progress < 1) bar(x + 3, y + h - 5, w - 6, b.progress, "#8b95a5");
  if (b.hp < 0.999) bar(x + 3, y - 18, w - 6, b.hp, b.side === HUMAN ? "#e8b04b" : "#e0465a");
}

function bar(x, y, w, v, c) {
  ctx.fillStyle = "#000a"; ctx.fillRect(x, y, w, 3);
  ctx.fillStyle = c; ctx.fillRect(x, y, w * Math.max(0, v), 3);
}

// Where each unit was last frame, to know whether it walks (legs move, a
// little bob) and to point a pet's tail.
const lastPos = new Map();

// Each unit has its own silhouette, readable at a glance: shape first,
// then the side's colour as an accent, never as the whole body.
function unit(u, t) {
  const x = u.x * T, y = u.y * T, c = SIDE(u.side, u.turned);
  const prev = lastPos.get(u.id), moving = prev && Math.hypot(prev[0] - u.x, prev[1] - u.y) > 0.002;
  lastPos.set(u.id, [u.x, u.y]);
  const step = moving ? Math.sin(t * 16 + u.id) : 0;
  const big = (u.kind === 7 ? 13 : u.kind === 4 ? 10 : u.kind >= 5 && u.kind <= 8 ? 8 : 6) * 1.3;
  // Shadow on the ground, and the selection ring with it.
  ctx.fillStyle = SHADOW; ctx.beginPath(); ctx.ellipse(x + 2.5, y + 3, big, big * 0.55, 0, 0, TAU); ctx.fill();
  if (u.sel) { ctx.strokeStyle = "#ffffffd0"; ctx.lineWidth = 1.2; ctx.beginPath(); ctx.ellipse(x, y + 2, big + 4, (big + 4) * 0.6, 0, 0, TAU); ctx.stroke(); }
  ctx.save(); ctx.translate(x, y - (moving ? Math.abs(step) * 0.8 : 0)); ctx.scale(1.3, 1.3); ctx.rotate(u.facing);
  ctx.lineWidth = 1; ctx.strokeStyle = "#0a0b0d";
  const circle = (cx, cy, r, fill) => { ctx.fillStyle = fill; ctx.beginPath(); ctx.arc(cx, cy, r, 0, TAU); ctx.fill(); ctx.stroke(); };
  switch (u.kind) {
    case 0: // Scavenger: a big pack, a hard hat
      ctx.fillStyle = "#57534a"; ctx.fillRect(-7, -4.5, 6, 9); ctx.strokeRect(-7, -4.5, 6, 9);
      if (u.work > 0) { ctx.fillStyle = "#b8874a"; ctx.fillRect(-9, -3, 3, 6); }
      circle(1, 0, 4, "#8a7a5a"); circle(1.5, 0, 2.6, c); break;
    case 1: // Rifleman: helmet, rifle out front
      ctx.strokeStyle = "#1a1c1f"; ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(1, 2); ctx.lineTo(11, 2); ctx.stroke(); ctx.lineWidth = 1; ctx.strokeStyle = "#0a0b0d";
      circle(0, 0, 4.6, "#4d5a3c"); ctx.fillStyle = c; ctx.fillRect(-1.5, -4.6, 3, 9.2); break;
    case 2: // EMP trooper: a dish on the shoulder, blue coil
      circle(0, 0, 4.6, "#3e4a5a");
      ctx.strokeStyle = "#8fd9ff"; ctx.lineWidth = 1.6; ctx.beginPath(); ctx.arc(3, 0, 5.5, -1, 1); ctx.stroke();
      ctx.fillStyle = "#bdf0ff"; ctx.fillRect(6, -1, 2, 2); ctx.fillStyle = c; ctx.fillRect(-3, -1, 3, 2); break;
    case 3: // Hacker: hood, glowing visor, antenna
      circle(0, 0, 4.4, "#262a33");
      ctx.fillStyle = "#4cd6e0"; ctx.fillRect(1.5, -2.5, 2.5, 5);
      ctx.strokeStyle = "#9aa1ab"; ctx.beginPath(); ctx.moveTo(-3, -2); ctx.lineTo(-7, -7); ctx.stroke();
      ctx.fillStyle = c; ctx.beginPath(); ctx.arc(-7, -7, 1.3, 0, TAU); ctx.fill(); break;
    case 4: // Technical: a pickup with a gun in the bed
      ctx.fillStyle = "#1b1d21"; for (const [wx, wy] of [[-6, -6], [4, -6], [-6, 5], [4, 5]]) ctx.fillRect(wx, wy, 4, 2);
      ctx.fillStyle = "#6a5a3a"; ctx.fillRect(-9, -5, 18, 10); ctx.strokeRect(-9, -5, 18, 10);
      ctx.fillStyle = "#2b2f36"; ctx.fillRect(3, -4, 5, 8); ctx.fillStyle = "#9bc3d6"; ctx.fillRect(6, -3, 1.5, 6);
      ctx.fillStyle = c; ctx.fillRect(-6, -1.5, 6, 3); ctx.strokeStyle = "#111"; ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(-3, 0); ctx.lineTo(4, 0); ctx.stroke(); break;
    case 5: { // Crawler: a spider of steel, legs ticking
      ctx.strokeStyle = u.turned ? c : "#8c8f96"; ctx.lineWidth = 1.4; ctx.beginPath();
      for (let k = 0; k < 6; k++) { const a = (k < 3 ? -1 : 1) * (0.6 + (k % 3) * 0.7) + step * 0.25 * (k % 2 ? 1 : -1); ctx.moveTo(0, 0); ctx.lineTo(Math.cos(a) * 5, Math.sin(a) * 5); ctx.lineTo(Math.cos(a) * 8.5, Math.sin(a) * 8.5 + 1); }
      ctx.stroke(); circle(0, 0, 3.6, "#2b2e33"); ctx.fillStyle = c; ctx.beginPath(); ctx.arc(2.2, 0, 1.4, 0, TAU); ctx.fill(); break;
    }
    case 6: // Sentry: a tripod with one big lens
      ctx.strokeStyle = "#8c8f96"; ctx.lineWidth = 1.6; ctx.beginPath();
      for (const a of [2.2, -2.2, 0.2]) { ctx.moveTo(0, 0); ctx.lineTo(Math.cos(a) * 8, Math.sin(a) * 8); }
      ctx.stroke(); circle(0, 0, 4.8, "#33363c"); circle(2.5, 0, 2.4, "#16090c"); ctx.fillStyle = c; ctx.beginPath(); ctx.arc(3, 0, 1.4, 0, TAU); ctx.fill(); break;
    case 7: // Titan: a walking fortress, two legs, a long cannon
      ctx.fillStyle = "#3b3f46"; ctx.fillRect(-5 + step * 3, -12, 7, 5); ctx.fillRect(-5 - step * 3, 7, 7, 5);
      ctx.fillStyle = "#4a4f57"; ctx.beginPath(); for (let k = 0; k < 6; k++) { const a = k * TAU / 6; ctx.lineTo(Math.cos(a) * 10, Math.sin(a) * 9); } ctx.closePath(); ctx.fill(); ctx.stroke();
      ctx.fillStyle = "#2a2d33"; ctx.fillRect(2, -2.5, 14, 5); ctx.strokeRect(2, -2.5, 14, 5);
      ctx.fillStyle = c; ctx.fillRect(-4, -2, 4, 4); break;
    case 8: // Reclaimer: a crab with a glowing tank and claws
      ctx.strokeStyle = "#8c8f96"; ctx.lineWidth = 1.6; ctx.beginPath(); ctx.moveTo(3, -3); ctx.lineTo(9, -6); ctx.lineTo(10, -3); ctx.moveTo(3, 3); ctx.lineTo(9, 6); ctx.lineTo(10, 3); ctx.stroke();
      ctx.fillStyle = "#3a3d43"; ctx.beginPath(); ctx.ellipse(0, 0, 7, 5, 0, 0, TAU); ctx.fill(); ctx.stroke();
      ctx.fillStyle = u.turned ? c : "#c0476a"; ctx.fillRect(-4, -2, 5, 4); break;
    case 9: { // Dog: four legs, ears, a tail that wags
      ctx.strokeStyle = "#2a1f16"; ctx.lineWidth = 1.4; ctx.beginPath();
      for (const [lx, s] of [[3, 1], [-3, -1]]) { ctx.moveTo(lx, -2); ctx.lineTo(lx + step * s * 1.5, -5); ctx.moveTo(lx, 2); ctx.lineTo(lx - step * s * 1.5, 5); }
      const wag = Math.sin(t * (moving ? 10 : 18) + u.id) * 0.7; ctx.moveTo(-5, 0); ctx.lineTo(-5 - Math.cos(wag) * 5, Math.sin(wag) * 5); ctx.stroke();
      ctx.fillStyle = "#8a5a32"; ctx.beginPath(); ctx.ellipse(0, 0, 5.5, 3.2, 0, 0, TAU); ctx.fill(); ctx.stroke();
      circle(5.5, 0, 2.8, "#9b6a3c"); ctx.fillStyle = "#4a2f1a"; ctx.fillRect(4.5, -3.8, 2, 1.6); ctx.fillRect(4.5, 2.2, 2, 1.6);
      ctx.fillStyle = c; ctx.fillRect(2.6, -2.2, 1.2, 4.4); break;
    }
    case 10: { // Cat: pointed ears, a long curling tail, eyes that shine at night
      const curl = Math.sin(t * 2 + u.id) * 0.5; ctx.strokeStyle = "#4a4a50"; ctx.lineWidth = 1.6;
      ctx.beginPath(); ctx.moveTo(-4, 0); ctx.quadraticCurveTo(-8, 4 * curl, -8, -4 + curl * 3); ctx.stroke(); ctx.lineWidth = 1; ctx.strokeStyle = "#0a0b0d";
      ctx.fillStyle = "#5b5b62"; ctx.beginPath(); ctx.ellipse(-0.5, 0, 4.5, 2.8, 0, 0, TAU); ctx.fill(); ctx.stroke();
      circle(4, 0, 2.6, "#66666e"); ctx.fillStyle = "#66666e";
      ctx.beginPath(); ctx.moveTo(4, -2); ctx.lineTo(6.5, -4.2); ctx.lineTo(5.5, -1.2); ctx.moveTo(4, 2); ctx.lineTo(6.5, 4.2); ctx.lineTo(5.5, 1.2); ctx.fill();
      ctx.fillStyle = "#b8f27a"; ctx.fillRect(5.2, -1.2, 1, 0.9); ctx.fillRect(5.2, 0.4, 1, 0.9);
      ctx.fillStyle = c; ctx.fillRect(1.5, -2, 1, 4); break;
    }
  }
  ctx.restore();
  if (u.stun) { ctx.strokeStyle = "#8fe9ff"; ctx.lineWidth = 1; for (let k = 0; k < 3; k++) { const a = t * 9 + k * 2.1; ctx.beginPath(); ctx.moveTo(x + Math.cos(a) * 7, y + Math.sin(a) * 7); ctx.lineTo(x + Math.cos(a + 0.5) * 11, y + Math.sin(a + 0.5) * 11); ctx.stroke(); } }
  if (u.hp < 0.999) bar(x - 8, y - big - 7, 16, u.hp, u.side === HUMAN || u.turned ? "#e8b04b" : "#e0465a");
}

function wreck(w) {
  // Salvage: a heap of rusted plates, bigger the more it holds.
  const x = w.x * T, y = w.y * T, s = Math.min(1.25, 0.55 + w.scrap / 260), seed = Math.round(w.x * 10) * 31 + Math.round(w.y * 10);
  ctx.fillStyle = SHADOW; ctx.beginPath(); ctx.ellipse(x + 3, y + 3, 9 * s, 5 * s, 0, 0, TAU); ctx.fill();
  for (let k = 0; k < 4; k++) {
    const a = hash(seed + k, k) * TAU, r = (3 + k * 2) * s, ox = Math.cos(a) * r * 0.6, oy = Math.sin(a) * r * 0.6;
    ctx.fillStyle = ["#7a4a2a", "#5b5f66", "#8d5a30", "#474a50"][k];
    ctx.beginPath(); ctx.moveTo(x + ox - 4 * s, y + oy - 2 * s); ctx.lineTo(x + ox + 3 * s, y + oy - 4 * s); ctx.lineTo(x + ox + 5 * s, y + oy + 2 * s); ctx.lineTo(x + ox - 2 * s, y + oy + 4 * s); ctx.closePath(); ctx.fill();
    ctx.fillStyle = "rgba(255,255,255,0.08)"; ctx.fillRect(x + ox - 3 * s, y + oy - 3 * s, 5 * s, 1);
  }
}

// A cache of power cells: crystals that glow cyan in the dark.
function cache(c, t) {
  const x = c.x * T, y = c.y * T, pulse = 0.6 + 0.4 * Math.sin(t * 3 + c.x);
  ctx.fillStyle = SHADOW; ctx.beginPath(); ctx.ellipse(x + 2, y + 3, 6, 3, 0, 0, TAU); ctx.fill();
  for (const [dx, h, wd] of [[-3, 9, 3], [1, 12, 3.5], [4, 7, 2.5]]) {
    ctx.fillStyle = `rgba(110,240,255,${0.55 + pulse * 0.4})`;
    ctx.beginPath(); ctx.moveTo(x + dx, y + 2); ctx.lineTo(x + dx - wd / 2, y + 2 - h * 0.6); ctx.lineTo(x + dx, y + 2 - h); ctx.lineTo(x + dx + wd / 2, y + 2 - h * 0.6); ctx.closePath(); ctx.fill();
    ctx.fillStyle = "rgba(255,255,255,0.7)"; ctx.fillRect(x + dx - 0.4, y + 2 - h * 0.85, 0.8, h * 0.4);
  }
}

// Particles: flashes, sparks, smoke and debris, in map pixels.
function emit(x, y, kind, n, opts = {}) {
  for (let k = 0; k < n; k++) {
    const a = Math.random() * TAU, sp = (opts.speed || 60) * (0.3 + Math.random());
    particles.push({ x, y, vx: Math.cos(a) * sp, vy: Math.sin(a) * sp - (kind === "smoke" ? 12 : 0), age: 0,
      life: kind === "smoke" ? 1.6 + Math.random() : kind === "debris" ? 0.7 + Math.random() * 0.4 : kind === "flash" ? 0.12 : 0.35 + Math.random() * 0.3,
      kind, size: opts.size || (kind === "smoke" ? 5 + Math.random() * 5 : kind === "flash" ? 14 : 1.6), color: opts.color || "255,190,110" });
  }
  if (particles.length > 900) particles.splice(0, particles.length - 900);
}

function stepParticles(dt) {
  for (const p of particles) {
    p.age += dt; p.x += p.vx * dt; p.y += p.vy * dt;
    const drag = p.kind === "smoke" ? 0.6 : 0.25; p.vx *= Math.pow(drag, dt); p.vy *= Math.pow(drag, dt);
    if (p.kind === "smoke") p.size += dt * 8;
  }
  for (let i = particles.length - 1; i >= 0; i--) if (particles[i].age >= particles[i].life) particles.splice(i, 1);
  for (const d of decals) d.age += dt;
  while (decals.length && decals[0].age > 120) decals.shift();
}

function drawParticles(pass) {
  for (const p of particles) {
    const a = 1 - p.age / p.life;
    if (pass === "low" && p.kind === "smoke") {
      ctx.fillStyle = `rgba(40,40,44,${0.35 * a})`; ctx.beginPath(); ctx.arc(p.x, p.y, p.size, 0, TAU); ctx.fill();
    } else if (pass === "low" && p.kind === "debris") {
      ctx.fillStyle = `rgba(70,66,60,${a})`; ctx.fillRect(p.x - 1.2, p.y - 1.2, 2.4, 2.4);
    } else if (pass === "glow" && (p.kind === "spark" || p.kind === "flash")) {
      const r = p.kind === "flash" ? p.size * (1 - a * 0.4) : p.size * 2.5;
      spriteAt(ctx, p.x, p.y, r, p.color, a);
    }
  }
}

function drawDecals() {
  for (const d of decals) {
    const a = Math.min(1, (120 - d.age) / 30) * 0.5;
    const g = ctx.createRadialGradient(d.x, d.y, 0, d.x, d.y, d.r);
    g.addColorStop(0, `rgba(12,10,8,${a})`); g.addColorStop(0.7, `rgba(20,16,12,${a * 0.6})`); g.addColorStop(1, "rgba(20,16,12,0)");
    ctx.fillStyle = g; ctx.beginPath(); ctx.ellipse(d.x, d.y, d.r, d.r * 0.75, 0, 0, TAU); ctx.fill();
  }
}

function effects(fx) {
  for (const f of fx) {
    const x0 = f.x0 * T, y0 = f.y0 * T, x1 = f.x1 * T, y1 = f.y1 * T, a = 1 - f.t;
    switch (f.kind) {
      case 0: { // a tracer with a hot head
        const g = ctx.createLinearGradient(x0, y0, x1, y1); g.addColorStop(0, "rgba(255,230,170,0)"); g.addColorStop(1, `rgba(255,236,190,${a})`);
        ctx.strokeStyle = g; ctx.lineWidth = 1.6; ctx.beginPath(); ctx.moveTo(x0, y0); ctx.lineTo(x1, y1); ctx.stroke(); break;
      }
      case 1: ctx.strokeStyle = `rgba(140,230,255,${a})`; ctx.lineWidth = 2.5 * a + 0.5; ctx.beginPath(); ctx.arc(x1, y1, 4 + f.t * 32, 0, TAU); ctx.stroke(); break;
      case 2: ctx.strokeStyle = "rgba(76,214,224,0.85)"; ctx.setLineDash([3, 4]); ctx.lineDashOffset = -performance.now() / 40; ctx.lineWidth = 1.5; ctx.beginPath(); ctx.moveTo(x0, y0); ctx.lineTo(x1, y1); ctx.stroke(); ctx.setLineDash([]); break;
      case 3: { const g = ctx.createRadialGradient(x1, y1, 0, x1, y1, 6 + f.t * 26); g.addColorStop(0, `rgba(255,220,150,${a})`); g.addColorStop(0.5, `rgba(255,120,40,${a * 0.6})`); g.addColorStop(1, "rgba(255,90,40,0)"); ctx.fillStyle = g; ctx.beginPath(); ctx.arc(x1, y1, 6 + f.t * 26, 0, TAU); ctx.fill(); break; }
      case 4: ctx.strokeStyle = `rgba(232,176,75,${a})`; ctx.lineWidth = 1; ctx.beginPath(); ctx.moveTo(x0, y0); ctx.lineTo(x1, y1); ctx.stroke(); break;
      case 5: ctx.strokeStyle = `rgba(120,240,255,${a})`; ctx.lineWidth = 2; ctx.beginPath(); ctx.arc(x1, y1, 4 + f.t * 18, 0, TAU); ctx.stroke(); break;
    }
  }
}

// Lights are drawn from pre-made sprites - one soft disc per colour, scaled
// and faded with drawImage - not a new gradient per light per frame: at night
// that was most of the frame (31 ms headless, 2026-11-01).
const lightSprites = new Map();
function lightSprite(rgb) {
  let c = lightSprites.get(rgb);
  if (!c) {
    c = document.createElement("canvas"); c.width = c.height = 128;
    const g = c.getContext("2d"), gr = g.createRadialGradient(64, 64, 0, 64, 64, 64);
    gr.addColorStop(0, `rgba(${rgb},1)`); gr.addColorStop(0.55, `rgba(${rgb},0.8)`); gr.addColorStop(1, `rgba(${rgb},0)`);
    g.fillStyle = gr; g.fillRect(0, 0, 128, 128);
    lightSprites.set(rgb, c);
  }
  return c;
}
function spriteAt(cx, x, y, r, rgb, a) {
  if (a <= 0.01 || r <= 0) return;
  cx.globalAlpha = Math.min(1, a); cx.drawImage(lightSprite(rgb), x - r, y - r, r * 2, r * 2); cx.globalAlpha = 1;
}

// How dark it is, 0..~0.9: dusk comes over the last tenth of the day, the
// night deepens all the way through ("darker and darker"), dawn comes fast.
function darkness(w) {
  if (!w.night) return w.dayT > 0.9 ? (w.dayT - 0.9) * 10 * 0.45 : w.dayT < 0.04 ? (0.04 - w.dayT) / 0.04 * 0.4 : 0;
  const deepen = 0.5 + 0.4 * Math.min(1, w.dayT / 0.85);
  return w.dayT > 0.9 ? deepen * (1 - (w.dayT - 0.9) / 0.1 * 0.55) : deepen;
}

// The light pass: the dark laid over everything, holes cut where the
// survivors' lights fall, then coloured light added on top - lamps warm,
// caches cyan, the Grid and the machines' eyes red, cats' eyes green.
function lighting(w, t) {
  const depth = darkness(w);
  // A warm grade at dawn and dusk, cold blue at night.
  if (!w.night && depth > 0.02) { ctx.fillStyle = `rgba(255,140,60,${depth * 0.12})`; ctx.fillRect(0, 0, W, H); }
  if (depth < 0.02) return;
  dctx.setTransform(SCALE, 0, 0, SCALE, 0, 0);
  dctx.globalCompositeOperation = "source-over";
  dctx.clearRect(0, 0, W, H);
  dctx.fillStyle = `rgba(3,6,18,${depth})`; dctx.fillRect(0, 0, W, H);
  dctx.globalCompositeOperation = "destination-out";
  const hole = (x, y, r, k = 1) => spriteAt(dctx, x, y, r, "0,0,0", k);
  for (const b of w.buildings) if (b.side === HUMAN) hole((b.tx + b.w / 2) * T, (b.ty + b.h / 2) * T, (b.kind === 6 ? 5.6 : 2.4 + b.w * 0.6) * T);
  for (const u of w.units) if (u.side === HUMAN) hole(u.x * T, u.y * T, (u.kind === 4 ? 3.4 : u.kind === 10 ? 4.6 : 2.3) * T);
  for (const c of w.caches) hole(c.x * T, c.y * T, 1.6 * T, 0.7);
  for (const p of particles) if (p.kind === "flash") hole(p.x, p.y, p.size * 3, 1 - p.age / p.life);
  ctx.drawImage(dark, 0, 0, W, H);
  // Coloured light on top.
  ctx.save(); ctx.globalCompositeOperation = "lighter";
  const glow = (x, y, r, rgb, a) => spriteAt(ctx, x, y, r, rgb, a);
  for (const b of w.buildings) {
    if (b.kind === 6 && b.side === HUMAN && b.progress >= 1) glow((b.tx + 0.5) * T, (b.ty + 0.5) * T, 5 * T, "255,215,150", 0.22 * depth);
    if (b.kind === 0) glow((b.tx + 1.5) * T, (b.ty + 1.5) * T, 3.4 * T, "255,180,90", 0.16 * depth);
    if (b.kind === 8) glow((b.tx + 1.5) * T, (b.ty + 1.5) * T, 4 * T, "255,40,70", (0.18 + 0.08 * Math.sin(t * 3)) * depth);
  }
  for (const [x, y] of windows) if (Math.sin(t * 0.7 + x * 3.1) > -0.6) glow(x, y, 9, "255,200,120", 0.5 * depth);
  for (const c of w.caches) glow(c.x * T, c.y * T, 26, "90,230,255", (0.35 + 0.15 * Math.sin(t * 3 + c.x)) * depth);
  for (const u of w.units) {
    if (u.side === MACHINE && !u.turned) glow(u.x * T + Math.cos(u.facing) * 3, u.y * T + Math.sin(u.facing) * 3, u.kind === 7 ? 14 : 8, "255,40,60", (0.5 + 0.3 * Math.sin(t * 6 + u.id)) * depth);
    if (u.kind === 10 && u.side === HUMAN) glow(u.x * T + Math.cos(u.facing) * 5, u.y * T + Math.sin(u.facing) * 5, 6, "170,255,110", 0.6 * depth);
  }
  drawParticles("glow");
  ctx.restore();
}

function placement() {
  if (placing === null || !hover) return;
  const p = PLACE[placing], tx = Math.floor(hover.x - p.w / 2 + 0.5), ty = Math.floor(hover.y - p.h / 2 + 0.5);
  const ok = wasm.game_can_place(game, placing, tx, ty) === 1;
  ctx.fillStyle = ok ? "rgba(232,176,75,0.25)" : "rgba(224,70,90,0.25)";
  ctx.strokeStyle = ok ? "#e8b04b" : "#e0465a"; ctx.lineWidth = 1.5;
  ctx.fillRect(tx * T, ty * T, p.w * T, p.h * T); ctx.strokeRect(tx * T + 0.5, ty * T + 0.5, p.w * T - 1, p.h * T - 1);
  if (p.b === 6 || p.b === 7 || p.b === 5) {
    const r = (p.b === 6 ? 5 : p.b === 7 ? 4.5 : 5.5) * T;
    ctx.setLineDash([4, 5]); ctx.beginPath(); ctx.arc((tx + 0.5) * T, (ty + 0.5) * T, r, 0, TAU); ctx.stroke(); ctx.setLineDash([]);
  }
}

let lastRender = 0;
function render(w) {
  const now = performance.now() / 1000, dt = Math.min(0.1, lastRender ? now - lastRender : 0.016);
  lastRender = now;
  if (!w.paused) stepParticles(dt);
  ctx.setTransform(SCALE, 0, 0, SCALE, 0, 0);
  drawTerrain();
  ctx.drawImage(terrain, 0, 0, W, H);
  water(w.time);
  drawDecals();
  grid(w.time);
  for (const r of w.wrecks) wreck(r);
  for (const c of w.caches) cache(c, w.time);
  // Buildings and units by their southern edge, so nearer things overlap.
  const things = [];
  for (const b of w.buildings) things.push([b.ty + b.h, () => building(b, w.time)]);
  for (const u of w.units) things.push([u.y + 0.3, () => unit(u, w.time)]);
  things.sort((a, b) => a[0] - b[0]);
  for (const [, draw] of things) draw();
  drawParticles("low");
  effects(w.fx);
  lighting(w, w.time);
  if (!(darkness(w) >= 0.02)) { ctx.save(); ctx.globalCompositeOperation = "lighter"; drawParticles("glow"); ctx.restore(); }
  placement();
  if (drag && drag.moved) {
    ctx.strokeStyle = "#e8b04b"; ctx.fillStyle = "rgba(232,176,75,0.08)"; ctx.lineWidth = 1;
    const x = Math.min(drag.x0, drag.x1) * T, y = Math.min(drag.y0, drag.y1) * T, ww = Math.abs(drag.x1 - drag.x0) * T, hh = Math.abs(drag.y1 - drag.y0) * T;
    ctx.fillRect(x, y, ww, hh); ctx.strokeRect(x + 0.5, y + 0.5, ww, hh);
  }
}

// ---- sound ------------------------------------------------------------------------------------
// Every sound is synthesised with Web Audio - no files to load or license.
// One bus: sounds -> a short room reverb and a dry path -> a compressor that
// keeps a firefight from clipping -> master volume. Sounds are placed left or
// right by where they happen on the map, and quieter the further they are
// from the Haven, the player's ear.
const sound = (() => {
  let ac = null, master, bus, verb, noise, on = localStorage.getItem("signal-sound") !== "off";
  let level = Number(localStorage.getItem("signal-volume") || 70) / 100, ambience = null, last = {};
  const ear = { x: 8, y: 15 };
  function start() {
    if (ac) { if (ac.state === "suspended") ac.resume(); return; }
    const C = window.AudioContext || window.webkitAudioContext;
    if (!C) return;
    ac = new C();
    master = ac.createGain(); master.gain.value = on ? level : 0;
    const comp = ac.createDynamicsCompressor();
    comp.threshold.value = -18; comp.knee.value = 12; comp.ratio.value = 4; comp.attack.value = 0.003; comp.release.value = 0.2;
    comp.connect(master); master.connect(ac.destination);
    bus = ac.createGain(); bus.gain.value = 0.9; bus.connect(comp);
    // A small dark room: decaying noise as the impulse response.
    verb = ac.createConvolver();
    const len = Math.round(ac.sampleRate * 1.4), ir = ac.createBuffer(2, len, ac.sampleRate);
    for (let ch = 0; ch < 2; ch++) { const d = ir.getChannelData(ch); for (let i = 0; i < len; i++) d[i] = (Math.random() * 2 - 1) * Math.pow(1 - i / len, 3.2); }
    verb.buffer = ir;
    const wet = ac.createGain(); wet.gain.value = 0.22; verb.connect(wet); wet.connect(comp);
    noise = ac.createBuffer(1, ac.sampleRate, ac.sampleRate);
    const d = noise.getChannelData(0); for (let i = 0; i < d.length; i++) d[i] = Math.random() * 2 - 1;
  }
  // A voice: gain envelope, optional pan, to the dry bus and the reverb.
  function out(at, vol, x, y, wet = 1) {
    const g = ac.createGain(); g.gain.value = 0;
    let node = g;
    if (x !== undefined && ac.createStereoPanner) {
      const p = ac.createStereoPanner(); p.pan.value = Math.max(-0.85, Math.min(0.85, (x / MW) * 1.7 - 0.85));
      g.connect(p); node = p;
      const d = Math.hypot(x - ear.x, y - ear.y);
      vol *= Math.max(0.28, 1 - d / 70);
    }
    node.connect(bus);
    const send = ac.createGain(); send.gain.value = wet; node.connect(send); send.connect(verb);
    return { g, vol };
  }
  function env(g, t, a, peak, d) {
    g.gain.setValueAtTime(0.0001, t); g.gain.exponentialRampToValueAtTime(peak, t + a); g.gain.exponentialRampToValueAtTime(0.0001, t + a + d);
  }
  function burst(t, dur, type, f0, f1, q, dest) {
    const s = ac.createBufferSource(); s.buffer = noise; s.playbackRate.value = 0.8 + Math.random() * 0.4;
    const f = ac.createBiquadFilter(); f.type = type; f.Q.value = q;
    f.frequency.setValueAtTime(f0, t); f.frequency.exponentialRampToValueAtTime(Math.max(30, f1), t + dur);
    s.connect(f); f.connect(dest); s.start(t, Math.random() * 0.5); s.stop(t + dur + 0.05);
  }
  function tone(t, dur, type, f0, f1, dest, detune = 0) {
    const o = ac.createOscillator(); o.type = type; o.detune.value = detune;
    o.frequency.setValueAtTime(f0, t); o.frequency.exponentialRampToValueAtTime(Math.max(20, f1), t + dur);
    o.connect(dest); o.start(t); o.stop(t + dur + 0.05);
  }
  // Not more than one of a kind every `gap` seconds: a volley is one crack, not forty.
  function ready(kind, gap) {
    const now = ac.currentTime;
    if (now - (last[kind] || 0) < gap) return false;
    last[kind] = now; return true;
  }
  const play = {
    rifle(x, y) { // a short, dry crack with a body under it
      const t = ac.currentTime, v = out(t, 0.55, x, y, 0.6);
      env(v.g, t, 0.002, v.vol, 0.09); burst(t, 0.09, "bandpass", 2600, 900, 0.9, v.g); tone(t, 0.07, "triangle", 180, 70, v.g);
    },
    turret(x, y) { // heavier, with a mechanical thump
      const t = ac.currentTime, v = out(t, 0.65, x, y, 0.7);
      env(v.g, t, 0.002, v.vol, 0.16); burst(t, 0.14, "lowpass", 3200, 400, 0.7, v.g); tone(t, 0.12, "sine", 120, 45, v.g);
    },
    truck(x, y) { // the Technical's fast mounted gun
      const t = ac.currentTime, v = out(t, 0.35, x, y, 0.4);
      env(v.g, t, 0.001, v.vol, 0.05); burst(t, 0.05, "highpass", 3000, 1800, 0.7, v.g);
    },
    laser(x, y) { // a machine's shot: a falling, buzzing beam
      const t = ac.currentTime, v = out(t, 0.32, x, y, 0.8);
      const f = ac.createBiquadFilter(); f.type = "bandpass"; f.frequency.value = 1400; f.Q.value = 1.5; f.connect(v.g);
      env(v.g, t, 0.004, v.vol, 0.18); tone(t, 0.18, "sawtooth", 1500, 260, f); tone(t, 0.18, "square", 1510, 270, f, 7);
    },
    emp(x, y) { // a charged swell, then a soft electric snap
      const t = ac.currentTime, v = out(t, 0.55, x, y, 1.2);
      env(v.g, t, 0.06, v.vol, 0.5); tone(t, 0.5, "sine", 90, 900, v.g); burst(t + 0.05, 0.4, "bandpass", 500, 5000, 3, v.g);
    },
    hack(x, y) { // a quiet run of data chirps
      const t = ac.currentTime, v = out(t, 0.12, x, y, 0.3);
      env(v.g, t, 0.005, v.vol, 0.22);
      for (let k = 0; k < 4; k++) tone(t + k * 0.05, 0.04, "square", 900 + ((k * 37) % 5) * 220, 900 + ((k * 37) % 5) * 220, v.g);
    },
    blast(x, y, big) { // a low, rolling explosion with debris
      const t = ac.currentTime, v = out(t, big ? 1.0 : 0.7, x, y, 1.3), d = big ? 1.4 : 0.8;
      env(v.g, t, 0.005, v.vol, d); burst(t, d, "lowpass", big ? 1800 : 2400, 60, 0.8, v.g); tone(t, d * 0.7, "sine", big ? 70 : 95, 32, v.g);
      burst(t + 0.05, 0.3, "highpass", 4000, 2000, 0.5, v.g);
    },
    zap(x, y) {
      const t = ac.currentTime, v = out(t, 0.12, x, y, 0.5);
      env(v.g, t, 0.003, v.vol, 0.12); burst(t, 0.12, "bandpass", 6000, 1500, 4, v.g);
    },
    click() {
      const t = ac.currentTime, v = out(t, 0.18, undefined, undefined, 0.1);
      env(v.g, t, 0.001, v.vol, 0.05); tone(t, 0.05, "sine", 1800, 1200, v.g);
    },
    place() { // a building goes down: a weighty thunk
      const t = ac.currentTime, v = out(t, 0.5, undefined, undefined, 0.6);
      env(v.g, t, 0.003, v.vol, 0.3); tone(t, 0.3, "sine", 140, 50, v.g); burst(t, 0.12, "lowpass", 900, 200, 0.7, v.g);
    },
    pickup() { // power cells: a bright rising run
      const t = ac.currentTime, v = out(t, 0.3, undefined, undefined, 0.8);
      env(v.g, t, 0.005, v.vol, 0.5); [784, 988, 1175, 1568].forEach((hz, k) => tone(t + k * 0.06, 0.25, "triangle", hz, hz, v.g));
    },
    pet() { // a new friend: two soft notes up
      const t = ac.currentTime, v = out(t, 0.25, undefined, undefined, 0.6);
      env(v.g, t, 0.01, v.vol, 0.45); tone(t, 0.2, "sine", 523, 523, v.g); tone(t + 0.14, 0.3, "sine", 784, 784, v.g);
    },
    deny() {
      const t = ac.currentTime, v = out(t, 0.2, undefined, undefined, 0.1);
      env(v.g, t, 0.003, v.vol, 0.18); tone(t, 0.18, "triangle", 220, 180, v.g); tone(t + 0.09, 0.12, "triangle", 165, 150, v.g);
    },
    sting(kind) { // nightfall, dawn, a Core down, a machine turned, the Haven hit
      const t = ac.currentTime, v = out(t, 0.42, undefined, undefined, 1.4);
      const chord = { night: [[110, 0], [130.8, 0.08], [164.8, 0.16]], dawn: [[261.6, 0], [329.6, 0.1], [392, 0.2], [523.3, 0.3]],
        core: [[196, 0], [246.9, 0.12], [293.7, 0.24], [392, 0.36]], turned: [[659.3, 0], [880, 0.08]], hit: [[155.6, 0], [146.8, 0.14]] }[kind];
      const d = kind === "night" ? 2.6 : 1.6;
      env(v.g, t, kind === "night" ? 0.4 : 0.02, v.vol, d);
      const f = ac.createBiquadFilter(); f.type = "lowpass"; f.frequency.value = kind === "night" ? 900 : 2600; f.connect(v.g);
      for (const [hz, at] of chord) { tone(t + at, d, kind === "night" || kind === "hit" ? "sawtooth" : "triangle", hz, hz, f, -6); tone(t + at, d, "sine", hz, hz, f, 6); }
    },
  };
  // A bed of wind under the night, faded in and out.
  function weather(night) {
    if (!ac) return;
    if (night && !ambience) {
      const s = ac.createBufferSource(); s.buffer = noise; s.loop = true; s.playbackRate.value = 0.35;
      const f = ac.createBiquadFilter(); f.type = "lowpass"; f.frequency.value = 420;
      const lfo = ac.createOscillator(), depth = ac.createGain(); lfo.frequency.value = 0.07; depth.gain.value = 180;
      lfo.connect(depth); depth.connect(f.frequency);
      const g = ac.createGain(); g.gain.value = 0.0001; g.gain.exponentialRampToValueAtTime(0.09, ac.currentTime + 4);
      s.connect(f); f.connect(g); g.connect(bus); s.start(); lfo.start();
      ambience = { s, g, lfo };
    } else if (!night && ambience) {
      const a = ambience; ambience = null;
      a.g.gain.setTargetAtTime(0.0001, ac.currentTime, 1.2); setTimeout(() => { a.s.stop(); a.lfo.stop(); }, 6000);
    }
  }
  return {
    start,
    get on() { return on; },
    toggle() { on = !on; localStorage.setItem("signal-sound", on ? "on" : "off"); if (master) master.gain.setTargetAtTime(on ? level : 0, ac.currentTime, 0.05); },
    volume(v) { level = v; localStorage.setItem("signal-volume", String(Math.round(v * 100))); if (master && on) master.gain.setTargetAtTime(level, ac.currentTime, 0.05); },
    level: () => level,
    ear(x, y) { ear.x = x; ear.y = y; },
    play(kind, ...a) { if (!ac || !on || ac.state !== "running") return; if (ready(kind, { rifle: 0.06, laser: 0.07, truck: 0.05, hack: 0.25, zap: 0.15, turret: 0.08 }[kind] || 0)) play[kind](...a); },
    weather,
  };
})();

// What the world did since the last frame, as sounds. Effects carry no id,
// so each is known by its kind and endpoints, and played once.
const heard = new Map();
function listen(w) {
  const now = performance.now();
  const haven = w.buildings.find((b) => b.kind === 0);
  if (haven) sound.ear(haven.tx + 1.5, haven.ty + 1.5);
  for (const f of w.fx) {
    const key = `${f.kind}:${f.x0.toFixed(2)}:${f.y0.toFixed(2)}:${f.x1.toFixed(2)}:${f.y1.toFixed(2)}`;
    if (heard.has(key)) continue;
    heard.set(key, now);
    // The page's own layer of the event: flash, sparks, smoke, debris, a scorch.
    const [px, py] = [f.x1 * T, f.y1 * T];
    if (f.kind === 0) { emit(f.x0 * T, f.y0 * T, "flash", 1, { size: 6, color: "255,220,150" }); emit(px, py, "spark", 3, { speed: 50 }); }
    else if (f.kind === 1) emit(px, py, "spark", 14, { speed: 90, color: "140,230,255" });
    else if (f.kind === 3) {
      const big = f.x0 === f.x1 && w.buildings.length < (listen.buildings || 0);
      emit(px, py, "flash", 1, { size: big ? 46 : 28 });
      emit(px, py, "spark", big ? 26 : 14, { speed: big ? 140 : 100 });
      emit(px, py, "debris", big ? 12 : 6, { speed: big ? 90 : 60 });
      emit(px, py, "smoke", big ? 9 : 5, { speed: 14 });
      decals.push({ x: px, y: py, r: big ? 22 : 12, age: 0 });
      if (decals.length > 80) decals.shift();
    } else if (f.kind === 5) emit(px, py, "spark", 18, { speed: 70, color: "120,240,255" });
    if (f.kind === 5) { sound.play("pickup"); continue; }
    if (f.kind === 0) {
      const from = w.units.find((u) => Math.hypot(u.x - f.x0, u.y - f.y0) < 0.6);
      if (!from) sound.play("turret", f.x0, f.y0);
      else if (from.side === MACHINE && !from.turned) sound.play("laser", f.x0, f.y0);
      else sound.play(from.kind === 4 ? "truck" : from.kind >= 5 ? "laser" : "rifle", f.x0, f.y0);
    } else if (f.kind === 1) sound.play("emp", f.x1, f.y1);
    else if (f.kind === 2) sound.play("hack", f.x0, f.y0);
    else if (f.kind === 3) sound.play("blast", f.x1, f.y1, f.x0 === f.x1 && w.buildings.length < (listen.buildings || 0));
    else if (f.kind === 4) sound.play("zap", f.x1, f.y1);
  }
  listen.buildings = w.buildings.length;
  for (const [k, t] of heard) if (now - t > 1500) heard.delete(k);
  sound.weather(w.night && w.state === PLAYING);
  const sting = { 1: "night", 2: "dawn", 8: "turned", 9: "core", 10: "hit" }[w.note];
  if (sting && (sting !== "hit" || !listen.hitAt || now - listen.hitAt > 8000)) { if (sting === "hit") listen.hitAt = now; sound.play("sting", sting); }
  if ([3, 4, 5, 6, 7, 14].includes(w.note)) sound.play("deny");
  if (w.note === 13) sound.play("pet");
}

// ---- the panels around the map -----------------------------------------------------------
// The largest 16:10 box that fits the stage: full height beside the panel on
// a wide screen, full width when the page stacks (phones, portrait).
function fit() {
  const stage = $("stage"), stacked = getComputedStyle(document.querySelector("main")).display === "flex";
  const cw = stage.clientWidth, ch = stacked ? Infinity : stage.clientHeight;
  const w = Math.max(200, Math.floor(Math.min(cw, ch * W / H))), h = Math.round(w * H / W);
  canvas.style.width = `${w}px`; canvas.style.height = `${h}px`;
  const s = Math.min(3, Math.max(1, (w * (window.devicePixelRatio || 1)) / W));
  if (Math.abs(s - SCALE) > 0.01 || canvas.width !== Math.round(W * s)) {
    SCALE = s;
    canvas.width = Math.round(W * s); canvas.height = Math.round(H * s);
    dark.width = canvas.width; dark.height = canvas.height;
    terrainKey = "";
  }
}

function clock(s) { s = Math.max(0, Math.ceil(s)); return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`; }

function hud(w) {
  $("scrap").textContent = w.scrap;
  $("waiting").textContent = w.waiting;
  $("people").textContent = `${w.people} / ${w.housing}`;
  $("nights").textContent = w.nights;
  $("cores").textContent = w.cores;
  $("score").textContent = w.score.toLocaleString();
  $("phase").textContent = w.night ? `NIGHT ${w.nights + 1}` : `DAY ${w.nights + 1}`;
  $("phase").parentElement.classList.toggle("night", w.night);
  $("until").textContent = clock(w.until);
  $("until-what").textContent = w.night ? (w.wave > 0 ? `to dawn · ${w.wave} still coming` : "to dawn") : "to nightfall";
  const have = new Set(w.buildings.filter((b) => b.side === HUMAN && b.progress >= 1).map((b) => b.kind));
  document.querySelectorAll("#build button").forEach((b, i) => { b.disabled = w.scrap < PLACE[i].cost; b.classList.toggle("on", placing === i); });
  document.querySelectorAll("#train button").forEach((b, i) => {
    const q = w.queue.filter((x) => x.kind === TRAIN[i].u).length;
    b.disabled = w.scrap < TRAIN[i].cost || (!TRAIN[i].pet && w.waiting < 1) || !have.has(TRAIN[i].from);
    b.querySelector("small").textContent = `${TRAIN[i].cost} scrap${q ? ` · ${q} in training` : ""}`;
  });
  const sel = w.units.filter((u) => u.sel), selB = w.buildings.filter((b) => b.sel);
  const counts = {};
  for (const u of sel) { const n = (u.turned ? "turned " : "") + UNITS[u.kind]; counts[n] = (counts[n] || 0) + 1; }
  const many = (n) => n.endsWith("man") ? n.slice(0, -3) + "men" : n + "s";
  $("selected").textContent = sel.length ? Object.entries(counts).map(([n, c]) => `${c} ${c > 1 ? many(n) : n}`).join(", ")
    : selB.length ? `${BUILDINGS[selB[0].kind]} · ${Math.round(selB[0].hp * 100)}%` : "nothing - click a unit, or drag a box";
  $("salvage").disabled = !selB.some((b) => b.kind !== 0);
  $("fight").classList.toggle("on", fight);
  $("pause").textContent = w.paused ? "Resume" : "Pause";
  if (w.note && NOTES[w.note]) note(...NOTES[w.note]);
}

function note(text, bad) {
  const n = $("note");
  n.textContent = text; n.classList.add("on"); n.classList.toggle("bad", !!bad);
  clearTimeout(noteTimer); noteTimer = setTimeout(() => n.classList.remove("on"), 3200);
}

function buttons() {
  $("build").replaceChildren(...PLACE.map((p, i) => {
    const b = document.createElement("button");
    b.innerHTML = `${BUILDINGS[p.b]}<small>${p.cost} scrap</small>`; b.title = p.tip;
    b.addEventListener("click", () => { placing = placing === i ? null : i; });
    return b;
  }));
  $("train").replaceChildren(...TRAIN.map((t) => {
    const b = document.createElement("button");
    b.innerHTML = `${UNITS[t.u]}<small></small>`; b.title = `${t.tip} · from the ${BUILDINGS[t.from]} · takes one waiting survivor`;
    b.addEventListener("click", () => { if (wasm.game_train(game, t.i)) sound.play("click"); });
    return b;
  }));
  $("stop").addEventListener("click", () => wasm.game_stop(game));
  const mute = () => { sound.start(); sound.toggle(); $("mute").textContent = sound.on ? "On" : "Off"; $("mute").classList.toggle("on", sound.on); };
  $("mute").textContent = sound.on ? "On" : "Off"; $("mute").classList.toggle("on", sound.on);
  $("mute").addEventListener("click", mute);
  $("volume").value = Math.round(sound.level() * 100);
  $("volume").addEventListener("input", (e) => { sound.start(); sound.volume(Number(e.target.value) / 100); });
  window.addEventListener("keydown", (e) => { if (e.code === "KeyM" && e.target.tagName !== "INPUT") mute(); });
  // Any click in the panel is a gesture the browser accepts to start audio.
  document.querySelector(".panel").addEventListener("pointerdown", () => sound.start());
  $("salvage").addEventListener("click", () => wasm.game_salvage(game));
  $("fight").addEventListener("click", () => { fight = !fight; });
  $("pause").addEventListener("click", () => wasm.game_pause(game, world && world.paused ? 0 : 1));
}

// ---- input: tiles from pointer positions ---------------------------------------------------
function at(e) {
  const r = canvas.getBoundingClientRect();
  return { x: (e.clientX - r.left) / r.width * MW, y: (e.clientY - r.top) / r.height * MH };
}

function input() {
  canvas.addEventListener("contextmenu", (e) => e.preventDefault());
  canvas.addEventListener("pointermove", (e) => {
    hover = at(e);
    if (drag) { drag.x1 = hover.x; drag.y1 = hover.y; drag.moved = drag.moved || Math.hypot(drag.x1 - drag.x0, drag.y1 - drag.y0) > 0.4; }
  });
  canvas.addEventListener("pointerdown", (e) => {
    e.preventDefault();
    const p = at(e);
    sound.start();
    if (e.button === 2) { wasm.game_order(game, p.x, p.y, e.shiftKey || fight ? 1 : 0); sound.play("click"); return; }
    if (placing !== null) {
      const s = PLACE[placing];
      const ok = wasm.game_build(game, placing, Math.floor(p.x - s.w / 2 + 0.5), Math.floor(p.y - s.h / 2 + 0.5));
      if (ok) sound.play("place");
      if (ok && !e.shiftKey) placing = null;
      return;
    }
    drag = { x0: p.x, y0: p.y, x1: p.x, y1: p.y, moved: false, touch: e.pointerType === "touch" };
    canvas.setPointerCapture(e.pointerId);
  });
  canvas.addEventListener("pointerup", (e) => {
    if (!drag) return;
    const d = drag; drag = null;
    if (d.moved) { wasm.game_select(game, d.x0, d.y0, d.x1, d.y1, e.shiftKey ? 1 : 0); return; }
    // A click on one of yours selects it; with something selected, a click
    // anywhere else is an order - so a phone needs no right button.
    const mine = world && world.units.some((u) => u.side === HUMAN && Math.hypot(u.x - d.x0, u.y - d.y0) < 0.75)
      || world && world.buildings.some((b) => b.side === HUMAN && d.x0 >= b.tx && d.x0 < b.tx + b.w && d.y0 >= b.ty && d.y0 < b.ty + b.h);
    const any = world && world.units.some((u) => u.sel);
    if (!mine && any) wasm.game_order(game, d.x0, d.y0, e.shiftKey || fight ? 1 : 0);
    else wasm.game_select(game, d.x0, d.y0, d.x0, d.y0, e.shiftKey ? 1 : 0);
  });
  window.addEventListener("keydown", (e) => {
    if (e.target.tagName === "INPUT") return;
    if (e.code === "Space") { e.preventDefault(); wasm.game_pause(game, world && world.paused ? 0 : 1); }
    if (e.code === "Escape") { placing = null; wasm.game_select(game, -9, -9, -9, -9, 0); }
    if (e.code === "KeyS") wasm.game_stop(game);
    if (e.code === "KeyF") fight = !fight;
    if (/^Digit[1-7]$/.test(e.code)) { const i = Number(e.code.slice(5)) - 1; placing = placing === i ? null : i; }
  });
}

// ---- the start and the end -------------------------------------------------------------------
let shown = PLAYING, started = false;

function ending(w) {
  if (w.state === shown) return;
  shown = w.state;
  $("overlay").classList.remove("hidden");
  $("headline").textContent = w.state === WON ? "Both Cores are out. The city is quiet." : "The Haven has fallen.";
  $("message").textContent = `${w.nights} night${w.nights === 1 ? "" : "s"} held · ${w.kills} machines destroyed · ${w.turned} turned · score ${w.score.toLocaleString()}.`;
  $("start").textContent = "Try again";
  $("save").classList.toggle("hidden", w.score === 0);
  $("saved").classList.add("hidden");
  $("name").value = localStorage.getItem("signal-name") || "";
}

function board(rows, mine) {
  $("board").classList.toggle("hidden", !rows.length);
  $("rows").replaceChildren(...rows.slice(0, 5).map((r, i) => {
    const li = document.createElement("li");
    if (i + 1 === mine) li.className = "mine";
    const name = document.createElement("span"), score = document.createElement("b");
    name.textContent = `${i + 1}. ${r.name} · ${r.nights} night${r.nights === 1 ? "" : "s"}`;
    score.textContent = r.score.toLocaleString();
    li.append(name, score);
    return li;
  }));
}

$("save").addEventListener("submit", (e) => {
  e.preventDefault();
  const name = $("name").value, button = $("save").querySelector("button");
  localStorage.setItem("signal-name", name);
  button.disabled = true;
  fetch("/api/scores", { method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name, score: world.score, nights: world.nights, seconds: world.time }) })
    .then((r) => r.json()).then((r) => {
      $("save").classList.add("hidden"); $("saved").classList.remove("hidden");
      $("saved").textContent = r.kept ? `saved: #${r.rank}` : r.error || "not in the top 20";
      if (r.board) board(r.board, r.rank);
    }).catch(() => { $("saved").classList.remove("hidden"); $("saved").textContent = "the server did not answer"; })
    .finally(() => { button.disabled = false; });
});

$("env").addEventListener("toggle", () => {
  if (!$("env").open) return;
  fetch("/api/env").then((r) => r.json()).then((env) => {
    const rows = [
      ["crate", `${env.crate.name} ${env.crate.version}`],
      ["module", env.module.bytes ? `${env.module.file} · ${env.module.bytes.toLocaleString()} bytes · built ${env.module.built_s_ago} s ago · ${env.module.imports} imports` : "not built: ./build-web.sh"],
      ["Rust", env.stack.join(", ") || "not from the stack"],
      ["server", env.server.join(", ")],
      ["board", `${env.board.runs} runs in ${env.board.file}`],
      ["process", `pid ${env.process.pid} · up ${env.process.uptime_s} s`],
    ];
    $("env-rows").replaceChildren(...rows.map(([k, v]) => {
      const row = document.createElement("div"), a = document.createElement("span"), b = document.createElement("span");
      row.className = "env-row"; a.className = "k"; b.className = "v"; a.textContent = k; b.textContent = v;
      row.append(a, b); return row;
    }));
  }).catch(() => { $("env-rows").textContent = "/api/env did not answer"; });
});

async function main() {
  // The module imports nothing (game.rs has no dependencies), so {} is all
  // it needs; a growing import list here means a dependency crept in.
  const res = await fetch("/static/game.wasm");
  if (!res.ok) throw new Error((await res.json().catch(() => ({}))).error || `game.wasm: ${res.status}`);
  const { instance } = await WebAssembly.instantiateStreaming(res, {});
  wasm = instance.exports;
  const fresh = () => { if (game) wasm.game_free(game); game = wasm.game_new((Math.random() * 0xffffffff) >>> 0); terrainKey = ""; };
  fresh();
  wasm.game_pause(game, 1);
  buttons();
  fit();
  window.addEventListener("resize", fit);
  new ResizeObserver(fit).observe($("stage"));
  input();
  $("start").addEventListener("click", () => {
    sound.start();
    if (started && shown !== PLAYING) { fresh(); shown = PLAYING; }
    started = true;
    $("overlay").classList.add("hidden");
    wasm.game_pause(game, 0);
  });
  let prev = 0;
  const frame = (now) => {
    const dt = Math.min((now - prev) / 1000 || 0.016, 0.1);
    prev = now;
    wasm.game_update(game, dt);
    world = read();
    listen(world);
    render(world);
    hud(world);
    if (started) ending(world);
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}

fetch("/api/scores").then((r) => r.json()).then((rows) => board(rows, null)).catch(() => {});
main().catch((err) => {
  console.error(err);
  $("message").textContent = `${err.message}. Build it with ./build.sh (or ./build-web.sh), then reload.`;
  $("start").disabled = true;
  $("start").textContent = "WebAssembly build needed";
});
