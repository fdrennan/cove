// Builds the step controls, runs the pipeline on load and again on every
// change (debounced, one request in flight), and draws each step's result:
// the embedding on a canvas, the small charts as SVG.
"use strict";

const $ = (id) => document.getElementById(id);
// Categorical slots in fixed order (validated set, dark steps); a cluster keeps
// its colour by number, which is why the server caps clusters at eight.
const CAT = ["#3987e5", "#d95926", "#199e70", "#c98500", "#d55181", "#008300", "#9085e9", "#e66767"];
const ARTEFACT = ["#c9cfdd", "#5d6680"];        // doublet, damaged/empty: neutral, not a series hue
const SEQ = ["#2c3656", "#104281", "#1c5cab", "#2a78d6", "#5598e7", "#86b6ef", "#cde2fb"];   // one hue, dark to light
const INK = "#e8ecf6", MUTED = "#9aa6c2", LINE = "rgba(255,255,255,.12)";

// The sliders. Bounds the page offers (narrower than the API allows) and how to show them.
const SLIDERS = {
  "s-qc": [
    ["min_counts", "min counts", 0, 3000, 50, (v) => v],
    ["max_counts", "max counts", 3000, 20000, 250, (v) => v],
    ["min_genes", "min genes", 0, 1000, 10, (v) => v],
    ["max_mito", "max mito %", 2, 60, 0.5, (v) => (+v).toFixed(1)],
  ],
  "s-norm": [["target", "counts per cell", 3, 6, 0.1, (v) => Math.round(10 ** v).toLocaleString(), true]],
  "s-hvg": [["n_top", "variable genes", 100, 2000, 50, (v) => v]],
  "s-pca": [["n_pcs", "PCs used", 2, 50, 1, (v) => v]],
  "s-embed": [["k", "neighbours (k)", 5, 50, 1, (v) => v], ["perplexity", "t-SNE perplexity", 5, 100, 1, (v) => v]],
  "s-cluster": [["n_clusters", "clusters", 2, 8, 1, (v) => v]],
  "s-markers": [["n_markers", "markers per cluster", 1, 8, 1, (v) => v]],
};
const EMBEDDINGS = { graph: "Graph layout", tsne: "t-SNE", pca: "PC1 / PC2" };
const METHODS = { ward: "Ward on kNN", spectral: "Spectral", kmeans: "k-means" };

const state = { colour: "cluster", gene: null, geneValues: null, selected: null };
let menu = null, params = {}, data = null, busy = false, queued = false, timer = 0, view = null;

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c]));
const ms = (v) => v === undefined ? "" : v < 5 ? "cached" : v < 1000 ? `${Math.round(v)} ms` : `${(v / 1000).toFixed(1)} s`;
const seq = (t) => SEQ[Math.max(0, Math.min(SEQ.length - 1, Math.round(t * (SEQ.length - 1))))];
const truthColour = (t) => t < 6 ? CAT[t] : ARTEFACT[t - 6];

// ---- controls ------------------------------------------------------------------------

function slider(box, [key, label, lo, hi, step, fmt, log]) {
  const v = log ? Math.log10(params[key]) : params[key];
  const el = document.createElement("label");
  el.className = "row";
  el.innerHTML = `${label} <output>${fmt(v)}</output><input type="range" min="${lo}" max="${hi}" step="${step}" value="${v}" aria-label="${label}">`;
  const input = el.querySelector("input"), out = el.querySelector("output");
  input.addEventListener("input", () => {
    out.textContent = fmt(input.value);
    params[key] = log ? Math.round(10 ** +input.value) : +input.value;
    if (box === "s-qc" && data) drawQC();        // thresholds move at once; the rerun follows
    if (key === "n_pcs" && data) drawElbow();
    changed();
  });
  $(box).append(el);
}

function segment(id, options, key) {
  $(id).innerHTML = Object.entries(options)
    .map(([k, label]) => `<button type="button" data-k="${k}" class="${k === params[key] ? "on" : ""}">${label}</button>`).join("");
  $(id).addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    params[key] = b.dataset.k;
    for (const x of $(id).children) x.classList.toggle("on", x === b);
    changed();
  });
}

