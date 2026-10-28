// Orbital, the page: React components over one full-screen canvas.
//
// The clock (Julian date), the view (yaw, pitch, zoom, pan) and the animation
// live in refs, so the canvas redraws 60 times a second without React
// re-rendering anything; the panels read a snapshot of the clock 8 times a
// second through state. Positions come from window.Orbits (orbits.js), the same
// file the server answers /api/positions with.
"use strict";

const { useState, useEffect, useRef, useCallback } = React;
const html = htm.bind(React.createElement);
const O = window.Orbits;

const YEAR = 365.25;
const SCRUB = [O.parseDate("1800-01-01"), O.parseDate("2100-12-31")];
const PRESETS = [["Inner", 2.0], ["Belt", 3.6], ["Jupiter", 6.0], ["All", 50]];
const fmt = (x, d = 3) => (Math.abs(x) >= 1e4 ? x.toExponential(2) : x.toFixed(d));
const dayOf = (jd) => O.isoDate(jd).slice(0, 10);

// ---- hooks ---------------------------------------------------------------------------------

// Calls fn(dtSeconds) every animation frame while mounted, with the latest fn.
function useAnimationFrame(fn) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    let id = 0, last = 0;
    const tick = (now) => {
      const dt = last ? Math.min(0.1, (now - last) / 1000) : 0;
      last = now;
      ref.current(dt);
      id = requestAnimationFrame(tick);
    };
    id = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(id);
  }, []);
}

// A ref's value copied into state every `ms`: panels refresh at a readable rate.
function useSampled(read, ms) {
  const [v, setV] = useState(read);
  useEffect(() => {
    const id = setInterval(() => setV(read()), ms);
    return () => clearInterval(id);
  }, [read, ms]);
  return v;
}

// GET a JSON URL whenever `url` changes; null url waits.
function useJson(url) {
  const [s, setS] = useState({ data: null, error: null, loading: false });
  useEffect(() => {
    if (!url) { setS({ data: null, error: null, loading: false }); return; }   // a cleared question clears its answer
    let live = true;
    setS((p) => ({ ...p, loading: true }));
    fetch(url).then(async (r) => {
      const d = await r.json();
      if (!r.ok) throw new Error(d.error || r.statusText);
      if (live) setS({ data: d, error: null, loading: false });
    }).catch((e) => live && setS({ data: null, error: e.message, loading: false }));
    return () => { live = false; };
  }, [url]);
  return s;
}

// ---- the canvas ----------------------------------------------------------------------------

// Fixed background stars, from a small deterministic generator so they do not jump on resize.
const STARS = (() => {
  let s = 7; const r = () => ((s = (s * 16807) % 2147483647) / 2147483647);
  return Array.from({ length: 260 }, () => [r(), r(), 0.2 + 0.6 * r() ** 3]);
})();

