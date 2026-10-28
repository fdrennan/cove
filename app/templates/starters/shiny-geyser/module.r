# __HOST__/module.r
#
# Geyser: Old Faithful, 272 eruptions (datasets::faithful, shipped with R),
# taken apart with the statistics R carries in the box.
#
#   density   a histogram of eruption length or waiting time with a kernel
#             density over it; drag the bandwidth, or snap it to Silverman's,
#             Scott's or Sheather-Jones' rule, and count the modes it finds.
#   mixture   two Gaussian clusters fitted to (eruption, wait) by EM, written
#             here in base R: step it one iteration at a time or play it, and
#             watch the log-likelihood climb and the ellipses settle.
#   predict   the ranger's forecast: the wait after an eruption, regressed on
#             how long that eruption lasted, with its prediction interval.
#   env       what this page runs on, read from the live R session.
#
# faithful$waiting is the wait to the next eruption (?faithful), so
# lm(waiting ~ eruptions) predicts the next one from the last one seen.

GEYSER <- list(
  col = c(short = "#5AC8FA", long = "#FF9650", fit = "#3DDC97", ink = "#e6e9ef"),
  # Bandwidth slider bounds per variable: eruptions are minutes ~1.6-5.1, waits ~43-96.
  bw = list(eruptions = c(0.02, 0.8, 0.01), waiting = c(0.3, 12, 0.1)),
  label = c(eruptions = "eruption length, minutes", waiting = "wait to the next eruption, minutes"),
  loaded = Sys.time()
)

faithful_xy <- function() as.matrix(datasets::faithful[, c("eruptions", "waiting")])

# The three textbook bandwidths, as R's stats package computes them.
bw_rules <- function(x) {
  c(Silverman = stats::bw.nrd0(x), Scott = stats::bw.nrd(x), `Sheather-Jones` = stats::bw.SJ(x))
}

# Local maxima of a density curve: the modes a bandwidth lets through. Peaks
# under 0.1% of the tallest are dropped: density()'s FFT leaves ripples of
# ~1e-17 in the empty tails, and a compact kernel counted ten "modes" there.
find_modes <- function(d) {
  i <- which(diff(sign(diff(d$y))) == -2) + 1
  d$x[i[d$y[i] > 1e-3 * max(d$y)]]
}

# ── EM for a two-component bivariate Gaussian mixture ───────────────────────
# A state is the parameters (pi, mu: 2x2 one row per component, S: two
# covariances) plus the log-likelihood after every iteration so far.

# Log density of the bivariate normal at each row of x.
ldmvn <- function(x, mu, S) {
  d <- sweep(x, 2, mu)
  -0.5 * rowSums((d %*% solve(S)) * d) - log(2 * pi) - 0.5 * log(det(S))
}

# Per-point log of pi_k * N(x | k), one column per component.
em_joint <- function(x, st) {
  cbind(log(st$pi[1]) + ldmvn(x, st$mu[1, ], st$S[[1]]),
        log(st$pi[2]) + ldmvn(x, st$mu[2, ], st$S[[2]]))
}

# Log-likelihood and responsibilities, by log-sum-exp so no point underflows.
em_estep <- function(x, st) {
  L <- em_joint(x, st)
  m <- pmax(L[, 1], L[, 2])
  lse <- m + log(exp(L[, 1] - m) + exp(L[, 2] - m))
  list(ll = sum(lse), r = exp(L - lse))
}

# Where EM starts. "spread" puts both means near the centre, tilted against
# the data's own correlation, so the climb is long enough to watch; "random"
# takes two observations as the means.
em_init <- function(x, start = c("spread", "random")) {
  start <- match.arg(start)
  mu <- if (start == "random") x[sample.int(nrow(x), 2), , drop = FALSE] else {
    c0 <- colMeans(x); rbind(c0 + c(-0.25, 3), c0 + c(0.25, -3))
  }
  st <- list(pi = c(0.5, 0.5), mu = unname(mu), S = list(stats::cov(x), stats::cov(x)), iter = 0L)
  st$ll <- em_estep(x, st)$ll
  st
}

