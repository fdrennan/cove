# __HOST__/module.r
#
# Monte Carlo Pricer: a European option priced by simulating the stock, and
# checked against the closed form.
#
#   paths     the stock under geometric Brownian motion, a few hundred of the
#             simulated paths drawn in full; the card prices the option from
#             all of them and sets the Black-Scholes price beside it.
#   evidence  how the Monte Carlo price settles toward Black-Scholes as paths
#             are added (its 95% band narrowing as 1/sqrt(n)), and where the
#             paths end: their histogram over the lognormal density theory
#             gives.
#
#   dS = r S dt + sigma S dW      S_T = S_0 exp((r - sigma^2 / 2) T + sigma sqrt(T) Z)
#
# Prices are risk-neutral: the stock drifts at the risk-free rate r and the
# payoff is discounted by exp(-r T). Black, F. and Scholes, M. (1973), "The
# Pricing of Options and Corporate Liabilities", J. Political Economy 81(3).

FIN <- list(col = c(path = "#5AC8FA", strike = "#FF9650", mc = "#3DDC97", bs = "#F0DC78", ink = "#e6e9ef"))

# Black-Scholes price of a European call or put.
black_scholes <- function(s, k, r, sigma, t, type = "call") {
  d1 <- (log(s / k) + (r + sigma^2 / 2) * t) / (sigma * sqrt(t))
  d2 <- d1 - sigma * sqrt(t)
  if (type == "call") s * stats::pnorm(d1) - k * exp(-r * t) * stats::pnorm(d2)
  else k * exp(-r * t) * stats::pnorm(-d2) - s * stats::pnorm(-d1)
}

# `n` terminal prices, drawn exactly (one normal each - no time steps needed).
terminal <- function(n, s, r, sigma, t) s * exp((r - sigma^2 / 2) * t + sigma * sqrt(t) * stats::rnorm(n))

# `m` whole paths of `steps` steps, one per column, for drawing.
paths <- function(m, steps, s, r, sigma, t) {
  dt <- t / steps
  z <- matrix(stats::rnorm(steps * m), steps, m)
  s * exp(rbind(0, apply((r - sigma^2 / 2) * dt + sigma * sqrt(dt) * z, 2, cumsum)))
}

payoff <- function(st, k, type) if (type == "call") pmax(st - k, 0) else pmax(k - st, 0)

tile <- function(value, label, tone = NULL) {
  box::use(shiny = shiny[tags])
  shiny$tags$div(class = paste("tile", tone), shiny$tags$b(value), shiny$tags$span(label))
}

money <- function(x) formatC(x, format = "f", digits = 3)

# Lines of right margin that keep a full-window plot clear of the card.
card_room <- function(session, out) {
  w <- session$clientData[[paste0("output_", session$ns(out), "_width")]]
  if (!is.null(w) && w > 900) 400 / 19.2 else 1.4
}

ink <- function(mar = c(3.6, 4.4, 3, 1.2)) {
  graphics::par(bg = NA, mar = mar, col.axis = "#c3cad8", col.lab = "#dfe4ee", fg = "#5b6478",
                cex.axis = 0.95, las = 1)
}

# ── paths ────────────────────────────────────────────────────────────────────

ui_paths <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, sliderInput, radioButtons, actionButton, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("paths"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Monte Carlo Pricer"),
      radioButtons(ns("type"), NULL, c("Call" = "call", "Put" = "put"), inline = TRUE),
      sliderInput(ns("s"), "Stock price today, S", 20, 200, 100, step = 1, width = "100%"),
      sliderInput(ns("k"), "Strike, K", 20, 200, 105, step = 1, width = "100%"),
      sliderInput(ns("sigma"), "Volatility, sigma (per year)", 0.05, 1, 0.25, step = 0.01, width = "100%"),
      sliderInput(ns("r"), "Risk-free rate, r (per year)", 0, 0.10, 0.03, step = 0.0025, width = "100%"),
      sliderInput(ns("t"), "Years to expiry, T", 0.1, 5, 1, step = 0.05, width = "100%"),
      sliderInput(ns("n"), "Simulated paths", 1000, 200000, 50000, step = 1000, width = "100%"),
      actionButton(ns("again"), "Simulate again", class = "btn btn-outline-light btn-sm w-100"),
      uiOutput(ns("tiles"))
    )
  )
}

