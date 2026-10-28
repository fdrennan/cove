// Dataset Detective: the server does the statistics (pandas, numpy, scipy) and
// answers JSON; every chart here is drawn on canvas from those numbers, so it
// redraws at the screen's own resolution and answers hover without a round trip.
"use strict";

const $ = (id) => document.getElementById(id);
const SURFACE = "#0b1020", INK = "#e8ecf6", MUTED = "#9aa6c2", GRID = "rgba(255,255,255,.07)";
// Diverging: blue (negative) -> neutral grey -> red (positive), the reference pair.
const NEG = [57, 135, 229], MID = [56, 56, 53], POS = [230, 103, 103];
// Sequential blue, dark to light: on a dark surface low values recede, high ones stand out.
const SEQ = ["#184f95", "#1c5cab", "#256abf", "#2a78d6", "#3987e5", "#5598e7", "#6da7ec", "#86b6ef", "#9ec5f4", "#b7d3f6"];
const KIND = { correlation: ["#3987e5", "correlation"], separation: ["#199e70", "separates"], skew: ["#d95926", "skew"],
  outlier: ["#e66767", "outliers"], missing: ["#c98500", "missing"], balance: ["#9085e9", "balance"] };

const S = { dataset: "iris", missing: 0, z: 3, iqr: 1.5, seed: 0, method: "pearson",
  col: null, by: "", bins: 24, log: false, sx: null, sy: null, sc: "", fit: true };
let menu = null, prof = null, colData = null, scData = null, groupColors = [];

// ---- small helpers ---------------------------------------------------------------------

const fmt = (v) => v == null ? "–" : Math.abs(v) >= 1e4 || (Math.abs(v) < 1e-3 && v !== 0) ? v.toExponential(2)
  : Math.abs(v) >= 100 ? v.toFixed(0) : Math.abs(v) >= 10 ? v.toFixed(1) : v.toFixed(2);
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const qs = (o) => new URLSearchParams(Object.entries(o).filter(([, v]) => v !== "" && v != null)).toString();
const common = () => ({ dataset: S.dataset, missing: S.missing, seed: S.seed });
const lerp = (a, b, t) => a.map((x, i) => Math.round(x + (b[i] - x) * t));
const rgb = (c) => `rgb(${c[0]},${c[1]},${c[2]})`;
const divColor = (r) => r == null ? "rgba(255,255,255,.03)" : rgb(lerp(MID, r < 0 ? NEG : POS, Math.min(1, Math.abs(r))));
const seqColor = (t) => SEQ[Math.max(0, Math.min(SEQ.length - 1, Math.round(t * (SEQ.length - 1))))];

async function getJSON(path, params) {
  const r = await fetch(`${path}?${qs(params)}`);
  const d = await r.json();
  if (!r.ok) throw new Error(typeof d.detail === "string" ? d.detail : JSON.stringify(d.detail));
  return d;
}

// Newest request wins: a slider dragged fast fires many; only the last one paints.
const latest = {};
async function fresh(key, fn) {
  const t = (latest[key] = (latest[key] || 0) + 1);
  const out = await fn();
  return t === latest[key] ? out : null;
}

function canvas(id) {
  const c = $(id), dpr = window.devicePixelRatio || 1;
  const w = c.clientWidth, h = c.clientHeight;
  if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) { c.width = Math.round(w * dpr); c.height = Math.round(h * dpr); }
  const ctx = c.getContext("2d");
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
  ctx.clearRect(0, 0, w, h);
  return { ctx, w, h };
}

function niceTicks(lo, hi, n = 5) {
  if (!(hi > lo)) return [lo];
  const raw = (hi - lo) / n, mag = 10 ** Math.floor(Math.log10(raw));
  const step = [1, 2, 2.5, 5, 10].map((m) => m * mag).find((s) => s >= raw);
  const out = [];
  for (let v = Math.ceil(lo / step) * step; v <= hi + step * 1e-9; v += step) out.push(+v.toFixed(12));
  return out;
}

function tip(html, e) {
  const t = $("tip");
  if (!html) { t.hidden = true; return; }
  t.innerHTML = html; t.hidden = false;
  const x = Math.min(e.clientX + 14, innerWidth - t.offsetWidth - 8), y = Math.min(e.clientY + 14, innerHeight - t.offsetHeight - 8);
  t.style.left = `${x}px`; t.style.top = `${y}px`;
}

