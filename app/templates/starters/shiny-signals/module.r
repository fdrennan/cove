# __HOST__/module.r
#
# Fourier Lab: a signal built from three waves and some noise, taken apart
# by the fast Fourier transform, and cleaned by a low-pass filter.
#
#   signal    the second of signal on screen: what was measured (waves plus
#             noise), the clean waves underneath, and what the filter leaves.
#   spectrum  the same signal as frequencies: each wave a spike at its
#             frequency, the noise a floor spread over all of them, and the
#             filter's cutoff; beside it, how much of the noise the filter
#             removed, as error against the clean waves.
#
# The filter is the plainest there is: transform, set every frequency above
# the cutoff to zero, transform back. Cooley, J. W. and Tukey, J. W. (1965),
# "An Algorithm for the Machine Calculation of Complex Fourier Series",
# Mathematics of Computation 19(90) - R's fft() is that algorithm.

SIG <- list(rate = 1000, secs = 1, col = c(raw = "#8fa3c8", clean = "#F0DC78", filtered = "#5AC8FA", cut = "#FF9650", ink = "#e6e9ef"))

# The three waves (frequencies in Hz, amplitudes), summed, at `rate` samples a second.
waves <- function(t, f, a) a[1] * sin(2 * pi * f[1] * t) + a[2] * sin(2 * pi * f[2] * t) + a[3] * sin(2 * pi * f[3] * t)

# One-sided amplitude spectrum: frequencies 0 .. rate/2 and their amplitudes.
spectrum <- function(x, rate) {
  n <- length(x)
  m <- Mod(stats::fft(x))[1:(n %/% 2 + 1)] / n
  m[-1] <- 2 * m[-1]
  list(f = (0:(n %/% 2)) * rate / n, a = m)
}

# Low-pass: every frequency above `cutoff` set to zero, both halves of the
# transform (the negative frequencies mirror the positive ones).
low_pass <- function(x, rate, cutoff) {
  n <- length(x)
  f <- c(0:(n %/% 2), -((n - 1) %/% 2):-1) * rate / n
  X <- stats::fft(x)
  X[abs(f) > cutoff] <- 0
  Re(stats::fft(X, inverse = TRUE)) / n
}

tile <- function(value, label, tone = NULL) {
  box::use(shiny = shiny[tags])
  shiny$tags$div(class = paste("tile", tone), shiny$tags$b(value), shiny$tags$span(label))
}

card_room <- function(session, out) {
  w <- session$clientData[[paste0("output_", session$ns(out), "_width")]]
  if (!is.null(w) && w > 900) 400 / 19.2 else 1.4
}

ink <- function(mar = c(3.6, 4.4, 3, 1.2)) {
  graphics::par(bg = NA, mar = mar, col.axis = "#c3cad8", col.lab = "#dfe4ee", fg = "#5b6478",
                cex.axis = 0.95, las = 1)
}

# ── signal ───────────────────────────────────────────────────────────────────

