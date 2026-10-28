// The observatory page. Everything shown comes from the server: GET /api/stream
// (Server-Sent Events, a snapshot every 500 ms) paints the board; the load
// generator is a second event stream, GET /api/load, closed when it says done.
"use strict";

const $ = (id) => document.getElementById(id);
const css = (name) => getComputedStyle(document.documentElement).getPropertyValue(name).trim();
const COLORS = { accent: css("--accent"), s2: css("--s2"), s4: css("--s4"), s5: css("--s5"), muted: css("--muted"), text: css("--text") };
const statusClass = (code) => (code >= 500 || code === 0 ? "s5" : code >= 400 ? "s4" : "s2");

function ms(v) {
  if (v == null) return "–";
  if (v < 1) return v.toFixed(2) + " ms";
  if (v < 100) return v.toFixed(1) + " ms";
  if (v < 1000) return Math.round(v) + " ms";
  return (v / 1000).toFixed(2) + " s";
}
const pct = (v, d = 1) => (v == null ? "–" : (v * 100).toFixed(d) + "%");
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

// ---- canvas helpers ---------------------------------------------------------------------

function sized(canvas) {
  const r = canvas.getBoundingClientRect(), dpr = window.devicePixelRatio || 1;
  const w = Math.round(r.width * dpr), h = Math.round(r.height * dpr);
  if (canvas.width !== w || canvas.height !== h) { canvas.width = w; canvas.height = h; }
  const g = canvas.getContext("2d");
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  g.clearRect(0, 0, r.width, r.height);
  return { g, W: r.width, H: r.height };
}

function bar(g, x, y, w, h, color) {   // rounded at the data end, square at the baseline
  if (h <= 0 || w <= 0) return;
  const r = Math.min(4, w / 2, h);
  g.fillStyle = color;
  g.beginPath();
  g.moveTo(x, y + h); g.lineTo(x, y + r); g.quadraticCurveTo(x, y, x + r, y);
  g.lineTo(x + w - r, y); g.quadraticCurveTo(x + w, y, x + w, y + r); g.lineTo(x + w, y + h);
  g.fill();
}

function tip(el, canvas, x, y, html) {
  if (html == null) { el.hidden = true; return; }
  el.innerHTML = html; el.hidden = false;
  const box = canvas.parentElement.getBoundingClientRect(), c = canvas.getBoundingClientRect();
  const left = c.left - box.left + x + 12, maxLeft = box.width - el.offsetWidth - 8;
  el.style.left = Math.max(8, Math.min(left, maxLeft)) + "px";
  el.style.top = c.top - box.top + Math.max(0, y - 40) + "px";
}

// ---- latency histogram (log x) -------------------------------------------------------------

let last = null;
const PAD = { l: 40, r: 12, t: 26, b: 24 };

