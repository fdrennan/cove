# __HOST__

A Shiny app the traditional way: `app.R` holds `ui`, `server` and
`shinyApp(ui, server)`. It runs as its own server (`shiny::runApp`) on the
port in `.cove/app.env`, with R and shiny from the ndexr stack.

## Files
- app.R - the whole app
- .cove/ - how Cove builds, starts and stops it (APP-CONTRACT.md)

Logging: r/log.r (log$log_info, log_fn in every ui_/server_), one format across languages; LOG_LEVEL=DEBUG for more.