function Stage({ sim, view, opts, selected, onSelect }) {
  const canvas = useRef(null);
  const paths = useRef({ key: null, rings: {} });

  // World (AU, ecliptic) -> screen. The radius can be drawn as sqrt(r) so Neptune
  // and Mercury fit one screen; directions stay true either way.
  const project = useCallback((x, y, z, w, h) => {
    const v = view.current;
    if (opts.sqrt) {
      const r = Math.hypot(x, y, z);
      if (r > 0) { const k = Math.sqrt(r) / r; x *= k; y *= k; z *= k; }
    }
    const cy = Math.cos(v.yaw), sy = Math.sin(v.yaw), cp = Math.cos(v.pitch), sp = Math.sin(v.pitch);
    const x1 = x * cy - y * sy, y1 = x * sy + y * cy;
    const up = y1 * cp + z * sp;
    return [w / 2 + v.panX + x1 * v.scale, h / 2 + v.panY - up * v.scale];
  }, [opts.sqrt]);

  useAnimationFrame((dt) => {
    const c = canvas.current;
    if (!c) return;
    const s = sim.current;
    if (s.playing) {
      s.jd += dt * s.speed;
      if (s.jd > SCRUB[1]) s.jd = SCRUB[0];
      if (s.jd < SCRUB[0]) s.jd = SCRUB[1];
    }
    const v = view.current;
    v.scale += (v.target - v.scale) * Math.min(1, dt * 8);    // ease zoom presets in
    const dpr = window.devicePixelRatio || 1, w = c.clientWidth, h = c.clientHeight;
    if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) { c.width = Math.round(w * dpr); c.height = Math.round(h * dpr); }
    const g = c.getContext("2d");
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    const P = (x, y, z) => project(x, y, z, w, h);

    for (const [sx, sy, a] of STARS) { g.fillStyle = `rgba(220,230,255,${a})`; g.fillRect(sx * w, sy * h, 1.2, 1.2); }

    // ecliptic rings at round distances, and the direction of the March equinox (+x)
    g.lineWidth = 1;
    for (const r of [1, 2, 5, 10, 20, 30, 40]) {
      g.strokeStyle = "rgba(255,255,255,0.05)";
      g.beginPath();
      for (let k = 0; k <= 96; k++) { const t = (2 * Math.PI * k) / 96, [px, py] = P(r * Math.cos(t), r * Math.sin(t), 0); k ? g.lineTo(px, py) : g.moveTo(px, py); }
      g.stroke();
      const [lx, ly] = P(r * Math.cos(-0.35), r * Math.sin(-0.35), 0);
      if (lx > 0 && lx < w && ly > 0 && ly < h) { g.fillStyle = "rgba(180,190,214,0.45)"; g.font = "11px system-ui"; g.fillText(`${r} AU`, lx + 3, ly); }
    }
    { const [ax, ay] = P(0, 0, 0), [bx, by] = P(opts.sqrt ? 7 : 45, 0, 0);
      g.strokeStyle = "rgba(255,255,255,0.08)"; g.setLineDash([4, 6]); g.beginPath(); g.moveTo(ax, ay); g.lineTo(bx, by); g.stroke(); g.setLineDash([]);
      g.fillStyle = "rgba(180,190,214,0.6)"; g.font = "12px system-ui"; g.fillText("♈ equinox", bx + 4, by); }

    const visible = O.BODIES.filter((b) => opts.small || !b.small);
    // orbits change slowly (rates per century): recompute the rings once a simulated year
    const key = `${Math.round(s.jd / YEAR)}`;
    if (paths.current.key !== key) { paths.current = { key, rings: {} }; }
    for (const b of visible) {
      const ring = paths.current.rings[b.id] || (paths.current.rings[b.id] = O.orbitPath(b, s.jd, b.small ? 360 : 240));
      g.strokeStyle = b.id === selected ? b.color : `${b.color}44`;
      g.lineWidth = b.id === selected ? 1.6 : 1;
      g.beginPath();
      for (let k = 0; k <= ring.length / 3; k++) { const j = (k % (ring.length / 3)) * 3, [px, py] = P(ring[j], ring[j + 1], ring[j + 2]); k ? g.lineTo(px, py) : g.moveTo(px, py); }
      g.stroke();
    }

    // the Sun
    const [sx, sy] = P(0, 0, 0);
    const glow = g.createRadialGradient(sx, sy, 0, sx, sy, 26);
    glow.addColorStop(0, "rgba(255,240,190,1)"); glow.addColorStop(0.25, "rgba(255,200,90,0.8)"); glow.addColorStop(1, "rgba(255,160,40,0)");
    g.fillStyle = glow; g.beginPath(); g.arc(sx, sy, 26, 0, 2 * Math.PI); g.fill();

    // trails, then bodies, far ones first so near ones sit on top
    const placed = [];
    for (const b of visible) {
      const p = O.position(b, s.jd);
      const [px, py] = P(p.x, p.y, p.z);
      placed.push({ b, p, px, py, depth: -(p.x * Math.sin(view.current.yaw) + p.y * Math.cos(view.current.yaw)) * Math.sin(view.current.pitch) + p.z * Math.cos(view.current.pitch) });
      if (opts.trails) {
        // the last 1/8 of an orbit (comets: 1/40, or the trail would cover the ellipse), brightening toward now
        const span = b.period / (b.kind === "comet" ? 40 : 8), n = 48;
        let prev = null;
        for (let k = n; k >= 0; k--) {
          const q = O.position(b, s.jd - (span * k) / n), pt = P(q.x, q.y, q.z);
          if (prev) { g.strokeStyle = b.color + Math.round(20 + 200 * (1 - k / n)).toString(16).padStart(2, "0"); g.lineWidth = 2; g.beginPath(); g.moveTo(...prev); g.lineTo(...pt); g.stroke(); }
          prev = pt;
        }
      }
    }
    placed.sort((m, n) => m.depth - n.depth);
    for (const { b, p, px, py } of placed) {
      const rad = b.size * Math.min(1.6, Math.max(0.7, Math.sqrt(view.current.scale / 120)));
      if (b.id === selected) {
        g.strokeStyle = "rgba(232,236,246,0.9)"; g.lineWidth = 1.4; g.beginPath(); g.arc(px, py, rad + 6, 0, 2 * Math.PI); g.stroke();
        g.strokeStyle = "rgba(232,236,246,0.25)"; g.setLineDash([3, 5]); g.beginPath(); g.moveTo(sx, sy); g.lineTo(px, py); g.stroke(); g.setLineDash([]);
      }
      g.fillStyle = b.color; g.beginPath(); g.arc(px, py, rad, 0, 2 * Math.PI); g.fill();
      if (b.id === "saturn") { g.strokeStyle = "rgba(243,223,162,0.7)"; g.lineWidth = 1.2; g.beginPath(); g.ellipse(px, py, rad * 2.1, rad * 0.7, -0.3, 0, 2 * Math.PI); g.stroke(); }
      if (b.kind === "comet") {
        // the tail points away from the Sun, longer near perihelion
        const r = Math.hypot(p.x, p.y, p.z), len = Math.min(60, 40 / Math.max(0.3, r));
        const dx = px - sx, dy = py - sy, d = Math.hypot(dx, dy) || 1;
        const tail = g.createLinearGradient(px, py, px + (dx / d) * len, py + (dy / d) * len);
        tail.addColorStop(0, `${b.color}cc`); tail.addColorStop(1, `${b.color}00`);
        g.strokeStyle = tail; g.lineWidth = 2.4; g.beginPath(); g.moveTo(px, py); g.lineTo(px + (dx / d) * len, py + (dy / d) * len); g.stroke();
      }
      if (opts.labels) { g.fillStyle = "rgba(232,236,246,0.85)"; g.font = "12px system-ui"; g.fillText(b.name, px + rad + 4, py - rad - 2); }
    }
    c._placed = placed;
  });

  // One finger or the mouse rotates; shift, right button or two fingers pan; wheel or pinch zooms.
  useEffect(() => {
    const c = canvas.current, pts = new Map();
    let moved = 0, pinch = null;
    const zoomAt = (k, mx, my) => {
      const v = view.current, cx = c.clientWidth / 2, cy = c.clientHeight / 2;
      const next = Math.max(4, Math.min(6000, v.target * k)), f = next / v.target;
      v.panX = mx - cx - (mx - cx - v.panX) * f; v.panY = my - cy - (my - cy - v.panY) * f;
      v.target = next; v.scale = next;
    };
    const down = (e) => { c.setPointerCapture(e.pointerId); pts.set(e.pointerId, [e.clientX, e.clientY, e.button === 2 || e.shiftKey]); moved = 0; };
    const move = (e) => {
      if (!pts.has(e.pointerId)) return;
      const [x0, y0, panning] = pts.get(e.pointerId), dx = e.clientX - x0, dy = e.clientY - y0, v = view.current;
      pts.set(e.pointerId, [e.clientX, e.clientY, panning]);
      moved += Math.abs(dx) + Math.abs(dy);
      if (pts.size === 2) {
        const [a, b] = [...pts.values()], d = Math.hypot(a[0] - b[0], a[1] - b[1]);
        if (pinch) zoomAt(d / pinch, (a[0] + b[0]) / 2, (a[1] + b[1]) / 2);
        pinch = d; v.panX += dx / 2; v.panY += dy / 2;
      } else if (panning) { v.panX += dx; v.panY += dy; }
      else { v.yaw += dx * 0.006; v.pitch = Math.max(0, Math.min(1.5, v.pitch + dy * 0.005)); }
    };
    const up = (e) => {
      if (pts.size === 1 && moved < 6 && c._placed) {
        // a tap, not a drag: select the nearest body within reach of the finger
        let best = null, bd = 22;
        for (const q of c._placed) { const d = Math.hypot(q.px - e.clientX, q.py - e.clientY); if (d < bd) { bd = d; best = q.b.id; } }
        onSelect(best);
      }
      pts.delete(e.pointerId); if (pts.size < 2) pinch = null;
    };
    const wheel = (e) => { e.preventDefault(); zoomAt(Math.exp(-e.deltaY * 0.0015), e.clientX, e.clientY); };
    const menu = (e) => e.preventDefault();
    c.addEventListener("pointerdown", down); c.addEventListener("pointermove", move);
    c.addEventListener("pointerup", up); c.addEventListener("pointercancel", up);
    c.addEventListener("wheel", wheel, { passive: false }); c.addEventListener("contextmenu", menu);
    return () => {
      c.removeEventListener("pointerdown", down); c.removeEventListener("pointermove", move);
      c.removeEventListener("pointerup", up); c.removeEventListener("pointercancel", up);
      c.removeEventListener("wheel", wheel); c.removeEventListener("contextmenu", menu);
    };
  }, [onSelect]);

  return html`<canvas id="stage" ref=${canvas} aria-label="The Solar System: drag to rotate, wheel or pinch to zoom, shift-drag to pan, tap a body to select it"></canvas>`;
}