function drawHist(s) {
  const { g, W, H } = sized($("hist"));
  const { edges, counts } = s.hist;
  const lo = Math.log10(edges[0]), hi = Math.log10(edges[edges.length - 1]);
  const X = (v) => PAD.l + ((Math.log10(v) - lo) / (hi - lo)) * (W - PAD.l - PAD.r);
  const max = Math.max(1, ...counts);
  const Y = (c) => H - PAD.b - (c / max) * (H - PAD.t - PAD.b);
  g.font = "12px system-ui, sans-serif"; g.textBaseline = "middle";
  g.strokeStyle = "rgba(255,255,255,.07)"; g.fillStyle = COLORS.muted; g.lineWidth = 1;
  for (const [v, label] of [[0.1, "0.1 ms"], [1, "1 ms"], [10, "10 ms"], [100, "100 ms"], [1000, "1 s"], [10000, "10 s"]]) {
    const x = X(v);
    g.beginPath(); g.moveTo(x, PAD.t); g.lineTo(x, H - PAD.b); g.stroke();
    g.textAlign = "center"; g.fillText(label, x, H - PAD.b / 2);
  }
  g.textAlign = "right"; g.fillText(String(max), PAD.l - 6, PAD.t); g.fillText("0", PAD.l - 6, H - PAD.b);
  counts.forEach((c, i) => {
    const x0 = X(edges[i]) + 1, x1 = X(edges[i + 1]) - 1;
    bar(g, x0, Y(c), x1 - x0, H - PAD.b - Y(c), COLORS.accent);
  });
  // percentile markers; labels step down when they would collide
  let prevX = -1e9, row = 0;
  for (const [k, label] of [["p50", "p50"], ["p95", "p95"], ["p99", "p99"]]) {
    const v = s[k]; if (v == null) continue;
    const x = X(Math.min(Math.max(v, edges[0]), edges[edges.length - 1]));
    row = x - prevX < 70 ? row + 1 : 0; prevX = x;
    g.strokeStyle = COLORS.text; g.setLineDash([4, 4]); g.lineWidth = 1.5;
    g.beginPath(); g.moveTo(x, PAD.t - 4 + row * 14); g.lineTo(x, H - PAD.b); g.stroke(); g.setLineDash([]);
    g.fillStyle = COLORS.text; g.textAlign = x > W - 90 ? "right" : "left";
    g.fillText(`${label} ${ms(v)}`, x + (x > W - 90 ? -5 : 5), PAD.t - 10 + row * 14);
  }
  $("hist").onmousemove = (e) => {
    const r = $("hist").getBoundingClientRect(), x = e.clientX - r.left;
    const v = 10 ** (lo + ((x - PAD.l) / (W - PAD.l - PAD.r)) * (hi - lo));
    const i = edges.findIndex((edge, j) => j < counts.length && v >= edge && v < edges[j + 1]);
    tip($("hist-tip"), $("hist"), x, e.clientY - r.top, i < 0 ? null : `${ms(edges[i])} – ${ms(edges[i + 1])}: <b>${counts[i]}</b> requests`);
  };
  $("hist").onmouseleave = () => tip($("hist-tip"), $("hist"), 0, 0, null);
}

// ---- requests per second, last 60 s ---------------------------------------------------------

function drawTimeline(s) {
  const { g, W, H } = sized($("timeline"));
  const { ok, err } = s.timeline;
  const pad = { l: 34, r: 6, t: 8, b: 18 };
  const max = Math.max(1, ...ok.map((v, i) => v + err[i]));
  const bw = (W - pad.l - pad.r) / ok.length, Y = (c) => (c / max) * (H - pad.t - pad.b);
  g.font = "12px system-ui, sans-serif"; g.fillStyle = COLORS.muted; g.textBaseline = "middle";
  g.textAlign = "right"; g.fillText(String(max), pad.l - 6, pad.t + 4); g.fillText("0", pad.l - 6, H - pad.b);
  g.textAlign = "left"; g.fillText("−60 s", pad.l, H - 6); g.textAlign = "right"; g.fillText("now", W - pad.r, H - 6);
  ok.forEach((o, i) => {
    const x = pad.l + i * bw + 1, w = Math.max(1, bw - 2), base = H - pad.b;
    const ho = Y(o), he = Y(err[i]);
    if (he > 0) {   // errors sit on top of the answered requests, a 2 px gap between
      g.fillStyle = COLORS.s2; g.fillRect(x, base - ho, w, ho);
      bar(g, x, base - ho - he - (ho > 0 ? 2 : 0), w, he, COLORS.s5);
    } else {
      bar(g, x, base - ho, w, ho, COLORS.s2);
    }
  });
  $("timeline").onmousemove = (e) => {
    const r = $("timeline").getBoundingClientRect(), x = e.clientX - r.left;
    const i = Math.floor((x - pad.l) / bw);
    tip($("tl-tip"), $("timeline"), x, e.clientY - r.top,
      i < 0 || i >= ok.length ? null : `${i - ok.length + 1} s: <b>${ok[i]}</b> under 500, <b>${err[i]}</b> 5xx`);
  };
  $("timeline").onmouseleave = () => tip($("tl-tip"), $("timeline"), 0, 0, null);
}

// ---- the rest of the board ------------------------------------------------------------------