// ---- the request -----------------------------------------------------------------------

async function run() {
  if (busy) { queued = true; return; }
  busy = true;
  document.body.classList.add("busy");
  try {
    const r = await fetch("/api/run", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(params) });
    const d = await r.json();
    if (!r.ok) throw new Error(typeof d.detail === "string" ? d.detail : JSON.stringify(d.detail));
    $("error").hidden = true;
    data = d;
    if (state.selected !== null && state.selected >= d.markers.length) state.selected = null;
    if (state.gene) await loadGene(state.gene); else drawAll();
  } catch (e) {
    $("error").textContent = e.message;
    $("error").hidden = false;
  } finally {
    busy = false;
    document.body.classList.remove("busy");
    if (queued) { queued = false; run(); }
  }
}

function changed() {
  clearTimeout(timer);
  timer = setTimeout(run, 250);
}

async function loadGene(name) {
  const r = await fetch("/api/gene", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify({ ...params, gene: name }) });
  const d = await r.json();
  if (!r.ok) { $("gene").setCustomValidity(d.detail); $("gene").reportValidity(); return; }
  $("gene").setCustomValidity("");
  state.gene = d.gene; state.geneValues = d; state.colour = "gene";
  $("gene").value = d.gene;
  for (const x of $("colour-by").children) x.classList.toggle("on", x.dataset.k === "gene");
  drawAll();
}

// ---- drawing -----------------------------------------------------------------------------

function drawAll() {
  $("empty").hidden = true;
  $("ari").textContent = data.ari.toFixed(3);
  $("kept").textContent = `${data.n_kept.toLocaleString()} / ${data.n_cells.toLocaleString()}`;
  const total = Object.values(data.ms).reduce((a, b) => a + b, 0);
  $("ms").textContent = ms(total);
  for (const k of ["qc", "hvg", "pca", "cluster", "markers"]) $("t-" + k).textContent = ms(data.ms[k === "pca" ? "pca" : k]);
  drawEmbedding(); drawLegend(); drawQC(); drawHVG(); drawElbow(); drawDot(); drawTable(); drawClusterCard();
}

function pointColour(i) {
  const e = data.embedding;
  if (state.colour === "truth") return truthColour(e.truth[i]);
  if (state.colour === "gene" && state.geneValues) return seq(state.geneValues.values[i] / (state.geneValues.max || 1));
  return CAT[e.cluster[i]];
}

function drawEmbedding() {
  const cv = $("embedding"), e = data.embedding;
  const dpr = window.devicePixelRatio || 1, w = cv.clientWidth, h = cv.clientHeight;
  cv.width = w * dpr; cv.height = h * dpr;
  const ctx = cv.getContext("2d");
  ctx.scale(dpr, dpr);
  const xs = e.x, ys = e.y, n = xs.length;
  const [x0, x1] = [Math.min(...xs), Math.max(...xs)], [y0, y1] = [Math.min(...ys), Math.max(...ys)];
  const pad = 28, s = Math.min((w - 2 * pad) / (x1 - x0 || 1), (h - 2 * pad) / (y1 - y0 || 1));
  const ox = (w - s * (x1 - x0)) / 2, oy = (h - s * (y1 - y0)) / 2;
  view = { px: (i) => ox + (xs[i] - x0) * s, py: (i) => h - oy - (ys[i] - y0) * s };

  // gene colouring draws low values first so expressing cells sit on top
  let order = [...Array(n).keys()];
  if (state.colour === "gene" && state.geneValues) order.sort((a, b) => state.geneValues.values[a] - state.geneValues.values[b]);
  const r = n > 2000 ? 2.4 : 3;
  for (const i of order) {
    const dim = state.selected !== null && e.cluster[i] !== state.selected;
    ctx.globalAlpha = dim ? 0.12 : 0.9;
    ctx.fillStyle = pointColour(i);
    ctx.beginPath(); ctx.arc(view.px(i), view.py(i), r, 0, 6.2832); ctx.fill();
  }
  ctx.globalAlpha = 1;

  // labels at each group's median: identity is never colour alone
  const groups = new Map();
  const key = state.colour === "truth" ? e.truth : e.cluster;
  for (let i = 0; i < n; i++) (groups.get(key[i]) || groups.set(key[i], []).get(key[i])).push(i);
  ctx.font = "600 13px system-ui, sans-serif"; ctx.textAlign = "center"; ctx.textBaseline = "middle";
  for (const [g, idx] of groups) {
    if (state.colour === "truth" && g >= 6) continue;
    const med = (f) => idx.map(f).sort((a, b) => a - b)[idx.length >> 1];
    const label = state.colour === "truth" ? menu.truth[g] : String(g + 1);
    const lx = med(view.px), ly = med(view.py), tw = ctx.measureText(label).width + 12;
    ctx.fillStyle = "rgba(11,16,32,.78)";
    ctx.beginPath(); ctx.roundRect(lx - tw / 2, ly - 11, tw, 22, 11); ctx.fill();
    ctx.fillStyle = INK; ctx.fillText(label, lx, ly);
  }
}