function fillSelect(sel, items, value) {
  sel.innerHTML = items.map(([v, label]) => `<option value="${esc(v)}" ${v === value ? "selected" : ""}>${esc(label)}</option>`).join("");
}

// ---- profile: facts, table, heatmap, findings -------------------------------------------

async function loadProfile(resetPicks) {
  document.body.classList.add("busy");
  try {
    const d = await fresh("profile", () => getJSON("/api/profile", { ...common(), z: S.z, iqr: S.iqr, method: S.method }));
    if (!d) return;
    prof = d;
    const num = d.columns.filter((c) => c.kind === "numeric").map((c) => c.name);
    const cats = d.columns.filter((c) => c.kind === "category").map((c) => c.name);
    const target = d.target;
    if (resetPicks || !num.includes(S.col)) {
      const corr = d.findings.find((f) => f.kind === "correlation");
      const sep = d.findings.find((f) => f.kind === "separation");
      S.col = sep?.show.col || sep?.show.x || num[0];
      S.by = cats.includes(target) ? target : "";
      S.sx = corr ? corr.show.x : num[0];
      S.sy = corr ? corr.show.y : num[1];
      S.sc = target;
      S.log = false; $("dist-log").checked = false;
    }
    fillSelect($("dist-col"), num.map((n) => [n, n]), S.col);
    fillSelect($("dist-by"), [["", "no split"], ...cats.map((n) => [n, `by ${n}`])], S.by);
    fillSelect($("sx"), num.map((n) => [n, `x: ${n}`]), S.sx);
    fillSelect($("sy"), num.map((n) => [n, `y: ${n}`]), S.sy);
    fillSelect($("sc"), [["", "no colour"], ...d.columns.map((c) => [c.name, `colour: ${c.name}`])], S.sc);

    $("f-rows").textContent = d.rows;
    $("f-cols").textContent = d.columns.length;
    $("f-complete").textContent = d.complete_rows;
    $("f-ms").textContent = Math.round(d.ms);
    $("about").textContent = menu.datasets[S.dataset].about;
    renderTable();
    drawHeat();
    renderFindings();
    await Promise.all([loadColumn(), loadScatter()]);
  } catch (e) {
    console.error(e);
  } finally {
    document.body.classList.remove("busy");
  }
}

function renderTable() {
  const cols = prof.columns;
  $("cols-hint").textContent = `${cols.filter((c) => c.kind === "numeric").length} numeric, ${cols.filter((c) => c.kind === "category").length} category · click a row`;
  const head = `<thead><tr><th>column</th><th>missing</th><th>mean</th><th>sd</th><th>min</th><th>median</th><th>max</th>` +
    `<th>skew</th><th>|z|&gt;${S.z}</th><th>IQR</th><th>shape</th></tr></thead>`;
  const rows = cols.map((c, i) => {
    const miss = `<td class="${c.missing ? "warn" : ""}">${c.missing ? `${c.missing} (${Math.round(100 * c.missing / prof.rows)}%)` : "0"}</td>`;
    if (c.kind === "category") {
      const lv = c.levels.map((l, j) => `${esc(l)} ${c.counts[j]}`).join(" · ");
      return `<tr data-i="${i}" class="${c.name === S.by ? "on" : ""}"><td>${esc(c.name)}<span class="type">category</span></td>${miss}` +
        `<td colspan="9" style="text-align:left">${lv}</td></tr>`;
    }
    const sk = c.skew != null && Math.abs(c.skew) >= 1 ? "warn" : "";
    return `<tr data-i="${i}" class="${c.name === S.col ? "on" : ""}"><td>${esc(c.name)}<span class="type">${c.dtype}</span></td>${miss}` +
      `<td>${fmt(c.mean)}</td><td>${fmt(c.std)}</td><td>${fmt(c.min)}</td><td>${fmt(c.median)}</td><td>${fmt(c.max)}</td>` +
      `<td class="${sk}">${c.skew == null ? "–" : c.skew.toFixed(2)}</td><td class="${c.out_z ? "warn" : ""}">${c.out_z}</td>` +
      `<td class="${c.out_iqr ? "warn" : ""}">${c.out_iqr}</td><td><canvas data-spark="${i}"></canvas></td></tr>`;
  }).join("");
  $("columns").innerHTML = head + `<tbody>${rows}</tbody>`;
  for (const cv of $("columns").querySelectorAll("canvas[data-spark]")) spark(cv, cols[+cv.dataset.spark].spark);
}

