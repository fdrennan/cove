# __HOST__/module.r
#
# Machine Dashboard: the computer serving this page, live - its processors,
# load and memory, and the R session answering you - read every two seconds
# from Linux's own counters in /proc. No database, no agent, no packages.
#
#   live      CPU use and load over the last four minutes, edge to edge; the
#             card holds the latest numbers.
#   inside    each processor's use this moment, and memory over time: what
#             programs hold, what the kernel keeps as cache, what is free.
#
# CPU use is the change in /proc/stat's counters between two readings: the
# share of time not idle. Load is /proc/loadavg: how many tasks wanted a
# processor, averaged over 1, 5 and 15 minutes. Memory is /proc/meminfo.
# On a machine with no /proc (macOS, Windows) the page says so.

DASH <- list(every = 2000, keep = 120, col = c(cpu = "#5AC8FA", load = "#F0DC78", used = "#FF9650", cache = "#3DDC97", ink = "#e6e9ef"))

# /proc/stat's CPU lines: one row per processor (the first is all of them),
# columns busy and total jiffies.
read_cpu <- function() {
  l <- grep("^cpu", readLines("/proc/stat"), value = TRUE)
  v <- lapply(strsplit(trimws(sub("^cpu[0-9]*", "", l)), "\\s+"), as.numeric)
  idle <- vapply(v, function(x) x[4] + x[5], numeric(1))
  total <- vapply(v, sum, numeric(1))
  data.frame(name = sub("\\s.*", "", l), busy = total - idle, total = total)
}

read_mem <- function() {
  m <- readLines("/proc/meminfo")
  kb <- function(k) as.numeric(sub("^\\D+(\\d+).*", "\\1", grep(paste0("^", k, ":"), m, value = TRUE)[1]))
  total <- kb("MemTotal"); avail <- kb("MemAvailable"); cache <- kb("Cached") + kb("Buffers")
  list(total = total / 1048576, used = (total - avail) / 1048576, cache = cache / 1048576, avail = avail / 1048576)
}

read_load <- function() as.numeric(strsplit(readLines("/proc/loadavg"), " ")[[1]][1:3])

# Seconds since the machine booted, as "3 d 4 h" or "5 h 12 min".
read_uptime <- function() {
  s <- as.numeric(strsplit(readLines("/proc/uptime"), " ")[[1]][1])
  d <- s %/% 86400; h <- (s %% 86400) %/% 3600; m <- (s %% 3600) %/% 60
  if (d > 0) sprintf("%d d %d h", d, h) else sprintf("%d h %d min", h, m)
}

gb <- function(x) paste(formatC(x, format = "f", digits = 1), "GB")

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

# ── live ─────────────────────────────────────────────────────────────────────

ui_live <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, plotOutput, uiOutput, actionButton])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("cpu"), height = "100%"),
    # Nothing is read until it is asked to (operator, 2026-10-25: "templates
    # need to wait to start when pushed").
    uiOutput(ns("start"), class = "stage-start"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Machine Dashboard"),
      tags$p("This machine, read every two seconds."),
      actionButton(ns("run"), "Start", class = "btn btn-primary btn-sm"),
      uiOutput(ns("tiles"))
    )
  )
}

# Returns the history, for the inside branch.
server_live <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, reactiveVal, observe, observeEvent, invalidateLater, isolate, renderPlot, renderUI, req, tags,
                  updateActionButton, actionButton],
    graphics[plot.new, plot.window, axis, lines, polygon, abline, mtext, legend]
  )
  moduleServer(id, function(input, output, session) {
    here <- file.exists("/proc/stat")
    hist <- reactiveVal(NULL)
    last <- NULL
    running <- reactiveVal(FALSE)
    started <- reactiveVal(FALSE)
    toggle <- function() {
      running(!isolate(running()))
      started(TRUE)
      updateActionButton(session, "run", label = if (isolate(running())) "Pause" else "Resume")
    }
    observeEvent(input$run, toggle())
    observeEvent(input$go, toggle())
    output$start <- renderUI({
      if (started()) return(NULL)
      actionButton(session$ns("go"), "Start", class = "btn btn-primary btn-lg")
    })
    # A reading every two seconds. The history is read inside isolate():
    # an observer that depends on what it sets re-runs at once and never
    # lets the page update (Cove's CONTEXT.md, "Shiny starters").
    observe({
      if (!running()) return()
      invalidateLater(DASH$every)
      if (!here) return()
      cpu <- read_cpu(); mem <- read_mem(); load <- read_load()
      if (!is.null(last)) {
        use <- (cpu$busy - last$busy) / pmax(cpu$total - last$total, 1)
        row <- list(t = Sys.time(), cpu = use[1], cores = use[-1], load = load, mem = mem, up = read_uptime())
        h <- isolate(hist())
        hist(utils::tail(c(h, list(row)), DASH$keep))
      }
      last <<- cpu
    })

    output$cpu <- renderPlot({
      h <- hist()
      req(length(h) >= 2)
      t <- as.numeric(vapply(h, function(r) as.numeric(r$t), numeric(1)) - as.numeric(h[[length(h)]]$t))
      cpu <- 100 * vapply(h, function(r) r$cpu, numeric(1))
      cores <- length(h[[1]]$cores)
      load <- vapply(h, function(r) r$load[1], numeric(1)) / cores * 100
      ink(c(3.8, 4.8, 3.4, card_room(session, "cpu")))
      plot.new(); plot.window(xlim = c(-DASH$keep * DASH$every / 1000, 0), ylim = c(0, max(100, load) * 1.05), xaxs = "i", yaxs = "i")
      # Load per processor passes 100% when tasks queue, so the scale follows it.
      ys <- pretty(c(0, max(100, load)))
      abline(h = ys, col = "#ffffff12")
      axis(1); axis(2, at = ys, labels = paste0(ys, "%"))
      polygon(c(t, rev(t)), c(cpu, rep(0, length(t))), col = "#5AC8FA2a", border = NA)
      lines(t, cpu, col = "#5AC8FA40", lwd = 8)
      lines(t, cpu, col = DASH$col[["cpu"]], lwd = 2.6)
      lines(t, load, col = DASH$col[["load"]], lwd = 2, lty = 2)
      legend("topleft", bty = "n", text.col = "#e6e9ef", lwd = c(2.6, 2), lty = c(1, 2), col = DASH$col[c("cpu", "load")],
             legend = c("CPU in use, all processors", sprintf("load, 1 minute, per processor (%d)", cores)))
      mtext_fit("the last four minutes", side = 3, line = 1.4, adj = 0, col = DASH$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("seconds ago", side = 1, line = 2.5, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      if (!here) return(tags$p("This machine has no /proc: the dashboard reads Linux's counters, so it shows nothing here."))
      h <- hist()
      req(length(h) >= 1)
      r <- h[[length(h)]]
      g <- gc()
      tags$div(
        class = "tiles",
        tile(paste0(round(100 * r$cpu), "%"), "CPU in use", if (r$cpu > 0.9) "tone-bad" else NULL),
        tile(length(r$cores), "processors"),
        tile(formatC(r$load[1], format = "f", digits = 1), sprintf("load, 1 min (15 min: %.1f)", r$load[3])),
        tile(paste0(formatC(r$mem$used, format = "f", digits = 1), " / ", formatC(r$mem$total, format = "f", digits = 0), " GB"), "memory in use",
             if (r$mem$avail / r$mem$total < 0.1) "tone-bad" else NULL),
        tile(gb(r$mem$cache), "kept as cache"),
        tile(paste(formatC(sum(g[, 2]), format = "f", digits = 0), "MB"), "this R session"),
        tile(r$up, "since the machine booted"),
        tile(paste0(gb(r$mem$avail)), "available")
      )
    })

    hist
  })
}

# ── inside ───────────────────────────────────────────────────────────────────

ui_inside <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, plotOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage-duo",
    tags$div(class = "glass", plotOutput(ns("cores"), height = "100%")),
    tags$div(class = "glass", plotOutput(ns("memory"), height = "100%"))
  )
}

