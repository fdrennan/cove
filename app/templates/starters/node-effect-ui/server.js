// __HOST__: the page, its scripts and a small JSON API, Node standard library
// only, no npm. Observations are kept in data/observations.json.
'use strict';
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const log = require('./log');

const PORT = Number(process.env.PORT);
const STATIC = path.join(__dirname, 'static');
const DATA = path.join(__dirname, 'data', 'observations.json');
const STARTED = Date.now();
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.txt': 'text/plain; charset=utf-8' };

function send(res, status, body, type = 'application/json') {
  const buf = Buffer.isBuffer(body) ? body : Buffer.from(typeof body === 'string' ? body : JSON.stringify(body));
  res.writeHead(status, { 'content-type': type, 'content-length': buf.length, 'cache-control': 'no-cache' });
  res.end(buf);
}

function version() {
  let newest = 0;
  let files = 0;
  for (const e of fs.readdirSync(STATIC, { withFileTypes: true, recursive: true })) {
    if (!e.isFile()) continue;
    files += 1;
    newest = Math.max(newest, fs.statSync(path.join(e.parentPath, e.name)).mtimeMs);
  }
  return `${STARTED}-${files}-${newest}`;
}

const observations = () => {
  try {
    return JSON.parse(fs.readFileSync(DATA, 'utf8'));
  } catch {
    return [];
  }
};

function body(req) {
  return new Promise((resolve, reject) => {
    let raw = '';
    req.on('data', (c) => {
      raw += c;
      if (raw.length > 1e5) reject(new Error('too large'));
    });
    req.on('end', () => {
      try {
        resolve(raw ? JSON.parse(raw) : {});
      } catch (e) {
        reject(e);
      }
    });
  });
}

// Add an API by adding a line: 'METHOD /path': (req) => what to answer as JSON
// (or a promise of it). A thrown error answers 400 with its message.
const ROUTES = {
  'GET /health': () => ({ ok: true }),
  'GET /__live': () => ({ version: version() }),
  'GET /api/observations': () => observations(),
  'POST /api/observations': async (req) => {
    const { value, note = '' } = await body(req);
    if (typeof value !== 'number' || !Number.isFinite(value)) throw new Error('value must be a number');
    const row = { id: crypto.randomUUID(), at: new Date().toISOString(), value, note: String(note).slice(0, 200) };
    const all = [row, ...observations()].slice(0, 500);
    fs.mkdirSync(path.dirname(DATA), { recursive: true });
    fs.writeFileSync(DATA, JSON.stringify(all, null, 1));
    log.info(`observation ${row.value} saved`);
    return row;
  },
};

http.createServer(async (req, res) => {
  log.request(req, res);
  const url = new URL(req.url, 'http://localhost');
  const route = ROUTES[`${req.method} ${url.pathname}`];
  if (route) {
    try {
      return send(res, 200, await route(req));
    } catch (e) {
      return send(res, 400, { error: e.message });
    }
  }
  const rel = url.pathname === '/' ? 'index.html' : decodeURIComponent(url.pathname).replace(/^\/+/, '').replace(/^static\//, '');
  const file = path.resolve(STATIC, rel);
  if (!file.startsWith(STATIC + path.sep)) return send(res, 403, { error: 'forbidden' });
  fs.readFile(file, (err, buf) => (err ? send(res, 404, { error: 'not found' }) : send(res, 200, buf, TYPES[path.extname(file)] || 'application/octet-stream')));
}).listen(PORT, '127.0.0.1', () => log.info(`__HOST__: Effect UI app serving on 127.0.0.1:${PORT}`));