function spark(cv, counts) {
  const dpr = window.devicePixelRatio || 1, w = 80, h = 22;
  cv.width = w * dpr; cv.height = h * dpr;
  const ctx = cv.getContext("2d");
  ctx.scale(dpr, dpr);
  const m = Math.max(...counts), bw = w / counts.length;
  ctx.fillStyle = "#5598e7";
  counts.forEach((n, i) => { const bh = (n / m) * (h - 2); ctx.fillRect(i * bw + 0.5, h - bh, bw - 1, bh); });
}

const heat = { cells: null };
function drawHeat() {
  if (!prof) return;
  const { ctx, w, h } = canvas("heat");
  const names = prof.corr.names, n = names.length, R = prof.corr.r;
  ctx.font = "12px system-ui, sans-serif";
  const labelW = Math.min(w * 0.3, Math.max(...names.map((s) => ctx.measureText(s).width)) + 8);
  const size = Math.min(w - labelW, h - labelW) / n;
  const showLabels = size >= 11;
  const ox = showLabels ? labelW : (w - size * n) / 2, oy = showLabels ? labelW : (h - size * n) / 2;
  const gap = size > 8 ? 2 : 1;
  for (let i = 0; i < n; i++) for (let j = 0; j < n; j++) {
    ctx.fillStyle = divColor(R[i][j]);
    ctx.beginPath();
    ctx.roundRect(ox + j * size + gap / 2, oy + i * size + gap / 2, size - gap, size - gap, Math.min(4, size / 6));
    ctx.fill();
    if (size >= 34 && R[i][j] != null) {
      ctx.fillStyle = Math.abs(R[i][j]) > 0.55 ? "#fff" : INK;
      ctx.textAlign = "center"; ctx.textBaseline = "middle";
      ctx.fillText(R[i][j].toFixed(2), ox + (j + 0.5) * size, oy + (i + 0.5) * size);
    }
  }
  if (showLabels) {
    ctx.fillStyle = MUTED; ctx.textBaseline = "middle"; ctx.textAlign = "right";
    const clip = (s) => { let t = s; while (ctx.measureText(t).width > labelW - 8 && t.length > 3) t = t.slice(0, -2); return t === s ? s : t + "…"; };
    names.forEach((s, i) => ctx.fillText(clip(s), ox - 6, oy + (i + 0.5) * size));
    ctx.save();
    names.forEach((s, j) => {
      ctx.save(); ctx.translate(ox + (j + 0.5) * size, oy - 6); ctx.rotate(-Math.PI / 2);
      ctx.textAlign = "left"; ctx.fillText(clip(s), 0, 0); ctx.restore();
    });
    ctx.restore();
  }
  heat.cells = { ox, oy, size, n };
}

function heatAt(e) {
  if (!heat.cells) return null;
  const r = $("heat").getBoundingClientRect(), { ox, oy, size, n } = heat.cells;
  const j = Math.floor((e.clientX - r.left - ox) / size), i = Math.floor((e.clientY - r.top - oy) / size);
  return i >= 0 && j >= 0 && i < n && j < n ? [i, j] : null;
}
$("heat").addEventListener("mousemove", (e) => {
  const at = heatAt(e);
  if (!at) return tip(null);
  const [i, j] = at, nm = prof.corr.names, r = prof.corr.r[i][j];
  tip(`<b>${esc(nm[i])}</b> × <b>${esc(nm[j])}</b><br>${prof.corr.method} ${r == null ? "–" : (r > 0 ? "+" : "") + r.toFixed(3)}` +
    (i !== j ? "<br>click to plot" : ""), e);
});
$("heat").addEventListener("mouseleave", () => tip(null));
$("heat").addEventListener("click", (e) => {
  const at = heatAt(e);
  if (!at || at[0] === at[1]) return;
  showScatter(prof.corr.names[at[1]], prof.corr.names[at[0]]);
});

