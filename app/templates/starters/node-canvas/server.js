// Project Canvas: a research task board kept live across every open tab.
//
// One Node process holds the board in memory, applies each change with the
// same BoardOps the browser runs (static/board-ops.js), writes it to
// data/board.json atomically, and streams every change to every open tab over
// Server-Sent Events. Node's event loop is the point: hundreds of idle SSE
// connections cost a socket each, not a thread each, and a change fans out to
// all of them from one callback.
//
// Standard library only: no npm, no node_modules.
"use strict";

// Every module this server imports; /api/env lists them from here.
const IMPORTS = ["node:http", "node:fs", "node:path", "node:os", "node:crypto"];
const http = require("node:http");
const fs = require("node:fs");
const path = require("node:path");
const os = require("node:os");
const crypto = require("node:crypto");
const BoardOps = require("./static/board-ops.js");

const PORT = Number(process.env.PORT);
if (!PORT) {
  console.error("PORT is unset: start with ./run.sh, whose env.sh sets it from app.env APP_PORT");
  process.exit(2);
}
const ROOT = __dirname;
const STATIC = path.join(ROOT, "static");
const DATA_DIR = path.join(ROOT, "data");
const BOARD_FILE = path.join(DATA_DIR, "board.json");
const STARTED = new Date();
const MAX_BODY = 256 * 1024;

// ---- sample board ----------------------------------------------------------
// A cell-culture lab four weeks into a hypoxia experiment. Dates are relative
// to the first start, so the burndown always shows the last few weeks.
function sampleBoard() {
  const now = Date.now(), D = 864e5;
  const ago = (d) => new Date(now - d * D).toISOString();
  const day = (d) => new Date(now + d * D).toISOString().slice(0, 10);
  const steps = (...xs) => xs.map((x) => (typeof x === "string" ? { text: x, done: false } : { text: x[0], done: true }));
  let n = 0;
  const card = (title, owner, tags, due, checklist, created, done = null) =>
    ({ id: "s" + String(++n).padStart(3, "0"), title, owner, tags, due, checklist, created: ago(created), done: done == null ? null : ago(done) });

  const backlog = [
    card("Test new FBS lot on one T75 before switching", "Tomás", ["reagents", "culture"], day(9), steps("Thaw lot 24B117 aliquot", "Split one flask 50/50 old vs new", "Compare doubling time at 72 h"), 6),
    card("Mycoplasma PCR before passage 15", "Lena", ["culture", "qc"], day(3), steps("Collect 1 mL spent medium", "Run PCR with positive control"), 4),
    card("Pre-register ΔΔCt analysis plan", "Priya", ["stats", "qpcr"], day(12), steps("Primary endpoint: CA9 fold change at 24 h", "Mixed model, replicate as random effect", "Upload to OSF"), 9),
    card("Draft methods: hypoxia chamber + O₂ monitoring", "Amara", ["writing"], day(18), [], 3),
  ];
  const running = [
    card("Hypoxia time course, rep 3 (1% O₂: 0/4/8/24 h)", "Jun", ["culture"], day(2),
      steps(["Seed 6-well plates, 3×10⁵ cells/well"], ["Equilibrate chamber 2 h"], ["Harvest 4 h"], "Harvest 8 h", "Harvest 24 h", "Snap-freeze lysates at −80 °C"), 8),
    card("Western blot: HIF-1α, rep 2", "Lena", ["western"], day(-1), steps(["Run 8% gel"], ["Transfer 90 min wet"], "Block + primary overnight", "Image on ChemiDoc"), 7),
    card("IF: HIF-1α nuclear localisation", "Jun", ["imaging"], day(5), steps(["Fix + permeabilise"], "Stain HIF-1α / DAPI", "Confocal, 40 fields per condition"), 5),
    card("CoCl₂ chemical-hypoxia positive control", "Tomás", ["culture", "controls"], day(4), steps(["Make 100 mM CoCl₂ stock"], "Treat 150 µM, 24 h"), 4),
  ];
  const analysing = [
    card("qPCR: VEGFA, CA9, PGK1 vs ACTB", "Priya", ["qpcr", "stats"], day(6),
      steps(["Export Cq values"], ["Check melt curves"], ["Compute ΔΔCt"], "Bootstrap 95% CIs", "Figure 2 draft"), 15),
    card("Densitometry of rep 1 blots in Fiji", "Lena", ["western", "stats"], day(1), steps(["Background subtract"], ["Normalise to β-actin"], "Plot vs time"), 12),
    card("Nuclei segmentation, rep 1 images", "Jun", ["imaging", "stats"], day(8), steps(["StarDist on DAPI"], "Nuclear/cytoplasmic HIF-1α ratio", "Blind check 5 fields by hand"), 11),
  ];
  const written = [
    card("STR authentication report, HEK293", "Lena", ["qc"], null, steps(["Send sample"], ["File report"]), 27, 20),
    card("Optimise seeding density (2–4×10⁵/well)", "Jun", ["culture"], null, steps(["Three densities, 48 h"], ["Pick 3×10⁵"]), 26, 18),
    card("Primer efficiency curves (90–105%)", "Priya", ["qpcr"], null, steps(["5-point dilution series"], ["Slopes −3.1 to −3.6"]), 24, 15),
    card("Hypoxia chamber O₂ calibration log", "Tomás", ["qc"], null, steps(["Two-point calibration"], ["Log sheet in lab drive"]), 22, 13),
    card("Rep 1 time course: lab-meeting slides", "Amara", ["writing"], null, steps(["Blots"], ["qPCR prelim"], ["Next steps"]), 16, 6),
    card("Order anti-HIF-1α (clone 54) + secondary", "Tomás", ["reagents"], null, [], 14, 10),
    card("Rep 2 time course: harvest + freeze", "Jun", ["culture"], null, steps(["All four time points"]), 13, 2),
  ];
  const ids = (cs) => cs.map((c) => c.id);
  const all = [...backlog, ...running, ...analysing, ...written];
  return {
    version: 1,
    updated: new Date(now).toISOString(),
    title: "Okafor lab · HIF-1α hypoxia response in HEK293",
    columns: [
      { id: "backlog", title: "Backlog", cards: ids(backlog) },
      { id: "running", title: "Running", cards: ids(running) },
      { id: "analysing", title: "Analysing", cards: ids(analysing) },
      { id: "written", title: "Written up", cards: ids(written) },
    ],
    cards: Object.fromEntries(all.map((c) => [c.id, c])),
  };
}

