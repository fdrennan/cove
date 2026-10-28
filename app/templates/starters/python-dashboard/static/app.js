// Polls /api/stats every 2 s for samples newer than the last one it holds,
// keeps five minutes of them and draws rolling charts on canvas.
"use strict";

const $ = (id) => document.getElementById(id);
const css = getComputedStyle(document.documentElement);
const C = { s1: css.getPropertyValue("--s1").trim(), s2: css.getPropertyValue("--s2").trim() };
const KEEP = 150, POLL = 2000;
let samples = [], lastT = 0, lastOk = 0;

// ---- formatting -------------------------------------------------------------

const bytes = (b) => {
  const u = ["B", "KB", "MB", "GB", "TB", "PB"];
  let i = 0;
  while (b >= 1024 && i < u.length - 1) { b /= 1024; i++; }
  return `${b < 10 && i ? b.toFixed(1) : Math.round(b)} ${u[i]}`;
};
const rate = (b) => bytes(b) + "/s";
const pct = (v) => `${Math.round(v)}%`;
const dur = (s) => {
  const d = Math.floor(s / 86400), h = Math.floor((s % 86400) / 3600), m = Math.floor((s % 3600) / 60);
  return d ? `${d}d ${h}h` : h ? `${h}h ${m}m` : `${m}m`;
};
const clock = (t) => new Date(t * 1000).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
const esc = (s) => String(s).replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);

// ---- a rolling line chart with a hover crosshair ---------------------------------

const tip = $("tip");
class Chart {
  constructor(canvas, series, { max = null, fmt = pct } = {}) {
    Object.assign(this, { canvas, series, max, fmt, hover: null });
    this.g = canvas.getContext("2d");
    canvas.addEventListener("pointermove", (e) => { this.hover = e; this.draw(); });
    canvas.addEventListener("pointerleave", () => { this.hover = null; tip.hidden = true; this.draw(); });
  }
  draw() {
    const { canvas, g } = this, dpr = Math.min(devicePixelRatio || 1, 2);
    const r = canvas.getBoundingClientRect(), w = r.width, h = r.height;
    if (canvas.width !== Math.round(w * dpr)) { canvas.width = w * dpr; canvas.height = h * dpr; }
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    const n = samples.length;
    const top = this.max ?? Math.max(1, ...this.series.flatMap((s) => samples.map(s.get))) * 1.15;
    g.font = "12px system-ui, sans-serif";
    const L = Math.ceil(g.measureText(this.fmt(top)).width) + 16, R = 8, T = 6, B = 20;
    const X = (i) => L + ((KEEP - n + i) / (KEEP - 1)) * (w - L - R);
    const Y = (v) => T + (1 - v / top) * (h - T - B);

    g.textAlign = "right";
    for (let k = 0; k <= 4; k++) {
      const v = (top * k) / 4, y = Y(v);
      g.strokeStyle = "rgba(255,255,255,0.06)"; g.lineWidth = 1;
      g.beginPath(); g.moveTo(L, y); g.lineTo(w - R, y); g.stroke();
      if (k) { g.fillStyle = "rgba(154,166,194,0.85)"; g.fillText(this.fmt(v), L - 8, y + 4); }
    }
    g.textAlign = "left"; g.fillStyle = "rgba(154,166,194,0.7)";
    g.fillText("−5 min", L, h - 4); g.textAlign = "right"; g.fillText("now", w - R, h - 4);
    if (n < 2) return;

    for (const s of this.series) {
      const fill = g.createLinearGradient(0, T, 0, h - B);
      fill.addColorStop(0, s.color + "55"); fill.addColorStop(1, s.color + "00");
      g.beginPath(); g.moveTo(X(0), Y(0));
      samples.forEach((p, i) => g.lineTo(X(i), Y(s.get(p))));
      g.lineTo(X(n - 1), Y(0)); g.closePath(); g.fillStyle = fill; g.fill();
      g.beginPath(); samples.forEach((p, i) => (i ? g.lineTo(X(i), Y(s.get(p))) : g.moveTo(X(i), Y(s.get(p)))));
      g.strokeStyle = s.color; g.lineWidth = 2; g.lineJoin = "round"; g.stroke();
    }

    if (this.hover) {
      const mx = this.hover.clientX - r.left;
      const i = Math.max(0, Math.min(n - 1, Math.round(((mx - L) / (w - L - R)) * (KEEP - 1) - (KEEP - n))));
      const p = samples[i], x = X(i);
      g.strokeStyle = "rgba(255,255,255,0.35)"; g.lineWidth = 1;
      g.beginPath(); g.moveTo(x, T); g.lineTo(x, h - B); g.stroke();
      for (const s of this.series) {
        g.beginPath(); g.arc(x, Y(s.get(p)), 4.5, 0, 7);
        g.fillStyle = s.color; g.fill(); g.lineWidth = 2; g.strokeStyle = "#0e1424"; g.stroke();
      }
      tip.innerHTML = `<div class="when">${clock(p.t)}</div>` +
        this.series.map((s) => `<div><i style="background:${s.color}"></i>${s.name} <b>${this.fmt(s.get(p))}</b></div>`).join("");
      tip.hidden = false;
      const tx = this.hover.clientX + 14, ty = this.hover.clientY + 14;
      tip.style.left = Math.min(tx, innerWidth - tip.offsetWidth - 8) + "px";
      tip.style.top = Math.min(ty, innerHeight - tip.offsetHeight - 8) + "px";
    }
  }
}