function renderFindings() {
  $("findings").innerHTML = prof.findings.map((f, i) => {
    const [c, tag] = KIND[f.kind] || ["#9aa6c2", f.kind];
    return `<li data-i="${i}" style="--c:${c}"><span class="tag">${tag}</span><b>${esc(f.title)}</b><span>${esc(f.detail)}</span></li>`;
  }).join("") || "<li><b>Nothing stands out</b></li>";
}
$("findings").addEventListener("click", (e) => {
  const li = e.target.closest("li[data-i]");
  if (!li) return;
  const s = prof.findings[+li.dataset.i].show;
  if (s.x) showScatter(s.x, s.y);
  else showColumn(s.col, s.by);
});

function showScatter(x, y) {
  S.sx = x; S.sy = y; $("sx").value = x; $("sy").value = y;
  loadScatter();
  $("scatter").closest(".card").scrollIntoView({ behavior: "smooth", block: "center" });
}
function showColumn(col, by) {
  S.col = col; $("dist-col").value = col;
  if (by !== undefined) { S.by = by; $("dist-by").value = by; }
  S.log = false; $("dist-log").checked = false;
  for (const tr of $("columns").querySelectorAll("tbody tr")) tr.classList.toggle("on", prof.columns[+tr.dataset.i].name === col);
  loadColumn();
  $("dist").closest(".card").scrollIntoView({ behavior: "smooth", block: "center" });
}
$("columns").addEventListener("click", (e) => {
  const tr = e.target.closest("tr[data-i]");
  if (!tr) return;
  const c = prof.columns[+tr.dataset.i];
  if (c.kind === "numeric") showColumn(c.name);
  else showColumn(S.col, c.name);
});

// ---- distribution ----------------------------------------------------------------------

async function loadColumn() {
  try {
    const d = await fresh("column", () => getJSON("/api/column", { ...common(), col: S.col, by: S.by, bins: S.bins, log: S.log, z: S.z, iqr: S.iqr }));
    if (!d) return;
    colData = d;
    drawDist();
    const b = d.box, lab = (v) => fmt(d.log ? 10 ** v : v);
    $("dist-stats").innerHTML = `median <b>${lab(b.median)}</b> · IQR <b>${lab(b.q1)}–${lab(b.q3)}</b> · mean <b>${lab(b.mean)}</b>` +
      ` · <b>${d.outliers.length}</b> outlier${d.outliers.length === 1 ? "" : "s"}` +
      (d.normality ? ` · Shapiro–Wilk p <b>${d.normality.shapiro_p < 0.001 ? "< 0.001" : d.normality.shapiro_p}</b>` : "") +
      ` · ${Math.round(d.ms)} ms`;
    $("dist-legend").innerHTML = d.by ? d.groups.map((g, i) => `<span><i style="background:${menu.colors[i] || MUTED}"></i>${esc(g.name)} · mean ${lab(g.mean)}</span>`).join("") : "";
  } catch (e) {
    $("dist-stats").textContent = e.message;
    if (S.log) { S.log = false; $("dist-log").checked = false; }
  }
}

