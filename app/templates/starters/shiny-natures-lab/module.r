# __HOST__/module.r
#
# Nature's Laboratory: people born with a gene switched off are experiments a
# drug developer could never run, and their genomes say what a drug against
# that gene will do. After Veera Rajagopal, "Nature's laboratory", Works in
# Progress, 27 November 2025.
#
#   trial      a natural trial: pick a drug target, sequence a cohort, and
#              compare people carrying 0, 1 or 2 broken copies - the effect a
#              drug would have, the disease it would prevent, and every trait
#              it would touch (the side effects a trial finds too late).
#   pipeline   why it matters: nine in ten drugs fail; genetically supported
#              targets are approved 2.6 times as often (Minikel et al. 2024).
#   ancestry   how many people must be sequenced to find a variant, population
#              by population - the CCR5 and PCSK9 lesson.
#   knockouts  why cousin marriage makes human knockouts thousands of times
#              commoner: the Pakistan cohorts against UK Biobank.
#
# Every cohort is simulated, so the truth is known and every finding can be
# checked against it. TARGETS holds each gene's story, allele frequencies by
# ancestry and true effects: change a number there and every branch follows.

NL <- list(
  col = c(ink = "#e6e9ef", pick = "#F0DC78", good = "#3DDC97", bad = "#FF7A7E", null = "#8a93a6",
          g0 = "#5AC8FA", g1 = "#C79BF0", g2 = "#F0DC78", with = "#3DDC97", without = "#8a93a6"),
  pops = c(EUR = "European", AFR = "African", SAS = "South Asian", EAS = "East Asian", AMR = "Admixed American"),
  pop_col = c(EUR = "#5AC8FA", AFR = "#F0DC78", SAS = "#FF7A7E", EAS = "#3DDC97", AMR = "#C79BF0")
)

# One row per target: what the drug is, the loss-of-function allele frequency
# by ancestry (orders of magnitude the literature reports), and the true effect
# of 1 and 2 broken copies - on the drug's biomarker (in its units), on the
# disease it would prevent (odds ratio) and on other traits (in SD).
TARGETS <- list(
  PCSK9 = list(
    drug = "evolocumab, alirocumab (approved 2015)",
    story = "Black Americans with a broken PCSK9 had far lower LDL and 88% less heart disease (Cohen 2006). Antibodies against PCSK9 followed within a decade.",
    q = c(EUR = 0.001, AFR = 0.02, SAS = 0.001, EAS = 0.002, AMR = 0.003),
    marker = list(name = "LDL cholesterol", unit = "mg/dL", mean = 130, sd = 35, eff = c(-38, -90)),
    disease = list(name = "coronary heart disease", prev = 0.06, or = c(0.12, 0.05)),
    traits = list()),
  CCR5 = list(
    drug = "maraviroc (approved 2007)",
    story = "Two copies of CCR5-delta32 block HIV's way into T cells, and its carriers are otherwise healthy (1996). Maraviroc blocks the same door.",
    q = c(EUR = 0.10, AFR = 0.002, SAS = 0.02, EAS = 0.001, AMR = 0.04),
    marker = list(name = "CCR5 on CD4 T cells", unit = "% of cells", mean = 16, sd = 5, eff = c(-6, -16)),
    disease = list(name = "HIV infection after exposure", prev = 0.10, or = c(0.8, 0.02)),
    traits = list()),
  HSD17B13 = list(
    drug = "RNAi silencers in trials",
    story = "A splice variant that lowers the enzyme protects against chronic liver disease (Regeneron, 2018), common enough to see in 100,000 exomes.",
    q = c(EUR = 0.26, AFR = 0.10, SAS = 0.17, EAS = 0.30, AMR = 0.20),
    marker = list(name = "ALT", unit = "U/L", mean = 28, sd = 12, eff = c(-1.8, -3.6)),
    disease = list(name = "chronic liver disease", prev = 0.04, or = c(0.75, 0.53)),
    traits = list()),
  APOC3 = list(
    drug = "olezarsen (approved 2024)",
    story = "In one Pakistani family, 33 people had both copies of APOC3 broken - none among 500,000 in UK Biobank. Their triglycerides barely rose after a fatty meal.",
    q = c(EUR = 0.0025, AFR = 0.002, SAS = 0.006, EAS = 0.001, AMR = 0.002),
    marker = list(name = "triglycerides", unit = "mg/dL", mean = 140, sd = 70, eff = c(-55, -110)),
    disease = list(name = "coronary heart disease", prev = 0.06, or = c(0.6, 0.4)),
    traits = list(HDL = c(0.3, 0.6))),
  HAO1 = list(
    drug = "lumasiran (approved 2020)",
    story = "A British Pakistani woman in Genes & Health had no working HAO1 and was healthy - strong evidence that blocking it for life is safe.",
    q = c(EUR = 0.0002, AFR = 0.0002, SAS = 0.0004, EAS = 0.0002, AMR = 0.0002),
    marker = list(name = "plasma glycolate", unit = "umol/L", mean = 8, sd = 3, eff = c(1, 60)),
    disease = list(name = "kidney stones", prev = 0.05, or = c(1, 1)),
    traits = list()),
  DGAT1 = list(
    drug = "DGAT1 inhibitors (trials stopped 2011)",
    story = "Inhibitors caused severe diarrhoea in trials. Two siblings born without DGAT1 had a congenital diarrhoea that killed the boy at eight months.",
    q = c(EUR = 0.0003, AFR = 0.0003, SAS = 0.0006, EAS = 0.0003, AMR = 0.0003),
    marker = list(name = "BMI", unit = "kg/m2", mean = 27, sd = 5, eff = c(-0.2, -4)),
    disease = list(name = "chronic diarrhoea", prev = 0.02, or = c(1.3, 400)),
    traits = list(Albumin = c(0, -2.5), Triglycerides = c(-0.1, -1.5))),
  GPR75 = list(
    drug = "no drug yet",
    story = "Carriers of a broken GPR75 weighed about 5 kg less (UK Biobank exomes, 2021). A target found in sequence before any drug existed.",
    q = c(EUR = 0.0004, AFR = 0.0004, SAS = 0.0004, EAS = 0.0004, AMR = 0.0004),
    marker = list(name = "BMI", unit = "kg/m2", mean = 27, sd = 5, eff = c(-1.8, -3.6)),
    disease = list(name = "obesity", prev = 0.30, or = c(0.46, 0.2)),
    traits = list()),
  ANGPTL7 = list(
    drug = "no drug yet",
    story = "Broken ANGPTL7 lowers eye pressure and protects against glaucoma, a UK Biobank exome finding with no harm attached.",
    q = c(EUR = 0.002, AFR = 0.001, SAS = 0.001, EAS = 0.001, AMR = 0.001),
    marker = list(name = "intraocular pressure", unit = "mmHg", mean = 15.5, sd = 3, eff = c(-1.2, -2.4)),
    disease = list(name = "glaucoma", prev = 0.03, or = c(0.5, 0.25)),
    traits = list())
)