function drawLegend() {
  const e = data.embedding;
  let html = "";
  if (state.colour === "cluster") {
    html = data.markers.map((m) => `<button type="button" data-c="${m.cluster}" class="${state.selected === m.cluster ? "on" : ""}">` +
      `<i style="background:${CAT[m.cluster]}"></i>${m.cluster + 1} · ${esc(m.major_type)} <span>${m.size}</span></button>`).join("");
  } else if (state.colour === "truth") {
    const n = menu.truth.map((_, t) => e.truth.filter((x) => x === t).length);
    html = menu.truth.map((name, t) => n[t] ? `<span><i style="background:${truthColour(t)}"></i>${esc(name)} <span>${n[t]}</span></span>` : "").join("");
  } else if (state.geneValues) {
    const g = state.geneValues;
    html = `<span class="ramp"><b>${esc(g.gene)}</b> 0 <i style="background:linear-gradient(90deg,${SEQ.join(",")})"></i> ${g.max.toFixed(2)} log1p` +
      ` <span>${g.pct_expressing}% of cells express it</span></span>`;
  }
  $("legend").innerHTML = html;
}

// SVG helpers: one frame per chart, recessive axes, ink for text
function frame(w, h, inner, axes = "") {
  return `<svg viewBox="0 0 ${w} ${h}" width="100%" role="img">${axes}${inner}</svg>`;
}
const text = (x, y, s, a = "middle", extra = "") => `<text x="${x}" y="${y}" fill="${MUTED}" font-size="11" text-anchor="${a}" ${extra}>${esc(s)}</text>`;

