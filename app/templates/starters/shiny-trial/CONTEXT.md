# Trial Simulator · __HOST__
A Shiny site, started from Cove's template: thousands of two-arm randomised
trials simulated in a blink, to show how sample size buys power and how
underpowered trials overstate the effects they find. Change first:
`simulate_trials()` - give it the analysis your own trial plans, and the
counted power curve follows it.

## Files
- module.r - the whole site, a Shiny module (the ndexr module contract:
  `ui_module` / `server_module`, wrapped in `measured`). Two branches, each a
  `ui_`/`server_` pair:
  - `power` - how often a trial of n patients per arm finds an effect of a
    given size: the t-test's power curve, in theory and as counted from the
    simulated trials, with the n that buys 80% and 90%
  - `results` - what those trials report: their p-values beside the same
    trials with no effect at all, and their effect estimates - the
    significant ones overstate the truth when power is low (the winner's curse)
  Each visual fills the window under the site's header, its controls in a
  glass card that folds to its title (a `<details>`; on a phone it starts
  folded). Save module.r and reload: the router picks the change up, no deploy.
- app.env, build.sh - a module app (no port of its own); build.sh checks that
  module.r parses

Outcomes are standardised (control mean 0, sd 1, treatment mean d), so d is
Cohen's effect size: 0.2 small, 0.5 medium, 0.8 large. A trial is simulated
from its sufficient statistics, not its patients: an arm's mean is normal with
sd 1/sqrt(n), and its variance is chi-squared on n - 1 degrees of freedom over
n - 1. The t-test sees exactly what it would from n draws, and 5000 trials of
500 patients cost four numbers each.

Logging: r/log.r (log$log_info, log_fn in every ui_/server_), one format across languages; LOG_LEVEL=DEBUG for more.

## Built on
Base R and Shiny only, from the stack's R; no dataset, key or database.
Cohen, J. (1988), "Statistical Power Analysis for the Behavioral Sciences",
2nd ed. Ioannidis, J. P. A. (2008), "Why Most Discovered True Associations
Are Inflated", Epidemiology 19(5).

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public).

## Open
- (none logged)
