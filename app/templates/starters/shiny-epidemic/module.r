# __HOST__/module.r
#
# Outbreak: an epidemic, two ways.
#
#   curves    the SEIR model - Susceptible, Exposed, Infectious, Recovered -
#             solved in base R for a year of days, with vaccination before the
#             outbreak and distancing that starts on a chosen day. Redrawn on
#             every slider move.
#   grid      the same disease loose in a town of people on a lattice, each
#             one a cell that catches it from its neighbours. Runs on Start.
#
#   dS/dt = -b S I     dE/dt = b S I - E / L     dI/dt = E / L - I / D     dR/dt = I / D
#
# with b = R0 / D, D the infectious period and L the latent one (L = 0 is SIR:
# the exposed become infectious the same day). Vaccinating a share v moves it
# from S to immune before day 0; distancing scales b down from its day on.

EPI <- list(
  h = 0.25,
  col = c(S = "#5AC8FA", E = "#F0DC78", I = "#FF5A5F", R = "#3DDC97", V = "#8E7CFF"),
  ground = "#080A10"
)

# The SEIR field for state s = c(S, E, I, R), fractions of the town.
seir_field <- function(s, b, latent, D) {
  inf <- b * s[1] * s[3]
  onset <- if (latent > 0) s[2] / latent else inf
  c(-inf, inf - onset, onset - s[3] / D, s[3] / D)
}

# Days 0..days, one row per day, by classical RK4 at a quarter-day step.
# Byte-compiled: box-loaded functions are not JIT-compiled, and the loop is
# the whole cost (cove.ndexr.io CONTEXT.md, measured 2026-10-24).
seir <- local({
  box::use(compiler)
  compiler$cmpfun(function(r0, D, latent, vacc, cut, cut_day, days, seed = 1e-4) {
    h <- EPI$h
    s <- c(1 - vacc - seed, 0, seed, 0)
    out <- matrix(0, days + 1, 4, dimnames = list(NULL, c("S", "E", "I", "R")))
    out[1, ] <- s
    per_day <- round(1 / h)
    for (d in seq_len(days)) {
      b <- r0 / D * (if (d > cut_day) 1 - cut else 1)
      for (k in seq_len(per_day)) {
        k1 <- seir_field(s, b, latent, D)
        k2 <- seir_field(s + h / 2 * k1, b, latent, D)
        k3 <- seir_field(s + h / 2 * k2, b, latent, D)
        k4 <- seir_field(s + h * k3, b, latent, D)
        s <- pmax(s + h / 6 * (k1 + 2 * k2 + 2 * k3 + k4), 0)
      }
      out[d + 1, ] <- s
    }
    out
  })
})

# Percent, with a second decimal once it is under 1% so a small outbreak is not "0.0%".
pct <- function(x, digits = 1) {
  paste0(vapply(x, function(v) formatC(100 * v, format = "f", digits = if (v > 0 && v < 0.01) 2 else digits), ""), "%")
}

tile <- function(value, label, tone = NULL) {
  box::use(shiny = shiny[tags])
  shiny$tags$div(class = paste("tile", tone), shiny$tags$b(value), shiny$tags$span(label))
}

# Lines of right margin that keep a full-window plot clear of the card: the
# card is 360 px wide on a desk, a margin line is 0.2 in, 19.2 px at res 96.
card_room <- function(session, out) {
  w <- session$clientData[[paste0("output_", session$ns(out), "_width")]]
  if (!is.null(w) && w > 900) 400 / 19.2 else 1.4
}

# Axes and labels for the dark stage.
ink <- function(mar = c(3.4, 4, 2.4, 1.2)) {
  graphics::par(bg = NA, mar = mar, col.axis = "#c3cad8", col.lab = "#dfe4ee", fg = "#5b6478",
                cex.axis = 0.95, las = 1)
}

# ── curves ───────────────────────────────────────────────────────────────────

ui_curves <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, sliderInput, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("curves"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Outbreak"),
      uiOutput(ns("tiles")),
      sliderInput(ns("r0"), "R0, people each case infects", 0.5, 12, 3, step = 0.1, width = "100%"),
      sliderInput(ns("D"), "Infectious for, days", 1, 21, 7, step = 0.5, width = "100%"),
      sliderInput(ns("latent"), "Latent period, days (0 is SIR)", 0, 14, 4, step = 0.5, width = "100%"),
      sliderInput(ns("vacc"), "Vaccinated before it arrives, %", 0, 95, 0, step = 1, width = "100%"),
      sliderInput(ns("cut"), "Distancing cuts contacts by, %", 0, 90, 0, step = 5, width = "100%"),
      sliderInput(ns("cut_day"), "Distancing starts on day", 0, 200, 30, step = 1, width = "100%"),
      sliderInput(ns("days"), "Days shown", 60, 730, 240, step = 10, width = "100%")
    )
  )
}