// ---- persistence -----------------------------------------------------------
// Write to a temp file in the same directory, fsync, then rename over the old
// file: rename is atomic on one filesystem, so a crash mid-write leaves the
// previous board, never half a JSON document. Writes are coalesced: while one
// is in flight, later changes only mark the board dirty and the newest state
// is written once the first finishes.
let board = load();
let writing = null, dirty = false;

function load() {
  try {
    const b = JSON.parse(fs.readFileSync(BOARD_FILE, "utf8"));
    const v = BoardOps.validate(b);
    v.title = typeof b.title === "string" ? b.title : "Project Canvas";
    return v;
  } catch (e) {
    if (e.code !== "ENOENT") console.error(`board.json unreadable (${e.message}); starting from the sample board`);
    const b = sampleBoard();
    fs.mkdirSync(DATA_DIR, { recursive: true });
    writeNow(b);
    return b;
  }
}

function writeNow(b) {
  const tmp = `${BOARD_FILE}.${process.pid}.${crypto.randomBytes(4).toString("hex")}.tmp`;
  const fd = fs.openSync(tmp, "w");
  try { fs.writeSync(fd, JSON.stringify(b, null, 1)); fs.fsyncSync(fd); } finally { fs.closeSync(fd); }
  fs.renameSync(tmp, BOARD_FILE);
}

async function writeBoard(b) {
  const tmp = `${BOARD_FILE}.${process.pid}.${crypto.randomBytes(4).toString("hex")}.tmp`;
  const fh = await fs.promises.open(tmp, "w");
  try { await fh.writeFile(JSON.stringify(b, null, 1)); await fh.sync(); } finally { await fh.close(); }
  await fs.promises.rename(tmp, BOARD_FILE);
}

function persist() {
  if (writing) { dirty = true; return writing; }
  writing = (async () => {
    do {
      dirty = false;
      try { await writeBoard(board); } catch (e) { console.error("write failed:", e.message); }
    } while (dirty);
    writing = null;
  })();
  return writing;
}

// ---- live sync (Server-Sent Events) ---------------------------------------
const clients = new Map(); // response -> { client, since }

function send(res, event, data, id) {
  res.write(`${id != null ? `id: ${id}\n` : ""}event: ${event}\ndata: ${JSON.stringify(data)}\n\n`);
}
function broadcast(event, data, id) {
  for (const res of clients.keys()) send(res, event, data, id);
}
function presence() {
  broadcast("presence", { clients: clients.size });
}