# The phenome every cohort is measured on: quantitative traits in SD units.
PHENOME <- c("LDL", "Triglycerides", "HDL", "ALT", "Albumin", "Bilirubin", "BMI", "Waist", "Height",
             "HbA1c", "Glucose", "Systolic BP", "Diastolic BP", "eGFR", "Creatinine", "Urate",
             "Glycolate", "Haemoglobin", "Platelets", "Neutrophils", "Lymphocytes", "CRP", "Eye pressure",
             "Bone density", "Grip strength", "Heart rate", "Vitamin D", "Testosterone", "Sleep hours")
# Which phenome trait each target's biomarker is.
MARKER_TRAIT <- c(PCSK9 = "LDL", CCR5 = "Lymphocytes", HSD17B13 = "ALT", APOC3 = "Triglycerides",
                  HAO1 = "Glycolate", DGAT1 = "BMI", GPR75 = "BMI", ANGPTL7 = "Eye pressure")

# Genotype counts: inbreeding F makes two copies identical by descent, so a
# knockout's odds are q^2 + F q (1 - q) rather than q^2.
genotype <- function(n, q, F) {
  p2 <- q^2 + F * q * (1 - q)
  p1 <- 2 * q * (1 - q) * (1 - F)
  sample(0:2, n, TRUE, c(1 - p1 - p2, p1, p2))
}

# A cohort for one target: genotypes, the biomarker, the disease and the phenome.
simulate_cohort <- function(target, pop, n, F, seed) {
  set.seed(seed)
  t <- TARGETS[[target]]
  g <- genotype(n, t$q[[pop]], F)
  m <- t$marker
  shift <- c(0, m$eff)[g + 1]
  marker <- stats::rnorm(n, m$mean + shift, m$sd)
  lo <- stats::qlogis(t$disease$prev) + log(c(1, t$disease$or))[g + 1]
  disease <- stats::runif(n) < stats::plogis(lo)
  ph <- matrix(stats::rnorm(n * length(PHENOME)), n, length(PHENOME), dimnames = list(NULL, PHENOME))
  ph[, MARKER_TRAIT[[target]]] <- (marker - m$mean) / m$sd
  for (k in names(t$traits)) ph[, k] <- ph[, k] + c(0, t$traits[[k]])[g + 1]
  list(target = target, pop = pop, g = g, marker = marker, disease = disease, ph = ph)
}

