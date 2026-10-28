// Dolphin Jump: a side-scrolling ocean game, and the score board every
// player of this site shares.
//
// The game runs in the browser (static/app.js). This process serves the page
// and keeps the board: the best runs, in memory and in data/scores.json,
// written atomically so a restart loses nothing.
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

const PORT = Number(process.env.PORT);
if (!PORT) {
  console.error("PORT is unset: start with ./run.sh, whose env.sh sets it from app.env APP_PORT");
  process.exit(2);
}
const ROOT = __dirname;
const STATIC = path.join(ROOT, "static");
const DATA_DIR = path.join(ROOT, "data");
const SCORES_FILE = path.join(DATA_DIR, "scores.json");
const STARTED = new Date();
const MAX_BODY = 4096;
// The board keeps this many runs; a run below the last of them is not kept.
const KEEP = 20;
// A star is worth at most 60, so a score past this is not from a run of the
// game; the board refuses it rather than letting one POST own the top row.
const MAX_SCORE = 100000;

// ---- the board ------------------------------------------------------------
function load() {
  try {
    const rows = JSON.parse(fs.readFileSync(SCORES_FILE, "utf8"));
    return Array.isArray(rows) ? rows.filter(valid).slice(0, KEEP) : [];
  } catch {
    return [];
  }
}
let scores = load();
let writing = null;

function valid(r) {
  return r && typeof r.name === "string" && Number.isInteger(r.score) && r.score >= 0 && r.score <= MAX_SCORE;
}

// Temp file, fsync, rename: a crash leaves the old board or the new one,
// never half of either.
function writeNow(rows) {
  fs.mkdirSync(DATA_DIR, { recursive: true });
  const tmp = SCORES_FILE + "." + process.pid + ".tmp";
  const fd = fs.openSync(tmp, "w");
  try {
    fs.writeSync(fd, JSON.stringify(rows, null, 1));
    fs.fsyncSync(fd);
  } finally {
    fs.closeSync(fd);
  }
  fs.renameSync(tmp, SCORES_FILE);
}
function persist() {
  writing = Promise.resolve().then(() => writeNow(scores)).catch((e) => console.error("scores not written:", e.message)).finally(() => { writing = null; });
}

// A name is what a person typed: trimmed, 1-16 visible characters, nothing
// that could be markup. React escapes it anyway; the file should be clean too.
function cleanName(s) {
  const name = String(s || "").replace(/[<>&"'`\u0000-\u001f\u007f]/g, "").trim().slice(0, 16);
  return name || "Anonymous dolphin";
}

function record(body) {
  const score = Number(body.score), stars = Number(body.stars), seconds = Number(body.seconds);
  if (!Number.isInteger(score) || score < 0 || score > MAX_SCORE) throw Object.assign(new Error(`score must be a whole number in 0..${MAX_SCORE}`), { status: 400 });
  const row = {
    name: cleanName(body.name),
    score,
    stars: Number.isInteger(stars) && stars >= 0 ? stars : 0,
    seconds: Number.isFinite(seconds) && seconds >= 0 ? Math.round(seconds) : 0,
    at: new Date().toISOString(),
  };
  scores.push(row);
  scores.sort((a, b) => b.score - a.score || a.at.localeCompare(b.at));
  const rank = scores.indexOf(row) + 1;
  scores = scores.slice(0, KEEP);
  if (rank <= KEEP) persist();
  return { rank: rank <= KEEP ? rank : null, kept: rank <= KEEP, board: scores.slice(0, 10) };
}

// ---- environment ----------------------------------------------------------
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
    runtime: { name: "Node.js", version: process.version.slice(1), platform: `${process.platform}-${process.arch}` },
    stack: { runtime: loaded.filter((m) => /^nodejs\//i.test(m)), loaded: loaded.length },
    server: IMPORTS.map((name) => ({ name, source: "built in" })),
    browser: vendorLibraries(),
    process: { pid: process.pid, started: STARTED.toISOString(), uptime_s: Math.round(process.uptime()) },
    host: { name: os.hostname(), cpus: os.cpus().length },
    board: { runs: scores.length, file: path.relative(ROOT, SCORES_FILE) },
  };
}

// ---- HTTP -----------------------------------------------------------------
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
      if (size > MAX_BODY) { reject(Object.assign(new Error("body over 4 KB"), { status: 413 })); req.destroy(); }
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
      "Cache-Control": vendor ? "public, max-age=86400" : "no-cache",
    });
    res.end(buf);
  });
}

async function route(req, res) {
  const url = new URL(req.url, "http://localhost");
  const p = url.pathname, m = req.method;

  if (p === "/health") return json(res, 200, { ok: true, runs: scores.length });
  if (p === "/" && m === "GET") return serveFile(res, "index.html");
  // /static/, not /vendor/: nginx's shared static_dirs.conf claims /vendor/
  // on every site (Cove's React starters sat on "Loading…", 2026-10-26).
  if (p.startsWith("/static/") && m === "GET") return serveFile(res, p.slice(8));
  if (p === "/api/env" && m === "GET") return json(res, 200, environment());
  if (p === "/api/scores") {
    if (m === "GET") return json(res, 200, scores.slice(0, 10));
    if (m === "POST") return json(res, 201, record(await readJson(req)));
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
  console.log(`Dolphin Jump on http://127.0.0.1:${PORT} · node ${process.version} · ${scores.length} runs on the board`));

// down.sh sends TERM: finish the write in flight, then exit.
for (const sig of ["SIGTERM", "SIGINT"]) {
  process.on(sig, async () => {
    server.close();
    if (writing) await writing;
    process.exit(0);
  });
}
