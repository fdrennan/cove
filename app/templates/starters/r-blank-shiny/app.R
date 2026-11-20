# __HOST__ - a Shiny app the traditional way: one file, ui and server.
# shiny::runApp(".") serves it (run.sh); App reloads when you save.
library(shiny)
# r/log.r: one log format across Cove's languages. box resolves ./ against
# the working directory, the project (run.sh cd's there).
box::use(./r/log)
log$log_info("__HOST__: Shiny app starting on 127.0.0.1:{Sys.getenv('PORT')}")

ui <- fluidPage(
  titlePanel("__HOST__"),
  sidebarLayout(
    sidebarPanel(sliderInput("n", "Points", min = 10, max = 500, value = 100)),
    mainPanel(plotOutput("plot"))
  )
)

server <- function(input, output, session) {
  log$log_session(session)
  output$plot <- renderPlot({
    plot(rnorm(input$n), rnorm(input$n), pch = 19, col = "#3b82f6", xlab = "", ylab = "")
  })
}

shinyApp(ui, server)