const dist = {};
function drawDist() {
  const d = colData;
  if (!d) return;
  const { ctx, w, h } = canvas("dist");
  const pad = { l: 40, r: 10, t: 10, b: 54 };
  const e = d.edges, nb = e.length - 1, x0 = e[0], x1 = e[nb];
  const totals = Array.from({ length: nb }, (_, i) => d.groups.reduce((s, g) => s + g.counts[i], 0));
  const ymax = Math.max(...totals, ...d.kde.y) * 1.08 || 1;
  const X = (v) => pad.l + ((v - x0) / (x1 - x0 || 1)) * (w - pad.l - pad.r);
  const plotB = h - pad.b, Y = (v) => plotB - (v / ymax) * (plotB - pad.t);

  ctx.font = "12px system-ui, sans-serif"; ctx.fillStyle = MUTED; ctx.strokeStyle = GRID; ctx.lineWidth = 1;
  ctx.textAlign = "right"; ctx.textBaseline = "middle";
  for (const t of niceTicks(0, ymax, 4)) { ctx.beginPath(); ctx.moveTo(pad.l, Y(t)); ctx.lineTo(w - pad.r, Y(t)); ctx.stroke(); ctx.fillText(t, pad.l - 6, Y(t)); }
  ctx.textAlign = "center"; ctx.textBaseline = "top";
  for (const t of niceTicks(x0, x1, Math.max(3, Math.floor(w / 90)))) ctx.fillText(fmt(d.log ? 10 ** t : t), X(t), h - 18);

  // stacked bars: each group a segment, a 2px surface gap between neighbours
  for (let i = 0; i < nb; i++) {
    let base = 0;
    const bx = X(e[i]) + 1, bw = Math.max(1, X(e[i + 1]) - X(e[i]) - 2);
    d.groups.forEach((g, k) => {
      const n = g.counts[i];
      if (!n) return;
      ctx.fillStyle = d.by ? menu.colors[k] || MUTED : "#3987e5";
      ctx.globalAlpha = 0.85;
      const top = Y(base + n), bot = Y(base) - (base ? 2 : 0);
      ctx.beginPath(); ctx.roundRect(bx, top, bw, Math.max(1, bot - top), [Math.min(4, bw / 3), Math.min(4, bw / 3), 0, 0]); ctx.fill();
      base += n;
    });
  }
  ctx.globalAlpha = 1;
  // density, scaled to counts per bin by the server
  ctx.strokeStyle = INK; ctx.lineWidth = 2; ctx.beginPath();
  d.kde.x.forEach((v, i) => (i ? ctx.lineTo : ctx.moveTo).call(ctx, X(v), Y(d.kde.y[i])));
  ctx.stroke();

  // box plot strip under the axis: whiskers to the last point inside the fences
  const by = plotB + 14, b = d.box;
  ctx.strokeStyle = MUTED; ctx.lineWidth = 1.5;
  ctx.beginPath(); ctx.moveTo(X(b.lo), by); ctx.lineTo(X(b.q1), by); ctx.moveTo(X(b.q3), by); ctx.lineTo(X(b.hi), by);
  ctx.moveTo(X(b.lo), by - 5); ctx.lineTo(X(b.lo), by + 5); ctx.moveTo(X(b.hi), by - 5); ctx.lineTo(X(b.hi), by + 5); ctx.stroke();
  ctx.fillStyle = "rgba(57,135,229,.35)"; ctx.strokeStyle = "#5598e7";
  ctx.beginPath(); ctx.roundRect(X(b.q1), by - 7, X(b.q3) - X(b.q1), 14, 4); ctx.fill(); ctx.stroke();
  ctx.strokeStyle = INK; ctx.lineWidth = 2; ctx.beginPath(); ctx.moveTo(X(b.median), by - 7); ctx.lineTo(X(b.median), by + 7); ctx.stroke();
  for (const o of d.outliers) {
    ctx.fillStyle = "#e66767"; ctx.strokeStyle = SURFACE; ctx.lineWidth = 1.5;
    ctx.beginPath(); ctx.arc(X(o.value), by, 4, 0, 7); ctx.fill(); ctx.stroke();
  }
  Object.assign(dist, { X, pad, w, nb, e, totals });
}
$("dist").addEventListener("mousemove", (ev) => {
  const d = colData;
  if (!d || !dist.X) return;
  const r = $("dist").getBoundingClientRect(), mx = ev.clientX - r.left;
  const i = d.edges.findIndex((v, k) => k < dist.nb && mx >= dist.X(v) && mx < dist.X(d.edges[k + 1]));
  if (i < 0) return tip(null);
  const lab = (v) => fmt(d.log ? 10 ** v : v);
  const parts = d.by ? d.groups.map((g) => `${esc(g.name)} ${g.counts[i]}`).join("<br>") : "";
  tip(`<b>${lab(d.edges[i])} – ${lab(d.edges[i + 1])}</b><br>${dist.totals[i]} rows${parts ? "<br>" + parts : ""}`, ev);
});
$("dist").addEventListener("mouseleave", () => tip(null));

// ---- scatter ---------------------------------------------------------------------------