# One EM iteration: responsibilities from the current parameters (E), then
# the weighted means and covariances that maximise the expected log-likelihood
# (M). EM never lowers the likelihood - the trace in the card is the proof.
em_step <- function(x, st) {
  r <- em_estep(x, st)$r
  nk <- colSums(r)
  mu <- t(r) %*% x / nk
  # The ridge keeps a component that has collapsed onto a few points invertible.
  S <- lapply(1:2, function(k) {
    d <- sweep(x, 2, mu[k, ])
    crossprod(d * sqrt(r[, k])) / nk[k] + diag(1e-6, 2)
  })
  nxt <- list(pi = nk / nrow(x), mu = unname(mu), S = S, iter = st$iter + 1L)
  nxt$ll <- c(st$ll, em_estep(x, nxt)$ll)
  nxt
}

em_converged <- function(st, tol = 1e-8) {
  n <- length(st$ll)
  n > 1 && abs(st$ll[n] - st$ll[n - 1]) < tol * abs(st$ll[n])
}

# Run to convergence, for the tests and the "Fit" button.
em_fit <- function(x, st = em_init(x), max_iter = 1000) {
  while (!em_converged(st) && st$iter < max_iter) st <- em_step(x, st)
  st
}

# Component order for display: the short eruptions first.
em_order <- function(st) order(st$mu[, 1])

# The contour of a bivariate normal that holds probability p.
ellipse <- function(mu, S, p, n = 120) {
  e <- eigen(S, symmetric = TRUE)
  t <- seq(0, 2 * pi, length.out = n)
  u <- cbind(cos(t), sin(t)) %*% (t(e$vectors) * sqrt(pmax(e$values, 0))) * sqrt(stats::qchisq(p, 2))
  sweep(u, 2, mu, "+")
}

# ── the forecast ─────────────────────────────────────────────────────────────

wait_model <- function() stats::lm(waiting ~ eruptions, data = datasets::faithful)

# Predicted wait after an eruption of `minutes`, with its prediction interval
# (one geyser's next wait) and confidence interval (the mean wait at that length).
forecast_wait <- function(fit, minutes, level = 0.95) {
  nd <- data.frame(eruptions = minutes)
  p <- stats::predict(fit, nd, interval = "prediction", level = level)
  ci <- stats::predict(fit, nd, interval = "confidence", level = level)
  data.frame(eruptions = minutes, fit = p[, "fit"], lwr = p[, "lwr"], upr = p[, "upr"],
             clwr = ci[, "lwr"], cupr = ci[, "upr"])
}

# ── the environment, read live ───────────────────────────────────────────────

# When this R process started: /proc/self/stat field 22 is clock ticks after
# boot (USER_HZ, 100 on Linux), /proc/stat btime is boot in epoch seconds.
# Off Linux, fall back to when this module was loaded.
proc_started <- function() {
  tryCatch({
    s <- readLines("/proc/self/stat", warn = FALSE)
    f <- strsplit(sub("^.*\\) ", "", s), " ")[[1]]
    btime <- as.numeric(sub("^btime ", "", grep("^btime ", readLines("/proc/stat", warn = FALSE), value = TRUE)))
    as.POSIXct(btime + as.numeric(f[20]) / 100, origin = "1970-01-01")
  }, error = function(e) GEYSER$loaded)
}

env_info <- function() {
  mods <- strsplit(Sys.getenv("LOADEDMODULES"), ":", fixed = TRUE)[[1]]
  pkgs <- c("shiny", "box", "stats", "graphics", "grDevices", "datasets")
  list(
    runtime = R.version.string,
    module = if (length(mods)) paste(grep("^(env|R)/", mods, value = TRUE), collapse = ", ") else "",
    packages = vapply(pkgs, function(p) as.character(utils::packageVersion(p)), ""),
    started = format(proc_started(), "%Y-%m-%d %H:%M:%S %Z"),
    cpus = parallel::detectCores(),
    platform = R.version$platform
  )
}