function events(req, res, url) {
  res.writeHead(200, {
    "Content-Type": "text/event-stream; charset=utf-8",
    "Cache-Control": "no-cache, no-transform",
    Connection: "keep-alive",
    "X-Accel-Buffering": "no", // nginx would otherwise hold events in its buffer
  });
  res.write("retry: 2000\n\n");
  clients.set(res, { client: url.searchParams.get("client") || "anon", since: Date.now() });
  // The snapshot travels with hello, so a tab that reconnects after missing
  // events is current again without a second request.
  send(res, "hello", { version: board.version, clients: clients.size, board }, board.version);
  presence();
  req.on("close", () => { clients.delete(res); presence(); });
}

// A comment line every 20 s keeps proxies from closing an idle stream.
setInterval(() => { for (const res of clients.keys()) res.write(": ping\n\n"); }, 20000).unref();

// ---- changes ---------------------------------------------------------------
// The one path every mutation takes: apply, bump the version, persist,
// broadcast. The response and the event carry the same board.
function commit(op, by) {
  const at = new Date().toISOString();
  const next = BoardOps.apply(board, op, at);
  next.version = board.version + 1;
  next.updated = at;
  next.title = board.title;
  board = next;
  persist();
  const event = { op: { ...op, at }, by, version: board.version, board };
  broadcast("change", event, board.version);
  return event;
}

function replace(input, by) {
  const next = BoardOps.validate(input);
  next.version = board.version + 1;
  next.updated = new Date().toISOString();
  next.title = typeof input.title === "string" ? input.title.slice(0, 120) : board.title;
  board = next;
  persist();
  const event = { op: { type: "replace" }, by, version: board.version, board };
  broadcast("change", event, board.version);
  return event;
}

// ---- environment (provenance) ---------------------------------------------
// Read live: the runtime, the stack modules this process was started under,
// what it imports, and the browser libraries it serves with their checksums.
function vendorLibraries() {
  const dir = path.join(STATIC, "vendor");
  let readme = "";
  try { readme = fs.readFileSync(path.join(dir, "README.md"), "utf8"); } catch { return []; }
  const listed = Object.fromEntries([...readme.matchAll(/^([0-9a-f]{64})\s+(\S+)$/gm)].map((m) => [m[2], m[1]]));
  return [...readme.matchAll(/^\|\s*(\S+\.js)\s*\|\s*([\w-]+)\s+([\d.]+)[^|]*\|\s*([^|]+?)\s*\|$/gm)].map(([, file, pkg, version, licence]) => {
    let sha = null;
    try { sha = crypto.createHash("sha256").update(fs.readFileSync(path.join(dir, file))).digest("hex"); } catch {}
    return { name: pkg, version, file, licence, sha256: sha, verified: !!sha && sha === listed[file] };
  });
}

