# Blank · __HOST__
A blank Node.js app started in Cove: one page, a health check and nothing else.
Whatever this project is told to become replaces it. Once it has a purpose,
change this file's first line from "Blank" to its name and rewrite this paragraph.

## Files
- static/index.html, static/style.css - the page
- static/live.js - reloads the page when the app changes; keep it
- server.js - Node standard library, no npm; the ROUTES table is where an API goes
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Seeing a change
Saves in static/ show within a second. server.js restarts itself when saved.

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1. Serve assets under /static/.

## Logging
log.js (`const log = require('./log')`), one format across languages; LOG_LEVEL=DEBUG for more.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
