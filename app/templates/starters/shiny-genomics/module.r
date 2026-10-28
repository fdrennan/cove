# __HOST__/module.r
#
# Expression: a differential-expression study of 2,000 genes, simulated so the
# truth is known and every discovery can be checked against it.
#
#   volcano   a Welch t-test per gene, Benjamini-Hochberg false discovery rate
#             control across all of them, and the volcano plot: effect against
#             evidence. Click a gene to see its samples.
#   overview  the MA plot (change against average expression) and a heatmap of
#             the strongest genes across every sample.
#
# The data: each gene has its own baseline (log2 expression, 2 to 14) and its
# own noise (gene-to-gene spread, as in real arrays); a chosen share of genes
# is truly changed in the treated group by about the chosen log2 fold change,
# up or down. Same settings and same dataset number, same data.

GENO <- list(genes = 2000L,
             col = c(up = "#FF6B6B", down = "#5AC8FA", flat = "#8a93a6", pick = "#F0DC78", ink = "#e6e9ef"))

# Gene symbols, made up but gene-shaped, fixed for every dataset.
GENES <- local({
  set.seed(1859)
  stems <- c("ABC", "KRT", "TNF", "SOX", "FOX", "MAPK", "CDK", "IL", "HOX", "ZNF", "SLC", "RPL", "GATA",
             "WNT", "NOTCH", "TP", "MYC", "EGR", "HSP", "COL", "CXCL", "ATP", "NDUF", "PRKC", "SMAD")
  make.unique(paste0(sample(stems, GENO$genes, TRUE), sample(1:40, GENO$genes, TRUE),
                     sample(c("", "", "", "A", "B", "L"), GENO$genes, TRUE)), sep = "-")
})

# Simulate the study: a genes x samples matrix, control columns first.
simulate_study <- function(n, frac, effect, noise, seed) {
  set.seed(seed)
  G <- GENO$genes
  base <- pmin(pmax(stats::rnorm(G, 7.5, 2.2), 2), 14)
  sd_g <- noise * sqrt(stats::rchisq(G, 6) / 6) * (0.7 + 0.6 * exp(-(base - 2) / 4))
  truth <- numeric(G)
  de <- sample(G, round(frac * G))
  truth[de] <- sample(c(-1, 1), length(de), TRUE) * effect * stats::runif(length(de), 0.5, 1.5)
  x <- matrix(stats::rnorm(G * 2 * n, base, sd_g), G, 2 * n)
  x[, n + seq_len(n)] <- x[, n + seq_len(n)] + truth
  dimnames(x) <- list(GENES, c(paste0("C", seq_len(n)), paste0("T", seq_len(n))))
  list(x = x, n = n, truth = truth)
}

# Welch's t-test on every row at once, then BH-adjusted q-values.
test_genes <- function(s) {
  a <- s$x[, seq_len(s$n), drop = FALSE]; b <- s$x[, s$n + seq_len(s$n), drop = FALSE]
  ma <- rowMeans(a); mb <- rowMeans(b)
  va <- rowSums((a - ma)^2) / (s$n - 1); vb <- rowSums((b - mb)^2) / (s$n - 1)
  se2 <- va / s$n + vb / s$n
  t <- (mb - ma) / sqrt(se2)
  df <- se2^2 / ((va / s$n)^2 / (s$n - 1) + (vb / s$n)^2 / (s$n - 1))
  p <- 2 * stats::pt(-abs(t), df)
  data.frame(gene = GENES, lfc = mb - ma, avg = (ma + mb) / 2, t = t, p = p, nlp = -log10(p),
             q = stats::p.adjust(p, "BH"), truth = s$truth != 0, stringsAsFactors = FALSE)
}

tile <- function(value, label, tone = NULL) {
  box::use(shiny = shiny[tags])
  shiny$tags$div(class = paste("tile", tone), shiny$tags$b(value), shiny$tags$span(label))
}

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

# Point colours: up, down or not called.
call_col <- function(r, called, alpha = "") {
  ifelse(!called, paste0(GENO$col[["flat"]], "55"),
         ifelse(r$lfc > 0, paste0(GENO$col[["up"]], alpha), paste0(GENO$col[["down"]], alpha)))
}

# ── volcano ──────────────────────────────────────────────────────────────────

