# Scientific API · __HOST__
A calculation service in Julia: HTTP.jl routes, JSON3 in and out, and the
numerical libraries of the ndexr stack's Julia set behind them - summary
statistics (StatsBase), fourteen probability distributions (Distributions),
least-squares and polynomial regression with confidence bands (GLM), and the
Lotka-Volterra predator-prey equations solved with Tsit5 (OrdinaryDiffEq).
The page at / calls every endpoint live and draws what comes back.

## Files
- server.jl - the routes and handlers; every answer is JSON, errors included ({error}, 400)
- static/index.html - the page: one card per endpoint, canvas plots, no external scripts
- run.sh - loads Julia 1.11.7 and its packages from the ndexr stack, then serves
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - precompile the packages server.jl loads;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Julia from the ndexr stack

## Endpoints
- GET  /api - the endpoint list
- POST /stats - {"numbers": [..]} -> n, mean, std, var, sem, median, mad, iqr, skewness, kurtosis, quantiles, histogram
- GET  /distributions/{name}?<params>&points= - normal, lognormal, gamma, beta, exponential, studentt, chisq, cauchy, weibull, uniform, poisson, binomial, geometric, negbinomial
- POST /fit/linear - {"x": [..], "y": [..], "degree": 1-5} -> coefficients with std. errors and p-values, R², AIC, 95% band
- GET  /ode/lotka-volterra?alpha&beta&gamma&delta&prey&predator&tmax&points - the two series
- GET  /health - ok

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` overrides it for one run); `./up.sh` and
`./down.sh` start and stop it in the background.
It listens once every endpoint has answered in-process: about 90 s on this
host (30 s loading packages, 60 s compiling the handlers), so the first
visitor never waits on the compiler. The first start on a new machine also
precompiles packages and can take several minutes.

## Built on
Bezanson, J. et al. (2017), "Julia: A Fresh Approach to Numerical Computing",
SIAM Review 59(1). Tsitouras, Ch. (2011), "Runge-Kutta pairs of order 5(4)
satisfying only the first column simplifying assumption", Computers &
Mathematics with Applications 62(2) - the Tsit5 solver. Volterra, V. (1926),
"Fluctuations in the abundance of a species considered mathematically",
Nature 118.

## Auth
Login-gated. The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
