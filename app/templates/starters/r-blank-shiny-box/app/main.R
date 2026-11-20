# The app: its layout, and the modules it is made of.
box::use(
  shiny[NS, moduleServer, fluidPage, titlePanel, sidebarLayout, sidebarPanel, mainPanel, sliderInput, reactive],
  app/view/scatter,
  r/log,
)

#' @export
ui <- function(id) {
  log$log_fn()
  ns <- NS(id)
  fluidPage(
    titlePanel("__HOST__"),
    sidebarLayout(
      sidebarPanel(sliderInput(ns("n"), "Points", min = 10, max = 500, value = 100)),
      mainPanel(scatter$ui(ns("scatter")))
    )
  )
}

#' @export
server <- function(id) {
  log$log_fn()
  moduleServer(id, function(input, output, session) {
    scatter$server("scatter", n = reactive(input$n))
  })
}
