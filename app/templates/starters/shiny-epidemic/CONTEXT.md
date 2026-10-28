# Epidemic Lab · __HOST__
A Shiny site, started from Cove's template: an epidemic two ways - the SEIR
equations solved in base R, and the same disease spreading person to person
across a town on a lattice - with vaccination before the outbreak and
distancing from a chosen day. Change first: `seir_field()`, the model's four
equations - add a compartment there (deaths, waning immunity) and the curves
follow.

## Files
- module.r - the whole site, a Shiny module (the ndexr module contract:
  `ui_module` / `server_module`, wrapped in `measured`). Two branches, each a
  `ui_`/`server_` pair:
  - `curves` - the SEIR model (Susceptible, Exposed, Infectious, Recovered)
    for a year of days, redrawn on every slider move
  - `grid` - the disease loose in a town of people on a lattice, each one a
    cell that catches it from its neighbours; runs on Start
  Each visual fills the window under the site's header, its controls in a
  glass card that folds to its title (a `<details>`; on a phone it starts
  folded). Save module.r and reload: the router picks the change up, no deploy.
- app.env, build.sh - a module app (no port of its own); build.sh checks that
  module.r parses

The model:

  dS/dt = -b S I     dE/dt = b S I - E / L     dI/dt = E / L - I / D     dR/dt = I / D

with b = R0 / D, D the infectious period and L the latent one (L = 0 is SIR:
the exposed become infectious the same day). Vaccinating a share v moves it
from S to immune before day 0; distancing scales b down from its day on.

## Built on
Base R and Shiny only, from the stack's R; no dataset, key or database.
Kermack, W. O. and McKendrick, A. G. (1927), "A Contribution to the
Mathematical Theory of Epidemics", Proc. R. Soc. Lond. A 115(772).

## Auth
Login-gated.

## Open
- (none logged)
