# Outbreak Simulator · __HOST__
A Python app, started from Cove's template: an agent-based epidemic. Every
person is a dot on a map of four neighbourhoods; they sleep in households and
by day go to a school or a market or wander near home. Infection passes by
proximity, a per-contact chance each tick (pairs found with a scipy cKDTree),
then incubation, an infectious spell and recovery. Distancing, closing schools
or markets, a vaccination rate and testing with isolation can be switched on
mid-run. Every infection records its infector, so R is read from the
transmission tree. One seed per run: the same seed and the same interventions
at the same ticks replay the same epidemic. Change first: `DISEASE` and
`POLICY` in app.py - a key there gets its slider on the page; add the same
name to `New` or `Policy` and Sim reads it as `self.q.<key>` or
`self.policy.<key>`.

## Files
- app.py - FastAPI and the model: DISEASE and POLICY (the page builds its
  sliders from them), class Sim (numpy arrays, one entry per person),
  GET /api/menu, POST /api/sim (`{"client","seed","n","p_contact",
  "incubation","infectious","initial","policy"}` -> map and frame 0),
  POST /api/sim/{client}/step (`{"ticks"}` -> positions, states, new
  infections, daily history, R), POST /api/sim/{client}/policy (interventions,
  from the next tick), GET /api/sim/{client} (the run so far: history, R by
  infection cohort, interventions by tick). At most MAX_SIMS runs are kept,
  least recently used dropped first.
- static/index.html, static/style.css, static/app.js - the page: the map
  fills the window, people glide between server frames, red lines show who
  infected whom, the epidemic curve is drawn live with interventions marked,
  controls sit in a glass panel that folds on a phone
- run.sh - loads Python from the ndexr stack and serves with uvicorn
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - byte-compile and import-check app.py;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Python from the ndexr stack

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
Python, fastapi, uvicorn, pydantic, numpy and scipy come from the stack's env
set (env/Python/3.13.15, else env/r4.6_py3.13-release); nothing is
pip-installed.

## Built on
Ferguson NM, Cummings DAT, Fraser C, Cajka JC, Cooley PC, Burke DS (2006).
Strategies for mitigating an influenza pandemic. Nature 442, 448-452.
doi:10.1038/nature04795. Their model places people in households, schools and
workplaces and tests school and workplace closure, household quarantine,
antiviral treatment and vaccination; this one is the same idea at toy scale,
on a plane instead of a census.

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
