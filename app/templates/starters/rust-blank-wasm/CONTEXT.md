# Blank · __HOST__
A blank Rust and WebAssembly app started in Cove: one page, a health check and nothing else.
Whatever this project is told to become replaces it. Once it has a purpose,
change this file's first line from "Blank" to its name and rewrite this paragraph.

## Files
- src/lib.rs - what runs in the browser, compiled to /app.wasm
- src/main.rs - the server; tiny_http, not compiled to wasm
- static/app.js - loads the wasm and calls it
- static/index.html, static/style.css - the page, read on each request
- static/live.js - reloads the page when the app changes; keep it
- build-web.sh - the wasm build; build.sh runs it
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Seeing a change
Saves in static/ show within a second. After a change to src/, run ./build.sh: a new
app.wasm shows at once, and run.sh starts a new server binary.

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1. Serve assets under /static/.

## Logging
Logging: src/log.rs (`log::info!`), one format across languages; LOG_LEVEL=DEBUG for more.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
