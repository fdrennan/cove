# Double Pendulum Lab · __HOST__
A Python app, started from Cove's template: a fan of double pendulums whose
first angles differ by a hair, integrated together by scipy's solve_ivp
(DOP853, RK45 or RK23) on the server and animated on a full-screen canvas in
the browser. Beside them, the distance of each tip from the first pendulum's
tip on a log scale, with an e^(λt) fit to the nearest one (a Lyapunov-style
growth rate), and the energy drift, which a frictionless pendulum should not
have: it is the integrator's error, made visible. A starting point: describe
what this site is for here, and make the first screen yours.

## Files
- app.py - FastAPI: GET / (the page), GET /health, GET /api/menu (slider
  limits and integrators, which the page builds its controls from), POST
  /api/run (`{"m1","m2","l1","l2","theta1","theta2","n","spread","rtol",
  "method","duration"}`, angles in degrees, spread and rtol as log10 ->
  angles at 30 frames/s as base64 float32, tip divergence, the λ fit, energy
  drift, evaluations and solve time). The whole fan is one ODE system, so
  every pendulum shares the step sizes.
- static/index.html, static/style.css, static/app.js - the page: pendulums on
  a full-screen canvas, interpolated between frames; the two log charts and
  the readouts in glass panels; the controls in a glass card that folds (on a
  phone it starts folded)
- run.sh - loads Python from the ndexr stack and serves with uvicorn
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - byte-compile and check imports; start in
  the background and wait for /health (pid and log in .run/); stop; load Python
  from the ndexr stack

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background. Python, fastapi, uvicorn, numpy and scipy come from the
stack's env set (env/Python/3.13.15, else env/r4.6_py3.13-release); nothing is
pip-installed. Tighter tolerances cost time: measured 2026-10-26 on a loaded
24-core box, 30 pendulums over 20 s took about 0.5 s with RK23 at 1e-3, 1 s
with RK45 at 1e-6 and 4 s with DOP853 at 1e-9, with worst energy drift
2e-2, 8e-5 and 1e-8 of the potential well.

Logging: log.py (from log import log), one format across languages; LOG_LEVEL=DEBUG for more.

## Built on
Shinbrot, T., Grebogi, C., Wisdom, J. and Yorke, J. A. (1992), "Chaos in a
double pendulum", American Journal of Physics 60(6), 491-499. Dormand, J. R.
and Prince, P. J. (1980), "A family of embedded Runge-Kutta formulae", J.
Computational and Applied Mathematics 6(1), 19-26.

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public). The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