# Each trait against copies carried (additive), by least squares in closed form;
# with knockouts present, a second test for two copies against none.
phewas <- function(cc) {
  d <- cc$g; ok <- stats::var(d) > 0
  out <- lapply(PHENOME, function(k) {
    y <- cc$ph[, k]
    if (!ok) return(c(beta = 0, p = 1))
    b <- stats::cov(d, y) / stats::var(d)
    r <- y - mean(y) - b * (d - mean(d))
    se <- sqrt(sum(r^2) / (length(y) - 2) / sum((d - mean(d))^2))
    c(beta = b, p = 2 * stats::pnorm(-abs(b / se)))
  })
  data.frame(trait = PHENOME, do.call(rbind, out), stringsAsFactors = FALSE)
}

# Disease odds ratio for 1 and 2 copies against none, Woolf intervals.
odds <- function(cc) {
  tab <- table(factor(cc$g, 0:2), factor(cc$disease, c(FALSE, TRUE)))
  sapply(1:2, function(k) {
    a <- tab[k + 1, 2]; b <- tab[k + 1, 1]; c0 <- tab[1, 2]; d0 <- tab[1, 1]
    if (a + b == 0) return(c(or = NA, lo = NA, hi = NA, n = 0))
    a <- a + 0.5; b <- b + 0.5; c0 <- c0 + 0.5; d0 <- d0 + 0.5
    or <- (a * d0) / (b * c0); se <- sqrt(1 / a + 1 / b + 1 / c0 + 1 / d0)
    c(or = or, lo = or * exp(-1.96 * se), hi = or * exp(1.96 * se), n = tab[k + 1, 1] + tab[k + 1, 2])
  })
}

tile <- function(value, label, tone = NULL) {
  box::use(shiny = shiny[tags])
  shiny$tags$div(class = paste("tile", tone), shiny$tags$b(value), shiny$tags$span(label))
}

fmt_n <- function(x) {
  ifelse(x >= 1e6, paste0(formatC(x / 1e6, format = "f", digits = if (x < 1e7) 1 else 0), "M"),
         ifelse(x >= 1e4, paste0(round(x / 1e3), "k"), format(round(x), big.mark = ",")))
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

target_choices <- stats::setNames(names(TARGETS), paste0(names(TARGETS), " - ", vapply(TARGETS, `[[`, "", "drug")))
pop_choices <- stats::setNames(names(NL$pops), NL$pops)

# ── trial ────────────────────────────────────────────────────────────────────

ui_trial <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, selectInput, sliderInput, actionButton, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("phewas"), height = "100%"),
    tags$div(class = "stage-plot glass", plotOutput(ns("dose"), height = "210px")),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("A natural trial"),
      uiOutput(ns("story")),
      uiOutput(ns("tiles")),
      selectInput(ns("target"), "Drug target", target_choices, selected = "PCSK9", width = "100%"),
      selectInput(ns("pop"), "Cohort ancestry", pop_choices, selected = "AFR", width = "100%"),
      selectInput(ns("n"), "People sequenced", c("5,000" = 5e3, "20,000" = 2e4, "100,000" = 1e5, "500,000" = 5e5),
                  selected = 2e4, width = "100%"),
      sliderInput(ns("cousins"), "Parents who are first cousins, %", 0, 70, 0, step = 5, width = "100%"),
      actionButton(ns("again"), "Sequence another cohort", class = "btn btn-outline-light btn-sm w-100"),
      tags$p(class = "credit", "After Veera Rajagopal, ",
             tags$a(href = "https://worksinprogress.co/issue/natures-laboratory/", target = "_blank", "Nature's laboratory"),
             ", Works in Progress (2025).")
    )
  )
}