async function loadScatter() {
  if (!S.sx || !S.sy) return;
  try {
    const d = await fresh("scatter", () => getJSON("/api/scatter", { ...common(), x: S.sx, y: S.sy, color: S.sc }));
    if (!d) return;
    scData = d;
    drawScatter();
    const f = d.fit;
    let line = `n <b>${d.n}</b>${d.dropped ? ` (${d.dropped} rows with blanks left out)` : ""} · Pearson r <b>${f.r.toFixed(3)}</b>` +
      ` · Spearman ρ <b>${f.spearman.toFixed(3)}</b> · R² <b>${f.r2.toFixed(3)}</b> · y = <b>${fmt(f.slope)}</b>x ${f.intercept < 0 ? "−" : "+"} <b>${fmt(Math.abs(f.intercept))}</b>`;
    if (d.groups?.length) {
      line += " · within groups: " + d.groups.map((g) => `${esc(g.name)} r <b>${g.r.toFixed(2)}</b>`).join(", ");
      // Simpson's paradox: every group trends one way, the pooled data the other.
      if (d.groups.every((g) => Math.sign(g.slope) !== Math.sign(f.slope))) line += " · <b>the pooled trend reverses inside every group (Simpson's paradox)</b>";
    }
    $("sc-stats").innerHTML = line + ` · ${Math.round(d.ms)} ms`;
    const c = d.color;
    $("sc-legend").innerHTML = !c ? "" : c.kind === "category"
      ? c.levels.map((l, i) => `<span><i style="background:${menu.colors[i] || MUTED}"></i>${esc(l)}</span>`).join("")
      : `<span>${esc(c.name)} ${fmt(c.min)} <i class="bar" style="background:linear-gradient(90deg,${SEQ.join(",")})"></i> ${fmt(c.max)}</span>`;
  } catch (e) {
    $("sc-stats").textContent = e.message;
  }
}

const sc = { pts: [] };
function drawScatter() {
  const d = scData;
  if (!d) return;
  const { ctx, w, h } = canvas("scatter");
  const pad = { l: 52, r: 14, t: 12, b: 40 };
  const ext = (a) => { const lo = Math.min(...a), hi = Math.max(...a), m = (hi - lo) * 0.05 || 1; return [lo - m, hi + m]; };
  const [x0, x1] = ext(d.xs), [y0, y1] = ext(d.ys);
  const X = (v) => pad.l + ((v - x0) / (x1 - x0)) * (w - pad.l - pad.r);
  const Y = (v) => h - pad.b - ((v - y0) / (y1 - y0)) * (h - pad.t - pad.b);

  ctx.font = "12px system-ui, sans-serif"; ctx.strokeStyle = GRID; ctx.fillStyle = MUTED; ctx.lineWidth = 1;
  ctx.textAlign = "center"; ctx.textBaseline = "top";
  for (const t of niceTicks(x0, x1, Math.max(3, Math.floor(w / 100)))) { ctx.beginPath(); ctx.moveTo(X(t), pad.t); ctx.lineTo(X(t), h - pad.b); ctx.stroke(); ctx.fillText(fmt(t), X(t), h - pad.b + 6); }
  ctx.textAlign = "right"; ctx.textBaseline = "middle";
  for (const t of niceTicks(y0, y1, 5)) { ctx.beginPath(); ctx.moveTo(pad.l, Y(t)); ctx.lineTo(w - pad.r, Y(t)); ctx.stroke(); ctx.fillText(fmt(t), pad.l - 6, Y(t)); }
  ctx.textAlign = "right"; ctx.textBaseline = "bottom"; ctx.fillStyle = INK;
  ctx.fillText(d.x + " →", w - pad.r, h - 2);
  ctx.save(); ctx.translate(12, pad.t); ctx.rotate(-Math.PI / 2); ctx.textAlign = "right"; ctx.textBaseline = "top"; ctx.fillText(d.y + " →", 0, 0); ctx.restore();

  const c = d.color;
  const colorOf = (i) => !c ? "#3987e5" : c.kind === "category" ? menu.colors[c.codes[i]] || MUTED
    : c.values[i] == null ? MUTED : seqColor((c.values[i] - c.min) / ((c.max - c.min) || 1));
  sc.pts = d.xs.map((v, i) => [X(v), Y(d.ys[i]), i]);
  const rad = d.n > 400 ? 3.5 : 4.5;
  for (const [px, py, i] of sc.pts) {
    ctx.beginPath(); ctx.arc(px, py, rad, 0, 7);
    ctx.fillStyle = colorOf(i); ctx.globalAlpha = 0.9; ctx.fill();
    ctx.globalAlpha = 1; ctx.strokeStyle = SURFACE; ctx.lineWidth = 1.5; ctx.stroke();
  }
  if (S.fit) {
    const line = (m, b, col, dash, a = x0, z = x1) => {
      ctx.setLineDash(dash); ctx.strokeStyle = col; ctx.lineWidth = 2;
      ctx.beginPath(); ctx.moveTo(X(a), Y(m * a + b)); ctx.lineTo(X(z), Y(m * z + b)); ctx.stroke(); ctx.setLineDash([]);
    };
    ctx.save(); ctx.beginPath(); ctx.rect(pad.l, pad.t, w - pad.l - pad.r, h - pad.t - pad.b); ctx.clip();
    // a group's fit spans only that group's x range: drawn wider it claims data it never saw
    (d.groups || []).forEach((g) => {
      const k = c.levels.indexOf(g.name), xs = d.xs.filter((_, i) => c.codes[i] === k);
      line(g.slope, g.intercept, menu.colors[k] || MUTED, [], Math.min(...xs), Math.max(...xs));
    });
    line(d.fit.slope, d.fit.intercept, INK, [6, 5]);
    ctx.restore();
  }
}
$("scatter").addEventListener("mousemove", (e) => {
  const d = scData;
  if (!d) return;
  const r = $("scatter").getBoundingClientRect(), mx = e.clientX - r.left, my = e.clientY - r.top;
  let best = null, bd = 144;
  for (const p of sc.pts) { const dd = (p[0] - mx) ** 2 + (p[1] - my) ** 2; if (dd < bd) { bd = dd; best = p; } }
  if (!best) return tip(null);
  const i = best[2], c = d.color;
  const cv = !c ? "" : `<br>${esc(c.name)} ${c.kind === "category" ? esc(c.levels[c.codes[i]]) : fmt(c.values[i])}`;
  tip(`<b>row ${d.row[i]}</b><br>${esc(d.x)} ${fmt(d.xs[i])}<br>${esc(d.y)} ${fmt(d.ys[i])}${cv}`, e);
});
$("scatter").addEventListener("mouseleave", () => tip(null));