// ---- panels --------------------------------------------------------------------------------

function Title({ jd, speed, playing, check }) {
  const inSpan = jd >= O.VALID[0] && jd <= O.VALID[1];
  return html`<header class="glass title">
    <h1>Orbital</h1>
    <p>The Solar System from JPL's Keplerian elements, Kepler's equation solved every frame</p>
    <div class="tiles">
      <div><span class="k">date (UTC)</span><b class=${inSpan ? "" : "warn"} title=${inSpan ? "" : "outside 1800-2050, the elements' fit span"}>${dayOf(jd)}</b></div>
      <div><span class="k">days / second</span><b>${playing ? (speed > 0 ? "" : "−") + fmt(Math.abs(speed), Math.abs(speed) < 10 ? 1 : 0) : "paused"}</b></div>
      <div title="largest gap between the server's /api/positions and this page's own positions, same date"><span class="k">server vs page</span><b>${check}</b></div>
    </div>
  </header>`;
}

function Info({ id, jd, onClose }) {
  if (!id) return null;
  const b = O.byId[id], s = O.state(b, jd), e = O.state(O.byId.earth, jd);
  const fromEarth = Math.hypot(s.x - e.x, s.y - e.y, s.z - e.z);
  const years = b.period / YEAR;
  return html`<section class="glass info" aria-live="polite">
    <button class="x" type="button" onClick=${onClose} aria-label="close">×</button>
    <h2><i style=${{ background: b.color }}></i>${b.name} <small>${b.kind}</small></h2>
    <dl>
      <dt>period</dt><dd>${years < 2 ? `${fmt(b.period, 2)} d` : `${fmt(years, 2)} y`}</dd>
      <dt class="more">a, e, i</dt><dd class="more">${fmt(s.a, 3)} AU, ${s.e.toFixed(3)}, ${s.i.toFixed(1)}°</dd>
      <dt>peri / aphelion</dt><dd>${fmt(s.a * (1 - s.e), 3)} / ${fmt(s.a * (1 + s.e), 2)} AU</dd>
      <dt>from the Sun</dt><dd>${s.r.toFixed(4)} AU</dd>
      <dt class="more">x, y, z (AU)</dt><dd class="more">${s.x.toFixed(3)}, ${s.y.toFixed(3)}, ${s.z.toFixed(3)}</dd>
      <dt>longitude, latitude</dt><dd>${s.lon.toFixed(2)}°, ${s.lat.toFixed(2)}°</dd>
      ${id !== "earth" && html`<dt>from Earth</dt><dd>${fromEarth.toFixed(3)} AU · ${(fromEarth * O.LIGHT_MIN_PER_AU).toFixed(1)} light-min</dd>`}
      <dt class="more">mean, true anomaly</dt><dd class="more">${s.M.toFixed(1)}°, ${s.nu.toFixed(1)}°</dd>
    </dl>
  </section>`;
}