function environment() {
  const loaded = (process.env.LOADEDMODULES || "").split(":").filter(Boolean);
  return {
    runtime: { name: "Node.js", version: process.version.slice(1), v8: process.versions.v8, platform: `${process.platform}-${process.arch}`, exec: process.execPath },
    stack: {
      env: loaded.filter((m) => m.startsWith("env/")),
      runtime: loaded.filter((m) => /^nodejs\//i.test(m)),
      loaded: loaded.length,
    },
    server: IMPORTS.map((name) => ({ name, version: process.version.slice(1), source: "built in" })),
    browser: vendorLibraries(),
    process: { pid: process.pid, started: STARTED.toISOString(), uptime_s: Math.round(process.uptime()), rss_mb: Math.round(process.memoryUsage().rss / 1048576) },
    host: { name: os.hostname(), cpus: os.cpus().length, load: os.loadavg().map((x) => +x.toFixed(2)), memory_gb: +(os.totalmem() / 2 ** 30).toFixed(1) },
    live: { sse_clients: clients.size, board_version: board.version, board_file: path.relative(ROOT, BOARD_FILE) },
  };
}

// ---- HTTP ------------------------------------------------------------------
const TYPES = { ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".css": "text/css; charset=utf-8", ".json": "application/json", ".svg": "image/svg+xml", ".md": "text/plain; charset=utf-8" };

function json(res, status, body) {
  const s = JSON.stringify(body);
  res.writeHead(status, { "Content-Type": "application/json", "Content-Length": Buffer.byteLength(s), "Cache-Control": "no-store" });
  res.end(s);
}

function readJson(req) {
  return new Promise((resolve, reject) => {
    let size = 0;
    const chunks = [];
    req.on("data", (c) => {
      size += c.length;
      if (size > MAX_BODY) { reject(Object.assign(new Error("body over 256 KB"), { status: 413 })); req.destroy(); }
      else chunks.push(c);
    });
    req.on("end", () => {
      if (!chunks.length) return resolve({});
      try { resolve(JSON.parse(Buffer.concat(chunks).toString("utf8"))); }
      catch { reject(Object.assign(new Error("body is not JSON"), { status: 400 })); }
    });
    req.on("error", reject);
  });
}

// Files under static/ only; a path that resolves outside it is a 404.
function serveFile(res, rel) {
  const file = path.resolve(STATIC, "." + path.posix.normalize("/" + rel));
  if (!file.startsWith(STATIC + path.sep)) return json(res, 404, { error: "not found" });
  fs.readFile(file, (err, buf) => {
    if (err) return json(res, 404, { error: "not found" });
    const vendor = file.startsWith(path.join(STATIC, "vendor") + path.sep);
    res.writeHead(200, {
      "Content-Type": TYPES[path.extname(file)] || "application/octet-stream",
      "Content-Length": buf.length,
      // Vendored files change only with their checksums; app files on every save.
      "Cache-Control": vendor ? "public, max-age=86400" : "no-cache",
    });
    res.end(buf);
  });
}

async function route(req, res) {
  const url = new URL(req.url, "http://localhost");
  const p = url.pathname, m = req.method;
  const by = req.headers["x-client-id"] || "api";

  if (p === "/health") return json(res, 200, { ok: true, version: board.version, clients: clients.size });
  if (p === "/" && m === "GET") return serveFile(res, "index.html");
  // The vendored libraries load from /static/vendor/: nginx's shared
  // static_dirs.conf claims /vendor/ on every site, so the page sat on
  // "Loading…" when it asked for /vendor/react… (2026-10-26).
  if (p.startsWith("/static/") && m === "GET") return serveFile(res, p.slice(8));
  if (p === "/api/env" && m === "GET") return json(res, 200, environment());
  if (p === "/api/events" && m === "GET") return events(req, res, url);

  if (p === "/api/board") {
    if (m === "GET") return json(res, 200, board);
    if (m === "PUT") return json(res, 200, replace(await readJson(req), by));
  }
  if (p === "/api/board/reset" && m === "POST") return json(res, 200, replace(sampleBoard(), by));

  if (p === "/api/cards" && m === "POST") {
    const body = await readJson(req);
    const { column, before, ...card } = body;
    return json(res, 201, commit({ type: "create", card, column, before }, by));
  }
  const hit = p.match(/^\/api\/cards\/([A-Za-z0-9_-]+)$/);
  if (hit) {
    const id = hit[1];
    if (m === "DELETE") return json(res, 200, commit({ type: "delete", id }, by));
    if (m === "PATCH") {
      // One PATCH may edit fields and move the card: two operations, two
      // events, and the response carries the board after both. Both are
      // checked before either is applied, so a bad column edits nothing.
      const { column, before, ...fields } = await readJson(req);
      if (!board.cards[id]) return json(res, 404, { error: "no card " + id });
      if (column !== undefined && !board.columns.some((c) => c.id === column)) return json(res, 400, { error: "no column " + column });
      let event;
      if (Object.keys(fields).length) event = commit({ type: "update", id, fields }, by);
      if (column !== undefined) event = commit({ type: "move", id, column, before: before || null }, by);
      return event ? json(res, 200, event) : json(res, 400, { error: "nothing to change" });
    }
  }
  json(res, 404, { error: "not found" });
}

const server = http.createServer((req, res) => {
  route(req, res).catch((e) => {
    if (!res.headersSent) json(res, e.status || 500, { error: e.message });
    if (!e.status) console.error(e);
  });
});
server.listen(PORT, "127.0.0.1", () =>
  console.log(`Project Canvas on http://127.0.0.1:${PORT} · node ${process.version} · board v${board.version} (${path.relative(ROOT, BOARD_FILE)})`));

// down.sh sends TERM: finish the write in flight, close streams, then exit,
// so the last move before a restart is on disk.
for (const sig of ["SIGTERM", "SIGINT"]) {
  process.on(sig, async () => {
    server.close();
    for (const res of clients.keys()) res.end();
    if (writing) await writing;
    process.exit(0);
  });
}
