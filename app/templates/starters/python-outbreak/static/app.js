// Builds the controls from GET /api/menu, asks the server for a few ticks at a
// time (one request in flight) and draws between answers: dots glide from the
// last frame to the next, so the map moves smoothly at any server latency.
"use strict";

const $ = (id) => document.getElementById(id);
const FRAME_MS = 66;                 // one step request per ~15 frames a second
const ORDER = ["S", "E", "I", "R", "V"];
const NAMES = { S: "susceptible", E: "incubating", I: "infectious", R: "recovered", V: "vaccinated" };

// One id per tab: the server keeps one simulation per id, so two tabs never
// step each other's epidemic.
const client = sessionStorage.getItem("outbreak-client") || (() => {
  const id = (crypto.randomUUID ? crypto.randomUUID() : Math.random().toString(36).slice(2) + Date.now().toString(36)).replace(/[^A-Za-z0-9_-]/g, "");
  sessionStorage.setItem("outbreak-client", id);
  return id;
})();

let menu = null, map = null, n = 0;
let seed = 1, disease = {}, policy = { distancing: 0, close_schools: false, close_markets: false, vaccination: 0, testing: 0 };
let running = false, ended = false, inflight = false, lastReq = 0;
let prev = null, next = null, st = null, tFrame = 0;   // positions (Float32Array, map units), states
let links = [];                                         // {a, b, t}: who infected whom, fading
let history = [], events = [], last = null, hover = null;
let run = 0;                                            // bumped per start, so a late step answer is dropped
let view = { s: 1, ox: 0, oy: 0, side: 0 };
let homesLayer = null;

// ---- server ----------------------------------------------------------------------------

async function api(path, body) {
  const r = await fetch(path, body === undefined ? {} : {
    method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body),
  });
  const d = await r.json();
  if (!r.ok) { const e = new Error(JSON.stringify(d.detail)); e.status = r.status; throw e; }
  return d;
}

const decode = (b64) => Uint8Array.from(atob(b64), (c) => c.charCodeAt(0));

function take(frame, glide) {
  const raw = new Uint16Array(decode(frame.xy).buffer);
  const xy = new Float32Array(raw.length);
  const k = map.world / 65535;
  for (let i = 0; i < raw.length; i++) xy[i] = raw[i] * k;
  prev = glide && next ? positions(performance.now()) : xy;
  next = xy;
  tFrame = performance.now();
  st = decode(frame.st);
  const now = performance.now();
  for (const [a, b] of frame.links) links.push({ a, b, t: now });
  history = glide ? history.concat(frame.history) : frame.history;
  last = frame;
  readout();
  drawCurve();
}

async function start() {
  const mine = ++run;
  const d = await api("/api/sim", { client, seed, policy, ...disease });
  if (mine !== run) return;
  map = d.map; n = d.params.n;
  links = []; events = []; ended = false;
  take(d.frame, false);
  layout();
  $("seed").textContent = seed;
}

async function step() {
  if (!running || inflight || !map) return;
  inflight = true; lastReq = performance.now();
  const mine = run;
  try {
    const f = await api(`/api/sim/${client}/step`, { ticks: +$("speed").value, since: history.length });
    if (mine !== run) return;
    take(f, true);
    if (f.now.E + f.now.I === 0) { running = false; ended = true; }
  } catch (e) {
    // 404: the server dropped this run to make room for newer ones; replay it.
    if (e.status === 404) await start(); else { console.error(e); running = false; }
  } finally {
    inflight = false;
    buttons();
  }
}

let policyTimer = 0;
function policyChanged(label) {
  clearTimeout(policyTimer);
  policyTimer = setTimeout(async () => {
    if (!map) return;
    try {
      const r = await api(`/api/sim/${client}/policy`, policy);
      const day = r.tick / menu.ticks_per_day;
      events = events.filter((e) => e.day !== day).concat({ day, label });
      drawCurve();
    } catch (e) { if (e.status === 404) await start(); else console.error(e); }
  }, 120);
}