function Seg({ items, value, onPick }) {
  return html`<div class="seg">${items.map(([label, v]) =>
    html`<button key=${label} type="button" class=${v === value ? "on" : ""} onClick=${() => onPick(v)}>${label}</button>`)}</div>`;
}

// Next dates two bodies line up (equal heliocentric longitude), asked of the server,
// and their separation on the shown date, computed here with the same orbits.js.
function Lineup({ jd, goTo }) {
  const [a, setA] = useState("earth"), [b, setB] = useState("mars");
  const [url, setUrl] = useState(null);
  const res = useJson(url);
  const sa = O.state(O.byId[a], jd), sb = O.state(O.byId[b], jd), e = O.state(O.byId.earth, jd);
  const sky = a !== "earth" && b !== "earth" && a !== b ? O.separation(sa, sb, e) : null;
  const sun = a !== b ? O.separation(sa, sb, { x: 0, y: 0, z: 0 }) : null;
  const pick = (v, set) => html`<select value=${v} onChange=${(ev) => { set(ev.target.value); setUrl(null); }}>
    ${O.BODIES.map((x) => html`<option key=${x.id} value=${x.id}>${x.name}</option>`)}</select>`;
  const d = res.data;
  return html`<div>
    <h3>Line up</h3>
    <div class="pick">${pick(a, setA)} and ${pick(b, setB)}</div>
    <p class="foot">${a === b ? "pick two different bodies" : html`On ${dayOf(jd)}: <b>${sun.toFixed(1)}°</b> apart from the Sun${sky != null ? html`, <b>${sky.toFixed(2)}°</b> on Earth's sky` : ""}`}</p>
    <div class="buttons"><button class="primary" type="button" disabled=${a === b || res.loading}
      onClick=${() => setUrl(`/api/lineup?a=${a}&b=${b}&from=${jd.toFixed(4)}&count=4`)}>${res.loading ? "Searching…" : "Next lineups"}</button></div>
    ${res.error && html`<p class="foot err">${res.error}</p>`}
    ${d && html`<div>
      <p class="foot">${d.what} · synodic period <b>${d.synodic_days.toFixed(1)} d</b>${d.mean_gap_days ? html` · mean gap <b>${d.mean_gap_days.toFixed(1)} d</b>` : ""} · found in ${d.search_ms} ms</p>
      <ul class="events">${d.events.length ? d.events.map((ev) => html`<li key=${ev.jd}>
        <span class=${ev.planets_in_fit_span ? "" : "warn"}>${ev.date}</span>
        <button type="button" onClick=${() => goTo(ev.jd)}>go</button></li>`) : html`<li>none within 400 years</li>`}</ul>
    </div>`}
  </div>`;
}

