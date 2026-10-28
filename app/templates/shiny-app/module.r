# __HOST__/module.r
#
# Your first experiment. Save this file and reload the page: the router picks
# up the change on the next request, no deploy. The page is one branch, a
# ui_/server_ pair below; a second branch is another pair, added to ui_module
# and .server_inner.
#
#   chaos     Chaos Lab: the Lorenz system solved in base R as you watch, two
#             runs a hair apart parting ways - the butterfly effect, measured.
#             The same experiment is Cove's Rust starter (src/chaos.rs).
#
# Three ideas run through it: reactive inputs (the sliders), scientific
# computation (rk4(), plain R), and graphical output (base R plotting).

# ── chaos ────────────────────────────────────────────────────────────────────
#
#   dx/dt = sigma (y - x)   dy/dt = x (rho - z) - y   dz/dt = x y - beta z
#
# Lorenz's 1963 model of air heated from below. Two runs start 10^gap apart;
# the lower plot is the distance between them on a log scale - a straight
# climb is exponential divergence, until it saturates at the attractor's size.

# The Lorenz field for every run at once: `s` is a 3 x n matrix (x, y, z down,
# one run per column).
lorenz <- function(s, p) {
  rbind(p$sigma * (s[2, ] - s[1, ]),
        s[1, ] * (p$rho - s[3, ]) - s[2, ],
        s[1, ] * s[2, ] - p$beta * s[3, ])
}

# One classical Runge-Kutta step of size h.
rk4 <- function(s, p, h) {
  k1 <- lorenz(s, p)
  k2 <- lorenz(s + h / 2 * k1, p)
  k3 <- lorenz(s + h / 2 * k2, p)
  k4 <- lorenz(s + h * k3, p)
  s + h / 6 * (k1 + 2 * k2 + 2 * k3 + k4)
}

CHAOS <- list(h = 0.005, trail = 2500, tick = 150, runs = c("#5AC8FA", "#FF9650"), gap = "#F0DC78", ground = "#080A10")

# Where the runs start: already on the attractor - 20 time units from (1, 1, 1)
# at Lorenz's values, computed once here. From (1, 1, 1) itself the runs spent
# their first ~10 seconds on screen in one slow spiral with the gap flat at
# 10^-6; from here, at 80 steps a frame, they part in about five (measured
# 2026-10-24: the gap reaches 1 at t = 14.1).
START <- local({
  s <- matrix(c(1, 1, 1), 3, 1)
  for (i in seq_len(4000)) s <- rk4(s, list(sigma = 10, rho = 28, beta = 8 / 3), CHAOS$h)
  as.vector(s)
})

# The attractor fills the window; the controls float over it in a card that
# folds to its title, and the gap plot sits in a corner (operator,
# 2026-10-24: "full screen for the apps themselves ... beautiful and
# immersive"). STAGE_CSS, at the end of this file, lays it out.
ui_chaos <- function(id) {
  box::use(shiny = shiny[NS, tags, sliderInput, actionButton, plotOutput, textOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("attractor"), height = "100%"),
    # Nothing runs until it is asked to (operator, 2026-10-25: "chaos lab
    # templates need to wait to start when pushed"): a new site no longer sets
    # every visitor's R session computing the moment the page loads.
    uiOutput(ns("start"), class = "stage-start"),
    tags$div(class = "stage-plot glass", plotOutput(ns("gaps"), height = "150px")),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Chaos Lab"),
      tags$p("Two runs of the Lorenz system start a hair apart. Move a slider and watch them part."),
      sliderInput(ns("sigma"), "σ  how fast the air turns over", 0.1, 30, 10, step = 0.1, width = "100%"),
      sliderInput(ns("rho"), "ρ  how hard it is heated", 0.1, 60, 28, step = 0.1, width = "100%"),
      sliderInput(ns("beta"), "β  the cell's shape", 0.1, 8, 2.67, step = 0.01, width = "100%"),
      sliderInput(ns("speed"), "Speed, steps per frame", 5, 120, 80, step = 5, width = "100%"),
      sliderInput(ns("gap"), "Starting gap, 10^x", -12, -1, -6, step = 1, width = "100%"),
      tags$div(
        class = "d-flex flex-wrap gap-2 mb-2",
        actionButton(ns("run"), "Start", class = "btn btn-primary btn-sm"),
        actionButton(ns("reset"), "Reset", class = "btn btn-outline-light btn-sm"),
        actionButton(ns("classic"), "Lorenz's values", class = "btn btn-outline-light btn-sm")
      ),
      tags$p(class = "font-monospace mb-0", textOutput(ns("readout"), inline = TRUE))
    )
  )
}

