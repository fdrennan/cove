# Blank Dash · __HOST__
A blank Dash app started in Cove: a slider, a Plotly chart and a health check. Whatever this project is told to become replaces it. Once it has a purpose,
change this file's first line from "Blank Dash" to its name and rewrite this paragraph.

## Files
- app.py - the layout, its callback, /health, and how it is served
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Seeing a change
Dash's hot reload refreshes the page on every save and its reloader restarts the server. Flask's debugger stays off: it runs code typed in the browser.
Packages come from the stack: env/Python/3.13.15 (`ml env/Python/3.13.15`); APP_NEEDS in app.env names them, and env.sh refuses to start without them.
Logging: log.py (from log import log), one format across languages; LOG_LEVEL=DEBUG for more.

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