function Controls({ sim, view, opts, setOpts, jd, speed, playing, setRun }) {
  const [open, setOpen] = useState(window.innerWidth >= 760);
  const logSpeed = Math.log10(Math.abs(speed));
  const setSpeed = (v) => { sim.current.speed = v; setRun(); };
  return html`<details class="glass card" open=${open} onToggle=${(e) => setOpen(e.target.open)}>
    <summary>Time and view</summary>
    <label class="row">date <output>${dayOf(jd)}</output>
      <input type="range" min=${SCRUB[0]} max=${SCRUB[1]} step="1" value=${jd} aria-label="date"
        onChange=${(e) => { sim.current.jd = +e.target.value; setRun(); }} /></label>
    <div class="pick gap">
      <input type="date" value=${dayOf(jd)} min="1800-01-01" max="2100-12-31" aria-label="go to date"
        onChange=${(e) => { if (e.target.value) { sim.current.jd = O.parseDate(e.target.value); setRun(); } }} />
      <button type="button" class="chip" onClick=${() => { sim.current.jd = O.jdFromMs(Date.now()); setRun(); }}>today</button>
    </div>
    <label class="row">speed (days per second) <output>${fmt(Math.abs(speed), Math.abs(speed) < 10 ? 1 : 0)}</output>
      <input type="range" min="-1" max="4" step="0.05" value=${logSpeed} aria-label="speed, days per second, log scale"
        onChange=${(e) => setSpeed(Math.sign(speed) * 10 ** +e.target.value)} /></label>
    <div class="buttons">
      <button type="button" onClick=${() => setSpeed(-speed)}>${speed > 0 ? "◀ Reverse" : "Forward ▶"}</button>
      <button type="button" class="primary" onClick=${() => { sim.current.playing = !playing; setRun(); }}>${playing ? "Pause" : "Play"}</button>
    </div>
    <h3>Zoom</h3>
    <${Seg} items=${PRESETS} value=${null} onPick=${(au) => {
      const v = view.current, c = document.getElementById("stage");
      const r = opts.sqrt ? Math.sqrt(au) : au;
      v.target = Math.min(c.clientWidth, c.clientHeight) * 0.42 / r; v.panX = 0; v.panY = 0;
    }} />
    <${Seg} items=${[["Top down", 0], ["Tilted", 0.6], ["Edge on", 1.5]]} value=${null} onPick=${(p) => { view.current.pitch = p; }} />
    <h3>Show</h3>
    <${Seg} items=${[["Trails", "trails"], ["Labels", "labels"], ["Comets & asteroids", "small"], ["√r scale", "sqrt"]].map(([l, k]) => [l + (opts[k] ? " ✓" : ""), k])}
      value=${null} onPick=${(k) => setOpts({ ...opts, [k]: !opts[k] })} />
    <${Lineup} jd=${jd} goTo=${(t) => { sim.current.jd = t; sim.current.playing = false; setRun(); }} />
  </details>`;
}

