// Logging for __HOST__, Node standard library only. Every line goes to
// stdout in the one format every Cove starter writes:
//
//   2026-11-18 12:04:31 INFO    [app] serving on 127.0.0.1:8834
//
// so .cove/run/app.log (Cove's Logs tab) reads the same in Node, R, Python,
// Julia and Rust. Times are UTC. Levels, shown when at or above LOG_LEVEL
// (default INFO): TRACE DEBUG INFO SUCCESS WARN ERROR.
//
//   const log = require('./log');
//   log.info(`serving on 127.0.0.1:${PORT}`);
//   const db = log.child('db');          // [db] on its lines
//   http.createServer((req, res) => { log.request(req, res); ... })
//                                        // GET /path 200 3 ms, once each
'use strict';

const LEVELS = { TRACE: 0, DEBUG: 1, INFO: 2, SUCCESS: 2, WARN: 3, ERROR: 4 };
const threshold = LEVELS[String(process.env.LOG_LEVEL || 'INFO').toUpperCase()] ?? LEVELS.INFO;

function line(level, name, parts) {
  if (LEVELS[level] < threshold) return;
  const stamp = new Date().toISOString().slice(0, 19).replace('T', ' ');
  const text = parts.map((p) => (p instanceof Error ? p.stack : typeof p === 'string' ? p : JSON.stringify(p))).join(' ');
  process.stdout.write(`${stamp} ${level.padEnd(7)} [${name}] ${text}\n`);
}

function child(name) {
  const l = { child };
  for (const level of Object.keys(LEVELS)) l[level.toLowerCase()] = (...parts) => line(level, name, parts);
  // One line per request when it finishes: method, path, status, time.
  // 5xx are ERROR, 4xx WARN; the health probes Cove sends every few seconds
  // are TRACE, so they do not bury the rest.
  l.request = (req, res) => {
    const t0 = process.hrtime.bigint();
    res.on('finish', () => {
      const ms = Number(process.hrtime.bigint() - t0) / 1e6;
      const path = req.url.split('?')[0];
      const level = res.statusCode >= 500 ? 'ERROR' : res.statusCode >= 400 ? 'WARN' : path === '/health' || path === '/__live' ? 'TRACE' : 'INFO';
      line(level, name, [`${req.method} ${path} ${res.statusCode} ${ms.toFixed(0)} ms`]);
    });
  };
  return l;
}

const log = child(process.env.LOG_NAME || 'app');
process.on('uncaughtException', (e) => {
  log.error('uncaught:', e);
  process.exit(1);
});
module.exports = log;
