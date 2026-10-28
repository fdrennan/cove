# __HOST__/module.r
#
# Trial: thousands of two-arm randomised trials, simulated in a blink.
#
#   power     how often a trial of n patients per arm finds an effect of a
#             given size: the t-test's power curve, in theory and as counted
#             from the simulated trials, with the n that buys 80% and 90%.
#   results   what those trials report: their p-values beside the p-values of
#             the same trials with no effect at all, and their effect
#             estimates - the significant ones among them overstate the truth
#             when power is low (the winner's curse).
#
# Outcomes are standardised (control mean 0, sd 1, treatment mean d), so d is
# Cohen's effect size: 0.2 small, 0.5 medium, 0.8 large.
#
# A trial is simulated from its sufficient statistics, not its patients: an
# arm's mean is normal with sd 1/sqrt(n), and its variance is chi-squared on
# n - 1 degrees of freedom over n - 1. The t-test sees exactly what it would
# from n draws, and 5000 trials of 500 patients cost four numbers each.

TRIAL <- list(col = c(treat = "#5AC8FA", null = "#FF9650", sig = "#3DDC97", ink = "#e6e9ef"))

# `k` trials of `n` patients per arm, true effect `d`: per-trial estimate and
# two-sided p-value of the pooled-variance t-test.
simulate_trials <- function(k, n, d) {
  m1 <- stats::rnorm(k, d, 1 / sqrt(n)); m0 <- stats::rnorm(k, 0, 1 / sqrt(n))
  v1 <- stats::rchisq(k, n - 1) / (n - 1); v0 <- stats::rchisq(k, n - 1) / (n - 1)
  t <- (m1 - m0) / sqrt((v1 + v0) / 2 * 2 / n)
  list(est = m1 - m0, p = 2 * stats::pt(-abs(t), 2 * n - 2))
}

# Power of the two-sided t-test, from the noncentral t.
power_t <- function(n, d, alpha) {
  df <- 2 * n - 2
  q <- stats::qt(1 - alpha / 2, df)
  ncp <- d * sqrt(n / 2)
  1 - stats::pt(q, df, ncp) + stats::pt(-q, df, ncp)
}

# Patients per arm for a target power: Inf with no effect to find, NA past 100,000.
n_for <- function(target, d, alpha) {
  if (d <= 0) return(Inf)
  if (power_t(1e5, d, alpha) < target) return(NA)
  ceiling(stats::uniroot(function(n) power_t(n, d, alpha) - target, c(2, 1e5))$root)
}

tile <- function(value, label, tone = NULL) {
  box::use(shiny = shiny[tags])
  shiny$tags$div(class = paste("tile", tone), shiny$tags$b(value), shiny$tags$span(label))
}

pct <- function(x, digits = 0) paste0(formatC(100 * x, format = "f", digits = digits), "%")
num <- function(x) if (is.na(x)) "> 100k" else if (is.infinite(x)) "never" else formatC(x, format = "d", big.mark = ",")

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

# ── power ────────────────────────────────────────────────────────────────────

