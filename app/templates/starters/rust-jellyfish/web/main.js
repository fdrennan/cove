// Jellyfish Drift, the page: draws what the Rust game (src/game.rs, compiled
// to /static/game.wasm by build-web.sh) says the world is.
//
// No wasm-bindgen and no bundler. The module exports five functions; each
// frame the page calls game_update, then reads the whole world as one
// Float64Array straight out of the module's memory (game_frame). The layout
// is game.rs `frame`: HEAD floats, then x, center, gap per gate, then x, y
// per pearl.
"use strict";

const W = 960, H = 560, TAU = Math.PI * 2, HEAD = 11, PLAYER_X = 230;
const READY = 0, PLAYING = 1, OVER = 2;
const $ = (id) => document.getElementById(id);
const canvas = $("ocean"), ctx = canvas.getContext("2d");
const overlay = $("overlay"), start = $("start");

// ---- drawing ----------------------------------------------------------------
function jelly(x, y, t, velocity) {
  ctx.save(); ctx.translate(x, y); ctx.rotate(Math.max(-0.23, Math.min(0.23, velocity / 1400)));
  const beat = 1 + Math.sin(t * 8) * 0.075; ctx.scale(beat, 1 / beat);
  ctx.shadowBlur = 38; ctx.shadowColor = "#f19dff"; ctx.fillStyle = "#c59afb";
  ctx.beginPath(); ctx.moveTo(-35, 5); ctx.bezierCurveTo(-38, -44, 34, -47, 35, 5); ctx.quadraticCurveTo(20, 23, 0, 13); ctx.quadraticCurveTo(-21, 24, -35, 5); ctx.fill();
  ctx.shadowBlur = 0;
  ctx.fillStyle = "#f4ceff"; ctx.beginPath(); ctx.ellipse(0, -14, 24, 14, 0, Math.PI, TAU); ctx.fill();
  ctx.strokeStyle = "#bd8df2"; ctx.lineWidth = 5; ctx.lineCap = "round";
  for (let i = -2; i <= 2; i++) {
    ctx.beginPath(); ctx.moveTo(i * 11, 12);
    ctx.bezierCurveTo(i * 11 + Math.sin(t * 5 + i) * 10, 35, i * 11 - Math.cos(t * 6 + i) * 12, 47, i * 10 + Math.sin(t * 4 + i) * 12, 66);
    ctx.stroke();
  }
  ctx.fillStyle = "#37275a";
  for (const i of [-10, 10]) { ctx.beginPath(); ctx.arc(i, -5, 3, 0, TAU); ctx.fill(); }
  ctx.strokeStyle = "#482b73"; ctx.lineWidth = 2; ctx.beginPath(); ctx.arc(0, -2, 5, 0, Math.PI); ctx.stroke();
  ctx.restore();
}

function coral(x, center, gap) {
  const top = center - gap / 2, bottom = center + gap / 2;
  ctx.lineWidth = 5; ctx.lineCap = "round";
  for (const [a, b, dir] of [[0, top, 1], [H, bottom, -1]]) {
    ctx.fillStyle = "#8b376f"; ctx.fillRect(x - 31, dir === 1 ? 0 : b, 62, dir === 1 ? b : H - b);
    ctx.strokeStyle = "#e87aa8"; ctx.beginPath(); ctx.moveTo(x, a); ctx.lineTo(x, b);
    for (let j = 0; j < 4; j++) {
      const yy = a + (b - a) * (j + 1) / 5;
      ctx.moveTo(x, yy); ctx.lineTo(x + (j % 2 ? 26 : -24), yy + (dir === 1 ? -24 : 24));
    }
    ctx.stroke();
  }
  ctx.fillStyle = "#ffd4ee";
  for (let i = 0; i < 4; i++) { ctx.beginPath(); ctx.arc(x + Math.sin(i * 2) * 10, top - 9 - i * 2, 2.2, 0, TAU); ctx.fill(); }
}