server_curves <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, reactive, req, renderPlot, renderUI, tags],
    graphics[plot.new, plot.window, axis, polygon, lines, abline, mtext, text, legend]
  )
  moduleServer(id, function(input, output, session) {
    run <- reactive({
      req(input$r0, input$D, !is.null(input$latent), !is.null(input$vacc), input$days)
      seir(input$r0, input$D, input$latent, input$vacc / 100, input$cut / 100, input$cut_day, input$days)
    })

    output$curves <- renderPlot({
      m <- run()
      v <- input$vacc / 100
      t <- seq(0, nrow(m) - 1)
      ink(c(3.6, 4.4, 3.2, card_room(session, "curves")))
      plot.new(); plot.window(xlim = range(t), ylim = c(0, 1), xaxs = "i", yaxs = "i")
      grid_y <- seq(0, 1, 0.25)
      abline(h = grid_y, col = "#ffffff14")
      axis(1); axis(2, at = grid_y, labels = pct(grid_y, 0))
      # The infectious share is small beside S and R; its glow is what to watch.
      polygon(c(t, rev(t)), c(m[, "I"], rep(0, length(t))), col = "#FF5A5F33", border = NA)
      polygon(c(t, rev(t)), c(m[, "E"], rep(0, length(t))), col = "#F0DC7822", border = NA)
      if (v > 0) abline(h = v, col = EPI$col[["V"]], lty = 3, lwd = 1.5)
      thr <- 1 / input$r0
      if (thr < 1) {
        abline(h = thr, col = "#ffffff55", lty = 2)
        text(max(t), thr, "S where it turns: 1/R0", adj = c(1.02, -0.5), col = "#dfe4ee", cex = 0.9)
      }
      if (input$cut > 0 && input$cut_day < max(t)) {
        abline(v = input$cut_day, col = "#ffffff40", lty = 3)
        text(input$cut_day, 0.97, " distancing", adj = c(0, 1), col = "#dfe4ee", cex = 0.9)
      }
      for (k in c("S", "E", "I", "R")) {
        lines(t, m[, k], col = paste0(EPI$col[[k]], "30"), lwd = 7)
        lines(t, m[, k], col = EPI$col[[k]], lwd = 2.2)
      }
      mtext_fit("share of the town", side = 3, line = 1.2, adj = 0, col = "#e6e9ef", cex = 1.1, font = 2)
      mtext_fit("day", side = 1, line = 2.4, col = "#c3cad8")
      # One row when it fits; on a phone it ran off the plot after
      # "exposed" (2026-10-27), so there it takes two columns.
      key <- list(legend = c("susceptible", "exposed", "infectious", "recovered", if (v > 0) "vaccinated"),
                  col = c(EPI$col[c("S", "E", "I", "R")], if (v > 0) EPI$col[["V"]]), lwd = 3, lty = c(1, 1, 1, 1, if (v > 0) 3))
      one_row <- legend("topleft", legend = key$legend, horiz = TRUE, cex = 0.95, lwd = 3, bty = "n", plot = FALSE)$rect$w <= 0.98 * diff(graphics::par("usr")[1:2])
      legend("topleft", inset = c(0.01, 0.02), bty = "n", horiz = one_row, ncol = if (one_row) 1 else 2, text.col = "#e6e9ef", cex = 0.95,
             legend = key$legend, col = key$col, lwd = key$lwd, lty = key$lty)
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      m <- run()
      v <- input$vacc / 100
      hit <- max(0, 1 - 1 / input$r0)
      re <- input$r0 * (1 - v)
      peak <- which.max(m[, "I"])
      ever <- 1 - m[nrow(m), "S"] - v
      tags$div(
        class = "tiles",
        tile(pct(hit, 0), "herd immunity at"),
        tile(formatC(re, format = "f", digits = 2), "R at the start",
             if (re < 1) "tone-good" else "tone-bad"),
        tile(pct(m[peak, "I"]), sprintf("ill at the peak, day %d", peak - 1)),
        tile(pct(ever, 0), "ever infected"),
        tile(if (v >= hit) "protected" else pct(hit - v, 0), if (v >= hit) "the town" else "more to vaccinate",
             if (v >= hit) "tone-good" else NULL),
        tile(formatC(round(ever * 1e6), format = "d", big.mark = ","), "cases per million")
      )
    })

    # The disease, for the grid: one disease, two views of it.
    or <- function(x, d) if (is.null(x)) d else x
    reactive(list(r0 = or(input$r0, 3), D = or(input$D, 7), latent = or(input$latent, 4),
                  vacc = or(input$vacc, 0) / 100, cut = or(input$cut, 0) / 100, cut_day = or(input$cut_day, 30)))
  })
}

