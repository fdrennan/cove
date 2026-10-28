// The drawing pad and what the models say about it. Strokes are kept as
// normalised points so a resize redraws them sharp. Every stroke change sends
// the pad as a 64x64 ink grid to POST /api/predict (one request in flight, the
// latest drawing queued); the server does the 8x8 reduction, so the page shows
// the exact image the model classified.
"use strict";

const $ = (id) => document.getElementById(id);
const GRID = 64;
const pad = $("draw"), ctx = pad.getContext("2d");
const state = { model: "svc", strokes: [], sample: null, last: null, report: null };
let inflight = false, queued = false;

// ---- drawing ---------------------------------------------------------------------------

function fit() {
  const r = pad.getBoundingClientRect(), dpr = window.devicePixelRatio || 1;
  pad.width = Math.round(r.width * dpr); pad.height = Math.round(r.height * dpr);
  redraw();
}

function redraw() {
  const w = pad.width, h = pad.height;
  ctx.clearRect(0, 0, w, h);
  if (state.sample) {
    // the 8x8 sample, smoothed up to 72% of the pad, as ink
    const s = document.createElement("canvas"); s.width = s.height = 8;
    const sc = s.getContext("2d"), img = sc.createImageData(8, 8);
    state.sample.forEach((v, i) => { img.data.set([255, 255, 255, Math.round(v / 16 * 255)], i * 4); });
    sc.putImageData(img, 0, 0);
    const side = 0.72 * Math.min(w, h);
    ctx.imageSmoothingEnabled = true; ctx.imageSmoothingQuality = "high";
    ctx.drawImage(s, (w - side) / 2, (h - side) / 2 - 0.04 * h, side, side);
  }
  ctx.strokeStyle = "#fff"; ctx.lineCap = ctx.lineJoin = "round";
  ctx.lineWidth = 0.055 * Math.min(w, h);
  for (const st of state.strokes) {
    ctx.beginPath();
    st.forEach(([x, y], i) => (i ? ctx.lineTo(x * w, y * h) : ctx.moveTo(x * w, y * h)));
    if (st.length === 1) ctx.lineTo(st[0][0] * w + 0.01, st[0][1] * h);
    ctx.stroke();
  }
  $("hint").hidden = state.strokes.length > 0 || !!state.sample;
}

function point(e) {
  const r = pad.getBoundingClientRect();
  return [(e.clientX - r.left) / r.width, (e.clientY - r.top) / r.height];
}

let drawing = false;
pad.addEventListener("pointerdown", (e) => {
  drawing = true; pad.setPointerCapture(e.pointerId);
  if (state.sample) { state.sample = null; $("truth").hidden = true; }
  state.strokes.push([point(e)]); redraw(); predict();
});
pad.addEventListener("pointermove", (e) => {
  if (!drawing) return;
  state.strokes[state.strokes.length - 1].push(point(e)); redraw(); predict();
});
for (const ev of ["pointerup", "pointercancel"]) pad.addEventListener(ev, () => { if (drawing) { drawing = false; predict(); } });

$("clear").addEventListener("click", () => {
  state.strokes = []; state.sample = null; $("truth").hidden = true; redraw(); show(null);
});
$("sample").addEventListener("click", async () => {
  const d = await (await fetch("/api/sample")).json();
  state.strokes = []; state.sample = d.pixels; redraw();
  $("truth").innerHTML = `held-out #${d.i} · labelled <b>${d.label}</b>`; $("truth").hidden = false;
  predict();
});

// ---- prediction ------------------------------------------------------------------------

function inkGrid() {
  // Two steps down (pad -> 256 -> 64) so thin strokes average rather than alias.
  const mid = document.createElement("canvas"); mid.width = mid.height = 256;
  mid.getContext("2d").drawImage(pad, 0, 0, 256, 256);
  const g = document.createElement("canvas"); g.width = g.height = GRID;
  const gc = g.getContext("2d"); gc.imageSmoothingQuality = "high";
  gc.drawImage(mid, 0, 0, GRID, GRID);
  const a = gc.getImageData(0, 0, GRID, GRID).data, ink = new Array(GRID * GRID);
  for (let i = 0; i < ink.length; i++) ink[i] = a[i * 4 + 3];
  return ink;
}

async function predict() {
  if (inflight) { queued = true; return; }
  if (!state.strokes.length && !state.sample) { show(null); return; }
  inflight = true;
  try {
    const r = await fetch("/api/predict", { method: "POST", headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ ink: inkGrid() }) });
    if (r.ok) show(await r.json());
  } catch (e) { console.error(e); }
  inflight = false;
  if (queued) { queued = false; predict(); }
}