server_trial <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, reactive, req, renderPlot, renderUI, tags],
    graphics[plot.new, plot.window, axis, points, abline, mtext, text, segments, rect]
  )
  moduleServer(id, function(input, output, session) {
    cohort <- reactive({
      req(input$target, input$pop, input$n)
      simulate_cohort(input$target, input$pop, as.numeric(input$n), input$cousins / 100 / 16, 26 + input$again)
    })
    scan <- reactive(phewas(cohort()))

    output$story <- renderUI({
      t <- TARGETS[[req(input$target)]]
      tags$p(class = "story", tags$b(input$target, ": "), t$story)
    })

    output$phewas <- renderPlot({
      cc <- cohort(); s <- scan()
      t <- TARGETS[[cc$target]]
      thr <- -log10(0.05 / length(PHENOME))
      y <- pmin(-log10(s$p), 300) * sign(s$beta)
      lim <- max(thr * 1.6, abs(y)) * 1.1
      truth <- s$trait %in% c(MARKER_TRAIT[[cc$target]], names(t$traits))
      ink(c(7.6, 4.6, 3.6, card_room(session, "phewas")))
      plot.new(); plot.window(xlim = c(0.5, length(PHENOME) + 0.5), ylim = c(-lim, lim))
      axis(2); axis(1, at = seq_along(PHENOME), labels = PHENOME, las = 2, tick = FALSE, cex.axis = 0.85)
      rect(0.5, -thr, length(PHENOME) + 0.5, thr, col = "#ffffff08", border = NA)
      abline(h = c(-thr, thr), col = "#ffffff40", lty = 2); abline(h = 0, col = "#ffffff30")
      hit <- abs(y) > thr
      col <- ifelse(hit, ifelse(truth, NL$col[["good"]], NL$col[["bad"]]), NL$col[["null"]])
      segments(seq_along(y), 0, seq_along(y), y, col = paste0(col, "88"), lwd = 3)
      points(seq_along(y), y, pch = ifelse(s$beta > 0, 24, 25), bg = col, col = col, cex = ifelse(hit, 1.6, 1))
      if (any(hit)) text(which(hit), y[hit], sprintf("%+.2f SD", s$beta[hit]), pos = ifelse(y[hit] > 0, 3, 1),
                         col = "#e6e9ef", cex = 0.85, offset = 0.6)
      mtext_fit(sprintf("what breaking %s does, trait by trait: %s carriers in %s %s people",
                    cc$target, format(sum(cc$g > 0), big.mark = ","), fmt_n(length(cc$g)), NL$pops[[cc$pop]]),
            side = 3, line = 1.6, adj = 0, col = NL$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit(sprintf("signed -log10 p (up: trait rises per broken copy); dashed: significant across %d traits", length(PHENOME)),
            side = 3, line = 0.3, adj = 0, col = "#c3cad8", cex = 0.9)
    }, bg = "transparent", res = 96)

    output$dose <- renderPlot({
      cc <- cohort()
      m <- TARGETS[[cc$target]]$marker
      ink(c(2.4, 4.2, 3, 0.8))
      have <- table(factor(cc$g, 0:2))
      mu <- tapply(cc$marker, factor(cc$g, 0:2), mean)
      plot.new(); plot.window(xlim = c(0.4, 3.6), ylim = stats::quantile(cc$marker, c(0.005, 0.995)) + c(-0.1, 0.1) * m$sd)
      axis(2); axis(1, at = 1:3, labels = sprintf("%d (n %s)", 0:2, format(have, big.mark = ",")), tick = FALSE, cex.axis = 0.85)
      cols <- NL$col[c("g0", "g1", "g2")]
      for (k in 0:2) {
        v <- cc$marker[cc$g == k]
        if (length(v) > 400) v <- sample(v, 400)
        if (length(v)) points(k + 1 + stats::runif(length(v), -0.22, 0.22), v, pch = 19, cex = 0.45, col = paste0(cols[k + 1], "66"))
        if (!is.na(mu[k + 1])) segments(k + 0.65, mu[k + 1], k + 1.35, mu[k + 1], col = "#ffffff", lwd = 3)
      }
      mtext_fit(sprintf("%s (%s) by broken copies", m$name, m$unit), side = 3, line = 1, adj = 0, col = NL$col[["pick"]], cex = 0.95, font = 2)
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      cc <- cohort()
      t <- TARGETS[[cc$target]]
      o <- odds(cc)
      mu <- tapply(cc$marker, factor(cc$g, 0:2), mean)
      d1 <- mu[2] - mu[1]
      or_txt <- function(k) if (is.na(o["or", k])) "-" else sprintf("%.2f", o["or", k])
      sig <- function(k) !is.na(o["hi", k]) && (o["hi", k] < 1 || o["lo", k] > 1)
      tags$div(
        class = "tiles",
        tile(format(sum(cc$g == 1), big.mark = ","), "carry one broken copy"),
        tile(format(sum(cc$g == 2), big.mark = ","), "human knockouts", if (sum(cc$g == 2) > 0) "tone-good"),
        tile(if (is.na(d1)) "-" else sprintf("%+.1f", d1), paste(t$marker$name, t$marker$unit, "per copy")),
        tile(or_txt(1), paste("odds of", t$disease$name, "(1 copy)"), if (sig(1)) "tone-good"),
        tile(or_txt(2), "odds with 2 copies", if (sig(2)) "tone-good"),
        tile(sum(abs(scan()$beta) > 0 & scan()$p < 0.05 / length(PHENOME)), "traits moved")
      )
    })
  })
}

