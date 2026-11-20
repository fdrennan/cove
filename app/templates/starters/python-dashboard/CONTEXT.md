# System dashboard · __HOST__
A Python app, started from Cove's template: a live view of the machine it runs
on - CPU per core, memory and swap, network and disk throughput, filesystems
and the busiest processes - read with psutil every 2 s into a five-minute
rolling history that the page polls and draws on canvas. Change first:
`sample()` in app.py - add a reading (a GPU, a queue's length) and it arrives
with every poll.

## Files
- app.py - FastAPI and the sampler: one background task samples psutil every
  2 s into a ring buffer (150 samples); GET /api/stats?since=<unix time> returns
  the newer samples plus filesystems and top processes; GET /api/info the
  machine's name, OS and CPU count
- static/index.html, static/style.css, static/app.js - the page: rolling charts
  with hover read-outs, a tile per core, filesystem bars, the process table
- run.sh - loads Python from the ndexr stack and serves with uvicorn
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - byte-compile and import-check app.py;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Python from the ndexr stack

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
Python, fastapi, uvicorn and psutil come from the stack's env set
(env/Python/3.13.15, else env/r4.6_py3.13-release); nothing is pip-installed.

Logging: log.py (from log import log), one format across languages; LOG_LEVEL=DEBUG for more.

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public). The server listens on 127.0.0.1 only; nginx in front decides who
reaches it. The page shows the host's process names and users: keep it gated.

## Open
- (none logged)
