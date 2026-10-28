// The landing page asks the API itself: the normal density follows the
// sliders; the three cards each show one live answer.
"use strict";

const $ = (id) => document.getElementById(id);
const dpr = Math.min(devicePixelRatio || 1, 2);

function fit(canvas) {
  const r = canvas.getBoundingClientRect();
  canvas.width = r.width * dpr; canvas.height = r.height * dpr;
  const g = canvas.getContext("2d");
  g.setTransform(dpr, 0, 0, dpr, 0, 0);
  return [g, r.width, r.height];
}

// Area plot of ys over xs with a glow; xr/yr fix the axes so motion reads.
function area(canvas, xs, ys, xr, yr, c1, c2) {
  const [g, w, h] = fit(canvas);
  const pad = 26, X = (x) => pad + ((x - xr[0]) / (xr[1] - xr[0])) * (w - 2 * pad);
  const Y = (y) => h - pad - ((y - yr[0]) / (yr[1] - yr[0])) * (h - 2 * pad);
  g.strokeStyle = "rgba(255,255,255,0.07)"; g.lineWidth = 1;
  g.fillStyle = "rgba(154,166,194,0.85)"; g.font = "13px system-ui, sans-serif";
  const step = (xr[1] - xr[0]) / 8;
  for (let i = 0; i <= 8; i++) {
    const x = xr[0] + i * step;
    g.beginPath(); g.moveTo(X(x), pad / 2); g.lineTo(X(x), h - pad); g.stroke();
    if (i % 2 === 0) g.fillText(+x.toFixed(1), X(x) - 8, h - 6);
  }
  g.beginPath(); g.rect(pad, 0, w - 2 * pad, h); g.clip();   // wide sigmas run past the axis
  const grad = g.createLinearGradient(0, Y(yr[1]), 0, Y(yr[0]));
  grad.addColorStop(0, c1 + "88"); grad.addColorStop(1, c1 + "00");
  g.beginPath(); g.moveTo(X(xs[0]), Y(yr[0]));
  xs.forEach((x, i) => g.lineTo(X(x), Y(ys[i])));
  g.lineTo(X(xs[xs.length - 1]), Y(yr[0])); g.closePath(); g.fillStyle = grad; g.fill();
  const line = g.createLinearGradient(pad, 0, w - pad, 0);
  line.addColorStop(0, c1); line.addColorStop(1, c2);
  g.beginPath(); xs.forEach((x, i) => (i ? g.lineTo(X(x), Y(ys[i])) : g.moveTo(X(x), Y(ys[i]))));
  g.strokeStyle = line; g.lineWidth = 3; g.shadowColor = c1; g.shadowBlur = 14; g.stroke(); g.shadowBlur = 0;
  return { g, X, Y };
}

// ---- GET /distributions/normal, live -------------------------------------------

let inflight = null, again = false;
async function normal() {
  if (inflight) { again = true; return; }
  const mu = +$("mu").value, sigma = +$("sigma").value;
  $("mu-v").textContent = mu.toFixed(1); $("sigma-v").textContent = sigma.toFixed(2);
  const url = `/distributions/normal?mu=${mu}&sigma=${sigma}&n=201`;
  $("url").textContent = url;
  inflight = fetch(url).then((r) => r.json());
  try {
    const d = await inflight;
    const top = Math.max(...d.density) * 1.3;   // the peak fills the frame; sigma shows as width
    const { g, X, Y } = area($("curve"), d.x, d.density, [-8, 8], [0, top], "#7cf0c5", "#8aa8ff");
    const lo = d.quantiles["0.025"], hi = d.quantiles["0.975"];
    g.fillStyle = "rgba(255,143,177,0.10)"; g.fillRect(X(lo), Y(top), X(hi) - X(lo), Y(0) - Y(top));
    g.fillStyle = "#ff8fb1"; g.font = "13px ui-monospace, monospace";
    g.fillText(`95%  [${lo.toFixed(2)}, ${hi.toFixed(2)}]`, X(lo) + 6, Y(top) + 16);
    $("ms").textContent = `${d.ms.toFixed(2)} ms`;
  } finally {
    inflight = null;
    if (again) { again = false; normal(); }
  }
}
$("mu").addEventListener("input", normal);
$("sigma").addEventListener("input", normal);
addEventListener("resize", normal);

// ---- the cards -------------------------------------------------------------------

const post = (url, body) => fetch(url, { method: "POST", headers: { "Content-Type": "application/json" },
  body: JSON.stringify(body) }).then((r) => r.json());
const f = (v, n = 3) => (v === null || v === undefined ? "–" : (+v).toFixed(n));

async function cards() {
  const sample = Array.from({ length: 60 }, (_, i) => +(50 + 12 * Math.sin(i * 1.7) + 8 * Math.cos(i * 0.37) + (i % 7)).toFixed(2));
  const s = await post("/stats/describe", { values: sample });
  $("describe").textContent =
    `n         ${s.n}\nmean      ${f(s.mean)}\nsd        ${f(s.sd)}\n95% CI    [${f(s.ci95_mean[0])}, ${f(s.ci95_mean[1])}]\n` +
    `median    ${f(s.median)}\nIQR       ${f(s.iqr)}\nskewness  ${f(s.skewness)}\n` +
    Object.entries(s.quantiles).map(([p, q]) => `q${p.padEnd(8)}${f(q)}`).join("\n") + `\n\n${f(s.ms, 2)} ms`;

  const x = Array.from({ length: 40 }, (_, i) => i / 4);
  const y = x.map((v, i) => +(1.5 + 2.2 * v + Math.sin(i * 2.3) * 1.4).toFixed(3));
  const l = await post("/fit/linear", { x, y });
  $("fit").textContent =
    `slope      ${f(l.slope, 4)} ± ${f(l.se_slope, 4)}\nintercept  ${f(l.intercept, 4)} ± ${f(l.se_intercept, 4)}\n` +
    `R²         ${f(l.r2, 4)}\nresid sd   ${f(l.residual_sd)}\np(slope)   ${(+l.p_slope).toExponential(2)}\n\n${f(l.ms, 2)} ms`;

  const o = await (await fetch("/ode/logistic?r=0.8&K=100&y0=2&t_end=16&n=161")).json();
  area($("logistic"), o.t, o.y, [0, 16], [0, 110], "#ff8fb1", "#8aa8ff");
  $("ode").textContent = `inflection t  ${f(o.inflection_t)}\nmax |error|   ${(+o.max_abs_error).toExponential(2)}\n` +
    `evaluations   ${o.evaluations}\n\n${f(o.ms, 2)} ms`;
}

normal();
cards().catch((e) => console.error(e));
