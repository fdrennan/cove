# Monte Carlo Pricer · __HOST__
A Shiny site, started from Cove's template: a European option priced by
simulating the stock under geometric Brownian motion, checked against the
Black-Scholes formula. Change first: `payoff()` - a new payoff (digital,
capped) prices from the same simulated paths.

## Files
- module.r - the whole site, a Shiny module (the ndexr module contract:
  `ui_module` / `server_module`, wrapped in `measured`). Two branches, each a
  `ui_`/`server_` pair:
  - `paths` - a few hundred simulated paths of the stock, full screen; the card
    prices the option from every path and sets Black-Scholes beside it
  - `evidence` - the price settling as paths are added (its 95% band), and
    where the paths end against the lognormal density theory gives
  Each visual fills the window under the site's header, its controls in a
  glass card that folds to its title (a `<details>`; on a phone it starts
  folded). Save module.r and reload: the router picks the change up, no deploy.
- app.env, build.sh - a module app (no port of its own); build.sh checks that
  module.r parses

Logging: r/log.r (log$log_info, log_fn in every ui_/server_), one format across languages; LOG_LEVEL=DEBUG for more.

## Built on
Base R and Shiny only, from the stack's R; no dataset, key or database.
Black, F. and Scholes, M. (1973), "The Pricing of Options and Corporate
Liabilities", J. Political Economy 81(3).

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public).

## Open
- (none logged)