# ── grid ─────────────────────────────────────────────────────────────────────
#
# Agents: one person per cell, states 0 susceptible, 1 exposed, 2 infectious,
# 3 recovered, 4 vaccinated. Each day an infectious cell infects each of its
# eight neighbours with probability p, set so a case in a wholly susceptible
# crowd infects R0 of them over its D days: 8 (1 - (1 - p)^D) = R0. A few
# infections a day also jump the lattice (travel), in proportion to how many
# are ill, so new fronts open far from the first.

GRID <- list(nx = 150L, ny = 90L, tick = 250, travel = 0.02,
             col = c("#16203a", EPI$col[["E"]], EPI$col[["I"]], "#1f6b54", "#4b3f8f"))

# Infectious neighbours of every cell, the lattice wrapping at its edges.
neighbours <- function(inf) {
  nx <- nrow(inf); ny <- ncol(inf)
  up <- c(nx, seq_len(nx - 1)); dn <- c(2:nx, 1)
  lf <- c(ny, seq_len(ny - 1)); rt <- c(2:ny, 1)
  inf[up, ] + inf[dn, ] + inf[, lf] + inf[, rt] +
    inf[up, lf] + inf[up, rt] + inf[dn, lf] + inf[dn, rt]
}

# One day on the lattice.
grid_day <- function(g, r0, D, latent, cut) {
  n <- length(g)
  p <- 1 - (1 - min(r0 * (1 - cut), 7.9) / 8)^(1 / D)
  k <- neighbours((g == 2) * 1)
  catch <- 1 - (1 - p)^k
  ill <- mean(g == 2)
  catch <- 1 - (1 - catch) * (1 - GRID$travel * ill * r0 * (1 - cut))
  u <- stats::runif(n)
  new_e <- g == 0 & u < catch
  onset <- g == 1 & stats::runif(n) < (if (latent > 0) 1 / latent else 1)
  heal <- g == 2 & stats::runif(n) < 1 / D
  g[heal] <- 3
  g[onset] <- 2
  g[new_e] <- if (latent > 0) 1 else 2
  g
}

grid_start <- function(vacc) {
  n <- GRID$nx * GRID$ny
  g <- matrix(0L, GRID$nx, GRID$ny)
  g[stats::runif(n) < vacc] <- 4L
  # Five cases in the middle of town.
  cx <- GRID$nx %/% 2; cy <- GRID$ny %/% 2
  g[cbind(cx + c(0, 1, -1, 0, 0), cy + c(0, 0, 0, 1, -1))] <- 2L
  g
}

ui_grid <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, actionButton, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("town"), height = "100%"),
    uiOutput(ns("start"), class = "stage-start"),
    tags$div(class = "stage-plot glass", plotOutput(ns("trace"), height = "150px")),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("The town"),
      tags$div(
        class = "d-flex flex-wrap gap-2 mb-2",
        actionButton(ns("run"), "Start", class = "btn btn-primary btn-sm"),
        actionButton(ns("reset"), "New town", class = "btn btn-outline-light btn-sm")
      ),
      uiOutput(ns("tiles"))
    )
  )
}