function histogram(values, groups, { lo, hi, bins = 40, log = false, cut = [], label }) {
  const W = 300, H = 150, L = 8, R = 8, T = 10, B = 30;
  const tf = (v) => log ? Math.log10(Math.max(v, 1)) : v;
  const [a, b] = [tf(lo), tf(hi)];
  const counts = [0, 1, 2].map(() => new Array(bins).fill(0));
  values.forEach((v, i) => {
    const k = Math.min(bins - 1, Math.max(0, Math.floor((tf(v) - a) / (b - a) * bins)));
    counts[groups[i]][k]++;
  });
  const top = Math.max(...counts[0].map((c, k) => c + counts[1][k] + counts[2][k]));
  const x = (v) => L + (tf(v) - a) / (b - a) * (W - L - R), bw = (W - L - R) / bins;
  const y = (c) => H - B - c / top * (H - B - T);
  let marks = "";
  const col = [CAT[0], ARTEFACT[0], CAT[4]];
  for (let k = 0; k < bins; k++) {
    let base = 0;
    for (const g of [0, 1, 2]) {
      const c = counts[g][k];
      if (!c) continue;
      marks += `<rect x="${L + k * bw + 1}" y="${y(base + c)}" width="${bw - 2}" height="${Math.max(0, y(base) - y(base + c) - 1)}" fill="${col[g]}" rx="1.5"><title>${c} cells</title></rect>`;
      base += c;
    }
  }
  // removed regions shaded, thresholds drawn
  for (const [v, side] of cut) {
    const cx = x(Math.min(hi, Math.max(lo, v)));
    marks += side < 0 ? `<rect x="${L}" y="${T}" width="${cx - L}" height="${H - B - T}" fill="rgba(255,255,255,.06)"/>`
      : `<rect x="${cx}" y="${T}" width="${W - R - cx}" height="${H - B - T}" fill="rgba(255,255,255,.06)"/>`;
    marks += `<line x1="${cx}" x2="${cx}" y1="${T - 4}" y2="${H - B}" stroke="${INK}" stroke-width="1.5" stroke-dasharray="3 3"/>`;
  }
  const ticks = log ? [100, 1000, 10000].filter((t) => t >= lo && t <= hi) : [lo, (lo + hi) / 2, hi];
  const axes = `<line x1="${L}" x2="${W - R}" y1="${H - B}" y2="${H - B}" stroke="${LINE}"/>` +
    ticks.map((t, i) => text(x(t), H - B + 14, log ? t.toLocaleString() : Math.round(t),
      x(t) < L + 12 ? "start" : x(t) > W - R - 12 ? "end" : "middle")).join("") + text(W / 2, H - 3, label);
  return frame(W, H, marks, axes);
}

function drawQC() {
  const q = data.qc, grp = q.truth.map((t) => t < 6 ? 0 : t === 6 ? 1 : 2);
  const p = params;
  $("qc").innerHTML =
    histogram(q.n_counts, grp, { lo: 100, hi: 20000, log: true, label: "counts per cell (log)", cut: [[p.min_counts, -1], [p.max_counts, 1]] }) +
    histogram(q.n_genes, grp, { lo: 0, hi: 2000, label: "genes detected", cut: [[p.min_genes, -1]] }) +
    histogram(q.pct_mito, grp, { lo: 0, hi: 70, label: "% mitochondrial", cut: [[p.max_mito, 1]] });
  // what the thresholds keep, counted here so it follows the slider before the server answers
  let kept = 0;
  for (let i = 0; i < q.truth.length; i++) {
    if (q.n_counts[i] >= p.min_counts && q.n_counts[i] <= p.max_counts && q.n_genes[i] >= p.min_genes && q.pct_mito[i] <= p.max_mito) kept++;
  }
  const rm = q.removed, tot = q.total;
  const cellsRemoved = menu.truth.slice(0, 6).reduce((a, t) => a + rm[t], 0);
  $("qc-note").innerHTML = `<span class="sw" style="background:${CAT[0]}"></span>cells · <span class="sw" style="background:${ARTEFACT[0]}"></span>doublets · ` +
    `<span class="sw" style="background:${CAT[4]}"></span>damaged or empty. Kept <b>${kept.toLocaleString()}</b>; removed ` +
    `<b>${rm["damaged / empty"]}/${tot["damaged / empty"]}</b> damaged or empty, <b>${rm.doublet}/${tot.doublet}</b> doublets ` +
    `and <b>${cellsRemoved}</b> healthy cells.`;
}