let diseaseTimer = 0;
function diseaseChanged() {
  clearTimeout(diseaseTimer);
  diseaseTimer = setTimeout(start, 250);
}

// ---- controls --------------------------------------------------------------------------

function fmt(spec, v) {
  if (spec.unit === "%") return `${+(v * 100).toFixed(spec.step < 0.01 ? 2 : 0)}%`;
  if (spec.unit) return `${v} ${spec.unit}`;
  return spec.integer ? String(Math.round(v)) : String(v);
}

function slider(spec, value, onInput) {
  const label = document.createElement("label");
  label.className = "row";
  label.innerHTML = `${spec.label} <output>${fmt(spec, value)}</output>` +
    `<input type="range" min="${spec.min}" max="${spec.max}" step="${spec.step}" value="${value}" aria-label="${spec.label}">`;
  const input = label.querySelector("input"), out = label.querySelector("output");
  input.addEventListener("input", () => { const v = +input.value; out.textContent = fmt(spec, v); onInput(v, out.textContent); });
  return label;
}

function buttons() {
  $("run").textContent = ended ? "Ended" : running ? "Pause" : last && last.tick > 0 ? "Resume" : "Start";
}

function legend() {
  const c = menu.colors;
  const shape = {
    S: `<rect x="3" y="3" width="6" height="6" fill="${c.S}"/>`,
    E: `<circle cx="6" cy="6" r="4" fill="none" stroke="${c.E}" stroke-width="2"/>`,
    I: `<circle cx="6" cy="6" r="6" fill="${c.I}" opacity=".25"/><circle cx="6" cy="6" r="3.5" fill="${c.I}"/>`,
    R: `<circle cx="6" cy="6" r="3" fill="${c.R}"/>`,
    V: `<rect x="2" y="2" width="8" height="8" fill="${c.V}"/>`,
  };
  $("legend").innerHTML = ORDER.map((k) => `<span><svg width="12" height="12">${shape[k]}</svg>${NAMES[k]}</span>`).join("") +
    `<span><svg width="12" height="12"><circle cx="6" cy="6" r="5" fill="none" stroke="#fff" stroke-width="1.2"/></svg>isolating</span>`;
}

function readout() {
  if (!last) return;
  const pct = (x) => `${(100 * x / n).toFixed(x && x < n / 100 ? 1 : 0)}%`;
  $("day").textContent = Math.floor(last.day);
  $("now-i").textContent = last.now.I;
  $("ever").textContent = pct(last.ever);
  const pk = history.reduce((m, h) => (h.I > (m ? m.I : -1) ? h : m), null);
  $("peak").textContent = pk ? `${pk.I} · d${pk.day}` : "–";
  $("r-early").textContent = last.r_early == null ? "–" : last.r_early.toFixed(2);
  $("r-recent").textContent = last.r_recent == null ? "–" : last.r_recent.toFixed(2);
}

// ---- the map ---------------------------------------------------------------------------

const mapCanvas = $("map"), mctx = mapCanvas.getContext("2d");
const curveCanvas = $("curve"), cctx = curveCanvas.getContext("2d");
const DPR = () => Math.min(2, window.devicePixelRatio || 1);

function layout() {
  const dpr = DPR(), W = innerWidth, H = innerHeight;
  mapCanvas.width = W * dpr; mapCanvas.height = H * dpr;
  mctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  // the map fills what the glass cards leave free
  const phone = matchMedia("(max-width: 860px)").matches;
  const left = phone ? 10 : $("panel").getBoundingClientRect().right + 18;
  const t = document.querySelector(".readout").getBoundingClientRect().bottom + 12;
  const b = document.querySelector(".curve").getBoundingClientRect().top - 12;
  const r = W - (phone ? 10 : 18);
  const side = Math.max(120, Math.min(r - left, b - t));
  view = { side, s: side / (map ? map.world : 100), ox: left + (r - left - side) / 2, oy: t + (b - t - side) / 2 };

  const cr = curveCanvas.getBoundingClientRect();
  curveCanvas.width = cr.width * dpr; curveCanvas.height = cr.height * dpr;
  cctx.setTransform(dpr, 0, 0, dpr, 0, 0);

  homesLayer = null;
  drawCurve();
}