ui_volcano <- function(id) {
  box::use(shiny = shiny[NS, tags, sliderInput, selectInput, selectizeInput, checkboxInput, actionButton,
                         plotOutput, uiOutput, clickOpts])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("volcano"), height = "100%", click = clickOpts(ns("pick"))),
    tags$div(class = "stage-plot glass", plotOutput(ns("samples"), height = "210px")),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Expression"),
      uiOutput(ns("tiles")),
      sliderInput(ns("n"), "Samples per group", 3, 30, 6, step = 1, width = "100%"),
      sliderInput(ns("frac"), "Genes truly changed, %", 0, 30, 8, step = 1, width = "100%"),
      sliderInput(ns("effect"), "Typical change, log2 fold", 0.25, 3, 1, step = 0.05, width = "100%"),
      sliderInput(ns("noise"), "Noise, sd", 0.1, 2, 0.6, step = 0.05, width = "100%"),
      selectInput(ns("fdr"), "False discovery rate", c("0.01", "0.05", "0.10", "0.20"), selected = "0.05", width = "100%"),
      sliderInput(ns("minfc"), "And at least |log2 fold|", 0, 2, 0.5, step = 0.05, width = "100%"),
      checkboxInput(ns("truth"), "Ring the genes that truly changed", FALSE),
      selectizeInput(ns("gene"), "Gene", choices = NULL, width = "100%"),
      actionButton(ns("again"), "New dataset", class = "btn btn-outline-light btn-sm w-100")
    )
  )
}