server_inside <- function(id, hist) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, renderPlot, req],
    graphics[plot.new, plot.window, axis, rect, polygon, lines, mtext, legend]
  )
  moduleServer(id, function(input, output, session) {
    output$cores <- renderPlot({
      h <- hist(); req(length(h) >= 1)
      u <- 100 * h[[length(h)]]$cores
      k <- length(u)
      ink(c(3.6, 4.6, 4.2, 1.2))
      plot.new(); plot.window(xlim = c(0.5, k + 0.5), ylim = c(0, 100), xaxs = "i", yaxs = "i")
      axis(1, at = seq_len(k), labels = if (k <= 32) seq_len(k) - 1 else FALSE); axis(2, at = seq(0, 100, 25))
      cols <- grDevices::colorRampPalette(c("#3DDC97", "#F0DC78", "#FF7A7E"))(101)[pmin(101, pmax(1, round(u) + 1))]
      rect(seq_len(k) - 0.38, 0, seq_len(k) + 0.38, u, col = cols, border = NA)
      mtext_fit("each processor", side = 3, line = 2.2, adj = 0, col = DASH$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("percent busy over the last two seconds", side = 3, line = 0.7, adj = 0, col = "#dfe4ee", cex = 0.95)
      mtext_fit("processor", side = 1, line = 2.4, col = "#c3cad8")
    }, bg = "transparent", res = 96)

    output$memory <- renderPlot({
      h <- hist(); req(length(h) >= 2)
      t <- as.numeric(vapply(h, function(r) as.numeric(r$t), numeric(1)) - as.numeric(h[[length(h)]]$t))
      used <- vapply(h, function(r) r$mem$used, numeric(1))
      cache <- vapply(h, function(r) r$mem$cache, numeric(1))
      total <- h[[1]]$mem$total
      ink(c(3.6, 4.6, 4.2, 1.2))
      plot.new(); plot.window(xlim = range(t), ylim = c(0, total), xaxs = "i", yaxs = "i")
      axis(1); axis(2)
      polygon(c(t, rev(t)), c(used, rep(0, length(t))), col = "#FF965066", border = DASH$col[["used"]])
      polygon(c(t, rev(t)), c(pmin(used + cache, total), rev(used)), col = "#3DDC9740", border = DASH$col[["cache"]])
      legend("topleft", bty = "n", text.col = "#e6e9ef", fill = c("#FF965066", "#3DDC9740"), border = DASH$col[c("used", "cache")],
             legend = c("held by programs", "kept by the kernel as cache"))
      mtext_fit(sprintf("memory, of %.0f GB", total), side = 3, line = 2.2, adj = 0, col = DASH$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("the rest is free; cache is given back when programs need it", side = 3, line = 0.7, adj = 0, col = "#dfe4ee", cex = 0.95)
      mtext_fit("seconds ago", side = 1, line = 2.4, col = "#c3cad8")
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
.stage-start { position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; pointer-events: none; }
.stage-start .btn { pointer-events: auto; padding: 0.7rem 2.4rem; font-size: 1.25rem; }
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
      ui_live(ns("live")),
      ui_inside(ns("inside"))
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell, ./r/log)
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    log$log_session(session)
    log$log_info("__HOST__: module server starting")
    x <- server_live("live")
    server_inside("inside", x)
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