ui_power <- function(id) {
  box::use(shiny = shiny[NS, tags, sliderInput, selectInput, actionButton, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("curve"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Trial"),
      sliderInput(ns("d"), "True effect, Cohen's d", 0, 1.5, 0.4, step = 0.05, width = "100%"),
      sliderInput(ns("n"), "Patients per arm", 4, 600, 60, step = 2, width = "100%"),
      selectInput(ns("alpha"), "Significance level, alpha", c("0.001", "0.01", "0.025", "0.05", "0.10"),
                  selected = "0.05", width = "100%"),
      sliderInput(ns("k"), "Trials simulated", 200, 10000, 4000, step = 200, width = "100%"),
      actionButton(ns("again"), "Run them again", class = "btn btn-outline-light btn-sm w-100"),
      uiOutput(ns("tiles"))
    )
  )
}

# Returns the simulated trials, for the results branch.
server_power <- function(id) {
  box::use(
    shiny = shiny[moduleServer, reactive, req, renderPlot, renderUI, tags],
    graphics[plot.new, plot.window, axis, lines, points, segments, abline, mtext, text, polygon]
  )
  moduleServer(id, function(input, output, session) {
    alpha <- reactive(as.numeric(req(input$alpha)))
    sims <- reactive({
      req(input$n, input$k, !is.null(input$d))
      input$again
      list(d = input$d, n = input$n, alpha = alpha(),
           treat = simulate_trials(input$k, input$n, input$d),
           null = simulate_trials(input$k, input$n, 0))
    })

    output$curve <- renderPlot({
      s <- sims()
      a <- s$alpha
      ns <- exp(seq(log(4), log(2000), length.out = 220))
      ink(c(3.8, 4.6, 3.4, card_room(session, "curve")))
      plot.new(); plot.window(xlim = range(ns), ylim = c(0, 1), log = "x", xaxs = "i", yaxs = "i")
      abline(h = c(0.25, 0.5, 0.75), col = "#ffffff12")
      abline(h = c(0.8, 0.9), col = "#ffffff40", lty = 2)
      text(4.3, c(0.8, 0.9), c("80%", "90%"), adj = c(0, -0.4), col = "#dfe4ee", cex = 0.9)
      axis(1, at = c(5, 10, 20, 50, 100, 200, 500, 1000, 2000))
      axis(2, at = seq(0, 1, 0.2), labels = pct(seq(0, 1, 0.2)))
      # Cohen's landmarks, faint, behind the effect being simulated.
      for (dd in c(0.2, 0.5, 0.8)) {
        pw <- power_t(ns, dd, a)
        lines(ns, pw, col = "#ffffff2e", lwd = 1.5)
        i <- which.min(abs(pw - 0.55))
        text(ns[i], pw[i], paste0("d = ", dd), col = "#c3cad8", cex = 0.85, pos = 4)
      }
      pw <- power_t(ns, s$d, a)
      polygon(c(ns, rev(ns)), c(pw, rep(0, length(ns))), col = "#5AC8FA14", border = NA)
      lines(ns, pw, col = "#5AC8FA30", lwd = 8)
      lines(ns, pw, col = TRIAL$col[["treat"]], lwd = 2.6)
      abline(h = a, col = TRIAL$col[["null"]], lty = 3, lwd = 1.5)
      text(2000, a, "alpha: power at no effect", adj = c(1.02, -0.5), col = TRIAL$col[["null"]], cex = 0.9)
      # The simulated power at this n, with its 95% interval.
      hit <- mean(s$treat$p < a)
      se <- sqrt(hit * (1 - hit) / length(s$treat$p))
      segments(s$n, max(0, hit - 1.96 * se), s$n, min(1, hit + 1.96 * se), col = "#ffffff", lwd = 2)
      abline(v = s$n, col = "#ffffff30")
      points(s$n, hit, pch = 21, cex = 2.4, bg = TRIAL$col[["sig"]], col = "#ffffff", lwd = 1.5)
      mtext_fit("power: the share of trials that find the effect", side = 3, line = 1.4, adj = 0,
            col = TRIAL$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("patients per arm", side = 1, line = 2.5, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      s <- sims()
      a <- s$alpha
      hit <- mean(s$treat$p < a)
      tags$div(
        class = "tiles",
        tile(pct(hit, 1), "power, simulated", if (hit >= 0.8) "tone-good" else if (hit < 0.5) "tone-bad" else NULL),
        tile(pct(power_t(s$n, s$d, a), 1), "power, in theory"),
        tile(pct(mean(s$null$p < a), 1), "false positives at d = 0"),
        tile(pct(1 - hit, 1), "beta, missed effects"),
        tile(num(n_for(0.8, s$d, a)), "per arm for 80%"),
        tile(num(n_for(0.9, s$d, a)), "per arm for 90%")
      )
    })

    sims
  })
}

# ── results ──────────────────────────────────────────────────────────────────

ui_results <- function(id) {
  box::use(shiny = shiny[NS, tags, plotOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage-duo",
    tags$div(class = "glass", plotOutput(ns("pvalues"), height = "100%")),
    tags$div(class = "glass", plotOutput(ns("estimates"), height = "100%"))
  )
}

server_results <- function(id, sims) {
  box::use(
    shiny = shiny[moduleServer, renderPlot],
    graphics[hist, plot.new, plot.window, axis, rect, abline, mtext, legend]
  )
  moduleServer(id, function(input, output, session) {
    # Bars of a histogram as rectangles, so two can share axes cleanly.
    bars <- function(h, col, border) rect(h$breaks[-length(h$breaks)], 0, h$breaks[-1], h$counts, col = col, border = border)

    output$pvalues <- renderPlot({
      s <- sims()
      br <- seq(0, 1, 0.025)
      ht <- hist(s$treat$p, breaks = br, plot = FALSE)
      hn <- hist(s$null$p, breaks = br, plot = FALSE)
      ink(c(3.6, 4.6, 4.2, 1.2))
      plot.new(); plot.window(xlim = c(0, 1), ylim = c(0, max(ht$counts, hn$counts) * 1.08), xaxs = "i", yaxs = "i")
      axis(1); axis(2)
      bars(hn, "#FF965055", "#FF9650")
      bars(ht, "#5AC8FA66", "#5AC8FA")
      abline(v = s$alpha, col = "#ffffff", lty = 2)
      mtext_fit("p-values of every trial", side = 3, line = 2.2, adj = 0, col = TRIAL$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit(sprintf("with the effect: %s below alpha    no effect: %s, flat",
                    pct(mean(s$treat$p < s$alpha), 1), pct(mean(s$null$p < s$alpha), 1)),
            side = 3, line = 0.7, adj = 0, col = "#dfe4ee", cex = 0.95)
      legend("topright", bty = "n", text.col = "#e6e9ef", fill = c("#5AC8FA66", "#FF965055"),
             border = c("#5AC8FA", "#FF9650"), legend = c(sprintf("d = %.2f", s$d), "d = 0"))
      mtext_fit("p", side = 1, line = 2.4, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    output$estimates <- renderPlot({
      s <- sims()
      e <- s$treat$est
      sig <- s$treat$p < s$alpha & e > 0
      br <- pretty(range(e, s$d), 50)
      ha <- hist(e, breaks = br, plot = FALSE)
      hs <- hist(c(e[sig], br[1] - 1), breaks = c(br[1] - 2, br), plot = FALSE)
      ink(c(3.6, 4.6, 4.2, 1.2))
      plot.new(); plot.window(xlim = range(br), ylim = c(0, max(ha$counts) * 1.08), xaxs = "i", yaxs = "i")
      axis(1); axis(2)
      bars(ha, "#ffffff1a", "#ffffff40")
      bars(list(breaks = br, counts = hs$counts[-1]), "#3DDC9766", "#3DDC97")
      abline(v = s$d, col = "#ffffff", lwd = 2)
      if (any(sig)) abline(v = mean(e[sig]), col = TRIAL$col[["sig"]], lwd = 2, lty = 2)
      mtext_fit("effect each trial reports", side = 3, line = 2.2, adj = 0, col = TRIAL$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit(if (any(sig) && s$d > 0)
              sprintf("true %.2f; the significant ones average %.2f, %.1fx the truth", s$d, mean(e[sig]), mean(e[sig]) / s$d)
            else sprintf("true %.2f", s$d),
            side = 3, line = 0.7, adj = 0, col = "#dfe4ee", cex = 0.95)
      legend("topright", bty = "n", text.col = "#e6e9ef", fill = c("#ffffff1a", "#3DDC9766"),
             border = c("#ffffff40", "#3DDC97"), legend = c("all trials", "significant"))
      mtext_fit("estimated d", side = 1, line = 2.4, col = "#c3cad8")
    }, bg = "transparent", res = 96)
  })
}

# ── the site ─────────────────────────────────────────────────────────────────

# Full-window stages with glass cards: the Chaos Lab layout. The second screen
# is two glass panels side by side, stacked on a phone.
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
      ui_power(ns("power")),
      ui_results(ns("results"))
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell)
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    sims <- server_power("power")
    server_results("results", sims)
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