function pearl(x, y, t) {
  ctx.save(); ctx.translate(x, y);
  ctx.shadowColor = "#8cfcff"; ctx.shadowBlur = 25; ctx.fillStyle = "#96fdff";
  ctx.beginPath(); ctx.arc(0, 0, 12 + Math.sin(t * 5) * 2, 0, TAU); ctx.fill();
  ctx.shadowBlur = 0; ctx.fillStyle = "#ffffff"; ctx.beginPath(); ctx.arc(-4, -4, 4, 0, TAU); ctx.fill();
  ctx.restore();
}

function background(t) {
  const gr = ctx.createLinearGradient(0, 0, 0, H);
  gr.addColorStop(0, "#071a55"); gr.addColorStop(0.55, "#0d386e"); gr.addColorStop(1, "#081b41");
  ctx.fillStyle = gr; ctx.fillRect(0, 0, W, H);
  for (let i = 0; i < 7; i++) {
    ctx.save(); ctx.translate(80 + i * 155, 0); ctx.rotate(0.2 + Math.sin(t * 0.15 + i) * 0.03);
    ctx.fillStyle = "#69cfff0a"; ctx.beginPath(); ctx.moveTo(0, 0); ctx.lineTo(64, H); ctx.lineTo(152, H); ctx.lineTo(22, 0); ctx.fill();
    ctx.restore();
  }
  for (let i = 0; i < 45; i++) {
    const x = ((i * 109 - t * (13 + i % 5 * 6)) % 1080 + 1080) % 1080, y = (i * 149) % 560, r = 2 + i % 4;
    ctx.strokeStyle = "#86baff48"; ctx.lineWidth = 1.4; ctx.beginPath(); ctx.arc(x, y + Math.sin(t * 2 + i) * 5, r, 0, TAU); ctx.stroke();
  }
  ctx.fillStyle = "#061431"; ctx.beginPath(); ctx.moveTo(0, H);
  for (let x = 0; x <= W; x += 20) ctx.lineTo(x, H - 25 + 8 * Math.sin(x * 0.03));
  ctx.lineTo(W, H); ctx.fill();
  for (let i = 0; i < 16; i++) {
    const x = i * 73 + 10;
    ctx.strokeStyle = i % 2 ? "#18667f" : "#357e91"; ctx.lineWidth = 4; ctx.beginPath(); ctx.moveTo(x, H - 19);
    ctx.quadraticCurveTo(x + Math.sin(t + i) * 14, H - 60, x + Math.sin(t + i) * 21, H - 75 - (i % 3) * 14); ctx.stroke();
  }
  ctx.fillStyle = "#b7c3ff88"; ctx.font = "600 13px system-ui"; ctx.fillText("THE DEEP BLUE • AN UNDERWATER ADVENTURE", 24, 32);
}

// ---- the module -------------------------------------------------------------
let wasm, game;

// The world this frame, as plain numbers.
function read() {
  const ptr = wasm.game_frame(game);
  const len = wasm.game_frame_len(game);
  // A fresh view every frame: the module's memory can grow, and a grown
  // memory detaches every view made on the old one.
  const f = new Float64Array(wasm.memory.buffer, ptr, len);
  const gates = f[7], pearls = f[8];
  const w = { y: f[0], velocity: f[1], time: f[2], score: f[3], best: f[4], lives: f[5], state: f[6], taken: f[9], played: f[10], gates: [], pearls: [] };
  let i = HEAD;
  for (let k = 0; k < gates; k++, i += 3) w.gates.push([f[i], f[i + 1], f[i + 2]]);
  for (let k = 0; k < pearls; k++, i += 2) w.pearls.push([f[i], f[i + 1]]);
  return w;
}

function render(w) {
  background(w.time);
  for (const [x, c, g] of w.gates) coral(x, c, g);
  for (const [x, y] of w.pearls) pearl(x, y, w.time);
  jelly(PLAYER_X, w.y, w.time, w.velocity);
}

// ---- the frame around the canvas ----------------------------------------------
let shown = -1, last = null;
const storedBest = () => Number(localStorage.getItem("jelly-best") || 0);

