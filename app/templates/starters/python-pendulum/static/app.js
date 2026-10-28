// Builds the controls from GET api/menu, integrates on load, then re-integrates
// on every change (debounced, one request in flight). The server sends angles at
// 30 frames a second; the page interpolates between them at the display's rate,
// so the motion is smooth without the server sending 60 or 120 frames.
"use strict";

const $ = (id) => document.getElementById(id);
const LN10 = Math.log(10), FLOOR = -15.9;   // the server floors log10 values at -16
const C_NEAR = "#4cc3df", C_MED = "#b49cf0", C_FIT = "#e8ecf6", C_EN = "#ef9a5c";
const state = { method: "DOP853", values: {} };
let run = null, busy = false, queued = false, timer = 0;
let simT = 0, playing = true, last = 0, hover = { divC: null, enC: null };

const sup = (k) => String(k).replace(/-/g, "⁻").replace(/\d/g, (d) => "⁰¹²³⁴⁵⁶⁷⁸⁹"[d]);
const decade = (v) => `10${sup(v % 1 ? v.toFixed(1) : v)}`;

// How each slider reads: log10 sliders as powers of ten, angles in degrees.
const show = (name, v) => {
  if (name === "spread" || name === "rtol") return decade(v);
  if (name.startsWith("theta")) return `${Math.round(v)}°`;
  if (name === "n" || name === "duration") return String(Math.round(v)) + (name === "duration" ? " s" : "");
  return (+v).toFixed(name.startsWith("m") ? 1 : 2);
};

function slider(name, spec) {
  const label = document.createElement("label");
  label.className = "row";
  label.innerHTML = `${spec.label.replace(/ \(.*\)$/, "")} <output>${show(name, spec.default)}</output>` +
    `<input type="range" min="${spec.min}" max="${spec.max}" step="${spec.step}" value="${spec.default}" aria-label="${spec.label}">`;
  const input = label.querySelector("input"), out = label.querySelector("output");
  input.addEventListener("input", () => { state.values[name] = +input.value; out.textContent = show(name, +input.value); changed(); });
  state.values[name] = spec.default;
  return label;
}

// ---- talking to the server ---------------------------------------------------------------

function decode(b64) {
  const bin = atob(b64), bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new Float32Array(bytes.buffer);
}

async function integrate() {
  if (busy) { queued = true; return; }
  busy = true;
  document.body.classList.add("busy");
  $("run").textContent = "Integrating…";
  try {
    const r = await fetch("api/run", { method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ ...state.values, method: state.method }) });
    const d = await r.json();
    if (!r.ok) throw new Error(typeof d.detail === "string" ? d.detail : JSON.stringify(d.detail));
    d.angles = decode(d.angles);
    run = d; simT = 0;
    const lam = d.lyapunov;
    $("lam").textContent = lam ? lam.lambda.toFixed(2) : "–";
    $("dbl").textContent = lam && lam.doubling ? `${lam.doubling.toFixed(2)} s` : "–";
    $("drift").textContent = d.energy.max_drift.toExponential(1);
    $("sub").textContent = `${d.n} pendulums, ${decade(state.values.spread)} rad apart in all`;
    $("foot").className = "foot";
    $("foot").textContent = `${d.method} at rtol ${d.rtol.toExponential(0)}: ${d.nfev.toLocaleString()} evaluations, ` +
      `${d.solve_ms < 1000 ? Math.round(d.solve_ms) + " ms" : (d.solve_ms / 1000).toFixed(1) + " s"} on the server`;
    $("div-legend").innerHTML =
      `<span><i style="border-color:${C_NEAR}"></i>nearest start</span>` +
      `<span><i style="border-color:${C_MED}"></i>median of ${d.n - 1}</span>` +
      (lam ? `<span><i class="dash" style="border-color:${C_FIT}"></i>e<sup>λt</sup> fit</span>` : "");
  } catch (e) {
    $("foot").className = "foot err";
    $("foot").textContent = `integration failed: ${e.message}`;
  } finally {
    busy = false;
    document.body.classList.remove("busy");
    $("run").textContent = "Integrate";
    if (queued) { queued = false; integrate(); }
  }
}

function changed() {
  clearTimeout(timer);
  timer = setTimeout(integrate, 400);
}

// ---- drawing -------------------------------------------------------------------------

function fit(canvas) {
  const dpr = window.devicePixelRatio || 1, w = canvas.clientWidth, h = canvas.clientHeight;
  if (canvas.width !== Math.round(w * dpr) || canvas.height !== Math.round(h * dpr)) {
    canvas.width = Math.round(w * dpr); canvas.height = Math.round(h * dpr);
  }
  const g = canvas.getContext("2d");
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  return [g, w, h];
}