# ── pipeline ─────────────────────────────────────────────────────────────────

# Phase-to-phase success for drugs without genetic support (BIO, Informa and
# QLS, "Clinical Development Success Rates 2011-2020", 2021: about 8% of
# programmes entering phase I are approved).
PHASES <- c("Phase I", "Phase II", "Phase III", "Review", "Approved")
P_BASE <- c(0.52, 0.29, 0.58, 0.91)

# Genetic support helps where drugs fail for want of efficacy: phase II and III.
# Find the multiplier on their success odds that makes overall approval `rs`
# times as likely.
supported_p <- function(rs) {
  boost <- function(k) { p <- P_BASE; o <- p[2:3] / (1 - p[2:3]) * k; p[2:3] <- o / (1 + o); p }
  if (rs <= 1) return(P_BASE)
  k <- stats::uniroot(function(k) prod(boost(k)) / prod(P_BASE) - rs, c(1, 1e4))$root
  boost(k)
}

ui_pipeline <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, sliderInput, actionButton, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("funnel"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Genetics in the pipeline"),
      tags$p(class = "story", "More than nine in ten drugs that reach people fail. Targets backed by human genetics are approved 2.6 times as often."),
      uiOutput(ns("tiles")),
      sliderInput(ns("programs"), "Programmes entering phase I", 100, 2000, 1000, step = 100, width = "100%"),
      sliderInput(ns("share"), "With genetic support, %", 0, 100, 10, step = 5, width = "100%"),
      sliderInput(ns("rs"), "Relative success of supported targets", 1, 4, 2.6, step = 0.1, width = "100%"),
      sliderInput(ns("cost"), "Cost of a phase III programme, $M", 50, 500, 250, step = 10, width = "100%"),
      actionButton(ns("again"), "Run the portfolio again", class = "btn btn-outline-light btn-sm w-100")
    )
  )
}

server_pipeline <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, reactive, req, renderPlot, renderUI, tags],
    graphics[plot.new, plot.window, axis, rect, text, mtext, legend]
  )
  moduleServer(id, function(input, output, session) {
    run <- reactive({
      req(input$programs, !is.null(input$share), input$rs)
      set.seed(7 + input$again)
      n <- input$programs; ng <- round(n * input$share / 100)
      walk <- function(m, p) { out <- m; for (k in seq_along(p)) { m <- stats::rbinom(1, m, p[k]); out <- c(out, m) }; out }
      list(with = walk(ng, supported_p(input$rs)), without = walk(n - ng, P_BASE), ps = supported_p(input$rs))
    })

    output$funnel <- renderPlot({
      r <- run()
      tot <- r$with + r$without
      ink(c(4, 4.6, 3.6, card_room(session, "funnel")))
      plot.new(); plot.window(xlim = c(0.5, 5.5), ylim = c(0, max(tot) * 1.12), yaxs = "i")
      axis(2); axis(1, at = 1:5, labels = PHASES, tick = FALSE)
      w <- 0.36
      rect(1:5 - w, 0, 1:5 + w, r$without, col = paste0(NL$col[["without"]], "aa"), border = NA)
      rect(1:5 - w, r$without, 1:5 + w, tot, col = paste0(NL$col[["with"]], "cc"), border = NA)
      text(1:5, tot, format(tot, big.mark = ","), pos = 3, col = "#e6e9ef", font = 2)
      legend("topright", inset = c(0.02, 0.02), bty = "n", text.col = "#dfe4ee", fill = c(NL$col[["with"]], NL$col[["without"]]),
             border = NA, legend = c("genetically supported", "not supported"))
      mtext_fit("programmes still alive at each stage", side = 3, line = 1.6, adj = 0, col = NL$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit(sprintf("supported targets pass phase II %.0f%% and phase III %.0f%% of the time, against %.0f%% and %.0f%%",
                    100 * r$ps[2], 100 * r$ps[3], 100 * P_BASE[2], 100 * P_BASE[3]),
            side = 3, line = 0.3, adj = 0, col = "#c3cad8", cex = 0.9)
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      r <- run()
      appr <- r$with[5] + r$without[5]
      n <- input$programs
      # Spend: phase I $25M, phase II $60M, phase III as set, review $5M.
      spend <- sum((r$with[1:4] + r$without[1:4]) * c(25, 60, input$cost, 5))
      tags$div(
        class = "tiles",
        tile(appr, "drugs approved"),
        tile(paste0(round(100 * (1 - appr / n)), "%"), "of programmes fail", "tone-bad"),
        tile(if (r$with[1] > 0) paste0(round(100 * r$with[5] / r$with[1], 1), "%") else "-", "approved, supported", "tone-good"),
        tile(if (r$without[1] > 0) paste0(round(100 * r$without[5] / r$without[1], 1), "%") else "-", "approved, not supported"),
        tile(if (appr > 0) paste0("$", format(round(spend / appr), big.mark = ","), "M") else "-", "spent per approval")
      )
    })
  })
}