server_chaos <- function(id) {
  box::use(
    shiny = shiny[moduleServer, reactiveValues, observeEvent, observe, invalidateLater, renderPlot, renderText, renderUI, isolate, updateSliderInput, updateActionButton, actionButton, tags],
    graphics[par, plot.new, plot.window, lines, points, axis, mtext, segments],
    grDevices[rgb, col2rgb]
  )
  moduleServer(id, function(input, output, session) {
    st <- reactiveValues(a = NULL, b = NULL, t = 0, gaps = numeric(0), times = numeric(0), running = FALSE, started = FALSE)
    params <- function() list(sigma = input$sigma, rho = input$rho, beta = input$beta)
    reset <- function() {
      start <- START
      st$a <- matrix(start, nrow = 1)
      st$b <- matrix(start + c(10^isolate(input$gap), 0, 0), nrow = 1)
      st$t <- 0
      st$gaps <- numeric(0)
      st$times <- numeric(0)
    }
    observeEvent(input$gap, reset())
    observeEvent(input$reset, reset())
    toggle <- function() {
      st$running <- !st$running
      st$started <- TRUE
      updateActionButton(session, "run", label = if (st$running) "Pause" else "Run")
    }
    observeEvent(input$run, toggle())
    observeEvent(input$go, toggle())
    output$start <- renderUI({
      if (st$started) return(NULL)
      actionButton(session$ns("go"), "Start", class = "btn btn-primary btn-lg")
    })
    observeEvent(input$classic, {
      updateSliderInput(session, "sigma", value = 10)
      updateSliderInput(session, "rho", value = 28)
      updateSliderInput(session, "beta", value = 2.67)
      reset()
    })

    # The clock: each frame advances both runs `speed` steps together.
    observe({
      if (!st$running || is.null(isolate(st$a))) return()
      invalidateLater(CHAOS$tick)
      p <- params()
      if (any(vapply(p, is.null, logical(1))) || is.null(input$speed)) return()
      a <- isolate(st$a); b <- isolate(st$b)
      s <- cbind(a[nrow(a), ], b[nrow(b), ])
      n <- input$speed
      out_a <- matrix(0, n, 3); out_b <- matrix(0, n, 3)
      for (i in seq_len(n)) {
        s <- rk4(s, p, CHAOS$h)
        out_a[i, ] <- s[, 1]; out_b[i, ] <- s[, 2]
      }
      a <- rbind(a, out_a); b <- rbind(b, out_b)
      keep <- max(1, nrow(a) - CHAOS$trail + 1):nrow(a)
      st$a <- a[keep, , drop = FALSE]; st$b <- b[keep, , drop = FALSE]
      t1 <- isolate(st$t) + n * CHAOS$h
      st$t <- t1
      st$times <- utils::tail(c(isolate(st$times), t1), 3000)
      st$gaps <- utils::tail(c(isolate(st$gaps), log10(max(sqrt(sum((s[, 1] - s[, 2])^2)), 1e-16))), 3000)
    })

    # `n` shades of `col`, faint to `top` opaque, for a trail fading to its tail.
    fade <- function(col, n, top = 1) {
      v <- col2rgb(col) / 255
      rgb(v[1], v[2], v[3], alpha = seq(0.06 * top, top, length.out = n))
    }
    # Drawn on a transparent page over the stage's night gradient: each run a
    # soft halo under a bright core, fading toward its tail, its head a glow.
    output$attractor <- renderPlot({
      a <- st$a; b <- st$b
      if (is.null(a)) return(invisible())
      par(bg = NA, mar = c(0, 0, 0, 0))
      plot.new(); plot.window(xlim = c(-25, 25), ylim = c(0, 52), asp = 1)
      for (run in list(list(a, CHAOS$runs[1]), list(b, CHAOS$runs[2]))) {
        m <- run[[1]]; n <- nrow(m)
        if (n > 1) {
          segments(m[-n, 1], m[-n, 3], m[-1, 1], m[-1, 3], col = fade(run[[2]], n - 1, 0.07), lwd = 6)
          segments(m[-n, 1], m[-n, 3], m[-1, 1], m[-1, 3], col = fade(run[[2]], n - 1), lwd = 1.6)
        }
        points(m[n, 1], m[n, 3], pch = 19, cex = 4.5, col = fade(run[[2]], 1, 0.18))
        points(m[n, 1], m[n, 3], pch = 19, cex = 1.7, col = run[[2]])
      }
    }, bg = "transparent")
    output$gaps <- renderPlot({
      par(bg = NA, mar = c(2.5, 3.5, 1.6, 0.8), col.axis = "grey75", fg = "grey40", cex.axis = 0.8)
      plot.new()
      tt <- st$times; g <- st$gaps
      plot.window(xlim = if (length(tt) > 1) range(tt) else c(0, 1), ylim = c(-12.5, 2))
      axis(1); axis(2, las = 1)
      if (length(tt) > 1) lines(tt, g, col = CHAOS$gap, lwd = 2)
      mtext("how far apart the runs are, log10", side = 3, col = "grey85", adj = 0, cex = 0.85)
    }, bg = "transparent")
    output$readout <- renderText({
      g <- st$gaps
      sprintf("t = %.1f   gap = %s", st$t, if (length(g)) format(10^g[length(g)], digits = 3) else "-")
    })
  })
}