# `disease` is the curves branch's reactive list of its sliders.
server_grid <- function(id, disease) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, reactiveValues, observeEvent, observe, invalidateLater, isolate, renderPlot,
                  renderUI, updateActionButton, actionButton, tags],
    graphics[par, image, plot.new, plot.window, axis, lines, mtext, abline]
  )
  moduleServer(id, function(input, output, session) {
    st <- reactiveValues(g = NULL, day = 0, ill = 0, ever = 0, hist = NULL, running = FALSE, started = FALSE)
    reset <- function() {
      v <- isolate(disease()$vacc)
      g <- grid_start(v)
      st$g <- g
      st$day <- 0
      st$hist <- matrix(c(0, mean(g == 2), 0), 1)
    }
    reset()
    observeEvent(input$reset, reset())
    # Until the first Start, the town follows the vaccination slider.
    observeEvent(disease()$vacc, if (!isolate(st$started)) reset(), ignoreInit = TRUE)
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

    # The clock: one day per frame, read and written inside isolate() so the
    # observer depends on the timer alone.
    observe({
      if (!st$running) return()
      invalidateLater(GRID$tick)
      isolate({
        d <- disease()
        cut <- if (st$day >= d$cut_day) d$cut else 0
        g <- grid_day(st$g, d$r0, d$D, d$latent, cut)
        st$g <- g
        st$day <- st$day + 1
        st$hist <- rbind(st$hist, c(st$day, mean(g == 2), mean(g == 1 | g == 2 | g == 3)))
        if (!any(g == 1 | g == 2)) toggle()
      })
    })

    output$town <- renderPlot({
      g <- st$g
      par(bg = NA, mar = c(0, 0, 0, 0))
      image(seq_len(nrow(g)), seq_len(ncol(g)), g, zlim = c(0, 4), col = GRID$col, axes = FALSE,
            xlab = "", ylab = "", useRaster = TRUE, asp = 1)
    }, bg = "transparent")

    output$trace <- renderPlot({
      h <- st$hist
      ink(c(2.4, 3.4, 1.8, 0.8))
      top <- max(0.02, h[, 3]) * 1.1
      plot.new(); plot.window(xlim = c(0, max(30, h[, 1])), ylim = c(0, top))
      axis(1); axis(2, at = c(0, top / 2, top), labels = pct(c(0, top / 2, top), 0))
      lines(h[, 1], h[, 3], col = EPI$col[["R"]], lwd = 2)
      lines(h[, 1], h[, 2], col = EPI$col[["I"]], lwd = 2.4)
      mtext_fit("ill now, and ever infected", side = 3, adj = 0, col = "#e6e9ef", cex = 0.9)
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      g <- st$g
      tags$div(
        class = "tiles",
        tile(st$day, "day"),
        tile(pct(mean(g == 2)), "infectious now"),
        tile(pct(mean(g == 1 | g == 2 | g == 3), 0), "ever infected"),
        tile(pct(mean(g == 4), 0), "vaccinated")
      )
    })
  })
}

# ── the site ─────────────────────────────────────────────────────────────────

# Full-window stages with glass cards: the Chaos Lab layout. Cards fold to
# their titles (a <details>: no script) and sit along the bottom on a phone.
STAGE_CSS <- "
.stage { position: relative; height: calc(100vh - 56px); min-height: 520px; overflow: hidden;
  background: radial-gradient(ellipse at 50% 35%, #16203d 0%, #090c16 55%, #04050a 100%); }
.stage > .shiny-plot-output { position: absolute; inset: 0; }
.stage .recalculating { opacity: 1 !important; transition: none !important; }
.glass { background: rgba(10, 13, 22, 0.80); border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: 14px; backdrop-filter: blur(8px); color: #e6e9ef; }
.stage-card { position: absolute; top: 16px; right: 16px; width: 360px; max-height: calc(100% - 32px);
  overflow: auto; padding: 12px 16px; }
.stage-card summary { font-size: 1.2rem; font-weight: 600; cursor: pointer; }
.stage-card[open] summary { margin-bottom: 8px; }
.stage-plot { position: absolute; left: 16px; bottom: 16px; width: min(460px, 40%); padding: 6px; }
.stage-start { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; pointer-events: none; }
.stage-start .btn { pointer-events: auto; padding: 0.7rem 2.4rem; font-size: 1.25rem; }
.tiles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin: 4px 0 12px; }
.tile { background: rgba(255, 255, 255, 0.06); border-radius: 10px; padding: 7px 10px; }
.tile b { display: block; font-size: 1.35rem; font-weight: 600; color: #fff; font-variant-numeric: tabular-nums; }
.tile span { font-size: 0.9rem; color: #d3d9e6; }
.tile.tone-good b { color: #3DDC97; }
.tile.tone-bad b { color: #FF7A7E; }
@media (max-width: 760px) {
  .stage-card { top: auto; bottom: 12px; left: 12px; right: 12px; width: auto; max-height: 46%; }
  .stage:has(.stage-card[open]) > .shiny-plot-output { bottom: 48%; }
  .stage-plot { top: 10px; bottom: auto; left: 10px; right: 10px; width: auto; padding: 2px; }
  .stage-plot .shiny-plot-output { height: 96px !important; }
}
"

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
      ui_curves(ns("curves")),
      ui_grid(ns("grid"))
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell, ./r/log)
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    log$log_session(session)
    log$log_info("__HOST__: module server starting")
    disease <- server_curves("curves")
    server_grid("grid", disease)
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