# ── ancestry ─────────────────────────────────────────────────────────────────

# Power to find a variant's effect on its biomarker in n people: the
# non-centrality of a 1-df test is n x (genotype variance) x (effect in SD)^2.
power_at <- function(n, q, b1, b2, alpha) {
  p1 <- 2 * q * (1 - q); p2 <- q^2
  m <- p1 * b1 + p2 * b2
  v <- p1 * (b1 - m)^2 + p2 * (b2 - m)^2 + (1 - p1 - p2) * m^2
  ncp <- n * v
  stats::pchisq(stats::qchisq(1 - alpha, 1), 1, ncp = ncp, lower.tail = FALSE)
}

ui_ancestry <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, selectInput, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("power"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Who gets sequenced"),
      tags$p(class = "story", "A variant common in one ancestry can be all but absent in another. Nearly 90% of UK Biobank is European; PCSK9 was found through African Americans."),
      uiOutput(ns("tiles")),
      selectInput(ns("target"), "Variant", target_choices, selected = "PCSK9", width = "100%"),
      selectInput(ns("alpha"), "Evidence required",
                  c("genome-wide, p < 5e-8" = 5e-8, "exome-wide, p < 2.5e-6" = 2.5e-6, "one planned test, p < 0.05" = 0.05),
                  selected = 5e-8, width = "100%")
    )
  )
}

server_ancestry <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, reactive, req, renderPlot, renderUI, tags],
    graphics[plot.new, plot.window, axis, lines, abline, mtext, text, points]
  )
  moduleServer(id, function(input, output, session) {
    curves <- reactive({
      t <- TARGETS[[req(input$target)]]
      b <- t$marker$eff / t$marker$sd
      a <- as.numeric(req(input$alpha))
      n <- 10^seq(3, 8, length.out = 240)
      pw <- sapply(names(NL$pops), function(p) power_at(n, t$q[[p]], b[1], b[2], a))
      need <- sapply(names(NL$pops), function(p) {
        f <- function(x) power_at(10^x, t$q[[p]], b[1], b[2], a) - 0.8
        if (f(9) < 0) Inf else 10^stats::uniroot(f, c(1, 9))$root
      })
      list(n = n, pw = pw, need = need, t = t)
    })

    output$power <- renderPlot({
      x <- curves()
      ink(c(4, 4.6, 3.6, card_room(session, "power")))
      plot.new(); plot.window(xlim = c(3, 8), ylim = c(0, 1), yaxs = "i")
      axis(1, at = 3:8, labels = c("1k", "10k", "100k", "1M", "10M", "100M"))
      axis(2, at = seq(0, 1, 0.2), labels = paste0(seq(0, 100, 20), "%"))
      abline(h = 0.8, col = "#ffffff40", lty = 2)
      abline(v = log10(c(5e5, 1e6)), col = "#ffffff20", lty = 3)
      text(log10(c(5e5, 1e6)), 0.03, c("UK Biobank", "All of Us"), srt = 90, adj = 0, col = "#8a93a6", cex = 0.8)
      for (p in names(NL$pops)) {
        lines(log10(x$n), x$pw[, p], col = NL$pop_col[[p]], lwd = 3)
        if (is.finite(x$need[[p]]) && x$need[[p]] <= 1e8) points(log10(x$need[[p]]), 0.8, pch = 21, bg = NL$pop_col[[p]], col = "#ffffff", cex = 1.6)
      }
      lab <- sapply(names(NL$pops), function(p) sprintf("%s  (%.2g%% of chromosomes)", NL$pops[[p]], 100 * x$t$q[[p]]))
      graphics::legend("topleft", inset = c(0.01, 0.02), bty = "n", lwd = 3, col = NL$pop_col, legend = lab, text.col = "#dfe4ee")
      mtext_fit(sprintf("chance of finding %s's effect on %s, by people sequenced", input$target, x$t$marker$name),
            side = 3, line = 1.6, adj = 0, col = NL$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("dots: the cohort size that finds it 4 times in 5", side = 3, line = 0.3, adj = 0, col = "#c3cad8", cex = 0.9)
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      x <- curves()
      ord <- order(x$need)
      best <- names(x$need)[ord[1]]; worst <- names(x$need)[ord[length(ord)]]
      shown <- function(v) if (is.finite(v)) fmt_n(v) else "never"
      tags$div(
        class = "tiles",
        tile(shown(x$need[[best]]), paste("people needed,", NL$pops[[best]]), "tone-good"),
        tile(shown(x$need[[worst]]), paste("people needed,", NL$pops[[worst]]), "tone-bad"),
        tile(if (is.finite(x$need[[worst]])) paste0(round(x$need[[worst]] / x$need[[best]]), "x") else "-", "more sequencing in the hardest ancestry"),
        tile(shown(x$need[["EUR"]]), "people needed, European")
      )
    })
  })
}

