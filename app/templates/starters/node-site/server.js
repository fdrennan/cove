// __HOST__
// Node standard library only: no npm, no node_modules. This process serves
// static/, answers the routes below, and tells the page when a file changed.
'use strict';

const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const log = require('./log');

const PORT = Number(process.env.PORT || process.env.APP_PORT);
if (!Number.isInteger(PORT) || PORT <= 0) {
  // env.sh sets PORT from app.env; a missing port is a launch mistake, not a default.
  log.error('PORT is not set: start with ./run.sh (it reads APP_PORT from app.env)');
  process.exit(2);
}
const STATIC = path.join(__dirname, 'static');
const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.webp': 'image/webp',
};

function send(res, status, body, type = 'application/json; charset=utf-8') {
  const buf = Buffer.isBuffer(body) ? body : Buffer.from(typeof body === 'string' ? body : JSON.stringify(body));
  res.writeHead(status, { 'content-type': type, 'content-length': buf.length, 'cache-control': 'no-cache' });
  res.end(buf);
}

// What static/ looks like now: live.js reloads the page when this changes. A
// question the page asks, not a stream, so it works through any proxy in front.
function version() {
  let newest = 0;
  let files = 0;
  for (const e of fs.readdirSync(STATIC, { withFileTypes: true, recursive: true })) {
    if (!e.isFile()) continue;
    files += 1;
    newest = Math.max(newest, fs.statSync(path.join(e.parentPath, e.name)).mtimeMs);
  }
  return `${files}-${newest}`;
}

// Add an API by adding a line: 'METHOD /path': (req) => what to answer as JSON.
const ROUTES = {
  'GET /health': () => ({ ok: true }),
  'GET /__live': () => ({ version: version() }),
};

// Resolve inside static/ only, so "/../server.js" cannot escape. Assets stay
// under /static/ or at the top level: nginx claims /vendor/, /images/, /css/ and
// /javascript/ on every ndexr site for the platform's own files.
function serveStatic(res, urlPath) {
  const rel = urlPath === '/' ? 'index.html' : decodeURIComponent(urlPath).replace(/^\/+/, '').replace(/^static\//, '');
  const file = path.resolve(STATIC, rel);
  if (!file.startsWith(STATIC + path.sep)) return send(res, 403, { error: 'forbidden' });
  fs.readFile(file, (err, buf) => {
    if (err) return send(res, 404, { error: 'not found', path: urlPath });
    send(res, 200, buf, TYPES[path.extname(file)] || 'application/octet-stream');
  });
}

http.createServer((req, res) => {
  log.request(req, res);
  const url = new URL(req.url, 'http://localhost');
  const route = ROUTES[`${req.method} ${url.pathname}`];
  if (route) {
    try {
      return send(res, 200, route(req));
    } catch (e) {
      return send(res, 500, { error: e.message });
    }
  }
  if (req.method !== 'GET' && req.method !== 'HEAD') return send(res, 405, { error: 'method not allowed' });
  serveStatic(res, url.pathname);
}).listen(PORT, '127.0.0.1', () => log.info(`__HOST__: site serving on 127.0.0.1:${PORT}`));
