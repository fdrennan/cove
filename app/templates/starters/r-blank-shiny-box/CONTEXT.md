# __HOST__

A Shiny app built from box modules. `app.R` imports `app/main.R`, the app's
layout; each piece of the app is a module under `app/view/` with its own
`ui(id)` and `server(id, ...)`, imported with `box::use` - never `library()`.
It runs as its own server (`shiny::runApp`) on the port in `.cove/app.env`,
with R, shiny and box from the ndexr stack.

## Files
- app.R - the entry: box.path, then `main$ui` / `main$server`
- app/main.R - the app's layout and the modules it uses
- app/view/scatter.R - one module: a plot of random points
- .cove/ - how Cove builds, starts and stops it (APP-CONTRACT.md)

Logging: r/log.r (log$log_info, log_fn in every ui_/server_), one format across languages; LOG_LEVEL=DEBUG for more.