# ── knockouts ────────────────────────────────────────────────────────────────

# Every gene's pooled loss-of-function allele frequency: most genes cannot
# tolerate loss and carry only ultra-rare broken alleles; a few in 20 carry
# common ones. Fitted so the model gives about 1,200 genes knocked out in
# 10,000 Pakistanis (reported: 1,317) and about 5,000 in 80,000 (reported:
# more than 5,000).
GENE_Q <- local({
  set.seed(2017)
  n <- 18000; k <- round(0.05 * n)
  pmin(c(10^stats::rnorm(n - k, -4.2, 0.45), 10^stats::rnorm(k, -2.7, 0.45)), 0.05)
})
p_ko <- function(q, F) q^2 + F * q * (1 - q)
genes_knocked_out <- function(N, F) vapply(N, function(n) sum(1 - (1 - p_ko(GENE_Q, F))^n), 0)
# How many people with unrelated parents it takes to knock out as many genes
# as `N` people at inbreeding `F`: the grey curve reaching the yellow one.
outbred_for <- function(N, F) {
  g <- genes_knocked_out(N, F)
  if (genes_knocked_out(1e12, 0) < g) return(Inf)
  10^stats::uniroot(function(x) genes_knocked_out(10^x, 0) - g, c(log10(N), 12))$root
}

# The cohorts the essay reports, as (people, genes with a knockout).
REPORTED <- data.frame(label = c("Pakistan, first 10,000 (2017)", "Pakistan, 80,000 (2022)"),
                       n = c(1e4, 8e4), genes = c(1317, 5000))

ui_knockouts <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, sliderInput, selectInput, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("ko"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Human knockouts"),
      tags$p(class = "story", "A child of cousins can inherit the same rare broken gene from both sides. That makes knockouts thousands of times commoner than in an outbred population."),
      uiOutput(ns("tiles")),
      sliderInput(ns("cousins"), "Marriages between first cousins, %", 0, 80, 60, step = 5, width = "100%"),
      sliderInput(ns("endogamy"), "Community endogamy, extra F x 1000", 0, 30, 10, step = 1, width = "100%"),
      selectInput(ns("n"), "Cohort size", c("10,000" = 1e4, "80,000" = 8e4, "500,000" = 5e5, "1 million" = 1e6),
                  selected = 8e4, width = "100%"),
      sliderInput(ns("odds"), "A knockout's odds when parents are unrelated: 1 in ... million", 1, 100, 30, step = 1, width = "100%")
    )
  )
}