function drawHVG() {
  const h = data.hvg, W = 300, H = 200, L = 30, R = 8, T = 8, B = 30;
  const xs = h.log_mean, zs = h.z.map((z) => z === null ? NaN : Math.max(-3, Math.min(12, z)));
  const fin = (a) => a.filter(Number.isFinite);
  const [x0, x1] = [Math.min(...fin(xs)), Math.max(...fin(xs))], [y0, y1] = [-3, 12];
  const x = (v) => L + (v - x0) / (x1 - x0) * (W - L - R), y = (v) => H - B - (v - y0) / (y1 - y0) * (H - B - T);
  const top = new Set(h.top), planted = new Set(h.planted);
  let rest = "", hv = "", pl = "";
  xs.forEach((v, g) => {
    if (!Number.isFinite(zs[g])) return;
    const c = `<circle cx="${x(v).toFixed(1)}" cy="${y(zs[g]).toFixed(1)}" r="${planted.has(g) ? 2.6 : 1.8}"`;
    const name = menu.genes[g];
    if (planted.has(g)) pl += `${c} fill="${CAT[1]}" stroke="#0b1020" stroke-width=".8"><title>${name} · planted</title></circle>`;
    else if (top.has(g)) hv += `${c} fill="${CAT[0]}"><title>${name}</title></circle>`;
    else rest += `${c} fill="#46506e"><title>${name}</title></circle>`;
  });
  const axes = `<line x1="${L}" x2="${W - R}" y1="${y(0)}" y2="${y(0)}" stroke="${LINE}"/>` +
    text(W / 2, H - 3, "log10 mean expression") + text(10, (H - B) / 2, "dispersion (z)", "middle", `transform="rotate(-90 10 ${(H - B) / 2})"`) +
    [0, 5, 10].map((t) => text(L - 4, y(t) + 4, t, "end")).join("");
  $("hvg").innerHTML = frame(W, H, rest + hv + pl, axes);
  $("hvg-note").innerHTML = `<span class="sw" style="background:${CAT[0]}"></span>selected · <span class="sw" style="background:#46506e"></span>other · ` +
    `<span class="sw" style="background:${CAT[1]}"></span>planted programme genes: <b>${h.planted_in_hvg}/${h.planted.length}</b> selected.`;
}

function drawElbow() {
  const v = data.pca.variance_ratio.map((x) => x * 100), W = 300, H = 200, L = 30, R = 8, T = 8, B = 30;
  const bw = (W - L - R) / v.length, top = Math.max(...v);
  const y = (p) => H - B - p / top * (H - B - T);
  const bars = v.map((p, i) => `<rect x="${(L + i * bw + 1).toFixed(1)}" y="${y(p).toFixed(1)}" width="${(bw - 2).toFixed(1)}" height="${(H - B - y(p)).toFixed(1)}" rx="1.5" ` +
    `fill="${i < params.n_pcs ? CAT[0] : "#46506e"}"><title>PC${i + 1}: ${p.toFixed(2)}% of variance</title></rect>`).join("");
  const axes = `<line x1="${L}" x2="${W - R}" y1="${H - B}" y2="${H - B}" stroke="${LINE}"/>` +
    [1, 10, 20, 30, 40, 50].map((k) => text(L + (k - 0.5) * bw, H - B + 14, k)).join("") + text(W / 2, H - 3, "principal component") +
    [0, top / 2, top].map((t) => text(L - 4, y(t) + 4, t.toFixed(1), "end")).join("") +
    text(10, (H - B) / 2, "% variance", "middle", `transform="rotate(-90 10 ${(H - B) / 2})"`);
  $("elbow").innerHTML = frame(W, H, bars, axes);
  const used = v.slice(0, params.n_pcs).reduce((a, b) => a + b, 0);
  $("pca-note").innerHTML = `The first <b>${params.n_pcs}</b> PCs hold <b>${used.toFixed(1)}%</b> of the variance in the selected genes; past the elbow, the bars are noise.`;
}