function hud(w) {
  last = w;
  const best = Math.max(w.best, storedBest());
  if (w.best > storedBest()) localStorage.setItem("jelly-best", String(w.best));
  $("score").textContent = w.score;
  $("pearls").textContent = w.taken;
  $("best").textContent = best;
  const lives = Math.max(0, w.lives);
  $("hearts").textContent = "♥".repeat(lives) + "♡".repeat(3 - lives);
  if (w.state === shown) return;
  shown = w.state;
  overlay.classList.toggle("hidden", w.state === PLAYING);
  if (w.state === OVER) {
    $("headline").textContent = "The ocean wins this round!";
    $("message").textContent = `You earned ${w.score} points and ${w.taken} pearls. Drift again?`;
    start.textContent = "Try again ↻";
    $("save").classList.toggle("hidden", w.score === 0);
    $("saved").classList.add("hidden");
    $("name").value = localStorage.getItem("jelly-name") || "";
  } else {
    $("save").classList.add("hidden");
  }
}

function board(rows, mine) {
  $("board").classList.toggle("hidden", !rows.length);
  const ol = $("rows");
  ol.replaceChildren(...rows.slice(0, 5).map((r, i) => {
    const li = document.createElement("li");
    if (i + 1 === mine) li.className = "mine";
    const name = document.createElement("span"), score = document.createElement("b");
    name.textContent = `${i + 1}. ${r.name}`;
    score.textContent = r.score.toLocaleString();
    li.append(name, score);
    return li;
  }));
}

$("save").addEventListener("submit", (e) => {
  e.preventDefault();
  const name = $("name").value;
  localStorage.setItem("jelly-name", name);
  const button = $("save").querySelector("button");
  button.disabled = true;
  fetch("/api/scores", {
    method: "POST", headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ name, score: last.score, pearls: last.taken, seconds: last.played }),
  }).then((r) => r.json()).then((r) => {
    $("save").classList.add("hidden");
    $("saved").classList.remove("hidden");
    $("saved").textContent = r.kept ? `saved: #${r.rank}` : r.error || "not in the top 20";
    if (r.board) board(r.board, r.rank);
  }).catch(() => { $("saved").classList.remove("hidden"); $("saved").textContent = "the server did not answer"; })
    .finally(() => { button.disabled = false; });
});
// Typing a name is not a pulse.
for (const ev of ["pointerdown", "keydown"]) $("save").addEventListener(ev, (e) => e.stopPropagation());

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
      row.className = "env-row"; a.className = "k"; b.className = "v";
      a.textContent = k; b.textContent = v;
      row.append(a, b);
      return row;
    }));
  }).catch(() => { $("env-rows").textContent = "/api/env did not answer"; });
});

// ---- start --------------------------------------------------------------------
function pulse() {
  wasm.game_pulse(game);
  hud(read());
}

async function main() {
  // The module imports nothing (game.rs has no dependencies), so {} is all
  // it needs; a growing import list here means a dependency crept in.
  const res = await fetch("/static/game.wasm");
  if (!res.ok) throw new Error((await res.json().catch(() => ({}))).error || `game.wasm: ${res.status}`);
  const { instance } = await WebAssembly.instantiateStreaming(res, {});
  wasm = instance.exports;
  game = wasm.game_new((Math.random() * 0xffffffff) >>> 0);

  canvas.addEventListener("pointerdown", (e) => { e.preventDefault(); pulse(); });
  start.addEventListener("click", (e) => { e.stopPropagation(); pulse(); });
  window.addEventListener("keydown", (e) => {
    if (["Space", "ArrowUp", "KeyW"].includes(e.code)) { e.preventDefault(); if (!e.repeat) pulse(); }
  });
  let prev = 0;
  const frame = (now) => {
    const dt = Math.min((now - prev) / 1000 || 0.016, 0.034);
    prev = now;
    wasm.game_update(game, dt);
    const w = read();
    render(w);
    hud(w);
    requestAnimationFrame(frame);
  };
  requestAnimationFrame(frame);
}

fetch("/api/scores").then((r) => r.json()).then((rows) => board(rows, null)).catch(() => {});
main().catch((err) => {
  console.error(err);
  $("message").textContent = `${err.message}. Build it with ./build.sh (or ./build-web.sh), then reload.`;
  start.disabled = true;
  start.textContent = "WebAssembly build needed";
});
