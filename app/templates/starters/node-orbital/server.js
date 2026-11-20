// Orbital · __HOST__ - the Solar System from Keplerian elements, Node standard library only.
//
// The orbital maths is static/orbits.js, the same file the browser loads, so
// GET /api/positions answers with exactly the equations the page animates with.
// Routes: / and static files, /health, /api/env, /api/bodies, /api/positions,
// /api/lineup, /api/separation. Listens on 127.0.0.1 only; nginx decides who reaches it.
"use strict";

const http = require("node:http");
const fs = require("node:fs");
const path = require("node:path");
const log = require("./log");
const os = require("node:os");
const crypto = require("node:crypto");
const O = require("./static/orbits.js");

const PORT = Number(process.env.PORT);
if (!Number.isInteger(PORT) || PORT <= 0) {
  // env.sh sets PORT from app.env APP_PORT; a port typed here would collide with another app's.
  log.error("PORT is not set: start with ./run.sh, which reads it from app.env");
  process.exit(2);
}
const STATIC = path.join(__dirname, "static");
const STARTED = new Date();
const TYPES = { ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8", ".md": "text/markdown; charset=utf-8", ".json": "application/json" };
const MAX_HORIZON_DAYS = 400 * 365.25;   // enough for Jupiter-Saturn (~20 y) and Uranus-Neptune (~172 y)

// ---- the API ---------------------------------------------------------------------------------

const round = (x, d) => Math.round(x * 10 ** d) / 10 ** d;
const inRange = (jd) => jd >= O.VALID[0] && jd <= O.VALID[1];

function body(id) {
  const b = O.byId[String(id || "").toLowerCase()];
  if (!b) throw httpError(400, `unknown body "${id}"; one of ${Object.keys(O.byId).join(", ")}`);
  return b;
}

function httpError(status, message) { return Object.assign(new Error(message), { status }); }

function positions(q) {
  const jd = O.parseDate(q.get("date"));
  const earth = O.state(O.byId.earth, jd);
  return {
    date: O.isoDate(jd), jd: round(jd, 8), T: round((jd - O.J2000) / 36525, 8),
    frame: "heliocentric, ecliptic and mean equinox of J2000, AU",
    planets_in_fit_span: inRange(jd),
    bodies: O.BODIES.map((b) => {
      const s = O.state(b, jd);
      const fromEarth = Math.hypot(s.x - earth.x, s.y - earth.y, s.z - earth.z);
      return { id: b.id, name: b.name, kind: b.kind, x: round(s.x, 10), y: round(s.y, 10), z: round(s.z, 10),
        r: round(s.r, 10), lon: round(s.lon, 5), lat: round(s.lat, 5), M: round(s.M, 5), nu: round(s.nu, 5),
        from_earth_au: b.id === "earth" ? 0 : round(fromEarth, 6),
        light_min: b.id === "earth" ? 0 : round(fromEarth * O.LIGHT_MIN_PER_AU, 3) };
    }),
  };
}

function bodies() {
  return {
    source: {
      planets: "E. M. Standish, Keplerian Elements for Approximate Positions of the Major Planets, JPL SSD, Table 1 (1800-2050 AD)",
      small_bodies: "rounded osculating elements, JPL Small-Body Database; two-body, no perturbations",
    },
    planets_fit_span: [O.isoDate(O.VALID[0]).slice(0, 10), O.isoDate(O.VALID[1]).slice(0, 10)],
    bodies: O.BODIES.map((b) => {
      const el = O.elementsAt(b, O.J2000);
      return { id: b.id, name: b.name, kind: b.kind, color: b.color, size: b.size,
        period_days: round(b.period, 3), period_years: round(b.period / 365.25, 4),
        a: round(el.a, 6), e: round(el.e, 6), i: round(el.i / O.DEG, 4),
        perihelion_au: round(el.a * (1 - el.e), 5), aphelion_au: round(el.a * (1 + el.e), 5) };
    }),
  };
}

// Next dates the heliocentric longitudes of a and b coincide (opposition of an
// outer planet seen from Earth; inferior conjunction of an inner one).
function lineup(q) {
  const a = body(q.get("a") || "earth"), b = body(q.get("b") || "mars");
  if (a === b) throw httpError(400, "a and b must differ");
  const from = O.parseDate(q.get("from"));
  const count = Math.max(1, Math.min(30, Number(q.get("count")) || 1));
  const t0 = process.hrtime.bigint();
  const { events, synodic } = O.lineups(a, b, from, count, MAX_HORIZON_DAYS);
  const ms = Number(process.hrtime.bigint() - t0) / 1e6;
  const what = a.id === "earth" ? (b.period > a.period ? "opposition" : "inferior conjunction")
    : b.id === "earth" ? (a.period > b.period ? "opposition" : "inferior conjunction") : "heliocentric conjunction";
  const gaps = events.slice(1).map((t, k) => t - events[k]);
  return {
    a: a.id, b: b.id, what, from: O.isoDate(from),
    synodic_days: round(synodic, 3),
    events: events.map((jd) => ({ date: O.isoDate(jd), jd: round(jd, 4), planets_in_fit_span: inRange(jd) })),
    mean_gap_days: gaps.length ? round(gaps.reduce((s, g) => s + g, 0) / gaps.length, 3) : null,
    search_ms: round(ms, 2),
  };
}

// Angle between two bodies on a date, as seen from Earth (the sky) and from the Sun.
function separation(q) {
  const a = body(q.get("a") || "jupiter"), b = body(q.get("b") || "saturn");
  const jd = O.parseDate(q.get("date"));
  const sa = O.state(a, jd), sb = O.state(b, jd), e = O.state(O.byId.earth, jd);
  const sun = { x: 0, y: 0, z: 0 };
  return {
    a: a.id, b: b.id, date: O.isoDate(jd),
    from_earth_deg: a.id === "earth" || b.id === "earth" ? null : round(O.separation(sa, sb, e), 4),
    from_sun_deg: round(O.separation(sa, sb, sun), 4),
    distance_au: round(Math.hypot(sa.x - sb.x, sa.y - sb.y, sa.z - sb.z), 6),
  };
}

// What runs, from which build: read from the live process, never typed.
const VENDOR = (() => {
  // The browser libraries are named and pinned in static/vendor/README.md; their
  // versions are read from the files' own banners so a swapped file shows.
  const out = [];
  for (const [file, name] of [["react.production.min.js", "react"], ["react-dom.production.min.js", "react-dom"], ["htm.umd.js", "htm"]]) {
    try {
      const buf = fs.readFileSync(path.join(STATIC, "vendor", file));
      const head = buf.subarray(0, 400).toString();
      const readme = fs.readFileSync(path.join(STATIC, "vendor", "README.md"), "utf8");
      const ver = (head.match(/v(\d+\.\d+\.\d+)/) || readme.match(new RegExp(`\\| ${file} \\| ${name} (\\S+)`)) || [])[1] || "unknown";
      out.push({ name, version: ver, where: "browser", sha256: crypto.createHash("sha256").update(buf).digest("hex").slice(0, 16) });
    } catch { out.push({ name, version: "missing", where: "browser" }); }
  }
  return out;
})();

function env() {
  const mods = (process.env.LOADEDMODULES || "").split(":").filter(Boolean);
  return {
    runtime: "Node.js", version: process.version, v8: process.versions.v8,
    executable: process.execPath,
    modules: mods.filter((m) => /^(nodejs|env)\//.test(m)),
    modules_loaded: mods.length,
    packages: [
      ...["node:http", "node:fs", "node:path", "node:os", "node:crypto"].map((name) => ({ name, version: process.version, where: "server" })),
      { name: "orbits.js", version: "this project", where: "server + browser" },
      ...VENDOR,
    ],
    started: STARTED.toISOString(), uptime_s: Math.round(process.uptime()),
    cpus: os.cpus().length, platform: `${os.platform()} ${os.arch()}`, hostname: os.hostname(),
  };
}

const API = { "/api/positions": positions, "/api/bodies": bodies, "/api/lineup": lineup,
  "/api/separation": separation, "/api/env": env };

// ---- HTTP ------------------------------------------------------------------------------------

function send(res, status, body, type) {
  res.writeHead(status, { "Content-Type": type, "Cache-Control": "no-cache", "X-Content-Type-Options": "nosniff" });
  res.end(body);
}

// /static/<file> and /<file> are the same file. The page loads the vendored
// libraries from /static/vendor/: nginx's shared static_dirs.conf claims /vendor/
// on every site for the platform's own files, so /vendor/react… never reached
// this server and the page sat on "Loading…" (2026-10-26).
function serveStatic(res, urlPath) {
  const rel = urlPath === "/" ? "index.html" : decodeURIComponent(urlPath).replace(/^\/+/, "").replace(/^static\//, "");
  const file = path.resolve(STATIC, rel);
  if (file !== STATIC && !file.startsWith(STATIC + path.sep)) return send(res, 404, "not found", "text/plain");
  fs.readFile(file, (err, data) => {
    if (err) return send(res, 404, "not found", "text/plain");
    send(res, 200, data, TYPES[path.extname(file)] || "application/octet-stream");
  });
}

const server = http.createServer((req, res) => {
  log.request(req, res);
  const url = new URL(req.url, "http://localhost");
  if (req.method !== "GET" && req.method !== "HEAD") return send(res, 405, "GET only", "text/plain");
  if (url.pathname === "/health") return send(res, 200, "ok", "text/plain");
  const handler = API[url.pathname];
  if (handler) {
    try {
      return send(res, 200, JSON.stringify(handler(url.searchParams)), "application/json");
    } catch (e) {
      return send(res, e.status || 400, JSON.stringify({ error: e.message }), "application/json");
    }
  }
  serveStatic(res, url.pathname);
});

server.listen(PORT, "127.0.0.1", () => log.info(`__HOST__: Orbital serving on 127.0.0.1:${PORT} (node ${process.version})`));
for (const sig of ["SIGTERM", "SIGINT"]) process.on(sig, () => server.close(() => process.exit(0)));
