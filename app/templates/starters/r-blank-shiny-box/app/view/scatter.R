# One piece of the app: a scatter of n random points. Its own ui and server,
# so main.R composes it like any other module.
box::use(
  shiny[NS, moduleServer, plotOutput, renderPlot],
  graphics[plot],
  stats[rnorm],
  r/log,
)

#' @export
ui <- function(id) {
  log$log_fn()
  plotOutput(NS(id, "plot"))
}

#' @export
server <- function(id, n) {
  log$log_fn()
  moduleServer(id, function(input, output, session) {
    output$plot <- renderPlot({
      plot(rnorm(n()), rnorm(n()), pch = 19, col = "#3b82f6", xlab = "", ylab = "")
    })
  })
}