// What runs, from which build: read live from GET /api/env each time it is opened.
function Env() {
  const [open, setOpen] = useState(false);
  const [n, setN] = useState(0);
  const { data, error } = useJson(open ? `/api/env?n=${n}` : null);
  const rows = data ? [
    ["runtime", `${data.runtime} ${data.version} (V8 ${data.v8})`],
    ["stack module", data.modules.length ? data.modules.join(", ") : "none loaded"],
    ...data.packages.map((p) => [p.name, `${p.version} · ${p.where}${p.sha256 ? ` · ${p.sha256}` : ""}`]),
    ["started", new Date(data.started).toLocaleString()],
    ["CPUs", `${data.cpus} · ${data.platform}`],
  ] : [];
  return html`<details class="glass env" onToggle=${(e) => { setOpen(e.target.open); if (e.target.open) setN((k) => k + 1); }}>
    <summary>Environment</summary>
    ${error && html`<p class="foot err">${error}</p>`}
    ${data && html`<table><tbody>${rows.map(([k, v]) => html`<tr key=${k}><td>${k}</td><td>${v}</td></tr>`)}</tbody></table>`}
  </details>`;
}

// ---- the app -------------------------------------------------------------------------------

function App() {
  const sim = useRef({ jd: O.jdFromMs(Date.now()), speed: 20, playing: true });
  const view = useRef({ yaw: 0, pitch: 0.55, scale: 140, target: 140, panX: 0, panY: 0 });
  const [opts, setOpts] = useState({ trails: true, labels: true, small: true, sqrt: false });
  const [selected, setSelected] = useState("earth");
  const [, bump] = useState(0);
  const setRun = useCallback(() => bump((k) => k + 1), []);
  const read = useCallback(() => ({ ...sim.current }), []);
  // a control change re-renders at once (setRun); otherwise the 8 Hz sample drives the panels
  useSampled(read, 125);
  const now = sim.current;

  // Fit the inner Solar System to the screen once, so a phone opens on Mars's orbit, not inside it.
  useEffect(() => {
    const v = view.current, s = Math.min(innerWidth, innerHeight) * 0.42 / 2.0;
    v.scale = v.target = s;
  }, []);

  // Ask the server where everything is on today's date and compare with this page's own
  // orbits.js: one set of equations, two runtimes, the gap should be rounding only.
  const [check, setCheck] = useState("…");
  useEffect(() => {
    const jd = sim.current.jd;
    fetch(`/api/positions?date=${jd.toFixed(6)}`).then((r) => r.json()).then((d) => {
      let worst = 0;
      for (const b of d.bodies) { const p = O.position(O.byId[b.id], d.jd); worst = Math.max(worst, Math.hypot(p.x - b.x, p.y - b.y, p.z - b.z)); }
      setCheck(`${worst.toExponential(0)} AU`);
    }).catch(() => setCheck("offline"));
  }, []);

  const onSelect = useCallback((id) => setSelected((cur) => id || cur), []);
  useEffect(() => {
    const key = (e) => { if (e.code === "Space" && e.target === document.body) { e.preventDefault(); sim.current.playing = !sim.current.playing; setRun(); } };
    addEventListener("keydown", key);
    return () => removeEventListener("keydown", key);
  }, [setRun]);

  return html`
    <${Stage} sim=${sim} view=${view} opts=${opts} selected=${selected} onSelect=${onSelect} />
    <${Title} jd=${now.jd} speed=${now.speed} playing=${now.playing} check=${check} />
    <${Info} id=${selected} jd=${now.jd} onClose=${() => setSelected(null)} />
    <aside class="side">
      <${Controls} sim=${sim} view=${view} opts=${opts} setOpts=${setOpts} jd=${now.jd} speed=${now.speed} playing=${now.playing} setRun=${setRun} />
      <${Env} />
    </aside>`;
}

ReactDOM.createRoot(document.getElementById("root")).render(html`<${App} />`);