function render(s) {
  last = s;
  $("k-rps").textContent = s.rps.toFixed(s.rps < 10 ? 2 : 0);
  $("k-n").textContent = `${s.n.toLocaleString()} in ${Math.round(s.span_s)} s`;
  for (const k of ["p50", "p95", "p99"]) $("k-" + k).textContent = ms(s[k]);
  $("k-err").textContent = pct(s.err_rate);
  $("k-errn").textContent = s.n ? `${s.errors.toLocaleString()} of ${s.n.toLocaleString()}` : "";
  $("k-con").textContent = s.contract_checked ? (s.contract_fail ? `${s.contract_fail} broken` : "all valid") : "–";
  $("k-con").className = s.contract_fail ? "bad" : s.contract_checked ? "good" : "";
  $("k-conn").textContent = s.contract_checked ? `${s.contract_checked.toLocaleString()} checked` : "";
  $("hist-sub").textContent = `last ${s.window_s} s, log scale, ${s.n.toLocaleString()} requests`;

  drawHist(s);
  drawTimeline(s);

  const total = Object.values(s.status).reduce((a, b) => a + b, 0);
  $("mix-sub").textContent = total ? `${total.toLocaleString()} in the window` : "no traffic yet";
  $("mix").innerHTML = Object.entries(s.status).map(([c, n]) => `<span class="${statusClass(+c)}" style="flex-grow:${n}" title="${c}: ${n}"></span>`).join("");
  $("mix-list").innerHTML = Object.entries(s.status).map(([c, n]) =>
    `<li><i class="${statusClass(+c)}"></i><span>${c}</span><b>${n.toLocaleString()}</b><em>${pct(n / total)}</em></li>`).join("");

  $("endpoints").tBodies[0].innerHTML = s.endpoints.map((e) => `<tr>
    <td><code>${esc(e.path)}</code></td><td>${e.n.toLocaleString()}</td><td>${e.rps.toFixed(1)}</td>
    <td class="${e.errors ? "bad" : ""}">${e.n ? pct(e.err_rate) : "–"}</td>
    <td>${ms(e.p50)}</td><td>${ms(e.p95)}</td><td>${ms(e.p99)}</td>
    <td class="${e.contract_fail ? "bad" : e.contract_checked ? "good" : ""}">${e.contract_checked ? (e.contract_fail ? `${e.contract_fail} broken` : "valid") : "–"}</td></tr>`).join("");

  $("feed").innerHTML = s.recent.map((r) => `<li><span class="st ${statusClass(r.status)}">${r.status}</span>
    <span>${esc(r.path)}</span><span>${ms(r.ms)}</span><span class="muted">${r.contract === false ? "✗ contract" : r.contract ? "✓" : ""}</span></li>`).join("")
    || `<li class="muted">Waiting for traffic - run the load generator.</li>`;
  $("violations").innerHTML = s.violations.map((v) => `<li><span><code>${esc(v.path)}</code> ${esc(v.error)}</span><span class="muted">${Math.round(v.age_s)} s ago</span></li>`).join("")
    || `<li class="muted">None: every checked response matched its model.</li>`;

  if (!settingsLoaded) { settingsLoaded = true; for (const [k, v] of Object.entries(s.settings)) setSlider(k, v); }
}

// ---- settings sliders -----------------------------------------------------------------------

let settingsLoaded = false, pending = {}, sendTimer = 0;
const FMT = {
  window_s: (v) => `${v} s`, flaky_rate: (v) => pct(v, 0), slow_median_ms: (v) => `${v} ms`,
  slow_sigma: (v) => (+v).toFixed(2), drift: (v) => pct(v, 0),
};
function setSlider(k, v) { if (!$(k)) return; $(k).value = v; $(k + "-v").textContent = FMT[k](v); }
for (const k of Object.keys(FMT)) {
  $(k).addEventListener("input", () => {
    $(k + "-v").textContent = FMT[k]($(k).value);
    pending[k] = +$(k).value;
    clearTimeout(sendTimer);
    sendTimer = setTimeout(async () => {
      const body = pending; pending = {};
      await fetch("/api/settings", { method: "POST", headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) });
    }, 150);
  });
}
$("reset").addEventListener("click", () => fetch("/api/reset", { method: "POST" }));

// ---- live stream ------------------------------------------------------------------------------

function connect() {
  const es = new EventSource("/api/stream");
  es.addEventListener("snapshot", (e) => render(JSON.parse(e.data)));
  es.onopen = () => $("live").classList.add("on");
  es.onerror = () => $("live").classList.remove("on");   // EventSource reconnects by itself
}
connect();
window.addEventListener("resize", () => last && (drawHist(last), drawTimeline(last)));

// ---- load generator ---------------------------------------------------------------------------

