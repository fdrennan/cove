# API Observatory · __HOST__
A Python app, started from Cove's template: a FastAPI service that watches its
own traffic. Five sample endpoints under /sample/ - fast, slow (log-normal
latency), flaky (503 for a set share of calls), cpu (a prime sieve) and items
(a paginated catalogue of 1,000 synthetic stars) - are the API being observed.
An ASGI middleware times every request to them, keeps the record in memory and
checks each response body against the pydantic model the route declares. The
page shows it live over Server-Sent Events: latency histogram with p50/p95/p99
over a sliding window, requests per second, 5xx share, status mix, a table per
endpoint, the request feed and every contract violation; a load generator in
the page (endpoint, concurrency, duration) streams its own results the same
way. FastAPI's /docs and /redoc browse the OpenAPI schema. What Python brings:
pydantic models that are at once the documentation, the validator and the
schema, and asyncio - the slow endpoint, the event streams and 64 load workers
share one event loop without a thread each. Change first: SAMPLES, CONTRACTS
and the /sample/ routes in app.py - put your own endpoints there and the
observatory, the contract check and /docs pick them up.

## Files
- app.py - FastAPI: the contracts (pydantic models), the sample routes, the
  Recorder middleware, the statistics, and the observatory routes:
  GET /api/stream (SSE, a snapshot every 500 ms), GET /api/stats (one
  snapshot), GET /api/samples (the raw latencies behind it), GET/POST
  /api/settings (`{"flaky_rate","slow_median_ms","slow_sigma","drift","window_s"}`,
  any subset), POST /api/reset, GET /api/load
  (`?endpoint=&concurrency=&duration=&limit=`, SSE `tick` then `done`), POST
  /api/contract (`?calls=`), GET /api/env (runtime, stack module, package
  versions, start time, CPUs)
- static/index.html, static/style.css, static/app.js - the page: the board
  fills the window, the controls and the Environment panel sit in a glass panel
- run.sh - loads Python from the ndexr stack and serves with uvicorn
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - byte-compile and import-check app.py;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Python from the ndexr stack

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
Python, fastapi, pydantic, uvicorn, numpy and httpx come from the stack's env
set (env/Python/3.13.15, else env/r4.6_py3.13-release); nothing is
pip-installed.
The record is in memory: the last 200,000 requests, gone on restart. The load
generator calls the API over loopback from the same process, so its latencies
include the client's share of the event loop; it runs one load at a time.
/openapi.json is served from here; the /docs and /redoc pages are FastAPI's
own, which fetch the Swagger UI and ReDoc scripts from jsDelivr in the browser
(the stack's Python set carries no offline copy).

## Built on
- Percentiles: Hyndman, R. J. and Fan, Y. (1996), "Sample quantiles in
  statistical packages", The American Statistician 50(4) - definition 7,
  numpy.percentile's default, written out in `percentile()`.
- Error-rate interval: Wilson, E. B. (1927), "Probable inference, the law of
  succession, and statistical inference", JASA 22(158).
- Why p99 and not the mean: Dean, J. and Barroso, L. A. (2013), "The Tail at
  Scale", Communications of the ACM 56(2).
- Prime sieve: Eratosthenes; the sieve answers pi(10^6) = 78,498.

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