# Returns the simulation, for the evidence branch.
server_paths <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, reactive, req, renderPlot, renderUI, tags],
    graphics[plot.new, plot.window, axis, lines, matlines, abline, mtext, text, rect]
  )
  moduleServer(id, function(input, output, session) {
    sim <- reactive({
      req(input$s, input$k, input$sigma, !is.null(input$r), input$t, input$n, input$type)
      input$again
      st <- terminal(input$n, input$s, input$r, input$sigma, input$t)
      disc <- exp(-input$r * input$t) * payoff(st, input$k, input$type)
      list(p = list(s = input$s, k = input$k, r = input$r, sigma = input$sigma, t = input$t, type = input$type),
           st = st, disc = disc,
           drawn = paths(240, 252, input$s, input$r, input$sigma, input$t),
           bs = black_scholes(input$s, input$k, input$r, input$sigma, input$t, input$type))
    })

    output$paths <- renderPlot({
      x <- sim(); p <- x$p
      tt <- seq(0, p$t, length.out = nrow(x$drawn))
      lim <- stats::quantile(x$drawn, c(0.005, 0.995))
      ink(c(3.8, 4.8, 3.4, card_room(session, "paths")))
      plot.new(); plot.window(xlim = c(0, p$t), ylim = range(lim, p$k), xaxs = "i")
      abline(h = pretty(lim), col = "#ffffff10")
      axis(1); axis(2)
      # Each path coloured by where it ends: in the money glows, out fades.
      itm <- payoff(x$drawn[nrow(x$drawn), ], p$k, p$type) > 0
      matlines(tt, x$drawn[, !itm, drop = FALSE], col = "#8fa3c81c", lty = 1, lwd = 1)
      matlines(tt, x$drawn[, itm, drop = FALSE], col = "#5AC8FA40", lty = 1, lwd = 1.2)
      abline(h = p$k, col = FIN$col[["strike"]], lwd = 2, lty = 2)
      text(0, p$k, paste("strike", p$k), adj = c(-0.1, -0.5), col = FIN$col[["strike"]])
      mtext_fit(sprintf("%d of %s simulated paths of the stock; the bright ones end in the money",
                    ncol(x$drawn), format(length(x$st), big.mark = ",")),
            side = 3, line = 1.4, adj = 0, col = FIN$col[["ink"]], cex = 1.05, font = 2)
      mtext_fit("years", side = 1, line = 2.5, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      x <- sim()
      mc <- mean(x$disc); se <- stats::sd(x$disc) / sqrt(length(x$disc))
      off <- (mc - x$bs) / se
      tags$div(
        class = "tiles",
        tile(money(mc), "Monte Carlo price"),
        tile(money(x$bs), "Black-Scholes price"),
        tile(paste("±", money(1.96 * se)), "95% interval, half width"),
        tile(sprintf("%+.2f se", off), "Monte Carlo minus closed form", if (abs(off) <= 2) "tone-good" else "tone-bad"),
        tile(paste0(round(100 * mean(x$disc > 0)), "%"), "paths ending in the money"),
        tile(money(stats::quantile(x$st, 0.5)), "median price at expiry")
      )
    })

    sim
  })
}

# ── evidence ─────────────────────────────────────────────────────────────────

ui_evidence <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, plotOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage-duo",
    tags$div(class = "glass", plotOutput(ns("converge"), height = "100%")),
    tags$div(class = "glass", plotOutput(ns("ends"), height = "100%"))
  )
}

