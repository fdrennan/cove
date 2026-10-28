# Machine Dashboard · __HOST__
A Shiny site, started from Cove's template: the machine serving it, live -
processors, load and memory from Linux's /proc every two seconds once Start
is pressed, and the R session answering the page. Change first: `read_cpu()`
and `read_mem()` - add a reader for another /proc file (/proc/net/dev for
the network) and a tile for it.

## Files
- module.r - the whole site, a Shiny module (the ndexr module contract:
  `ui_module` / `server_module`, wrapped in `measured`). Two branches, each a
  `ui_`/`server_` pair:
  - `live` - CPU in use and load over the last four minutes, full screen; the
    card holds the latest numbers
  - `inside` - each processor's use this moment, and memory over time (held by
    programs, kept as cache)
  Each visual fills the window under the site's header, its controls in a
  glass card that folds to its title (a `<details>`; on a phone it starts
  folded). Save module.r and reload: the router picks the change up, no deploy.
- app.env, build.sh - a module app (no port of its own); build.sh checks that
  module.r parses

## Built on
Base R and Shiny only, from the stack's R; no dataset, key or database.
Linux's /proc/stat, /proc/loadavg and /proc/meminfo; on a machine without
/proc the page says so.

## Auth
Login-gated.

## Open
- (none logged)