function drawDot() {
  const d = data.dot, k = data.markers.length, G = d.genes.length;
  const planted = new Set(data.markers.flatMap((m) => m.genes.filter((g) => g.planted).map((g) => g.gene)));
  const cw = 22, rh = 24, L = 150, T = 70, W = L + G * cw + 10, H = T + k * rh + 34;
  let marks = "";
  d.genes.forEach((g, j) => {
    const cx = L + j * cw + cw / 2;
    marks += `<text x="${cx}" y="${T - 8}" fill="${planted.has(g) ? INK : MUTED}" font-size="11" font-weight="${planted.has(g) ? 650 : 400}" ` +
      `transform="rotate(-55 ${cx} ${T - 8})" class="gene-label" data-gene="${esc(g)}">${esc(g)}</text>`;
  });
  data.markers.forEach((m, c) => {
    const cy = T + c * rh + rh / 2;
    marks += `<g class="row-label" data-c="${c}"><circle cx="10" cy="${cy}" r="5" fill="${CAT[c]}"/>` +
      `<text x="22" y="${cy + 4}" fill="${INK}" font-size="12">${c + 1} · ${esc(m.major_type)}</text></g>`;
    d.genes.forEach((g, j) => {
      const f = d.frac[c][j], s = d.scaled[c][j];
      marks += `<circle cx="${L + j * cw + cw / 2}" cy="${cy}" r="${(1 + 9 * f).toFixed(1)}" fill="${seq(0.15 + 0.85 * s)}">` +
        `<title>${g} in cluster ${c + 1}: ${(f * 100).toFixed(0)}% of cells, relative mean ${s.toFixed(2)}</title></circle>`;
    });
  });
  const ly = T + k * rh + 20;
  const legend = [0.25, 0.5, 1].map((f, i) => `<circle cx="${L + i * 46}" cy="${ly}" r="${1 + 9 * f}" fill="${MUTED}" opacity=".6"/>` +
    text(L + i * 46 + 14, ly + 4, `${f * 100}%`, "start")).join("") + text(L - 8, ly + 4, "share expressing", "end") +
    `<rect x="${L + 150}" y="${ly - 5}" width="80" height="10" rx="5" fill="url(#seq)"/>` + text(L + 236, ly + 4, "mean, scaled per gene", "start") +
    `<defs><linearGradient id="seq">${SEQ.map((c, i) => `<stop offset="${i / (SEQ.length - 1)}" stop-color="${c}"/>`).join("")}</linearGradient></defs>`;
  $("dot").innerHTML = `<svg viewBox="0 0 ${Math.max(W, L + 400)} ${H}" width="${Math.max(W, L + 400)}" role="img">${marks}${legend}</svg>` +
    `<p class="note">Wilcoxon rank-sum, each cluster against the rest; bold genes are the ones the simulation planted in that cluster's type. Click a gene to colour the cells by it.</p>`;
}

function drawTable() {
  const t = data.table, names = menu.truth;
  const head = `<tr><th></th>${names.map((n) => `<th>${esc(n)}</th>`).join("")}</tr>`;
  const rows = t.map((row, c) => {
    const sum = row.reduce((a, b) => a + b, 0) || 1;
    return `<tr><th><i style="background:${CAT[c]}"></i>${c + 1}</th>` +
      row.map((v) => `<td style="background:${v ? seq(0.1 + 0.9 * v / sum) : "transparent"};color:${v / sum > 0.55 ? "#0b1020" : INK}">${v || ""}</td>`).join("") + "</tr>";
  }).join("");
  $("table").innerHTML = `<table class="heat">${head}${rows}</table><p class="note">Rows are clusters, columns the simulated truth. ` +
    `ARI ${data.ari.toFixed(3)} over the singlets kept: 1 is a perfect match, 0 is chance.</p>`;
}

function drawClusterCard() {
  const c = state.selected;
  $("cluster-card").hidden = c === null;
  if (c === null) return;
  const m = data.markers[c];
  $("cc-title").innerHTML = `<i style="background:${CAT[c]}"></i>Cluster ${c + 1}`;
  $("cc-sub").textContent = `${m.size} cells · ${(m.purity * 100).toFixed(0)}% ${m.major_type}`;
  $("cc-table").innerHTML = `<tr><th>gene</th><th>z</th><th>log2 FC</th><th>% in</th><th>adj. p</th></tr>` +
    (m.genes.length ? m.genes.map((g) => `<tr><td><button type="button" class="link" data-gene="${esc(g.gene)}">${esc(g.gene)}</button>${g.planted ? " ✓" : ""}</td>` +
      `<td>${g.z}</td><td>${g.lfc}</td><td>${g.pct_in}</td><td>${g.padj.toExponential(1)}</td></tr>`).join("")
      : `<tr><td colspan="5">no gene passes log2 FC > 0.5 and adj. p < 0.01</td></tr>`);
}