server_knockouts <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(
    shiny = shiny[moduleServer, reactive, req, renderPlot, renderUI, tags],
    graphics[plot.new, plot.window, axis, lines, points, abline, mtext, text, legend]
  )
  moduleServer(id, function(input, output, session) {
    F_pop <- reactive(input$cousins / 100 / 16 + input$endogamy / 1000)

    output$ko <- renderPlot({
      F <- F_pop()
      n <- 10^seq(3, 7, length.out = 80)
      a <- genes_knocked_out(n, 0); b <- genes_knocked_out(n, F)
      N <- as.numeric(req(input$n))
      ink(c(4, 4.8, 3.6, card_room(session, "ko")))
      plot.new(); plot.window(xlim = c(3, 7), ylim = c(0, max(b) * 1.08), yaxs = "i")
      axis(1, at = 3:7, labels = c("1k", "10k", "100k", "1M", "10M")); axis(2)
      lines(log10(n), a, col = NL$col[["without"]], lwd = 3)
      lines(log10(n), b, col = NL$col[["pick"]], lwd = 3)
      abline(v = log10(N), col = "#ffffff30", lty = 2)
      points(log10(REPORTED$n), REPORTED$genes, pch = 23, bg = NL$col[["bad"]], col = "#ffffff", cex = 1.6)
      text(log10(REPORTED$n), REPORTED$genes, REPORTED$label, pos = 4, col = "#e6e9ef", cex = 0.85)
      legend("topleft", inset = c(0.01, 0.02), bty = "n", lwd = 3, col = c(NL$col[["pick"]], NL$col[["without"]], NA),
             pch = c(NA, NA, 23), pt.bg = c(NA, NA, NL$col[["bad"]]), text.col = "#dfe4ee",
             legend = c(sprintf("this population (F = %.3f)", F), "parents unrelated (F = 0)", "reported in the essay"))
      mtext_fit("genes with at least one person who has both copies broken", side = 3, line = 1.6, adj = 0,
            col = NL$col[["ink"]], cex = 1.1, font = 2)
      mtext_fit("x: people sequenced; the model's 18,000 genes, most of which tolerate no loss", side = 3, line = 0.3, adj = 0, col = "#c3cad8", cex = 0.9)
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      F <- F_pop()
      N <- as.numeric(req(input$n))
      q <- sqrt(1 / (input$odds * 1e6))
      one <- 1 / p_ko(q, F)
      tot_F <- N * sum(p_ko(GENE_Q, F))
      need <- outbred_for(N, F)
      tags$div(
        class = "tiles",
        tile(paste("1 in", fmt_n(one)), "that knockout's odds here", "tone-good"),
        tile(paste0(round(input$odds * 1e6 / one), "x"), "as likely as with unrelated parents"),
        tile(format(round(genes_knocked_out(N, F)), big.mark = ","), paste("genes knocked out in", fmt_n(N))),
        tile(format(round(tot_F), big.mark = ","), "knockouts in all"),
        tile(if (is.finite(need)) fmt_n(need) else "never", "people with unrelated parents for as many genes"),
        tile(format(round(genes_knocked_out(N, 0)), big.mark = ","), "genes, same size, unrelated parents")
      )
    })
  })
}

# ── the site ─────────────────────────────────────────────────────────────────

# Full-window stages with glass cards, one per branch, in the essay's order.
STAGE_CSS <- "
.stage { position: relative; height: calc(100vh - 56px); min-height: 560px; overflow: hidden;
  background: radial-gradient(ellipse at 40% 30%, #1b1638 0%, #0a0a18 55%, #04040a 100%); }
.stage + .stage { border-top: 1px solid rgba(255, 255, 255, 0.08); }
.stage > .shiny-plot-output { position: absolute; inset: 0; }
.glass { background: rgba(10, 13, 22, 0.82); border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: 14px; backdrop-filter: blur(8px); color: #e6e9ef; }
.stage-card { position: absolute; top: 16px; right: 16px; width: 360px; max-height: calc(100% - 32px);
  overflow: auto; padding: 12px 16px; }
.stage-card summary { font-size: 1.2rem; font-weight: 600; cursor: pointer; }
.stage-card[open] summary { margin-bottom: 8px; }
.stage-card .story { font-size: 0.95rem; line-height: 1.4; color: #e6e9ef; margin: 0 0 10px; }
.stage-card .credit { font-size: 0.85rem; color: #d3d9e6; margin: 10px 0 0; }
.stage-card .credit a { color: #F0DC78; }
.stage-plot { position: absolute; left: 76px; bottom: 150px; width: min(400px, 34%); padding: 6px; }
.tiles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin: 4px 0 12px; }
.tile { background: rgba(255, 255, 255, 0.06); border-radius: 10px; padding: 7px 10px; }
.tile b { display: block; font-size: 1.3rem; font-weight: 600; color: #fff; font-variant-numeric: tabular-nums; }
.tile span { font-size: 0.88rem; color: #d3d9e6; }
.tile.tone-good b { color: #3DDC97; }
.tile.tone-bad b { color: #FF7A7E; }
@media (max-width: 760px) {
  .stage-card { top: auto; bottom: 12px; left: 12px; right: 12px; width: auto; max-height: 46%; }
  .stage:has(.stage-card[open]) > .shiny-plot-output { bottom: 48%; }
  .stage:has(.stage-plot) > .shiny-plot-output { top: 150px; }
  .stage-plot { top: 8px; bottom: auto; left: 8px; right: 8px; width: auto; padding: 2px; }
  .stage-plot .shiny-plot-output { height: 136px !important; }
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
      ui_trial(ns("trial")),
      ui_pipeline(ns("pipeline")),
      ui_ancestry(ns("ancestry")),
      ui_knockouts(ns("knockouts"))
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell, ./r/log)
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    log$log_session(session)
    log$log_info("__HOST__: module server starting")
    server_trial("trial")
    server_pipeline("pipeline")
    server_ancestry("ancestry")
    server_knockouts("knockouts")
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