ui_signal <- function(id) {
  box::use(shiny = shiny[NS, tags, sliderInput, actionButton, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("signal"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Fourier Lab"),
      sliderInput(ns("f1"), "Wave 1, Hz", 1, 100, 5, step = 1, width = "100%"),
      sliderInput(ns("f2"), "Wave 2, Hz", 1, 200, 40, step = 1, width = "100%"),
      sliderInput(ns("f3"), "Wave 3, Hz", 1, 400, 120, step = 1, width = "100%"),
      sliderInput(ns("a3"), "Wave 3's height (waves 1 and 2: 1 and 0.5)", 0, 1, 0.3, step = 0.05, width = "100%"),
      sliderInput(ns("noise"), "Noise, standard deviation", 0, 2, 0.6, step = 0.05, width = "100%"),
      sliderInput(ns("cutoff"), "Low-pass cutoff, Hz", 1, 500, 60, step = 1, width = "100%"),
      actionButton(ns("again"), "New noise", class = "btn btn-outline-light btn-sm w-100"),
      uiOutput(ns("tiles"))
    )
  )
}

# Returns the signal, for the spectrum branch.
server_signal <- function(id) {
  box::use(
    shiny = shiny[moduleServer, reactive, req, renderPlot, renderUI, tags],
    graphics[plot.new, plot.window, axis, lines, abline, mtext, legend]
  )
  moduleServer(id, function(input, output, session) {
    sig <- reactive({
      req(input$f1, input$f2, input$f3, !is.null(input$a3), !is.null(input$noise), input$cutoff)
      input$again
      t <- seq(0, SIG$secs, length.out = SIG$rate * SIG$secs + 1)[-1]
      f <- c(input$f1, input$f2, input$f3); a <- c(1, 0.5, input$a3)
      clean <- waves(t, f, a)
      raw <- clean + stats::rnorm(length(t), 0, input$noise)
      list(t = t, f = f, a = a, clean = clean, raw = raw, cutoff = input$cutoff,
           filtered = low_pass(raw, SIG$rate, input$cutoff))
    })

    output$signal <- renderPlot({
      s <- sig()
      lim <- range(s$raw, s$clean)
      ink(c(3.8, 4.6, 3.4, card_room(session, "signal")))
      plot.new(); plot.window(xlim = range(s$t), ylim = lim, xaxs = "i")
      abline(h = 0, col = "#ffffff18")
      axis(1); axis(2)
      lines(s$t, s$raw, col = "#8fa3c855", lwd = 1)
      lines(s$t, s$clean, col = "#F0DC7888", lwd = 5)
      lines(s$t, s$filtered, col = "#5AC8FA40", lwd = 7)
      lines(s$t, s$filtered, col = SIG$col[["filtered"]], lwd = 2)
      legend("topleft", bty = "n", text.col = "#e6e9ef", lwd = c(1, 5, 2), col = c("#8fa3c8", "#F0DC78", "#5AC8FA"),
             legend = c("measured: waves plus noise", "the waves alone", sprintf("low-passed below %d Hz", s$cutoff)))
      mtext_fit("one second of signal", side = 3, line = 1.4, adj = 0, col = SIG$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("seconds", side = 1, line = 2.5, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      s <- sig()
      rms <- function(x) sqrt(mean(x^2))
      before <- rms(s$raw - s$clean); after <- rms(s$filtered - s$clean)
      kept <- sum(s$f <= s$cutoff)
      tags$div(
        class = "tiles",
        tile(formatC(before, format = "f", digits = 3), "error before the filter (rms)"),
        tile(formatC(after, format = "f", digits = 3), "error after it", if (after < before) "tone-good" else "tone-bad"),
        tile(paste0(kept, " of 3"), "waves below the cutoff", if (kept < 3) "tone-bad" else NULL),
        tile(sprintf("%.1f dB", 20 * log10(rms(s$clean) / max(after, 1e-12))), "signal to noise, filtered")
      )
    })

    sig
  })
}

# ── spectrum ─────────────────────────────────────────────────────────────────

ui_spectrum <- function(id) {
  box::use(shiny = shiny[NS, tags, plotOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage-duo",
    tags$div(class = "glass", plotOutput(ns("spectrum"), height = "100%")),
    tags$div(class = "glass", plotOutput(ns("zoom"), height = "100%"))
  )
}

server_spectrum <- function(id, sig) {
  box::use(
    shiny = shiny[moduleServer, renderPlot],
    graphics[plot.new, plot.window, axis, lines, segments, abline, mtext, text, rect]
  )
  moduleServer(id, function(input, output, session) {
    output$spectrum <- renderPlot({
      s <- sig()
      sp <- spectrum(s$raw, SIG$rate)
      ink(c(3.6, 4.6, 4.2, 1.2))
      plot.new(); plot.window(xlim = c(0, SIG$rate / 2), ylim = c(0, max(sp$a[-1], s$a) * 1.12), xaxs = "i", yaxs = "i")
      rect(s$cutoff, 0, SIG$rate / 2, max(sp$a[-1], s$a) * 1.12, col = "#FF965014", border = NA)
      axis(1); axis(2)
      segments(sp$f, 0, sp$f, sp$a, col = "#5AC8FA", lwd = 1.2)
      abline(v = s$cutoff, col = SIG$col[["cut"]], lwd = 2, lty = 2)
      text(s$f, s$a, paste(s$f, "Hz"), pos = 4, xpd = NA, col = SIG$col[["clean"]], cex = 0.9)
      mtext_fit("the measured signal as frequencies (FFT)", side = 3, line = 2.2, adj = 0, col = SIG$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("each wave a spike, the noise a floor under all of them; shaded: what the filter removes",
            side = 3, line = 0.7, adj = 0, col = "#dfe4ee", cex = 0.95)
      mtext_fit("Hz", side = 1, line = 2.4, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    # A close look: a tenth of a second, the waves against what the filter left.
    output$zoom <- renderPlot({
      s <- sig()
      keep <- s$t <= 0.1
      ink(c(3.6, 4.6, 4.2, 1.2))
      plot.new(); plot.window(xlim = c(0, 0.1), ylim = range(s$raw[keep], s$clean[keep]), xaxs = "i")
      axis(1); axis(2)
      lines(s$t[keep], s$raw[keep], col = "#8fa3c8", lwd = 1, type = "o", pch = 20, cex = 0.5)
      lines(s$t[keep], s$clean[keep], col = SIG$col[["clean"]], lwd = 3)
      lines(s$t[keep], s$filtered[keep], col = SIG$col[["filtered"]], lwd = 2)
      mtext_fit("the first tenth of a second, sample by sample", side = 3, line = 2.2, adj = 0, col = SIG$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("dots: the 100 measured samples; gold: the waves; blue: the filtered signal",
            side = 3, line = 0.7, adj = 0, col = "#dfe4ee", cex = 0.95)
      mtext_fit("seconds", side = 1, line = 2.4, col = "#c3cad8")
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
      ui_signal(ns("signal")),
      ui_spectrum(ns("spectrum"))
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell)
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    x <- server_signal("signal")
    server_spectrum("spectrum", x)
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
