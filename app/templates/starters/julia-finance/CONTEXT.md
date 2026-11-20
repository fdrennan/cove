# Monte Carlo Pricer · __HOST__
A Julia app, started from Cove's template: a European option priced by
simulating the stock under geometric Brownian motion - up to two million
paths, split across every core - and checked against the Black-Scholes
formula. The simulated paths fill the screen; the card holds the option and
the prices. Change first: `terminal!()` in server.jl - another model of the
stock (jumps, stochastic volatility) prices on every core the same way.

## Files
- server.jl - HTTP.jl: GET / (the page), GET /health, POST /api/price
  ({s, k, r, sigma, t, n, type, seed} -> Monte Carlo price and its standard
  error, Black-Scholes, paths to draw, the running estimate, and where the
  stock ends against the lognormal density). Distributions.jl for the normal
  CDF and the lognormal density.
- static/index.html - the page: paths on a full-screen canvas (bright ones end
  in the money), the convergence and terminal-price plots in glass panels, a
  glass card that folds (on a phone it starts folded)
- run.sh - loads Julia from the ndexr stack (env/Julia/1.11.7, else
  Julia/1.11.7 and the stack's Julia site library); nothing is installed
- app.env - language, kind (port), port and health path, read by the verbs
- build.sh, up.sh, down.sh, env.sh - precompile the packages server.jl loads;
  start in the background and wait for /health (pid and log in .run/); stop;
  load Julia from the ndexr stack

## Run
`./run.sh` serves on 127.0.0.1:$APP_PORT, the port app.env gives this
project (`PORT=<n> ./run.sh` for one run). A million paths price in
about 30 ms on 24 threads (measured 2026-10-25: 9.128 +/- 0.016 against
Black-Scholes 9.1218 for S 100, K 105, sigma 0.25, r 3%, one year).

## Built on
Black, F. and Scholes, M. (1973), "The Pricing of Options and Corporate
Liabilities", J. Political Economy 81(3).

## Logging
Logging: log.jl (`Log.info`), one format across languages; LOG_LEVEL=DEBUG for more.

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public). The server listens on 127.0.0.1 only; nginx in front decides who
reaches it.

## Open
- (none logged)