// Angles of pendulum k at time t, interpolated between the two frames around it.
// The integrator's angles are continuous (never wrapped to ±π), so a straight
// blend between neighbouring frames is exact enough at 30 frames a second.
function angles(t, k) {
  const f = Math.min(t * run.fps, run.frames - 1), i = Math.floor(f), j = Math.min(i + 1, run.frames - 1), u = f - i;
  const A = run.angles, n = run.n, p = (i * n + k) * 2, q = (j * n + k) * 2;
  return [A[p] + (A[q] - A[p]) * u, A[p + 1] + (A[q + 1] - A[p + 1]) * u];
}

function drawStage(now) {
  const canvas = $("stage"), [g, w, h] = fit(canvas);
  g.clearRect(0, 0, w, h);
  if (!run) return;
  // The charts cover the bottom, and on a phone the title covers the top: centre in what is left.
  const top = w < 760 ? document.querySelector(".title").getBoundingClientRect().bottom + 6 : 0;
  const bottom = document.querySelector(".charts").getBoundingClientRect().top - 6;
  const reach = run.l1 + run.l2, R = Math.min(w * 0.44, (bottom - top) * 0.47);
  const s = R / reach, cx = w / 2, cy = (top + bottom) / 2;
  const tip = (a, b) => [cx + s * (run.l1 * Math.sin(a) + run.l2 * Math.sin(b)), cy + s * (run.l1 * Math.cos(a) + run.l2 * Math.cos(b))];
  const n = run.n, hue = (k) => 190 + 150 * k / Math.max(1, n - 1);   // cyan → violet → rose across the fan
  const alpha = Math.max(0.25, Math.min(0.9, 6 / n));

  // trails: the tip over the last 1.2 s, three passes from faint to bright
  const trail = 1.2, step = 1 / run.fps;
  for (const [from, to, a] of [[trail, trail * 0.55, 0.12], [trail * 0.55, trail * 0.2, 0.25], [trail * 0.2, 0, 0.45]]) {
    for (let k = n - 1; k >= 0; k--) {
      g.strokeStyle = `hsla(${hue(k)}, 85%, 62%, ${a})`;
      g.lineWidth = 1.4;
      g.beginPath();
      const t0 = Math.max(0, now - from), t1 = Math.max(0, now - to);
      g.moveTo(...tip(...angles(t0, k)));
      for (let t = t0 + step; t < t1; t += step) g.lineTo(...tip(...angles(t, k)));
      g.lineTo(...tip(...angles(t1, k)));
      g.stroke();
    }
  }
  // rods and bobs, reference (k = 0) drawn last so it sits on top
  const r1 = 3 + 3 * Math.sqrt(run.m1), r2 = 3 + 3 * Math.sqrt(run.m2);
  for (let k = n - 1; k >= 0; k--) {
    const [a, b] = angles(now, k);
    const x1 = cx + s * run.l1 * Math.sin(a), y1 = cy + s * run.l1 * Math.cos(a), [x2, y2] = tip(a, b);
    g.strokeStyle = `hsla(${hue(k)}, 60%, 80%, ${alpha})`;
    g.lineWidth = k === 0 ? 2 : 1.2;
    g.beginPath(); g.moveTo(cx, cy); g.lineTo(x1, y1); g.lineTo(x2, y2); g.stroke();
    g.fillStyle = `hsla(${hue(k)}, 85%, 64%, ${Math.min(1, alpha + 0.2)})`;
    g.beginPath(); g.arc(x1, y1, r1, 0, 2 * Math.PI); g.fill();
    g.beginPath(); g.arc(x2, y2, r2, 0, 2 * Math.PI); g.fill();
  }
  g.fillStyle = "#e8ecf6";
  g.beginPath(); g.arc(cx, cy, 4, 0, 2 * Math.PI); g.fill();
}