const charts = [
  new Chart($("cpu"), [{ name: "CPU", color: C.s1, get: (p) => p.cpu }], { max: 100 }),
  new Chart($("mem"), [{ name: "Memory", color: C.s1, get: (p) => p.mem.percent },
                       { name: "Swap", color: C.s2, get: (p) => p.mem.swap_percent }], { max: 100 }),
  new Chart($("net"), [{ name: "Received", color: C.s1, get: (p) => p.net.rx },
                       { name: "Sent", color: C.s2, get: (p) => p.net.tx }], { fmt: rate }),
  new Chart($("io"), [{ name: "Read", color: C.s1, get: (p) => p.disk_io.read },
                      { name: "Write", color: C.s2, get: (p) => p.disk_io.write }], { fmt: rate }),
];
const legend = (a, b) => `<i style="background:${C.s1}"></i>${a}&nbsp;&nbsp;&nbsp;<i style="background:${C.s2}"></i>${b}`;

// ---- the cards ----------------------------------------------------------------------

function cores(list) {
  const box = $("cores");
  if (box.children.length !== list.length) {
    box.innerHTML = list.map((_, i) => `<div class="core"><div class="fill"></div><small>${i}</small><span></span></div>`).join("");
  }
  list.forEach((v, i) => {
    const el = box.children[i];
    el.firstChild.style.height = `${v}%`;
    el.lastChild.textContent = pct(v);
  });
}

function render(d) {
  const p = samples[samples.length - 1];
  if (p) {
    $("cpu-now").textContent = pct(p.cpu);
    $("load").textContent = p.load.join(" · ");
    $("mem-now").textContent = pct(p.mem.percent);
    $("mem-sub").innerHTML = legend(`${bytes(p.mem.used)} of ${bytes(p.mem.total)}`,
      p.mem.swap_total ? `swap ${bytes(p.mem.swap_used)} of ${bytes(p.mem.swap_total)}` : "no swap");
    $("net-now").textContent = rate(p.net.rx + p.net.tx);
    $("net-sub").innerHTML = legend(`in ${rate(p.net.rx)}`, `out ${rate(p.net.tx)}`);
    $("io-now").textContent = rate(p.disk_io.read + p.disk_io.write);
    $("io-sub").innerHTML = legend(`read ${rate(p.disk_io.read)}`, `write ${rate(p.disk_io.write)}`);
    cores(p.cores);
  }
  $("uptime").textContent = dur(d.uptime);
  $("disks").innerHTML = d.disks.map((k) => `
    <div class="disk">
      <div class="row"><b title="${esc(k.mount)}">${esc(k.mount)}</b><span>${bytes(k.used)} / ${bytes(k.total)} · ${pct(k.percent)}</span></div>
      <div class="bar"><div class="${k.percent > 85 ? "hot" : ""}" style="width:${k.percent}%"></div></div>
    </div>`).join("");
  $("procs").innerHTML = d.processes.map((q) => `
    <tr><td>${q.pid}</td><td class="name">${esc(q.name)}</td><td>${esc(q.user)}</td>
    <td class="n">${q.cpu.toFixed(1)}</td><td class="n">${bytes(q.rss)}</td></tr>`).join("");
  charts.forEach((c) => c.draw());
}

async function poll() {
  try {
    const d = await (await fetch(`/api/stats?since=${lastT}`, { cache: "no-store" })).json();
    samples.push(...d.samples);
    if (samples.length > KEEP) samples = samples.slice(-KEEP);
    if (samples.length) lastT = samples[samples.length - 1].t;
    lastOk = Date.now();
    render(d);
  } catch (e) {
    console.error(e);
  }
  const stale = Date.now() - lastOk > 3 * POLL;
  $("live").classList.toggle("stale", stale);
  $("age").textContent = stale ? "reconnecting" : "live";
  setTimeout(poll, POLL);
}

fetch("/api/info").then((r) => r.json()).then((i) => {
  $("host").textContent = `${i.host} · ${i.os} · ${i.cpus_logical} threads` +
    (i.cpus_physical ? ` on ${i.cpus_physical} cores` : "") + (i.cpu_mhz ? ` · ${(i.cpu_mhz / 1000).toFixed(1)} GHz` : "");
});
addEventListener("resize", () => charts.forEach((c) => c.draw()));
poll();