// ---- wiring ----------------------------------------------------------------------------

let timer = 0;
const later = (fn, ms = 160) => { clearTimeout(timer); timer = setTimeout(fn, ms); };

for (const [id, key, show] of [["missing", "missing", (v) => Math.round(v * 100) + "%"], ["z", "z", (v) => (+v).toFixed(1)], ["iqr", "iqr", (v) => (+v).toFixed(1)]]) {
  $(id).addEventListener("input", () => { S[key] = +$(id).value; $(id + "-v").textContent = show(S[key]); later(() => loadProfile(false)); });
}
$("reseed").addEventListener("click", () => { S.seed = (S.seed + 1) % 10000; if (S.missing > 0) loadProfile(false); });
$("method").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  S.method = b.dataset.k;
  for (const x of $("method").children) x.classList.toggle("on", x === b);
  loadProfile(false);
});
$("dist-col").addEventListener("change", () => showColumn($("dist-col").value));
$("dist-by").addEventListener("change", () => { S.by = $("dist-by").value; loadColumn(); });
$("dist-log").addEventListener("change", () => { S.log = $("dist-log").checked; loadColumn(); });
$("bins").addEventListener("input", () => { S.bins = +$("bins").value; $("bins-v").textContent = S.bins; later(loadColumn, 80); });
$("sx").addEventListener("change", () => { S.sx = $("sx").value; loadScatter(); });
$("sy").addEventListener("change", () => { S.sy = $("sy").value; loadScatter(); });
$("sc").addEventListener("change", () => { S.sc = $("sc").value; loadScatter(); });
$("sfit").addEventListener("change", () => { S.fit = $("sfit").checked; drawScatter(); });

new ResizeObserver(() => { drawHeat(); drawDist(); drawScatter(); }).observe(document.querySelector(".board"));
// Phones: fold the tuning sliders so the findings and charts come first.
if (innerWidth < 860) document.querySelector(".tune").removeAttribute("open");

fetch("/api/menu").then((r) => r.json()).then((m) => {
  menu = m;
  $("datasets").innerHTML = Object.entries(m.datasets)
    .map(([k, d]) => `<button type="button" data-k="${k}" class="${k === S.dataset ? "on" : ""}">${esc(d.label)}</button>`).join("");
  $("datasets").addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b || b.dataset.k === S.dataset) return;
    S.dataset = b.dataset.k;
    for (const x of $("datasets").children) x.classList.toggle("on", x === b);
    loadProfile(true);
  });
  loadProfile(true);
});
