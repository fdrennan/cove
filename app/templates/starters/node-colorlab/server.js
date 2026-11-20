// Color Laboratory · __HOST__
// Node standard library only: no npm, no node_modules. Every colour formula
// runs in the browser (static/app.js); this process serves the page, says what
// it runs on (/api/env) and keeps named palettes in data/palettes.json.
'use strict';

const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const log = require('./log');
const os = require('node:os');
const crypto = require('node:crypto');

const PORT = Number(process.env.PORT || process.env.APP_PORT);
if (!Number.isInteger(PORT) || PORT <= 0) {
  // env.sh sets PORT from app.env; a missing port is a launch mistake, not a default.
  log.error('PORT is not set: start with ./run.sh (it reads APP_PORT from app.env)');
  process.exit(2);
}
const ROOT = __dirname;
const STATIC = path.join(ROOT, 'static');
const DATA = path.join(ROOT, 'data', 'palettes.json');
const STARTED = new Date();
const MAX_PALETTES = 200;
const MAX_BODY = 64 * 1024;

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.md': 'text/markdown; charset=utf-8',
  '.svg': 'image/svg+xml',
};

function send(res, status, body, type = 'application/json; charset=utf-8') {
  const buf = Buffer.isBuffer(body) ? body : Buffer.from(typeof body === 'string' ? body : JSON.stringify(body));
  res.writeHead(status, { 'content-type': type, 'content-length': buf.length, 'cache-control': 'no-cache' });
  res.end(buf);
}