# ── drawing helpers ──────────────────────────────────────────────────────────

tile <- function(value, label, tone = NULL) {
  box::use(shiny = shiny[tags])
  shiny$tags$div(class = paste("tile", tone), shiny$tags$b(value), shiny$tags$span(label))
}

f2 <- function(x, d = 2) formatC(x, format = "f", digits = d)

# Lines of right margin that keep a full-window plot clear of the card (360 px
# on a desk; a margin line is 19.2 px at res 96).
card_room <- function(session, out) {
  w <- session$clientData[[paste0("output_", session$ns(out), "_width")]]
  if (!is.null(w) && w > 900) 400 / 19.2 else 1.4
}

ink <- function(mar = c(3.6, 4.4, 3, 1.2)) {
  graphics::par(bg = NA, mar = mar, col.axis = "#c3cad8", col.lab = "#dfe4ee", fg = "#5b6478",
                cex.axis = 0.95, las = 1)
}

# ── density ──────────────────────────────────────────────────────────────────

ui_density <- function(id) {
  box::use(shiny = shiny[NS, tags, sliderInput, selectInput, radioButtons, actionButton, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  b <- GEYSER$bw$eruptions
  tags$section(
    class = "stage",
    plotOutput(ns("hist"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Density"),
      radioButtons(ns("var"), NULL, c("Eruption length" = "eruptions", "Waiting time" = "waiting"), inline = TRUE),
      sliderInput(ns("bw"), "Bandwidth, minutes", b[1], b[2], 0.14, step = b[3], width = "100%"),
      tags$div(
        class = "rules",
        actionButton(ns("silverman"), "Silverman", class = "btn btn-outline-light btn-sm"),
        actionButton(ns("scott"), "Scott", class = "btn btn-outline-light btn-sm"),
        actionButton(ns("sj"), "Sheather-Jones", class = "btn btn-outline-light btn-sm")
      ),
      selectInput(ns("kernel"), "Kernel", c("gaussian", "epanechnikov", "rectangular", "triangular", "biweight", "cosine"),
                  width = "100%"),
      sliderInput(ns("bins"), "Histogram bins", 8, 60, 30, step = 1, width = "100%"),
      uiOutput(ns("tiles"))
    )
  )
}

server_density <- function(id) {
  box::use(
    shiny = shiny[moduleServer, reactive, req, observeEvent, updateSliderInput, updateActionButton, renderPlot, renderUI, tags],
    graphics[hist, plot.new, plot.window, axis, rect, lines, polygon, rug, mtext, legend, abline]
  )
  moduleServer(id, function(input, output, session) {
    x <- reactive(datasets::faithful[[req(input$var)]])
    rules <- reactive(bw_rules(x()))
    snap <- function(v) updateSliderInput(session, "bw", value = round(v, 2))

    # A new variable rescales the slider and starts it at Sheather-Jones, the
    # rule that reads the data's own curvature.
    observeEvent(input$var, {
      b <- GEYSER$bw[[input$var]]
      r <- rules()
      updateSliderInput(session, "bw", min = b[1], max = b[2], step = b[3], value = round(r[["Sheather-Jones"]], 2))
      updateActionButton(session, "silverman", label = paste("Silverman", f2(r[["Silverman"]])))
      updateActionButton(session, "scott", label = paste("Scott", f2(r[["Scott"]])))
      updateActionButton(session, "sj", label = paste("Sheather-Jones", f2(r[["Sheather-Jones"]])))
    })
    observeEvent(input$silverman, snap(rules()[["Silverman"]]))
    observeEvent(input$scott, snap(rules()[["Scott"]]))
    observeEvent(input$sj, snap(rules()[["Sheather-Jones"]]))

    kde <- reactive({
      req(input$bw, input$kernel)
      stats::density(x(), bw = input$bw, kernel = input$kernel, n = 1024)
    })

    output$hist <- renderPlot({
      v <- x(); d <- kde(); r <- rules()
      h <- hist(v, breaks = seq(min(v), max(v), length.out = req(input$bins) + 1), plot = FALSE)
      ink(c(3.8, 4.6, 3.4, card_room(session, "hist")))
      plot.new()
      plot.window(xlim = range(d$x), ylim = c(0, max(h$density, d$y) * 1.12), xaxs = "i", yaxs = "i")
      axis(1); axis(2)
      rect(h$breaks[-length(h$breaks)], 0, h$breaks[-1], h$density, col = "#ffffff14", border = "#ffffff38")
      # The three rules, faint, so the chosen bandwidth is read against them.
      tones <- c("#ffffff40", "#ffffff60", "#ffffff88")
      for (i in seq_along(r)) lines(stats::density(v, bw = r[[i]], kernel = input$kernel), col = tones[i], lty = i + 1)
      polygon(c(d$x, rev(d$x)), c(d$y, rep(0, length(d$y))), col = "#3DDC9722", border = NA)
      lines(d, col = "#3DDC9740", lwd = 8)
      lines(d, col = GEYSER$col[["fit"]], lwd = 2.6)
      rug(v, col = "#5AC8FA88", ticksize = 0.025)
      legend("topleft", bty = "n", text.col = "#e6e9ef", col = c(GEYSER$col[["fit"]], tones), lty = c(1, 2:4),
             lwd = c(2.6, 1, 1, 1), legend = c(sprintf("bandwidth %s", f2(input$bw)), names(r)), inset = c(0.01, 0.02))
      mtext_fit(sprintf("kernel density of %s", GEYSER$label[[input$var]]), side = 3, line = 1.4, adj = 0,
            col = GEYSER$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("minutes", side = 1, line = 2.5, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      d <- kde(); r <- rules()
      peaks <- find_modes(d)
      k <- length(peaks)
      tags$div(
        class = "tiles",
        tile(k, if (k == 1) "mode" else "modes", if (k == 2) "tone-good" else NULL),
        tile(if (length(peaks)) paste(f2(peaks, if (input$var == "waiting") 0 else 1), collapse = " / ") else "-", "peaks at"),
        tile(f2(r[["Silverman"]]), "Silverman, bw.nrd0"),
        tile(f2(r[["Sheather-Jones"]]), "Sheather-Jones, bw.SJ")
      )
    })
  })
}

# ── mixture ──────────────────────────────────────────────────────────────────

ui_mixture <- function(id) {
  box::use(shiny = shiny[NS, tags, selectInput, actionButton, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("scatter"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Mixture, by EM"),
      selectInput(ns("start"), "Start from", c("Both near the centre" = "spread", "Two random eruptions" = "random"),
                  width = "100%"),
      tags$div(
        class = "rules",
        actionButton(ns("step"), "Step", class = "btn btn-outline-light btn-sm"),
        actionButton(ns("play"), "Play", class = "btn btn-outline-light btn-sm"),
        actionButton(ns("fit"), "Fit", class = "btn btn-outline-light btn-sm"),
        actionButton(ns("reset"), "Restart", class = "btn btn-outline-light btn-sm")
      ),
      plotOutput(ns("trace"), height = "150px"),
      uiOutput(ns("tiles"))
    )
  )
}

server_mixture <- function(id) {
  box::use(
    shiny = shiny[moduleServer, reactiveVal, observe, observeEvent, invalidateLater, isolate, renderPlot, renderUI,
                  updateActionButton, tags],
    graphics[plot.new, plot.window, axis, points, lines, polygon, mtext, legend, abline],
    grDevices[rgb, colorRamp]
  )
  moduleServer(id, function(input, output, session) {
    x <- faithful_xy()
    st <- reactiveVal(em_init(x))
    playing <- reactiveVal(FALSE)
    stop_play <- function() { playing(FALSE); updateActionButton(session, "play", label = "Play") }

    observeEvent(list(input$reset, input$start), { stop_play(); st(em_init(x, input$start)) })
    observeEvent(input$step, { stop_play(); st(em_step(x, st())) })
    observeEvent(input$fit, { stop_play(); st(em_fit(x, st())) })
    observeEvent(input$play, {
      on <- !playing(); playing(on)
      updateActionButton(session, "play", label = if (on) "Pause" else "Play")
    })
    # Playing: one iteration per tick until the likelihood stops moving.
    observe({
      if (!playing()) return()
      invalidateLater(220)
      s <- isolate(st())
      if (em_converged(s)) stop_play() else st(em_step(x, s))
    })

    output$scatter <- renderPlot({
      s <- st()
      o <- em_order(s)
      r <- em_estep(x, s)$r[, o]
      # Each point is coloured by its responsibility: blue short, orange long.
      ramp <- colorRamp(c(GEYSER$col[["short"]], GEYSER$col[["long"]]))
      pc <- rgb(ramp(r[, 2]), maxColorValue = 255, alpha = 210)
      ink(c(3.8, 4.6, 3.4, card_room(session, "scatter")))
      plot.new()
      plot.window(xlim = c(1.3, 5.5), ylim = c(38, 100), xaxs = "i", yaxs = "i")
      axis(1); axis(2)
      abline(h = seq(40, 100, 10), v = 2:5, col = "#ffffff10")
      points(x, pch = 21, bg = pc, col = "#ffffff55", cex = 1.25)
      cols <- GEYSER$col[c("short", "long")]
      for (j in 1:2) {
        k <- o[j]
        for (p in c(0.95, 0.5)) {
          e <- ellipse(s$mu[k, ], s$S[[k]], p)
          if (p == 0.5) polygon(e, col = paste0(cols[[j]], "22"), border = NA)
          lines(e, col = cols[[j]], lwd = if (p == 0.5) 2.4 else 1.4, lty = if (p == 0.5) 1 else 2)
        }
        points(s$mu[k, 1], s$mu[k, 2], pch = 3, cex = 2.2, lwd = 3, col = "#ffffff")
      }
      legend("bottomright", bty = "n", text.col = "#e6e9ef", col = "#e6e9ef", lty = c(1, 2), lwd = c(2.4, 1.4),
             legend = c("50% of a cluster", "95% of a cluster"), inset = c(0.01, 0.02))
      mtext_fit(sprintf("two Gaussian clusters, EM iteration %d%s", s$iter, if (em_converged(s)) ", converged" else ""),
            side = 3, line = 1.4, adj = 0, col = GEYSER$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("eruption length, minutes", side = 1, line = 2.5, col = "#c3cad8")
      mtext("wait to the next, minutes", side = 2, line = 3.2, col = "#c3cad8", las = 0)
    }, bg = "transparent", res = 96)

    output$trace <- renderPlot({
      ll <- st()$ll
      it <- seq_along(ll) - 1
      graphics::par(bg = NA, mar = c(2.2, 3.6, 1.2, 0.6), col.axis = "#c3cad8", fg = "#5b6478", cex.axis = 0.8, las = 1)
      plot.new()
      plot.window(xlim = c(0, max(5, max(it))), ylim = range(ll) + c(-1, 1) * max(1, diff(range(ll)) * 0.06))
      axis(1); axis(2)
      lines(it, ll, col = GEYSER$col[["fit"]], lwd = 2)
      points(utils::tail(it, 1), utils::tail(ll, 1), pch = 19, col = "#ffffff")
      mtext_fit("log-likelihood by iteration", side = 3, line = 0.1, adj = 0, col = "#dfe4ee", cex = 0.85)
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      s <- st()
      o <- em_order(s)
      tags$div(
        class = "tiles",
        tile(f2(s$ll[length(s$ll)], 1), "log-likelihood", if (em_converged(s)) "tone-good" else NULL),
        tile(s$iter, "iterations"),
        tile(sprintf("%s / %s", f2(s$mu[o[1], 1]), f2(s$mu[o[2], 1])), "mean eruption, min"),
        tile(sprintf("%s / %s", f2(s$mu[o[1], 2], 0), f2(s$mu[o[2], 2], 0)), "mean wait, min"),
        tile(sprintf("%s%%", f2(100 * s$pi[o[1]], 0)), "short eruptions"),
        tile(sprintf("%s%%", f2(100 * s$pi[o[2]], 0)), "long eruptions")
      )
    })
  })
}

# ── predict ──────────────────────────────────────────────────────────────────

ui_predict <- function(id) {
  box::use(shiny = shiny[NS, tags, sliderInput, selectInput, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("fit"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Predict the next eruption"),
      sliderInput(ns("minutes"), "The eruption you saw lasted, minutes", 1.5, 5.2, 4.0, step = 0.05, width = "100%"),
      selectInput(ns("level"), "Interval", c("80%" = "0.8", "90%" = "0.9", "95%" = "0.95", "99%" = "0.99"),
                  selected = "0.95", width = "100%"),
      uiOutput(ns("tiles"))
    )
  )
}

server_predict <- function(id) {
  box::use(
    shiny = shiny[moduleServer, reactive, req, renderPlot, renderUI, tags],
    graphics[plot.new, plot.window, axis, points, lines, polygon, segments, mtext, legend, abline, text]
  )
  moduleServer(id, function(input, output, session) {
    fit <- wait_model()
    level <- reactive(as.numeric(req(input$level)))
    now <- reactive(forecast_wait(fit, req(input$minutes), level()))

    output$fit <- renderPlot({
      grid <- forecast_wait(fit, seq(1.3, 5.5, length.out = 120), level())
      p <- now()
      ink(c(3.8, 4.6, 3.4, card_room(session, "fit")))
      plot.new()
      plot.window(xlim = c(1.3, 5.5), ylim = c(35, 105), xaxs = "i", yaxs = "i")
      axis(1); axis(2)
      abline(h = seq(40, 100, 10), v = 2:5, col = "#ffffff10")
      polygon(c(grid$eruptions, rev(grid$eruptions)), c(grid$lwr, rev(grid$upr)), col = "#5AC8FA1c", border = NA)
      polygon(c(grid$eruptions, rev(grid$eruptions)), c(grid$clwr, rev(grid$cupr)), col = "#3DDC9750", border = NA)
      points(datasets::faithful, pch = 21, bg = "#ffffff30", col = "#ffffff70", cex = 1.1)
      lines(grid$eruptions, grid$fit, col = GEYSER$col[["fit"]], lwd = 2.6)
      abline(v = p$eruptions, col = "#ffffff30")
      segments(p$eruptions, p$lwr, p$eruptions, p$upr, col = GEYSER$col[["long"]], lwd = 4)
      points(p$eruptions, p$fit, pch = 21, cex = 2.4, bg = GEYSER$col[["long"]], col = "#ffffff", lwd = 1.5)
      text(p$eruptions, p$upr, sprintf("%.0f min", p$fit), pos = 3, col = "#ffffff", font = 2)
      legend("topleft", bty = "n", text.col = "#e6e9ef", fill = c("#5AC8FA1c", "#3DDC9750"), border = NA,
             legend = c(sprintf("%s%% prediction interval: one wait", 100 * level()),
                        sprintf("%s%% confidence interval: the mean wait", 100 * level())), inset = c(0.01, 0.02))
      co <- stats::coef(fit)
      mtext_fit(sprintf("wait = %.1f + %.2f x eruption length", co[1], co[2]), side = 3, line = 1.4, adj = 0,
            col = GEYSER$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("eruption length, minutes", side = 1, line = 2.5, col = "#c3cad8")
      mtext("wait to the next, minutes", side = 2, line = 3.2, col = "#c3cad8", las = 0)
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      p <- now()
      sm <- summary(fit)
      tags$div(
        class = "tiles",
        tile(sprintf("%.0f min", p$fit), "next eruption in", "tone-good"),
        tile(sprintf("%.0f-%.0f", p$lwr, p$upr), sprintf("%s%% of waits", 100 * level())),
        tile(f2(stats::coef(fit)[[2]]), "min of wait per min erupted"),
        tile(f2(sm$r.squared), "R squared"),
        tile(f2(sm$sigma, 1), "residual sd, min"),
        tile(nrow(datasets::faithful), "eruptions fitted")
      )
    })
  })
}

# ── environment ──────────────────────────────────────────────────────────────

ui_env <- function(id) {
  box::use(shiny = shiny[NS, tags, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "env-strip",
    tags$details(class = "glass env-card", tags$summary("Environment"), uiOutput(ns("env")))
  )
}

server_env <- function(id) {
  box::use(shiny = shiny[moduleServer, renderUI, tags])
  moduleServer(id, function(input, output, session) {
    output$env <- renderUI({
      e <- env_info()
      row <- function(k, v) tags$tr(tags$th(k), tags$td(v))
      tags$table(
        class = "env-table",
        row("Runtime", e$runtime),
        row("Stack module", if (nzchar(e$module)) e$module else "none loaded"),
        row("Platform", e$platform),
        tags$tbody(lapply(names(e$packages), function(p) row(p, e$packages[[p]]))),
        row("Process started", e$started),
        row("CPUs", e$cpus)
      )
    })
  })
}

# ── the site ─────────────────────────────────────────────────────────────────

# Full-window stages with glass cards: the Chaos Lab layout.
STAGE_CSS <- "
.stage { position: relative; height: calc(100vh - 56px); min-height: 520px; overflow: hidden;
  background: radial-gradient(ellipse at 50% 35%, #16203d 0%, #090c16 55%, #04050a 100%); }
.stage > .shiny-plot-output { position: absolute; inset: 0; }
.glass { background: rgba(10, 13, 22, 0.80); border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: 14px; backdrop-filter: blur(8px); color: #e6e9ef; }
.stage-card { position: absolute; top: 16px; right: 16px; width: 360px; max-height: calc(100% - 32px);
  overflow: auto; padding: 12px 16px; }
.stage-card summary, .env-card summary { font-size: 1.2rem; font-weight: 600; cursor: pointer; }
.stage-card[open] summary { margin-bottom: 8px; }
.stage-card .radio-inline { color: #e6e9ef; margin-right: 10px; }
.rules { display: flex; flex-wrap: wrap; gap: 6px; margin: 4px 0 12px; }
.tiles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin-top: 10px; }
.tile { background: rgba(255, 255, 255, 0.06); border-radius: 10px; padding: 7px 10px; }
.tile b { display: block; font-size: 1.35rem; font-weight: 600; color: #fff; font-variant-numeric: tabular-nums; }
.tile span { font-size: 0.9rem; color: #d3d9e6; }
.tile.tone-good b { color: #3DDC97; }
.env-strip { padding: 16px; background: #04050a; }
.env-card { padding: 12px 16px; max-width: 640px; }
.env-card[open] summary { margin-bottom: 8px; }
.env-table { width: 100%; font-variant-numeric: tabular-nums; }
.env-table th { font-weight: 600; padding: 3px 16px 3px 0; color: #d3d9e6; white-space: nowrap; vertical-align: top; }
.env-table td { padding: 3px 0; color: #fff; word-break: break-word; }
@media (max-width: 760px) {
  .stage-card { top: auto; bottom: 12px; left: 12px; right: 12px; width: auto; max-height: 46%; }
  .stage:has(.stage-card[open]) > .shiny-plot-output { bottom: 48%; }
  .env-strip { padding: 10px; }
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
      ui_density(ns("density")),
      ui_mixture(ns("mixture")),
      ui_predict(ns("predict")),
      ui_env(ns("env"))
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell)
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    server_density("density")
    server_mixture("mixture")
    server_predict("predict")
    server_env("env")
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