function homes() {
  // Households never move: drawn once per layout into a layer, not every frame.
  if (homesLayer || !map) return homesLayer;
  const dpr = DPR();
  homesLayer = document.createElement("canvas");
  homesLayer.width = mapCanvas.width; homesLayer.height = mapCanvas.height;
  const g = homesLayer.getContext("2d");
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.fillStyle = "rgba(255,255,255,.07)";
  const h = map.homes, s = view.s, sz = Math.max(2, s * 0.9);
  for (let i = 0; i < h.length; i += 2) g.fillRect(view.ox + h[i] * s - sz / 2, view.oy + h[i + 1] * s - sz / 2, sz, sz);
  return homesLayer;
}

function positions(now) {
  const a = Math.min(1, (now - tFrame) / FRAME_MS);
  if (a >= 1 || !prev || prev.length !== next.length) return next;
  const out = new Float32Array(next.length);
  for (let i = 0; i < next.length; i++) out[i] = prev[i] + (next[i] - prev[i]) * a;
  return out;
}

function drawMap(now) {
  const W = innerWidth, H = innerHeight;
  mctx.clearRect(0, 0, W, H);
  if (!map || !next) return;
  const { s, ox, oy, side } = view, c = menu.colors;
  const X = (x) => ox + x * s, Y = (y) => oy + y * s;

  mctx.strokeStyle = "rgba(255,255,255,.08)"; mctx.lineWidth = 1;
  mctx.beginPath(); mctx.roundRect(ox - 6, oy - 6, side + 12, side + 12, 14); mctx.stroke();
  mctx.drawImage(homes(), 0, 0, W, H);

  // hubs: the spread people stand over, wider under distancing; dashed when closed
  mctx.font = "500 12px system-ui, sans-serif"; mctx.textAlign = "center";
  for (const hub of map.hubs) {
    const closed = hub.kind === "school" ? policy.close_schools : policy.close_markets;
    const r = map.hub_radius * (1 + policy.distancing) * s;
    mctx.beginPath(); mctx.arc(X(hub.x), Y(hub.y), r, 0, 2 * Math.PI);
    mctx.fillStyle = closed ? "rgba(255,255,255,.015)" : "rgba(255,255,255,.045)"; mctx.fill();
    mctx.setLineDash(closed ? [4, 4] : []); mctx.strokeStyle = "rgba(255,255,255,.18)"; mctx.stroke(); mctx.setLineDash([]);
    mctx.fillStyle = "rgba(232,236,246,.55)";
    mctx.fillText(closed ? `${hub.kind} · closed` : hub.kind, X(hub.x), Y(hub.y) - r - 5);
  }

  const p = positions(now);
  const u = Math.max(1, side / 560);

  // transmission tree: a line from infector to infectee, fading over 1.5 s
  links = links.filter((l) => now - l.t < 1500);
  mctx.lineWidth = 1.2 * u;
  for (const l of links) {
    mctx.strokeStyle = `rgba(230,103,103,${0.7 * (1 - (now - l.t) / 1500)})`;
    mctx.beginPath(); mctx.moveTo(X(p[2 * l.a]), Y(p[2 * l.a + 1])); mctx.lineTo(X(p[2 * l.b]), Y(p[2 * l.b + 1])); mctx.stroke();
  }

  // one path per state; shape and size carry state as well as colour
  const by = [[], [], [], [], []], iso = [];
  for (let i = 0; i < st.length; i++) { by[st[i] & 7].push(i); if (st[i] & 8) iso.push(i); }
  const sq = (ids, color, w) => { mctx.fillStyle = color; for (const i of ids) mctx.fillRect(X(p[2 * i]) - w / 2, Y(p[2 * i + 1]) - w / 2, w, w); };
  const dots = (ids, r) => { mctx.beginPath(); for (const i of ids) { const x = X(p[2 * i]), y = Y(p[2 * i + 1]); mctx.moveTo(x + r, y); mctx.arc(x, y, r, 0, 2 * Math.PI); } };
  sq(by[0], c.S, 2 * u);
  sq(by[4], c.V, 2.8 * u);
  dots(by[3], 1.6 * u); mctx.fillStyle = c.R; mctx.fill();
  dots(by[1], 2.6 * u); mctx.strokeStyle = c.E; mctx.lineWidth = 1.4 * u; mctx.stroke();
  dots(by[2], 6 * u); mctx.fillStyle = "rgba(230,103,103,.18)"; mctx.fill();
  dots(by[2], 3 * u); mctx.fillStyle = c.I; mctx.fill();
  dots(iso, 5 * u); mctx.strokeStyle = "rgba(255,255,255,.85)"; mctx.lineWidth = 1; mctx.stroke();
}

