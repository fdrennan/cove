# Blank · __HOST__
A blank Julia app started in Cove: one page, a health check and nothing else.
Whatever this project is told to become replaces it. Once it has a purpose,
change this file's first line from "Blank" to its name and rewrite this paragraph.

## Files
- server.jl - the routes, in `route`; `handle` logs each request
- static/index.html, static/style.css - the page, read on each request
- static/live.js - reloads the page when the app changes; keep it
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Seeing a change
Saves in static/ show within a second. run.sh restarts the server when server.jl is
saved, and the page follows. Packages come from the stack: env/Julia/1.11.7.

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1. Serve assets under /static/.

## Logging
Logging: log.jl (`Log.info`), one format across languages; LOG_LEVEL=DEBUG for more.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
