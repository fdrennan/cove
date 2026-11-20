# Blank R Markdown · __HOST__
A blank R Markdown report started in Cove, published as a page. Its parameters come from the address (`?points=1000&seed=7`) - a parameterized report. Whatever this project is told to become replaces it. Once it has a purpose,
change this file's first line from "Blank R Markdown" to its name and rewrite this paragraph.

## Files
- report.Rmd - the report and its `params:`
- server.R - renders it per set of parameters (kept until report.Rmd changes) and serves it
- app.env, build.sh, up.sh, down.sh, run.sh, env.sh - how the app is checked,
  started and stopped

Logging: r/log.r (log$log_info, log_fn in every ui_/server_), one format across languages; LOG_LEVEL=DEBUG for more.

## Seeing a change
A save of report.Rmd renders on the next visit, and the open page reloads itself; a save of server.R restarts the server.
Packages come from the stack: env/r4.6_py3.13-release (`ml env/r4.6_py3.13-release`); APP_NEEDS in app.env names them, and env.sh refuses to start without them.

## Paths nginx keeps for itself
A request to /vendor/, /images/, /css/, /javascript/, /shared/, /sites/,
/uploads/, /videos/ or /domains/ never reaches this app on the web, though it works
on 127.0.0.1.

## Auth
Set under Project > Access in Cove. The server listens on 127.0.0.1 only; nginx in
front decides who reaches it.