# Returns the study (data, results, calls) for the overview branch.
server_volcano <- function(id) {
  box::use(
    shiny = shiny[moduleServer, reactive, req, observeEvent, renderPlot, renderUI, tags, nearPoints,
                  updateSelectizeInput],
    graphics[plot.new, plot.window, axis, points, abline, mtext, text, segments]
  )
  moduleServer(id, function(input, output, session) {
    study <- reactive({
      req(input$n, !is.null(input$frac), input$effect, input$noise)
      s <- simulate_study(input$n, input$frac / 100, input$effect, input$noise, 1 + input$again)
      list(s = s, r = test_genes(s))
    })
    calls <- reactive({
      r <- study()$r
      r$q < as.numeric(req(input$fdr)) & abs(r$lfc) >= input$minfc
    })
    # The gene on show: the one picked, else the dataset's strongest. 2,000
    # names go to the browser as it searches (server-side selectize).
    gene <- reactive({
      r <- study()$r
      g <- input$gene
      if (is.null(g) || !(g %in% r$gene)) r$gene[which.min(r$p)] else g
    })
    show <- function(g) updateSelectizeInput(session, "gene", choices = GENES, selected = g, server = TRUE)
    observeEvent(study(), {
      r <- study()$r
      show(r$gene[which.min(r$p)])
    })
    observeEvent(input$pick, {
      hit <- nearPoints(study()$r, input$pick, xvar = "lfc", yvar = "nlp", maxpoints = 1, threshold = 18)
      if (nrow(hit)) show(hit$gene)
    })

    output$volcano <- renderPlot({
      r <- study()$r
      called <- calls()
      lim <- max(abs(r$lfc)) * 1.05
      ink(c(3.8, 4.6, 3.4, card_room(session, "volcano")))
      plot.new(); plot.window(xlim = c(-lim, lim), ylim = c(0, max(r$nlp) * 1.06), yaxs = "i")
      axis(1); axis(2)
      abline(v = c(-1, 1) * input$minfc, col = "#ffffff30", lty = 2)
      if (any(called)) abline(h = min(r$nlp[called]), col = "#ffffff30", lty = 2)
      o <- order(called)
      points(r$lfc[o], r$nlp[o], pch = 19, cex = ifelse(called[o], 0.9, 0.55), col = call_col(r[o, ], called[o], "cc"))
      if (isTRUE(input$truth)) points(r$lfc[r$truth], r$nlp[r$truth], pch = 1, cex = 1.3, col = "#ffffffaa", lwd = 0.8)
      top <- utils::head(order(r$p), 8)
      text(r$lfc[top], r$nlp[top], r$gene[top], pos = ifelse(r$lfc[top] > 0, 4, 2), col = "#e6e9ef", cex = 0.85, offset = 0.5)
      g <- match(gene(), r$gene)
      points(r$lfc[g], r$nlp[g], pch = 21, cex = 2.6, col = GENO$col[["pick"]], lwd = 2.2)
      text(r$lfc[g], r$nlp[g], r$gene[g], pos = 3, offset = 1, col = GENO$col[["pick"]], font = 2)
      mtext_fit("evidence against effect, every gene", side = 3, line = 1.4, adj = 0, col = GENO$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("log2 fold change, treated / control", side = 1, line = 2.5, col = "#c3cad8")
      mtext("-log10 p", side = 2, line = 3, col = "#c3cad8", las = 0)
    }, bg = "transparent", res = 96)

    output$samples <- renderPlot({
      st <- study()
      g <- match(gene(), st$r$gene)
      n <- st$s$n
      v <- st$s$x[g, ]
      grp <- rep(1:2, each = n)
      ink(c(2.2, 3.6, 3.2, 0.8))
      plot.new(); plot.window(xlim = c(0.4, 2.6), ylim = range(v) + c(-0.3, 0.3))
      axis(2); axis(1, at = 1:2, labels = c("control", "treated"), tick = FALSE, cex.axis = 1)
      x <- grp + stats::runif(2 * n, -0.16, 0.16)
      cols <- c(GENO$col[["down"]], GENO$col[["up"]])
      points(x, v, pch = 21, bg = paste0(cols[grp], "aa"), col = cols[grp], cex = 1.4)
      m <- tapply(v, grp, mean)
      segments(1:2 - 0.3, m, 1:2 + 0.3, m, col = "#ffffff", lwd = 3)
      r <- st$r[g, ]
      mtext_fit(sprintf("%s  log2FC %+.2f  q %s", r$gene, r$lfc, format(signif(r$q, 2))),
            side = 3, line = 1.5, adj = 0, col = GENO$col[["pick"]], cex = 0.95, font = 2)
      mtext_fit(if (r$truth) "truly changed" else "truly unchanged", side = 3, line = 0.3, adj = 0, col = "#dfe4ee", cex = 0.9)
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      r <- study()$r
      called <- calls()
      tp <- sum(called & r$truth)
      fdp <- if (any(called)) 1 - tp / sum(called) else 0
      sens <- if (any(r$truth)) tp / sum(r$truth) else NA
      tags$div(
        class = "tiles",
        tile(sum(called), "genes called"),
        tile(tp, "of them truly changed"),
        tile(paste0(round(100 * fdp, 1), "%"), "false discoveries",
             if (fdp > as.numeric(input$fdr)) "tone-bad" else "tone-good"),
        tile(if (is.na(sens)) "-" else paste0(round(100 * sens), "%"), "of real changes found")
      )
    })

    reactive(list(st = study(), called = calls(), gene = gene()))
  })
}

# ── overview ─────────────────────────────────────────────────────────────────

ui_overview <- function(id) {
  box::use(shiny = shiny[NS, tags, plotOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage-duo",
    tags$div(class = "glass", plotOutput(ns("ma"), height = "100%")),
    tags$div(class = "glass", plotOutput(ns("heat"), height = "100%"))
  )
}

server_overview <- function(id, study) {
  box::use(
    shiny = shiny[moduleServer, renderPlot],
    graphics[plot.new, plot.window, axis, points, abline, mtext, image, text, lines],
    grDevices[colorRampPalette]
  )
  moduleServer(id, function(input, output, session) {
    output$ma <- renderPlot({
      x <- study()
      r <- x$st$r
      called <- x$called
      ink(c(3.8, 4.6, 3.4, 1.2))
      plot.new(); plot.window(xlim = range(r$avg), ylim = c(-1, 1) * max(abs(r$lfc)))
      axis(1); axis(2)
      abline(h = 0, col = "#ffffff40")
      o <- order(called)
      points(r$avg[o], r$lfc[o], pch = 19, cex = ifelse(called[o], 0.85, 0.5), col = call_col(r[o, ], called[o], "cc"))
      # The noise funnel: how far log2FC spreads at each expression level.
      lo <- stats::lowess(r$avg, abs(r$lfc), f = 0.3)
      lines(lo$x, lo$y, col = "#ffffff99", lwd = 2); lines(lo$x, -lo$y, col = "#ffffff99", lwd = 2)
      g <- match(x$gene, r$gene)
      points(r$avg[g], r$lfc[g], pch = 21, cex = 2.4, col = GENO$col[["pick"]], lwd = 2.2)
      mtext_fit("MA plot: change against average expression", side = 3, line = 1.4, adj = 0, col = GENO$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("average log2 expression", side = 1, line = 2.5, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    output$heat <- renderPlot({
      x <- study()
      r <- x$st$r
      top <- utils::head(order(r$p), 40)
      m <- x$st$s$x[top, , drop = FALSE]
      z <- (m - rowMeans(m)) / pmax(apply(m, 1, stats::sd), 1e-9)
      # Genes ordered by direction then strength; samples stay in their groups.
      z <- z[order(r$lfc[top] > 0, -r$nlp[top]), , drop = FALSE]
      n <- x$st$s$n
      pal <- colorRampPalette(c("#2b6cff", "#0d1430", "#ff4d6d"))(101)
      ink(c(2.6, 6.2, 3.4, 1))
      plot.new(); plot.window(xlim = c(0.5, ncol(z) + 0.5), ylim = c(0.5, nrow(z) + 0.5), xaxs = "i", yaxs = "i")
      image(seq_len(ncol(z)), seq_len(nrow(z)), t(pmin(pmax(z, -2.5), 2.5)), zlim = c(-2.5, 2.5),
            col = pal, add = TRUE, useRaster = TRUE)
      abline(v = n + 0.5, col = "#ffffff", lwd = 2)
      axis(1, at = c((n + 1) / 2, n + (n + 1) / 2), labels = c("control", "treated"), tick = FALSE)
      axis(2, at = seq_len(nrow(z)), labels = rownames(z), tick = FALSE, cex.axis = if (nrow(z) > 30) 0.62 else 0.8)
      mtext_fit("the 40 strongest genes, every sample (z-score)", side = 3, line = 1.4, adj = 0, col = GENO$col[["ink"]], cex = 1.1, font = 2)
    }, bg = "transparent", res = 96)
  })
}

# ── the site ─────────────────────────────────────────────────────────────────

# Full-window stages with glass cards: the Chaos Lab layout. The second screen
# is two glass panels side by side, stacked on a phone.
STAGE_CSS <- "
.stage { position: relative; height: calc(100vh - 56px); min-height: 560px; overflow: hidden;
  background: radial-gradient(ellipse at 50% 35%, #16203d 0%, #090c16 55%, #04050a 100%); }
.stage > .shiny-plot-output { position: absolute; inset: 0; }
.glass { background: rgba(10, 13, 22, 0.80); border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: 14px; backdrop-filter: blur(8px); color: #e6e9ef; }
.stage-card { position: absolute; top: 16px; right: 16px; width: 360px; max-height: calc(100% - 32px);
  overflow: auto; padding: 12px 16px; }
.stage-card summary { font-size: 1.2rem; font-weight: 600; cursor: pointer; }
.stage-card[open] summary { margin-bottom: 8px; }
.stage-plot { position: absolute; left: 76px; bottom: 64px; width: min(380px, 34%); padding: 6px; }
.stage-duo { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; padding: 16px;
  height: calc(100vh - 56px); min-height: 520px; background: linear-gradient(180deg, #04050a 0%, #0b1022 100%); }
.stage-duo > .glass { position: relative; padding: 8px; min-height: 0; }
.stage-duo .shiny-plot-output { position: absolute; inset: 8px; height: auto !important; }
.tiles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin: 4px 0 12px; }
.tile { background: rgba(255, 255, 255, 0.06); border-radius: 10px; padding: 7px 10px; }
.tile b { display: block; font-size: 1.35rem; font-weight: 600; color: #fff; font-variant-numeric: tabular-nums; }
.tile span { font-size: 0.9rem; color: #d3d9e6; }
.tile.tone-good b { color: #3DDC97; }
.tile.tone-bad b { color: #FF7A7E; }
@media (max-width: 760px) {
  .stage-card { top: auto; bottom: 12px; left: 12px; right: 12px; width: auto; max-height: 46%; }
  .stage:has(.stage-card[open]) > .shiny-plot-output { bottom: 48%; }
  .stage > .shiny-plot-output { top: 150px; }
  .stage-plot { top: 8px; bottom: auto; left: 8px; right: 8px; width: auto; padding: 2px; }
  .stage-plot .shiny-plot-output { height: 136px !important; }
  .stage-duo { grid-template-columns: 1fr; height: auto; padding: 10px; gap: 10px; }
  .stage-duo > .glass { height: 380px; }
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
      ui_volcano(ns("volcano")),
      ui_overview(ns("overview"))
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell)
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    study <- server_volcano("volcano")
    server_overview("overview", study)
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
