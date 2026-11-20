// __HOST__: the game's page and its scripts, Node standard library only, no npm.
'use strict';
const http = require('node:http');
const fs = require('node:fs');
const path = require('node:path');
const log = require('./log');

const PORT = Number(process.env.PORT);
const STATIC = path.join(__dirname, 'static');
const STARTED = Date.now();
const TYPES = { '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.css': 'text/css; charset=utf-8', '.json': 'application/json', '.svg': 'image/svg+xml', '.png': 'image/png', '.jpg': 'image/jpeg' };

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

// Add an API by adding a line: 'METHOD /path': (req) => what to answer as JSON.
const ROUTES = {
  'GET /health': () => ({ ok: true }),
  'GET /__live': () => ({ version: version() }),
};

http.createServer((req, res) => {
  log.request(req, res);
  const url = new URL(req.url, 'http://localhost');
  const route = ROUTES[`${req.method} ${url.pathname}`];
  if (route) return send(res, 200, route(req));
  const rel = url.pathname === '/' ? 'index.html' : decodeURIComponent(url.pathname).replace(/^\/+/, '').replace(/^static\//, '');
  const file = path.resolve(STATIC, rel);
  if (!file.startsWith(STATIC + path.sep)) return send(res, 403, { error: 'forbidden' });
  fs.readFile(file, (err, buf) => (err ? send(res, 404, { error: 'not found' }) : send(res, 200, buf, TYPES[path.extname(file)] || 'application/octet-stream')));
}).listen(PORT, '127.0.0.1', () => log.info(`__HOST__: game serving on 127.0.0.1:${PORT}`));