// ---- the epidemic curve ----------------------------------------------------------------

function drawCurve() {
  if (!menu) return;
  const r = curveCanvas.getBoundingClientRect(), W = r.width, H = r.height;
  cctx.clearRect(0, 0, W, H);
  const L = 40, R = 96, T = 10, B = 22, w = W - L - R, h = H - T - B;
  if (w < 40 || h < 40) return;
  const days = Math.max(60, Math.ceil(((history.length ? history[history.length - 1].day : 0) + 5) / 10) * 10);
  const X = (d) => L + (d / days) * w, Y = (v) => T + h - (v / (n || 1)) * h;

  cctx.font = "12px system-ui, sans-serif"; cctx.fillStyle = "#9aa6c2"; cctx.strokeStyle = "rgba(255,255,255,.07)"; cctx.lineWidth = 1;
  cctx.textAlign = "right"; cctx.textBaseline = "middle";
  for (const q of [0, 0.25, 0.5, 0.75, 1]) {
    cctx.beginPath(); cctx.moveTo(L, Y(q * n)); cctx.lineTo(L + w, Y(q * n)); cctx.stroke();
    cctx.fillText(`${q * 100}%`, L - 6, Y(q * n));
  }
  cctx.textAlign = "center"; cctx.textBaseline = "top";
  const every = days > 150 ? 30 : days > 80 ? 20 : 10;
  for (let d = 0; d <= days; d += every) cctx.fillText(d ? `d${d}` : "day 0", X(d), T + h + 6);

  // interventions: a dashed rule on the day they changed
  cctx.setLineDash([3, 4]); cctx.strokeStyle = "rgba(255,255,255,.28)";
  cctx.textAlign = "left"; cctx.textBaseline = "top"; cctx.fillStyle = "#9aa6c2";
  for (const e of events) {
    cctx.beginPath(); cctx.moveTo(X(e.day), T); cctx.lineTo(X(e.day), T + h); cctx.stroke();
    cctx.fillText(e.label, X(e.day) + 4, T + 2);
  }
  cctx.setLineDash([]);
  if (!history.length) return;

  const c = menu.colors;
  cctx.lineWidth = 2; cctx.lineJoin = "round";
  for (const k of ORDER) {
    cctx.strokeStyle = c[k]; cctx.beginPath();
    cctx.moveTo(X(0), Y(k === "S" ? n - (history[0].I + history[0].E) : 0));
    for (const row of history) cctx.lineTo(X(row.day), Y(row[k]));
    cctx.stroke();
  }

  // direct labels at the line ends, nudged apart so none overlap
  const end = history[history.length - 1];
  const labels = ORDER.map((k) => ({ k, y: Y(end[k]) })).sort((a, b) => a.y - b.y);
  // push down from the top, then up from the bottom: only the crowded ones move
  for (let i = 0; i < labels.length; i++) labels[i].y = Math.max(labels[i].y, i ? labels[i - 1].y + 13 : T + 6);
  for (let i = labels.length - 1; i >= 0; i--) labels[i].y = Math.min(labels[i].y, i < labels.length - 1 ? labels[i + 1].y - 13 : T + h);
  cctx.textBaseline = "middle";
  for (const l of labels) {
    const x = X(end.day) + 8;
    cctx.fillStyle = c[l.k]; cctx.beginPath(); cctx.arc(x + 3, l.y, 3, 0, 2 * Math.PI); cctx.fill();
    cctx.fillStyle = "#e8ecf6"; cctx.fillText(`${NAMES[l.k]} ${end[l.k]}`, x + 10, l.y);
  }

  if (hover != null) {
    const row = history[Math.max(0, Math.min(history.length - 1, Math.round(hover) - 1))];
    cctx.strokeStyle = "rgba(255,255,255,.4)"; cctx.beginPath(); cctx.moveTo(X(row.day), T); cctx.lineTo(X(row.day), T + h); cctx.stroke();
    for (const k of ORDER) { cctx.fillStyle = c[k]; cctx.beginPath(); cctx.arc(X(row.day), Y(row[k]), 4, 0, 2 * Math.PI); cctx.fill(); }
    const tip = $("tip");
    tip.hidden = false;
    tip.innerHTML = `<b>day ${row.day}</b> · ${row.new} new<br>` +
      ORDER.map((k) => `<i style="background:${c[k]}"></i>${NAMES[k]} <b>${row[k]}</b>`).join("<br>") +
      (row.isolated ? `<br>isolating <b>${row.isolated}</b>` : "");
    const tx = X(row.day) + 14;
    tip.style.left = `${tx + tip.offsetWidth > W ? X(row.day) - tip.offsetWidth - 4 : tx}px`;
  } else $("tip").hidden = true;
}