const TARGETS = { fast: "fast", slow: "slow", flaky: "flaky", cpu: "cpu", items: "items" };
let target = "flaky", run = null;
$("targets").innerHTML = Object.entries(TARGETS).map(([k, l]) => `<button type="button" data-k="${k}" class="${k === target ? "on" : ""}">${l}</button>`).join("");
$("targets").addEventListener("click", (e) => {
  const b = e.target.closest("button"); if (!b) return;
  target = b.dataset.k;
  for (const x of $("targets").children) x.classList.toggle("on", x === b);
});
$("conc").addEventListener("input", () => ($("conc-v").textContent = $("conc").value));
$("dur").addEventListener("input", () => ($("dur-v").textContent = $("dur").value + " s"));

function showRun(d) {
  $("bar").style.width = Math.min(100, (d.elapsed / d.duration) * 100) + "%";
  const rows = [
    ["requests", d.done.toLocaleString()], ["rate", `${d.rps.toFixed(0)} /s`],
    ["p50 · p95 · p99", `${ms(d.p50)} · ${ms(d.p95)} · ${ms(d.p99)}`],
    ["5xx", d.err_rate == null ? "–" : `${pct(d.err_rate)} (95% CI ${pct(d.err_ci[0])}–${pct(d.err_ci[1])})`],
  ];
  if (d.setting != null) rows.push(["flaky setting", pct(d.setting)]);
  rows.push(["status", Object.entries(d.codes).map(([c, n]) => `${c === "0" ? "conn err" : c}×${n}`).join(" ") || "–"]);
  $("run-out").innerHTML = rows.map(([k, v]) => `<dt>${k}</dt><dd>${v}</dd>`).join("");
}

function stopRun(label) { if (run) run.close(); run = null; $("run").textContent = label || "Run load"; $("run").classList.add("primary"); }

$("run").addEventListener("click", () => {
  if (run) { stopRun(); return; }
  const q = new URLSearchParams({ endpoint: target, concurrency: $("conc").value, duration: $("dur").value });
  run = new EventSource("/api/load?" + q);
  $("run").textContent = "Stop"; $("run").classList.remove("primary");
  $("run-out").innerHTML = ""; $("bar").style.width = "0";
  run.addEventListener("tick", (e) => showRun(JSON.parse(e.data)));
  run.addEventListener("done", (e) => { showRun(JSON.parse(e.data)); $("bar").style.width = "100%"; stopRun(); });
  run.addEventListener("busy", () => { $("run-out").innerHTML = "<dt>busy</dt><dd>another load run is in progress</dd>"; stopRun(); });
  run.onerror = () => stopRun();   // the stream ended or failed: do not let EventSource retry a load run
});

// ---- contract check ---------------------------------------------------------------------------

$("check").addEventListener("click", async () => {
  $("check").disabled = true;
  try {
    const r = await fetch("/api/contract?calls=25", { method: "POST" });
    const d = await r.json();
    $("check-out").innerHTML = d.endpoints.map((e) => `<li>
      <span><b class="${e.failed ? "bad" : "good"}">${e.failed ? "✗" : "✓"}</b> <code>${esc(e.path)}</code>
      ${e.passed}/${e.checked} valid <span class="muted">· ${Object.entries(e.models).map(([s, m]) => `${s} → ${m}`).join(", ")}</span></span>
      ${e.first_error ? `<span class="err">${esc(e.first_error)}</span>` : ""}</li>`).join("");
  } finally {
    $("check").disabled = false;
  }
});

// ---- environment: read live each time the fold opens --------------------------------------------

$("env-fold").addEventListener("toggle", async () => {
  if (!$("env-fold").open) return;
  const e = await (await fetch("/api/env")).json();
  const rows = [
    ["runtime", `${e.runtime} ${e.version}`], ["stack module", e.stack_module || "none loaded"],
    ...Object.entries(e.packages), ["started", new Date(e.started).toLocaleString()],
    ["uptime", `${Math.round(e.uptime_s)} s`], ["CPUs", e.cpus], ["pid", e.pid], ["python", e.executable],
  ];
  $("env").innerHTML = rows.map(([k, v]) => `<dt>${esc(k)}</dt><dd>${esc(v)}</dd>`).join("");
});

// narrow screens: the board comes first and the panel's folds start closed
if (matchMedia("(max-width: 860px)").matches) for (const d of document.querySelectorAll(".fold")) d.open = false;
