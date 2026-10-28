# Scientific API · __HOST__
A Python app, started from Cove's template: a numerical HTTP service in
FastAPI, with numpy and scipy doing the work - summary statistics, least-squares
fits, the normal distribution and the logistic ODE - each answered with the
server's own compute time, and FastAPI's interactive reference at /docs. The
page at / calls the API live. Change first: app.py - add a route beside
/fit/linear for the computation this site serves; /docs lists it.

## Files
- app.py - the API: the routes, their input models, the numerics
- static/index.html, static/style.css, static/app.js - the landing page
- run.sh - loads Python from the ndexr stack and serves with uvicorn
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - byte-compile and import-check app.py;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Python from the ndexr stack

## Endpoints
- GET  /health
- POST /stats/describe        `{"values":[...], "probs":[0.05,0.5,0.95]}` -> n, mean, sd, se, 95% CI, median, IQR, skewness, kurtosis, quantiles
- POST /fit/linear            `{"x":[...], "y":[...]}` -> slope, intercept, standard errors, r2, residual sd, p(slope), fitted
- GET  /distributions/normal?mu=0&sigma=1&n=201 -> x, density, cdf, quantiles
- GET  /ode/logistic?r=0.8&K=100&y0=2&t_end=20&n=101 -> t, y (solve_ivp RK45), error vs the closed form
- GET  /docs, /redoc, /openapi.json - FastAPI's reference (the /docs and /redoc
  pages load Swagger UI / ReDoc from a CDN; the landing page loads nothing
  external)

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
Python, fastapi, uvicorn, numpy and scipy come from the stack's env set
(env/Python/3.13.15, else env/r4.6_py3.13-release); nothing is pip-installed.

## Built on
Virtanen, P. et al. (2020), "SciPy 1.0: fundamental algorithms for scientific
computing in Python", Nature Methods 17.

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
