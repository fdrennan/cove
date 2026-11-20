# Blank · __HOST__
A blank Streamlit app started in Cove: one page, a health check and nothing else.
Whatever this project is told to become replaces it. Once it has a purpose,
change this file's first line from "Blank" to its name and rewrite this paragraph.

## Files
- app.py - the whole app, run top to bottom
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

## Seeing a change
Streamlit reruns on every save and the page updates itself.
Packages come from the stack: env/Python/3.13.15 (`ml env/Python/3.13.15`).
Logging: log.py (from log import log), one format across languages; LOG_LEVEL=DEBUG for more.

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1. Serve assets under /static/.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