// ---- interaction -------------------------------------------------------------------------

function nearest(ev) {
  if (!data || !view) return -1;
  const b = $("embedding").getBoundingClientRect(), mx = ev.clientX - b.left, my = ev.clientY - b.top;
  let best = -1, bd = 100;                                   // within 10 px
  for (let i = 0; i < data.embedding.x.length; i++) {
    const d = (view.px(i) - mx) ** 2 + (view.py(i) - my) ** 2;
    if (d < bd) { bd = d; best = i; }
  }
  return best;
}

function select(c) {
  state.selected = state.selected === c ? null : c;
  if (state.colour === "gene") setColour("cluster");
  else { drawEmbedding(); drawLegend(); drawClusterCard(); }
}

function setColour(k) {
  if (k === "gene" && !state.geneValues) { $("gene").focus(); return; }
  state.colour = k;
  for (const x of $("colour-by").children) x.classList.toggle("on", x.dataset.k === k);
  drawEmbedding(); drawLegend(); drawClusterCard();
}

$("embedding").addEventListener("mousemove", (ev) => {
  const i = nearest(ev), tip = $("tip");
  if (i < 0) { tip.hidden = true; return; }
  const e = data.embedding;
  tip.innerHTML = `cluster <b>${e.cluster[i] + 1}</b> · ${esc(menu.truth[e.truth[i]])}` +
    (state.geneValues && state.colour === "gene" ? ` · ${esc(state.geneValues.gene)} <b>${state.geneValues.values[i].toFixed(2)}</b>` : "");
  tip.style.left = `${view.px(i) + 12}px`; tip.style.top = `${view.py(i) + 12}px`;
  tip.hidden = false;
});
$("embedding").addEventListener("mouseleave", () => { $("tip").hidden = true; });
$("embedding").addEventListener("click", (ev) => { const i = nearest(ev); if (i >= 0) select(data.embedding.cluster[i]); });
$("legend").addEventListener("click", (e) => { const b = e.target.closest("button[data-c]"); if (b) select(+b.dataset.c); });
$("colour-by").addEventListener("click", (e) => { const b = e.target.closest("button"); if (b) setColour(b.dataset.k); });
$("gene-form").addEventListener("submit", (e) => { e.preventDefault(); if ($("gene").value.trim()) loadGene($("gene").value.trim()); });
$("gene").addEventListener("change", () => { if (menu.genes.includes($("gene").value)) loadGene($("gene").value); });
$("cc-close").addEventListener("click", () => select(state.selected));
document.addEventListener("click", (e) => {
  const g = e.target.closest("[data-gene]");
  if (g) loadGene(g.dataset.gene);
  const r = e.target.closest(".row-label");
  if (r) select(+r.dataset.c);
});
$("reseed").addEventListener("click", () => {
  params.seed = (params.seed + 1) % 10001;
  $("seed-v").textContent = params.seed;
  run();
});
new ResizeObserver(() => { if (data) drawEmbedding(); }).observe($("embedding"));

// on a phone the steps start folded to their titles, so the embedding and the cards come first
if (matchMedia("(max-width: 860px)").matches) for (const d of document.querySelectorAll("details")) d.open = false;

fetch("/api/menu").then((r) => r.json()).then((m) => {
  menu = m;
  for (const [k, s] of Object.entries(m.settings)) params[k] = s.default;
  for (const [box, list] of Object.entries(SLIDERS)) list.forEach((s) => slider(box, s));
  segment("embedding-k", EMBEDDINGS, "embedding");
  segment("method", METHODS, "method");
  $("genes").innerHTML = m.genes.filter((g) => !/^G\d{4}$/.test(g)).map((g) => `<option value="${g}">`).join("");
  run();
});