// One log-scale chart: decade gridlines, the series against t, a cursor at the
// playback time (or wherever the pointer is), and the value under the cursor.
function chart(id, series, outId, fmt, now) {
  const canvas = $(id), [g, w, h] = fit(canvas);
  g.clearRect(0, 0, w, h);
  if (!run) return;
  const T = run.duration, pad = { l: 34, r: 6, t: 6, b: 16 };
  let lo = Infinity, hi = -Infinity;
  // the floor (an exact zero, as energy drift is at t = 0) stays out of the range and is drawn at the bottom
  for (const s of series) for (const v of s.y) { if (v > FLOOR && v < lo) lo = v; if (v > hi) hi = v; }
  if (lo > hi) lo = hi - 2;
  lo = Math.floor(lo); hi = Math.ceil(hi + 1e-9); if (hi - lo < 2) hi = lo + 2;
  const X = (t) => pad.l + (w - pad.l - pad.r) * t / T, Y = (v) => pad.t + (h - pad.t - pad.b) * (hi - Math.max(v, lo)) / (hi - lo);
  const every = Math.ceil((hi - lo) / 4);
  g.font = "11px system-ui, sans-serif"; g.textAlign = "right"; g.textBaseline = "middle";
  for (let v = lo; v <= hi; v += every) {
    g.strokeStyle = "rgba(255,255,255,0.08)"; g.lineWidth = 1;
    g.beginPath(); g.moveTo(pad.l, Y(v)); g.lineTo(w - pad.r, Y(v)); g.stroke();
    g.fillStyle = "#b4bed6"; g.fillText(decade(v), pad.l - 4, Y(v));
  }
  g.textAlign = "left"; g.textBaseline = "alphabetic";
  g.fillText("0 s", pad.l, h - 3); g.textAlign = "right"; g.fillText(`${T} s`, w - pad.r, h - 3);
  for (const s of series) {
    g.strokeStyle = s.color; g.lineWidth = 2; g.setLineDash(s.dash ? [5, 4] : []);
    g.beginPath();
    s.y.forEach((v, i) => { const x = X(s.t ? s.t[i] : i / run.fps), y = Y(v); i ? g.lineTo(x, y) : g.moveTo(x, y); });
    g.stroke();
  }
  g.setLineDash([]);
  const tc = hover[id] ?? now, i = Math.min(run.frames - 1, Math.round(tc * run.fps));
  g.strokeStyle = "rgba(232,236,246,0.55)"; g.lineWidth = 1;
  g.beginPath(); g.moveTo(X(tc), pad.t); g.lineTo(X(tc), h - pad.b); g.stroke();
  for (const s of series) if (!s.t) { g.fillStyle = s.color; g.beginPath(); g.arc(X(tc), Y(s.y[i]), 4, 0, 2 * Math.PI); g.fill(); }
  $(outId).textContent = `t ${tc.toFixed(1)} s · ${fmt(i)}`;
  canvas._x = (px) => Math.max(0, Math.min(T, (px - pad.l) / (w - pad.l - pad.r) * T));
}

function drawCharts(now) {
  if (!run) return;
  const d = run.divergence, ly = run.lyapunov;
  const series = [{ y: d.median, color: C_MED }, { y: d.near, color: C_NEAR }];
  if (ly) series.push({ y: [ly.intercept / LN10, (ly.intercept + ly.lambda * ly.until) / LN10], t: [0, ly.until], color: C_FIT, dash: true });
  chart("divC", series, "div-v", (i) => `nearest ${(10 ** d.near[i]).toExponential(1)} m`, now);
  const e = run.energy.log_drift;
  chart("enC", [{ y: e, color: C_EN }], "en-v", (i) => `${(10 ** e[i]).toExponential(1)} of the well`, now);
}

function frame(now) {
  const dt = last ? Math.min(0.1, (now - last) / 1000) : 0;
  last = now;
  if (run && playing) {
    simT += dt * +$("speed").value;
    if (simT > run.duration + 1.5) simT = 0;           // hold the last frame a moment, then loop
  }
  if (run) {
    const t = Math.min(simT, run.duration);
    drawStage(t); drawCharts(t);
    $("t").textContent = `${t.toFixed(1)} s`;
  }
  requestAnimationFrame(frame);
}

// ---- wiring --------------------------------------------------------------------------

for (const id of ["divC", "enC"]) {
  const c = $(id);
  const at = (e) => { if (c._x) hover[id] = c._x(e.clientX - c.getBoundingClientRect().left); };
  c.addEventListener("pointermove", at);
  c.addEventListener("pointerdown", at);
  c.addEventListener("pointerleave", () => { hover[id] = null; });
}
$("speed").addEventListener("input", () => { $("speed-v").textContent = `${(+$("speed").value).toFixed(2).replace(/0$/, "")}×`; });
$("run").addEventListener("click", () => { clearTimeout(timer); integrate(); });
$("play").addEventListener("click", () => { playing = !playing; $("play").textContent = playing ? "Pause" : "Play"; });
if (innerWidth < 760) $("card").open = false;

fetch("api/menu").then((r) => r.json()).then((m) => {
  for (const [name, spec] of Object.entries(m.limits)) $("sliders").append(slider(name, spec));
  $("methods").innerHTML = Object.entries(m.methods)
    .map(([k, label]) => `<button type="button" data-k="${k}" title="${label}" class="${k === state.method ? "on" : ""}">${k}</button>`).join("");
  $("methods").addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    state.method = b.dataset.k;
    for (const x of $("methods").children) x.classList.toggle("on", x === b);
    changed();
  });
  integrate();
  requestAnimationFrame(frame);
});