server_evidence <- function(id, sim) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, renderPlot],
    graphics[plot.new, plot.window, axis, lines, polygon, abline, mtext, rect, hist, legend]
  )
  moduleServer(id, function(input, output, session) {
    output$converge <- renderPlot({
      x <- sim()
      n <- length(x$disc)
      at <- unique(round(exp(seq(log(100), log(n), length.out = 300))))
      cs <- cumsum(x$disc); cq <- cumsum(x$disc^2)
      m <- cs[at] / at
      se <- sqrt(pmax(cq[at] / at - m^2, 0) / at)
      ink(c(3.6, 4.8, 4.2, 1.2))
      plot.new(); plot.window(xlim = range(at), ylim = range(m - 2 * se, m + 2 * se, x$bs), log = "x")
      axis(1); axis(2)
      polygon(c(at, rev(at)), c(m - 1.96 * se, rev(m + 1.96 * se)), col = "#3DDC9726", border = NA)
      lines(at, m, col = FIN$col[["mc"]], lwd = 2.4)
      abline(h = x$bs, col = FIN$col[["bs"]], lwd = 2, lty = 2)
      mtext_fit("the price as paths are added", side = 3, line = 2.2, adj = 0, col = FIN$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("Monte Carlo with its 95% band; dashed: Black-Scholes", side = 3, line = 0.7, adj = 0, col = "#dfe4ee", cex = 0.95)
      mtext_fit("paths (log scale)", side = 1, line = 2.4, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    output$ends <- renderPlot({
      x <- sim(); p <- x$p
      lim <- stats::quantile(x$st, c(0.001, 0.995))
      st <- x$st[x$st >= lim[1] & x$st <= lim[2]]
      h <- hist(st, breaks = 80, plot = FALSE)
      xs <- seq(lim[1], lim[2], length.out = 400)
      # The lognormal density theory gives for S_T.
      mu <- log(p$s) + (p$r - p$sigma^2 / 2) * p$t; sd <- p$sigma * sqrt(p$t)
      dens <- stats::dlnorm(xs, mu, sd)
      ink(c(3.6, 4.8, 4.2, 1.2))
      plot.new(); plot.window(xlim = lim, ylim = c(0, max(h$density, dens) * 1.08), xaxs = "i", yaxs = "i")
      axis(1); axis(2, labels = FALSE)
      rect(h$breaks[-length(h$breaks)], 0, h$breaks[-1], h$density, col = "#5AC8FA55", border = "#5AC8FA")
      lines(xs, dens, col = FIN$col[["bs"]], lwd = 2.4)
      abline(v = p$k, col = FIN$col[["strike"]], lwd = 2, lty = 2)
      mtext_fit("where the stock ends", side = 3, line = 2.2, adj = 0, col = FIN$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("simulated prices at expiry; the curve is the lognormal theory gives; dashed: the strike",
            side = 3, line = 0.7, adj = 0, col = "#dfe4ee", cex = 0.95)
      mtext_fit("price at expiry", side = 1, line = 2.4, col = "#c3cad8")
    }, bg = "transparent", res = 96)
  })
}

# ── the site ─────────────────────────────────────────────────────────────────

# The stages: a visual filling the window under the site's header, its
# controls in a glass card that folds to its title (a <details>: no script),
# top right on a desk and along the bottom on a phone.
STAGE_CSS <- "
.stage { position: relative; height: calc(100vh - 56px); min-height: 520px; overflow: hidden;
  background: radial-gradient(ellipse at 50% 35%, #16203d 0%, #090c16 55%, #04050a 100%); }
.stage > .shiny-plot-output { position: absolute; inset: 0; }
.glass { background: rgba(10, 13, 22, 0.80); border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: 14px; backdrop-filter: blur(8px); color: #e6e9ef; }
.stage-card { position: absolute; top: 16px; right: 16px; width: 360px; max-height: calc(100% - 32px);
  overflow: auto; padding: 12px 16px; }
.stage-card summary { font-size: 1.2rem; font-weight: 600; cursor: pointer; }
.stage-card[open] summary { margin-bottom: 8px; }
.stage-duo { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; padding: 16px;
  height: calc(100vh - 56px); min-height: 520px; background: linear-gradient(180deg, #04050a 0%, #0b1022 100%); }
.stage-duo > .glass { position: relative; padding: 8px; min-height: 0; }
.stage-duo .shiny-plot-output { position: absolute; inset: 8px; height: auto !important; }
.tiles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin-top: 10px; }
.tile { background: rgba(255, 255, 255, 0.06); border-radius: 10px; padding: 7px 10px; }
.tile b { display: block; font-size: 1.35rem; font-weight: 600; color: #fff; font-variant-numeric: tabular-nums; }
.tile span { font-size: 0.9rem; color: #d3d9e6; }
.tile.tone-good b { color: #3DDC97; }
.tile.tone-bad b { color: #FF7A7E; }
@media (max-width: 760px) {
  .stage-card { top: auto; bottom: 12px; left: 12px; right: 12px; width: auto; max-height: 46%; }
  .stage:has(.stage-card[open]) > .shiny-plot-output { bottom: 48%; }
  .stage-duo { grid-template-columns: 1fr; height: auto; padding: 10px; gap: 10px; }
  .stage-duo > .glass { height: 360px; }
}
"

# On a phone the cards start folded: the visual gets the whole screen.
STAGE_JS <- "document.addEventListener('DOMContentLoaded', function () {
  if (window.innerWidth < 760) document.querySelectorAll('.stage-card').forEach(function (d) { d.open = false; });
});"


# A title or axis label that fits its figure on a phone (2026-10-27: on a
# 390 px screen "the 20 most frequent words" ran past the plot's edge). When
# the text is wider than the room from where it is anchored, it first moves -
# a left-aligned title to the figure's left edge, a centred one to the
# figure's centre - and only then shrinks. On a wide plot nothing changes.
mtext_fit <- function(text, side = 3, line = 0, adj = 0.5, cex = 1, font = 1, at = NA, ...) {
  if (side %in% c(1, 3) && is.na(at[1])) {
    usr <- graphics::par("usr")
    lo <- graphics::grconvertX(0, "nfc", "user"); hi <- graphics::grconvertX(1, "nfc", "user")
    pad <- 0.02 * (hi - lo)
    anchor <- usr[1] + adj * diff(usr[1:2])
    room <- if (adj <= 0) hi - pad - anchor else if (adj >= 1) anchor - lo - pad else 2 * min(anchor - lo, hi - anchor) - 2 * pad
    w <- max(graphics::strwidth(text, cex = cex, font = font))
    if (is.finite(w) && w > room) {
      if (adj <= 0) { at <- lo + pad; room <- hi - lo - 2 * pad }
      else if (adj >= 1) { at <- hi - pad; room <- hi - lo - 2 * pad }
      else { at <- (lo + hi) / 2; room <- hi - lo - 2 * pad }
      if (w > room) cex <- cex * room / w
    }
  }
  graphics::mtext(text, side = side, line = line, adj = adj, cex = cex, font = font, at = at, ...)
}

#' @export
ui_module <- function(id, request) {
  box::use(./r/log)
  log$log_fn()
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
    body = shiny$tagList(
      # HTML(): as plain text the child selectors (">") would be escaped to &gt; and dropped.
      shiny$tags$style(shiny$HTML(STAGE_CSS)),
      shiny$tags$script(shiny$HTML(STAGE_JS)),
      ui_paths(ns("paths")),
      ui_evidence(ns("evidence"))
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell, ./r/log)
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    log$log_session(session)
    log$log_info("__HOST__: module server starting")
    x <- server_paths("paths")
    server_evidence("evidence", x)
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
