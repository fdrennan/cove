# Blank · __HOST__
A blank Rust app started in Cove: one page, a health check and nothing else.
Whatever this project is told to become replaces it. Once it has a purpose,
change this file's first line from "Blank" to its name and rewrite this paragraph.

## Files
- src/main.rs - the routes, in `main`; tiny_http only
- static/index.html, static/style.css - the page, read on each request
- static/live.js - reloads the page when the app changes; keep it
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Seeing a change
Saves in static/ show within a second, with nothing to compile. After a change to
src/, run ./build.sh: run.sh starts the new binary and the page follows.

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1. Serve assets under /static/.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
