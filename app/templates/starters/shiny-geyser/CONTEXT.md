# Geyser Laboratory · __HOST__
A Shiny site, started from Cove's template: Old Faithful's 272 eruptions
(`datasets::faithful`, shipped inside R) explored with the statistics R
carries in the box - a kernel density whose bandwidth you drag, a Gaussian
mixture fitted by EM that you step through one iteration at a time, and a
regression that forecasts the next eruption with its prediction interval.
R's strength on show: `density`, `bw.SJ`, `lm` and `predict` are base R, and
EM is forty lines of matrix algebra without a package. Change first:
`em_step()` - give it a third component, or a shared covariance, and the
ellipses and log-likelihood trace follow.

## Files
- module.r - the whole site, a Shiny module (the ndexr module contract:
  `ui_module` / `server_module`, wrapped in `measured`). Four branches, each a
  `ui_`/`server_` pair:
  - `density` - a histogram of eruption length or waiting time under a kernel
    density; drag the bandwidth or snap it to Silverman's (`bw.nrd0`),
    Scott's (`bw.nrd`) or Sheather-Jones' (`bw.SJ`) rule, pick the kernel,
    and count the modes the bandwidth lets through
  - `mixture` - two bivariate Gaussians fitted to (eruption, wait) by EM:
    Step, Play, Fit or Restart from a poor or a random start; points coloured
    by responsibility, 50% and 95% ellipses, and the log-likelihood rising
    with every iteration
  - `predict` - `lm(waiting ~ eruptions)`: the wait after an eruption you
    saw, with its prediction interval (one wait) and confidence interval
    (the mean wait) at 80-99%
  - `env` - a folded Environment card read from the live R session: R
    version, the stack module (`LOADEDMODULES`), each package's version, when
    the R process started and the host's CPU count
  Each visual fills the window under the site's header, its controls in a
  glass card that folds to its title (a `<details>`; on a phone it starts
  folded). Save module.r and reload: the router picks the change up, no deploy.
- app.env, build.sh - a module app (no port of its own); build.sh checks that
  module.r parses

`faithful$waiting` is the wait to the next eruption (`?faithful`). Fitted to
all 272: wait = 33.5 + 10.73 x eruption length, R squared 0.81; EM settles on
clusters with mean eruptions 2.04 and 4.29 minutes, waits 54.5 and 80.0.

## Built on
Base R and Shiny only, from the stack's R; no dataset file, key or database.
Azzalini, A. and Bowman, A. W. (1990), "A Look at Some Data on the Old
Faithful Geyser", Applied Statistics 39(3). Dempster, A. P., Laird, N. M. and
Rubin, D. B. (1977), "Maximum Likelihood from Incomplete Data via the EM
Algorithm", JRSS B 39(1). Sheather, S. J. and Jones, M. C. (1991), "A Reliable
Data-Based Bandwidth Selection Method for Kernel Density Estimation", JRSS B
53(3). Silverman, B. W. (1986), "Density Estimation for Statistics and Data
Analysis".

## Auth
Login-gated.

## Open
- (none logged)