// Static files: resolve inside static/ only, so "/../server.js" cannot escape.
// /static/<file> and /<file> are the same file. The page loads the vendored
// libraries from /static/vendor/: nginx's shared static_dirs.conf claims /vendor/
// on every site for the platform's own files, so /vendor/react… never reached
// this server and the page sat on "Loading…" (2026-10-26).
function serveStatic(res, urlPath) {
  const rel = urlPath === '/' ? 'index.html' : decodeURIComponent(urlPath).replace(/^\/+/, '').replace(/^static\//, '');
  const file = path.resolve(STATIC, rel);
  if (!file.startsWith(STATIC + path.sep)) return send(res, 403, { error: 'forbidden' });
  fs.readFile(file, (err, buf) => {
    if (err) return send(res, 404, { error: 'not found', path: urlPath });
    send(res, 200, buf, TYPES[path.extname(file)] || 'application/octet-stream');
  });
}

// --- /api/env: provenance read from the live process, never typed ---------
// The browser libraries carry their version inside the file; read it and hash
// the bytes so the panel shows exactly which build the page loaded.
function vendorInfo(file, pattern) {
  try {
    const buf = fs.readFileSync(path.join(STATIC, 'vendor', file));
    const m = buf.toString('utf8').match(pattern);
    return { version: m ? m[1] : null, sha256: crypto.createHash('sha256').update(buf).digest('hex').slice(0, 16) };
  } catch {
    return { version: null, sha256: null };
  }
}

function envInfo() {
  const loaded = (process.env.LOADEDMODULES || '').split(':').filter(Boolean);
  return {
    runtime: { name: 'Node.js', version: process.version, v8: process.versions.v8, openssl: process.versions.openssl },
    // The env set and the nodejs module are the two that say which stack build this is.
    stack_modules: loaded.filter((m) => /^(env\/|nodejs\/)/.test(m)),
    loaded_module_count: loaded.length,
    exec_path: process.execPath,
    packages: [
      { name: 'node:http, node:fs, node:path, node:os, node:crypto', where: 'server', version: process.version },
      { name: 'react', where: 'browser', ...vendorInfo('react.production.min.js', /"(\d+\.\d+\.\d+)"/) },
      { name: 'react-dom', where: 'browser', ...vendorInfo('react-dom.production.min.js', /"(\d+\.\d+\.\d+)"/) },
      // htm's minified UMD has no version string; the vendor README pins it, the hash proves the bytes.
      { name: 'htm', where: 'browser', ...vendorInfo('htm.umd.js', /(?!)/), version: readmeVersion('htm') },
    ],
    started: STARTED.toISOString(),
    uptime_s: Math.round(process.uptime()),
    cpus: os.cpus().length,
    platform: `${os.platform()} ${os.arch()}`,
    hostname: os.hostname(),
  };
}

function readmeVersion(pkg) {
  try {
    const txt = fs.readFileSync(path.join(STATIC, 'vendor', 'README.md'), 'utf8');
    const m = txt.match(new RegExp(`\\|\\s*${pkg}\\s+(\\d+\\.\\d+\\.\\d+)`));
    return m ? m[1] : null;
  } catch {
    return null;
  }
}

// --- /api/palettes: a JSON file, written whole and renamed into place -----
function readPalettes() {
  try {
    const v = JSON.parse(fs.readFileSync(DATA, 'utf8'));
    return Array.isArray(v) ? v : [];
  } catch {
    return [];
  }
}

const HEX = /^#[0-9a-f]{6}$/i;
function validPalette(p) {
  if (!p || typeof p !== 'object') return 'body must be a JSON object';
  if (typeof p.name !== 'string' || !p.name.trim() || p.name.length > 60) return 'name: 1-60 characters';
  if (!p.colors || typeof p.colors !== 'object') return 'colors: object of role -> #rrggbb';
  const roles = Object.keys(p.colors);
  if (roles.length === 0 || roles.length > 24) return 'colors: 1-24 roles';
  for (const r of roles) {
    if (!/^[a-z][a-z0-9-]{0,30}$/.test(r)) return `colors: bad role name "${r}"`;
    if (!HEX.test(p.colors[r])) return `colors.${r}: must be #rrggbb`;
  }
  return null;
}

function savePalette(p) {
  const list = readPalettes().filter((x) => x.name !== p.name.trim());
  const entry = {
    name: p.name.trim(),
    colors: Object.fromEntries(Object.entries(p.colors).map(([k, v]) => [k, v.toLowerCase()])),
    base: typeof p.base === 'string' && HEX.test(p.base) ? p.base.toLowerCase() : undefined,
    harmony: typeof p.harmony === 'string' ? p.harmony.slice(0, 30) : undefined,
    mode: p.mode === 'light' || p.mode === 'dark' ? p.mode : undefined,
    saved: new Date().toISOString(),
  };
  list.unshift(entry);
  list.length = Math.min(list.length, MAX_PALETTES);
  fs.mkdirSync(path.dirname(DATA), { recursive: true });
  // Write to a temp file then rename: a crash mid-write never leaves half a JSON file.
  const tmp = `${DATA}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, JSON.stringify(list, null, 2) + '\n');
  fs.renameSync(tmp, DATA);
  return entry;
}

function readBody(req) {
  return new Promise((resolve, reject) => {
    let size = 0;
    const chunks = [];
    req.on('data', (c) => {
      size += c.length;
      if (size > MAX_BODY) { reject(new Error('body too large')); req.destroy(); return; }
      chunks.push(c);
    });
    req.on('end', () => resolve(Buffer.concat(chunks).toString('utf8')));
    req.on('error', reject);
  });
}

const server = http.createServer(async (req, res) => {
  log.request(req, res);
  const url = new URL(req.url, 'http://localhost');
  const p = url.pathname;
  try {
    if (p === '/health') return send(res, 200, { ok: true });
    if (p === '/api/env' && req.method === 'GET') return send(res, 200, envInfo());
    if (p === '/api/palettes' && req.method === 'GET') return send(res, 200, readPalettes());
    if (p === '/api/palettes' && req.method === 'POST') {
      let body;
      try { body = JSON.parse(await readBody(req)); } catch (e) { return send(res, 400, { error: `bad JSON: ${e.message}` }); }
      const err = validPalette(body);
      if (err) return send(res, 400, { error: err });
      return send(res, 201, savePalette(body));
    }
    if (p.startsWith('/api/')) return send(res, 404, { error: 'no such endpoint' });
    if (req.method !== 'GET' && req.method !== 'HEAD') return send(res, 405, { error: 'method not allowed' });
    return serveStatic(res, p);
  } catch (e) {
    log.error(`${req.method} ${p}:`, e);
    return send(res, 500, { error: e.message });
  }
});

// 127.0.0.1 only: nginx in front decides who reaches it.
server.listen(PORT, '127.0.0.1', () => log.info(`__HOST__: Color Laboratory serving on 127.0.0.1:${PORT}`));
for (const sig of ['SIGTERM', 'SIGINT']) process.on(sig, () => server.close(() => process.exit(0)));