# ── the site ─────────────────────────────────────────────────────────────────

# The stages: a visual filling the window under the site's header, its
# controls in a glass card that folds to its title (a <details>: no script),
# top right on a desk and along the bottom on a phone. Shiny dims an output
# while it recalculates; an animation recalculates every frame, so that is
# turned off here or the attractor would flicker.
STAGE_CSS <- "
.stage { position: relative; height: calc(100vh - 56px); min-height: 520px; overflow: hidden;
  background: radial-gradient(ellipse at 50% 35%, #16203d 0%, #090c16 55%, #04050a 100%); }
.stage > .shiny-plot-output { position: absolute; inset: 0; }
.stage .recalculating { opacity: 1 !important; transition: none !important; }
.glass { background: rgba(10, 13, 22, 0.78); border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: 14px; backdrop-filter: blur(8px); color: #e6e9ef; }
.stage-card { position: absolute; top: 16px; right: 16px; width: 340px; max-height: calc(100% - 32px);
  overflow: auto; padding: 12px 16px; }
.stage-card summary { font-size: 1.15rem; font-weight: 600; cursor: pointer; }
.stage-card[open] summary { margin-bottom: 8px; }
.stage-plot { position: absolute; left: 16px; bottom: 16px; width: min(460px, 40%); padding: 6px; }
.stage-start { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; pointer-events: none; }
.stage-start .btn { pointer-events: auto; padding: 0.7rem 2.4rem; font-size: 1.25rem; }
@media (max-width: 760px) {
  .stage-card { top: auto; bottom: 12px; left: 12px; right: 12px; width: auto; max-height: 42%; }
  .stage:has(.stage-card[open]) > .shiny-plot-output { bottom: 44%; }
  .stage-plot { top: 10px; bottom: auto; left: 10px; right: 10px; width: auto; padding: 2px; }
  .stage-plot .shiny-plot-output { height: 96px !important; }
}
"

# On a phone the cards start folded: the visual gets the whole screen, and a
# tap on a card's title opens it.
STAGE_JS <- "document.addEventListener('DOMContentLoaded', function () {
  if (window.innerWidth < 760) document.querySelectorAll('.stage-card').forEach(function (d) { d.open = false; });
});"

#' @export
ui_module <- function(id, request) {
  box::use(
    shiny = shiny[NS, tagList, tags, HTML],
    r / shell / shell
  )
  ns <- shiny$NS(id)
  shell$ui_shell(
    ns        = ns,
    request   = request,
    head_args = list(
      title      = "__HOST__",
      domain     = "__HOST__",
      stylesheet = "css/home.css"
    ),
    # The experiment edge to edge, a screen of its own. No
    # instructions under them (operator, 2026-10-25: "we dont need
    # instructions like these in the inspirational apps").
    body = shiny$tagList(
      shiny$tags$style(shiny$HTML(STAGE_CSS)),
      shiny$tags$script(shiny$HTML(STAGE_JS)),
      ui_chaos(ns("chaos"))
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell)
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    server_chaos("chaos")
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
