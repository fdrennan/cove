// Builds the controls from GET /api/menu, trains on Train, then retrains on
// every change (debounced, one request in flight).
"use strict";

const $ = (id) => document.getElementById(id);
const state = { dataset: "moons", model: "svm", params: {}, seed: 0 };
let menu = null, trained = false, busy = false, queued = false, timer = 0;

const show = (spec, v) => {
  if (spec.log) { const x = 10 ** v; return x >= 100 ? x.toFixed(0) : x >= 1 ? x.toFixed(2) : x.toPrecision(2); }
  return spec.integer ? String(Math.round(v)) : v.toFixed(2);
};

function slider(id, spec, value, onInput) {
  const label = document.createElement("label");
  label.className = "row";
  label.innerHTML = `${spec.label} <output>${show(spec, value)}</output>` +
    `<input type="range" min="${spec.min}" max="${spec.max}" step="${spec.step}" value="${value}" aria-label="${spec.label}">`;
  const input = label.querySelector("input"), out = label.querySelector("output");
  input.addEventListener("input", () => { const v = +input.value; out.textContent = show(spec, v); onInput(v); });
  return label;
}

function buildParams() {
  const box = $("params");
  box.innerHTML = "";
  state.params = {};
  for (const [name, spec] of Object.entries(menu.models[state.model].params)) {
    state.params[name] = spec.default;
    box.append(slider(name, spec, spec.default, (v) => { state.params[name] = v; changed(); }));
  }
}

function buildLegend(k) {
  const cls = menu.colors.slice(0, k).map((c, i) => `<span><i style="background:${c}"></i>class ${i + 1}</span>`).join("");
  $("legend").innerHTML = cls + `<span><i class="test" style="background:transparent"></i>held out</span>`;
}

async function train() {
  if (busy) { queued = true; return; }
  busy = true; trained = true;
  document.querySelector(".plot").classList.add("busy");
  const body = {
    dataset: state.dataset, model: state.model, params: state.params, seed: state.seed,
    n: +$("n").value, noise: +$("noise").value, test: +$("test").value,
  };
  try {
    const r = await fetch("/api/train", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
    const d = await r.json();
    if (!r.ok) throw new Error(JSON.stringify(d.detail));
    $("surface").src = d.png; $("surface").hidden = false; $("empty").hidden = true;
    $("acc-train").textContent = (d.train_acc * 100).toFixed(1) + "%";
    $("acc-test").textContent = (d.test_acc * 100).toFixed(1) + "%";
    $("fitms").textContent = d.fit_ms < 1000 ? `${Math.round(d.fit_ms)} ms` : `${(d.fit_ms / 1000).toFixed(1)} s`;
    buildLegend(d.classes);
  } catch (e) {
    console.error(e);
  } finally {
    busy = false;
    document.querySelector(".plot").classList.remove("busy");
    if (queued) { queued = false; train(); }
  }
}

function changed() {
  if (!trained) return;
  clearTimeout(timer);
  timer = setTimeout(train, 180);
}

for (const [id, fmt] of [["n", (v) => v], ["noise", (v) => (+v).toFixed(2)], ["test", (v) => Math.round(v * 100) + "%"]]) {
  $(id).addEventListener("input", () => { $(id + "-v").textContent = fmt($(id).value); changed(); });
}
$("train-btn").addEventListener("click", train);
$("reseed").addEventListener("click", () => { state.seed = (state.seed + 1) % 10000; if (trained) train(); });
$("model").addEventListener("change", () => { state.model = $("model").value; buildParams(); changed(); });

fetch("/api/menu").then((r) => r.json()).then((m) => {
  menu = m;
  $("datasets").innerHTML = Object.entries(m.datasets)
    .map(([k, label]) => `<button type="button" data-k="${k}" class="${k === state.dataset ? "on" : ""}">${label}</button>`).join("");
  $("datasets").addEventListener("click", (e) => {
    const b = e.target.closest("button");
    if (!b) return;
    state.dataset = b.dataset.k;
    for (const x of $("datasets").children) x.classList.toggle("on", x === b);
    changed();
  });
  $("model").innerHTML = Object.entries(m.models)
    .map(([k, v]) => `<option value="${k}" ${k === state.model ? "selected" : ""}>${v.label}</option>`).join("");
  buildParams();
  buildLegend(2);
});
