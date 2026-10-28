// Fits on load and refits on every change (debounced, one request in flight);
// draws the map, the topics and the k sweep from what the server answers.
"use strict";

const $ = (id) => document.getElementById(id);
const NS = "http://www.w3.org/2000/svg";
const S = { method: "nmf", k: 6, ngram: 1, min_df: 2, max_df: 0.5, perplexity: 15, seed: 0 };
let menu = null, fit = null, busy = false, queued = false, timer = 0, sweepKey = "", selected = null, placed = null;

const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const ms = (v) => (v < 1000 ? `${Math.round(v)} ms` : `${(v / 1000).toFixed(1)} s`);
const sx = (x) => 500 + x * 440;
const sy = (y) => 500 - y * 440;

// A marker shape per topic, so a topic reads by shape as well as by colour.
const SHAPES = [
  (x, y, r) => `M${x - r},${y}a${r},${r} 0 1,0 ${2 * r},0a${r},${r} 0 1,0 ${-2 * r},0`,
  (x, y, r) => { r *= 0.9; return `M${x - r},${y - r}h${2 * r}v${2 * r}h${-2 * r}z`; },
  (x, y, r) => { r *= 1.2; return `M${x},${y - r}L${x + r * 0.87},${y + r / 2}L${x - r * 0.87},${y + r / 2}z`; },
  (x, y, r) => { r *= 1.25; return `M${x},${y - r}L${x + r},${y}L${x},${y + r}L${x - r},${y}z`; },
  (x, y, r) => { r *= 1.2; return `M${x},${y + r}L${x + r * 0.87},${y - r / 2}L${x - r * 0.87},${y - r / 2}z`; },
  (x, y, r) => { const a = r * 0.4, b = r * 1.1; return `M${x - a},${y - b}h${2 * a}v${b - a}h${b - a}v${2 * a}h${a - b}v${b - a}h${-2 * a}v${a - b}h${a - b}v${-2 * a}h${b - a}z`; },
  (x, y, r) => star(x, y, r * 1.35, r * 0.6, 5),
  (x, y, r) => star(x, y, r * 1.1, r * 1.1 * 0.87, 6),
];
function star(x, y, ro, ri, n) {
  let d = "";
  for (let i = 0; i < 2 * n; i++) {
    const a = (Math.PI * i) / n - Math.PI / 2, r = i % 2 ? ri : ro;
    d += `${i ? "L" : "M"}${(x + r * Math.cos(a)).toFixed(1)},${(y + r * Math.sin(a)).toFixed(1)}`;
  }
  return d + "z";
}
const swatch = (t) => `<svg class="sw" viewBox="0 0 16 16" aria-hidden="true"><path d="${SHAPES[t](8, 8, 5)}" fill="${menu.colors[t]}"/></svg>`;
const topicName = (t) => `T${t + 1}`;
const topicWords = (t, n = 3) => fit.topics[t].words.slice(0, n).map((w) => w[0]).join(", ");

function el(tag, attrs, parent) {
  const e = document.createElementNS(NS, tag);
  for (const [k, v] of Object.entries(attrs)) e.setAttribute(k, v);
  if (parent) parent.append(e);
  return e;
}