curveCanvas.addEventListener("pointermove", (e) => {
  if (!history.length) return;
  const r = curveCanvas.getBoundingClientRect(), w = r.width - 40 - 96;
  const days = Math.max(60, Math.ceil((history[history.length - 1].day + 5) / 10) * 10);
  hover = ((e.clientX - r.left - 40) / w) * days;
  drawCurve();
});
curveCanvas.addEventListener("pointerleave", () => { hover = null; drawCurve(); });

// ---- wiring ----------------------------------------------------------------------------

function frame(now) {
  if (running && !inflight && now - lastReq >= FRAME_MS) step();
  drawMap(now);
  requestAnimationFrame(frame);
}

$("run").addEventListener("click", async () => {
  if (ended) await start();      // an ended run starts over from the same seed
  running = !running;
  buttons();
});
$("reset").addEventListener("click", async () => { await start(); buttons(); });
$("reseed").addEventListener("click", async () => { seed = 1 + Math.floor(Math.random() * 999_999); await start(); buttons(); });
$("speed").addEventListener("input", () => {
  const t = +$("speed").value;
  $("speed-v").textContent = `${(t * 1000 / FRAME_MS / menu.ticks_per_day).toFixed(1)} days/s`;
});
$("closures").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  policy[b.dataset.k] = !policy[b.dataset.k];
  b.classList.toggle("on", policy[b.dataset.k]);
  const what = b.dataset.k === "close_schools" ? "schools" : "markets";
  policyChanged(`${what} ${policy[b.dataset.k] ? "closed" : "open"}`);
});
$("panel").querySelector("summary").addEventListener("click", (e) => {
  if (!matchMedia("(max-width: 860px)").matches) e.preventDefault();   // folds on a phone only
});
$("panel").addEventListener("toggle", layout);
addEventListener("resize", layout);

api("/api/menu").then(async (m) => {
  menu = m;
  for (const [k, spec] of Object.entries(m.disease)) {
    disease[k] = spec.default;
    $("disease").append(slider(spec, spec.default, (v) => { disease[k] = v; diseaseChanged(); }));
  }
  for (const [k, spec] of Object.entries(m.policy)) {
    policy[k] = spec.default;
    $("policy").append(slider(spec, spec.default, (v, shown) => { policy[k] = v; policyChanged(`${spec.short} ${shown}`); }));
  }
  $("speed").dispatchEvent(new Event("input"));
  legend();
  if (matchMedia("(max-width: 860px)").matches) $("panel").open = false;
  await start();
  buttons();
  requestAnimationFrame(frame);
});