function show(d) {
  state.last = d;
  const m = d && d.models[state.model];
  $("guess").textContent = m ? m.digit : "?";
  $("conf").textContent = m ? (m.probs[m.digit] * 100).toFixed(1) + "%" : "–";
  $("ms").textContent = d ? `${d.ms} ms` : "";
  $("bars").innerHTML = Array.from({ length: 10 }, (_, k) => {
    const p = m ? m.probs[k] : 0;
    return `<div class="bar${m && k === m.digit ? " top" : ""}"><span class="d">${k}</span>` +
      `<span class="track"><span class="fill" style="width:${(p * 100).toFixed(1)}%"></span></span>` +
      `<span class="v">${m ? (p * 100).toFixed(1) + "%" : ""}</span></div>`;
  }).join("");
  // the 8x8 the server built, drawn 0-16 as black-white
  const sc = $("seen").getContext("2d"), img = sc.createImageData(8, 8);
  const seen = d ? d.seen : new Array(64).fill(0);
  seen.forEach((v, i) => { const g = Math.round(v / 16 * 255); img.data.set([g, g, g, 255], i * 4); });
  sc.putImageData(img, 0, 0);
  if (state.report) {
    $("others").innerHTML = Object.entries(state.report.models).map(([k, r]) => {
      const o = d && d.models[k];
      return `<li data-k="${k}"><span>${r.label}</span><b>${o ? `${o.digit} · ${(o.probs[o.digit] * 100).toFixed(0)}%` : "–"}</b></li>`;
    }).join("");
  }
}

// ---- models and the confusion matrix -------------------------------------------------

function selectModel(k) {
  state.model = k;
  for (const b of $("model-seg").children) b.classList.toggle("on", b.dataset.k === k);
  const r = state.report.models[k];
  $("acc-train").textContent = (r.train_acc * 100).toFixed(1) + "%";
  $("acc-test").textContent = (r.test_acc * 100).toFixed(1) + "%";
  $("fit-ms").textContent = r.fit_ms < 1000 ? `${Math.round(r.fit_ms)} ms` : `${(r.fit_ms / 1000).toFixed(1)} s`;
  drawCM();
  show(state.last);
}

const CM = { pad: 22 };
function drawCM() {
  const c = $("cm"), r = c.getBoundingClientRect(), dpr = window.devicePixelRatio || 1;
  if (!state.report || !r.width) return;
  c.width = Math.round(r.width * dpr); c.height = Math.round(r.width * dpr);
  const g = c.getContext("2d"); g.setTransform(dpr, 0, 0, dpr, 0, 0);
  const W = r.width, cell = (W - CM.pad) / 10, m = state.report.models[state.model].confusion;
  g.clearRect(0, 0, W, W);
  g.font = `500 ${Math.max(10, cell * 0.36)}px system-ui, sans-serif`;
  g.textAlign = "center"; g.textBaseline = "middle";
  for (let i = 0; i < 10; i++) {
    g.fillStyle = "#9aa6c2";
    g.fillText(i, CM.pad + (i + 0.5) * cell, CM.pad / 2);          // predicted, across the top
    g.fillText(i, CM.pad / 2, CM.pad + (i + 0.5) * cell);          // true, down the side
    const row = m[i].reduce((a, b) => a + b, 0) || 1;
    for (let j = 0; j < 10; j++) {
      const f = m[i][j] / row, x = CM.pad + j * cell, y = CM.pad + i * cell;
      // one hue, stronger with the share of the true row; 2px surface gap between cells
      g.fillStyle = m[i][j] ? `rgba(20,160,190,${0.12 + 0.83 * f})` : "rgba(255,255,255,0.035)";
      g.beginPath(); g.roundRect(x + 1, y + 1, cell - 2, cell - 2, 3); g.fill();
      if (m[i][j]) { g.fillStyle = f > 0.5 ? "#fff" : "#e8ecf6"; g.fillText(m[i][j], x + cell / 2, y + cell / 2 + 1); }
    }
  }
}

$("cm").addEventListener("pointermove", (e) => {
  if (!state.report) return;
  const r = $("cm").getBoundingClientRect(), cell = (r.width - CM.pad) / 10;
  const j = Math.floor((e.clientX - r.left - CM.pad) / cell), i = Math.floor((e.clientY - r.top - CM.pad) / cell);
  const tip = $("cm-tip");
  if (i < 0 || j < 0 || i > 9 || j > 9) { tip.hidden = true; return; }
  const n = state.report.models[state.model].confusion[i][j];
  tip.textContent = i === j ? `${n} × ${i} read correctly` : `${n} × true ${i} read as ${j}`;
  tip.style.left = `${e.clientX - r.left}px`; tip.style.top = `${e.clientY - r.top + 28}px`;
  tip.hidden = false;
});
$("cm").addEventListener("pointerleave", () => { $("cm-tip").hidden = true; });

$("model-seg").addEventListener("click", (e) => { const b = e.target.closest("button"); if (b) selectModel(b.dataset.k); });
$("others").addEventListener("click", (e) => { const li = e.target.closest("li"); if (li) selectModel(li.dataset.k); });
window.addEventListener("resize", () => { fit(); drawCM(); });

fetch("/api/models").then((r) => r.json()).then((rep) => {
  state.report = rep;
  $("n-test").textContent = rep.n_test;
  $("model-seg").innerHTML = Object.entries(rep.models)
    .map(([k, m]) => `<button type="button" data-k="${k}">${m.label}</button>`).join("");
  selectModel(state.model);
  $("sample").click();   // open on a held-out digit so the page shows the models at work
});
fit();
show(null);