async function post(path, body) {
  const r = await fetch(path, { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
  const d = await r.json();
  if (!r.ok) throw new Error(typeof d.detail === "string" ? d.detail : JSON.stringify(d.detail));
  return d;
}

// ---- fitting ----------------------------------------------------------------------------

async function refit() {
  if (busy) { queued = true; return; }
  busy = true;
  document.querySelector(".plot").classList.add("busy");
  try {
    fit = await post("/api/model", S);
    $("npmi").textContent = fit.coherence.npmi.toFixed(2);
    $("umass").textContent = fit.coherence.umass.toFixed(2);
    $("ami").textContent = fit.ami.toFixed(2);
    $("fitms").textContent = ms(fit.fit_ms + fit.map_ms);
    $("empty").hidden = true;
    drawMap(); drawTopics(); drawLegend();
    if (selected !== null) select(selected);
    if (placed) place();
    const key = [S.method, S.ngram, S.min_df, S.max_df, S.seed].join("|");
    if (key !== sweepKey) { sweepKey = key; sweep(); } else drawSweep();
  } catch (e) {
    $("fitms").textContent = "error";
    console.error(e);
    alertIn("detail-meta", e.message);
  } finally {
    busy = false;
    document.querySelector(".plot").classList.remove("busy");
    if (queued) { queued = false; refit(); }
  }
}

function changed() {
  clearTimeout(timer);
  timer = setTimeout(refit, 200);
}

let sweepRows = null;
async function sweep() {
  sweepRows = null; drawSweep();
  const key = sweepKey;
  try {
    const d = await post("/api/sweep", S);
    if (key === sweepKey) { sweepRows = d.rows; drawSweep(); }
  } catch (e) { console.error(e); }
}

// ---- drawing ----------------------------------------------------------------------------

// The map is drawn in a 1000-unit box; on a narrow screen that box shrinks,
// so marks and labels grow by z to stay legible.
let z = 1;
function drawMap() {
  const svg = $("map");
  z = Math.max(1, 640 / (svg.clientWidth || 640));
  svg.style.setProperty("--z", z);
  svg.innerHTML = "";
  const links = el("g", { class: "links" }, svg);
  const pts = el("g", { class: "pts" }, svg);
  for (const d of fit.docs) {
    const p = el("path", { d: SHAPES[d.topic](sx(d.x), sy(d.y), 9 * z), fill: menu.colors[d.topic], class: "pt", "data-i": d.i }, pts);
    if (d.i === selected) p.classList.add("sel");
  }
  // Each topic is named where its documents sit: the median of their positions.
  const labs = el("g", { class: "labs" }, svg);
  fit.topics.forEach((t, k) => {
    const m = fit.docs.filter((d) => d.topic === k);
    if (!m.length) return;
    const med = (a) => { a.sort((p, q) => p - q); return a[Math.floor(a.length / 2)]; };
    const x = sx(med(m.map((d) => d.x))), y = sy(med(m.map((d) => d.y))) - 26 * z;
    const txt = el("text", { x, y, "text-anchor": "middle", class: "lab", "font-size": 22 * z }, labs);
    txt.textContent = `${topicName(k)} · ${topicWords(k, 2)}`;
    // Keep the whole label inside the box when its topic sits at an edge.
    const half = txt.getComputedTextLength() / 2 + 8;
    txt.setAttribute("x", Math.min(1000 - half, Math.max(half, x)));
  });
  el("g", { id: "you" }, svg);
  svg.links = links;
}

function drawTopics() {
  $("topics").innerHTML = fit.topics.map((t) => {
    const max = t.words[0][1];
    const words = t.words.map(([w, v]) => `<span class="w" style="--a:${(0.45 + 0.55 * v / max).toFixed(2)}">${esc(w)}</span>`).join(" ");
    const field = t.field ? `${t.size} docs, ${Math.round(t.field_share * 100)}% ${esc(t.field)}` : "no documents";
    return `<li>${swatch(t.id)}<div><b>${topicName(t.id)}</b> <span class="meta">${field} · NPMI ${t.npmi.toFixed(2)} · UMass ${t.umass.toFixed(2)}</span><p>${words}</p></div></li>`;
  }).join("");
}

function drawLegend() {
  $("legend").innerHTML = fit.topics.map((t) => `<span>${swatch(t.id)}${topicName(t.id)} ${esc(topicWords(t.id, 2))}</span>`).join("")
    + `<span><svg class="sw" viewBox="0 0 16 16" aria-hidden="true"><path d="${star(8, 8, 7, 3, 4)}" fill="#fff"/></svg>your text</span>`;
}

// Two small charts, one scale each: coherence alone keeps rising as topics
// narrow (NMF here: NPMI 0.19 at k = 6, 0.31 at k = 8), while agreement with
// the fields peaks at the true number. Read together, they choose k.
const SWEEP = [["npmi", "NPMI"], ["ami", "vs fields"]];
function drawSweep() {
  const svg = $("sweep");
  svg.innerHTML = "";
  if (!sweepRows) { el("text", { x: 150, y: 80, "text-anchor": "middle", class: "dim" }, svg).textContent = "fitting every k…"; return; }
  const w = 270 / sweepRows.length;
  SWEEP.forEach(([key, name], row) => {
    const y0 = row * 80, vals = sweepRows.map((r) => r[key]);
    const lo = Math.min(0, ...vals), hi = Math.max(0, ...vals);
    const y = (v) => y0 + 16 + (hi - v) / (hi - lo || 1) * 46;
    const best = sweepRows.reduce((a, b) => (b[key] > a[key] ? b : a));
    el("text", { x: 0, y: y0 + 10, class: "dim" }, svg).textContent = name;
    for (const [i, r] of sweepRows.entries()) {
      const x = 30 + i * w, g = el("g", { class: "bar" + (r.k === S.k ? " on" : ""), "data-k": r.k }, svg);
      el("rect", { x, y: y0, width: w, height: 78, fill: "transparent" }, g);
      const top = Math.min(y(r[key]), y(0)), h = Math.max(2, Math.abs(y(r[key]) - y(0)));
      el("rect", { x: x + 5, y: top, width: w - 10, height: h, rx: 3 }, g);
      el("text", { x: x + w / 2, y: y0 + 76, "text-anchor": "middle", class: "dim" }, g).textContent = r.k;
      if (r === best || r.k === S.k) el("text", { x: x + w / 2, y: top - 3, "text-anchor": "middle", class: "val" }, g).textContent = r[key].toFixed(2);
      el("title", {}, g).textContent = `k = ${r.k}\nNPMI ${r.npmi.toFixed(3)}\nUMass ${r.umass.toFixed(3)}\nvs fields ${r.ami.toFixed(2)}`;
    }
    el("line", { x1: 30, x2: 300, y1: y(0), y2: y(0), class: "base" }, svg);
  });
}

function mixBar(mix) {
  return mix.map((v, t) => (v < 0.005 ? "" : `<i style="flex:${v};background:${menu.colors[t]}" title="${topicName(t)} ${Math.round(v * 100)}%"></i>`)).join("");
}

function neighbourList(rows) {
  $("neighbours").innerHTML = rows.map((r) =>
    `<li data-i="${r.i}">${swatch(r.topic)}<span>${esc(r.title)}<small>${esc(r.field)} · cosine ${r.score.toFixed(2)}</small></span></li>`).join("");
}

function alertIn(id, msg) { $(id).textContent = msg; }

// ---- picking and placing ----------------------------------------------------------------

async function select(i) {
  selected = i;
  for (const p of document.querySelectorAll(".pt")) p.classList.toggle("sel", +p.dataset.i === i);
  const doc = menu.corpus[i], d = fit.docs[i];
  $("detail-title").textContent = doc.title;
  $("detail-meta").innerHTML = `${esc(doc.field)} · ${topicName(d.topic)} (${esc(topicWords(d.topic))})`;
  $("detail-mix").innerHTML = mixBar(d.mix);
  $("detail-text").textContent = doc.text;
  try {
    const r = await post("/api/similar", { ...S, doc: i, n: 6 });
    if (selected === i) { neighbourList(r.neighbours); drawLinks(d, r.neighbours); }
  } catch (e) { alertIn("detail-meta", e.message); }
}

function drawLinks(from, rows) {
  const g = $("map").links;
  g.innerHTML = "";
  for (const r of rows) {
    const to = fit.docs[r.i];
    el("line", { x1: sx(from.x), y1: sy(from.y), x2: sx(to.x), y2: sy(to.y), "stroke-opacity": (0.25 + r.score).toFixed(2) }, g);
  }
}

async function place() {
  const text = $("paste").value.trim();
  if (!text || !fit) return;
  try {
    const r = await post("/api/place", { ...S, text, n: 6 });
    placed = r; selected = null;
    for (const p of document.querySelectorAll(".pt.sel")) p.classList.remove("sel");
    const you = $("you");
    you.innerHTML = "";
    el("path", { d: star(sx(r.x), sy(r.y), 20 * z, 8 * z, 4), class: "you" }, you);
    drawLinks(r, r.neighbours);
    $("placed").hidden = false;
    $("placed").textContent = `Lands in ${topicName(r.topic)} (${topicWords(r.topic)}), ${Math.round(r.mix[r.topic] * 100)}% of its mix. ${r.known_terms} of its terms are in the vocabulary.`;
    $("detail-title").textContent = "Your text";
    $("detail-meta").textContent = `${topicName(r.topic)} · strongest terms: ${r.terms.slice(0, 8).join(", ")}`;
    $("detail-mix").innerHTML = mixBar(r.mix);
    $("detail-text").textContent = text.length > 600 ? text.slice(0, 600) + "…" : text;
    neighbourList(r.neighbours);
  } catch (e) {
    placed = null;
    $("you").innerHTML = "";
    $("placed").hidden = false;
    $("placed").textContent = e.message;
  }
}

// ---- wiring -----------------------------------------------------------------------------

const tip = $("tip");
$("map").addEventListener("mousemove", (e) => {
  const p = e.target.closest(".pt");
  if (!p) { tip.hidden = true; return; }
  const i = +p.dataset.i, d = fit.docs[i], doc = menu.corpus[i];
  tip.innerHTML = `<b>${esc(doc.title)}</b><br>${esc(doc.field)} · ${topicName(d.topic)} ${Math.round(d.mix[d.topic] * 100)}%`;
  const box = document.querySelector(".plot").getBoundingClientRect();
  tip.style.left = `${Math.min(e.clientX - box.left + 14, box.width - 260)}px`;
  tip.style.top = `${e.clientY - box.top + 14}px`;
  tip.hidden = false;
});
$("map").addEventListener("mouseleave", () => { tip.hidden = true; });
$("map").addEventListener("click", (e) => { const p = e.target.closest(".pt"); if (p) select(+p.dataset.i); });
$("neighbours").addEventListener("click", (e) => { const li = e.target.closest("li"); if (li) select(+li.dataset.i); });
$("sweep").addEventListener("click", (e) => {
  const g = e.target.closest(".bar");
  if (!g) return;
  S.k = +g.dataset.k; $("k").value = S.k; $("k-v").textContent = S.k; changed();
});

const fmt = { k: (v) => v, ngram: (v) => v, min_df: (v) => v, max_df: (v) => Math.round(v * 100) + "%", perplexity: (v) => v };
for (const id of Object.keys(fmt)) {
  $(id).addEventListener("input", () => { S[id] = +$(id).value; $(id + "-v").textContent = fmt[id](S[id]); changed(); });
}
$("method").addEventListener("click", (e) => {
  const b = e.target.closest("button");
  if (!b) return;
  S.method = b.dataset.k;
  for (const x of $("method").children) x.classList.toggle("on", x === b);
  changed();
});
let resized = 0;
window.addEventListener("resize", () => {
  clearTimeout(resized);
  resized = setTimeout(() => {
    if (!fit) return;
    drawMap();
    if (placed) place(); else if (selected !== null) select(selected);
  }, 200);
});
$("reseed").addEventListener("click", () => { S.seed = (S.seed + 1) % 10000; changed(); });
$("place-btn").addEventListener("click", place);
$("paste").addEventListener("keydown", (e) => { if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) place(); });

fetch("/api/menu").then((r) => r.json()).then((m) => {
  menu = m;
  $("ndocs").textContent = m.corpus.length;
  $("nfields").textContent = m.fields.length;
  $("k").max = m.k_max;
  refit();
});
