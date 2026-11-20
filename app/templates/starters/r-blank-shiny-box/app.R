# __HOST__ - a Shiny app built from box modules: app/main.R is the app, and
# each piece of it a module under app/view/, imported with box::use - never
# library(). shiny::runApp(".") serves this file (run.sh).
options(box.path = getwd())
box::use(app/main, r/log)

log$log_info("__HOST__: Shiny app starting on 127.0.0.1:{Sys.getenv('PORT')}")
shiny::shinyApp(main$ui("app"), function(input, output, session) {
  log$log_session(session)
  main$server("app")
})
