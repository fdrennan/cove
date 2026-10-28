# Fourier Lab · __HOST__
A Shiny site, started from Cove's template: a signal of three waves and
noise, taken apart by the fast Fourier transform and cleaned by a low-pass
filter. Change first: `low_pass()` - a band-pass or notch filter is a
different set of frequencies set to zero.

## Files
- module.r - the whole site, a Shiny module (the ndexr module contract:
  `ui_module` / `server_module`, wrapped in `measured`). Two branches, each a
  `ui_`/`server_` pair:
  - `signal` - one second of signal, full screen: measured, the waves alone,
    and what the filter leaves; the card sets the waves, the noise and the cutoff
  - `spectrum` - the FFT spectrum with the cutoff, and the first tenth of a
    second sample by sample
  Each visual fills the window under the site's header, its controls in a
  glass card that folds to its title (a `<details>`; on a phone it starts
  folded). Save module.r and reload: the router picks the change up, no deploy.
- app.env, build.sh - a module app (no port of its own); build.sh checks that
  module.r parses

## Built on
Base R and Shiny only, from the stack's R; no dataset, key or database.
Cooley, J. W. and Tukey, J. W. (1965), "An Algorithm for the Machine
Calculation of Complex Fourier Series", Math. Comp. 19(90).

## Auth
Login-gated.

## Open
- (none logged)
