# __HOST__/module.r
#
# Model Workbench: regression models built term by term on R's own datasets.
#
#   model     pick a response and predictors, add logs, polynomials and
#             interactions; R writes the formula (or type your own), fits lm
#             or glm (gaussian, binomial, poisson), and draws the four
#             diagnostic plots of plot.lm, residuals to Cook's distance.
#   terms     every coefficient with its 95% interval, as a table and a
#             forest plot, and an added-variable plot for each term: the
#             partial relationship the coefficient measures.
#   compare   models kept as you go, side by side: AIC, BIC, adjusted R2 or
#             deviance explained, 10-fold cross-validated RMSE, and the F or
#             likelihood-ratio test of each model against the one before it
#             whenever one is nested in the other.
#   env       what runs: R, the stack module, each package and its version.
#
# R's strength on show: the formula interface and model objects. One formula
# drives the fit, the model matrix, predict() in cross-validation, anova()
# between models and the diagnostics; nothing here re-implements a fit.

WB <- list(col = c(pt = "#5AC8FA", fit = "#FF9650", good = "#3DDC97", bad = "#FF7A7E", ink = "#e6e9ef"))

# Built-in datasets, complete cases only, so every model of one dataset sees
# the same rows and AIC, BIC and anova() compare like with like.
DATASETS <- list(
  mtcars      = list(y = "mpg", x = "wt", family = "gaussian"),
  airquality  = list(y = "Ozone", x = c("Temp", "Wind"), family = "gaussian"),
  ToothGrowth = list(y = "len", x = c("supp", "dose"), family = "gaussian"),
  iris        = list(y = "Sepal.Length", x = c("Petal.Length", "Species"), family = "gaussian"),
  trees       = list(y = "Volume", x = c("Girth", "Height"), family = "gaussian"),
  swiss       = list(y = "Fertility", x = c("Education", "Agriculture"), family = "gaussian"),
  warpbreaks  = list(y = "breaks", x = c("wool", "tension"), family = "poisson"),
  InsectSprays = list(y = "count", x = "spray", family = "poisson")
)

load_data <- function(name) {
  d <- get(name, envir = asNamespace("datasets"))
  d <- stats::na.omit(as.data.frame(d))
  attr(d, "na.action") <- NULL
  d
}

is_binary <- function(v) length(unique(v)) == 2
is_count <- function(v) is.numeric(v) && all(v >= 0) && all(v == round(v))

# A response can be any numeric column or a two-level factor.
responses <- function(d) names(d)[vapply(d, function(v) is.numeric(v) || (is.factor(v) && nlevels(v) == 2), TRUE)]

families_for <- function(v) {
  c(if (is.numeric(v)) "gaussian", if (is_binary(v)) "binomial", if (is_count(v) && !is_binary(v)) "poisson")
}

default_family <- function(name, y, v) {
  fam <- families_for(v)
  if ("binomial" %in% fam) "binomial"
  else if (y == DATASETS[[name]]$y && DATASETS[[name]]$family %in% fam) DATASETS[[name]]$family
  else fam[1]
}

# ── the formula ──────────────────────────────────────────────────────────────

# The builder's choices as formula text: log() on chosen positive predictors,
# poly() on numeric ones, (a + b + c)^2 for every pairwise interaction.
build_formula <- function(d, y, x, logs = character(), degree = 1, pairwise = FALSE, log_y = FALSE) {
  positive <- function(v) is.numeric(v) && all(v > 0)
  lhs <- if (log_y && positive(d[[y]])) sprintf("log(%s)", y) else y
  x <- setdiff(intersect(x, names(d)), y)
  if (!length(x)) return(paste(lhs, "~ 1"))
  term <- function(v) {
    t <- if (v %in% logs && positive(d[[v]])) sprintf("log(%s)", v) else v
    if (degree > 1 && is.numeric(d[[v]]) && length(unique(d[[v]])) > degree) sprintf("poly(%s, %d)", t, degree) else t
  }
  rhs <- paste(vapply(x, term, ""), collapse = " + ")
  if (pairwise && length(x) > 1) rhs <- sprintf("(%s)^2", rhs)
  paste(lhs, "~", rhs)
}

# A typed formula runs inside the shared R process, and a formula is code:
# `mpg ~ I(system("..."))` would run. Only these functions and the dataset's
# own columns get through.
FORMULA_FUNS <- c("~", "+", "-", "*", "/", ":", "^", "(", "%in%", "I", "log", "log2", "log10", "log1p",
                  "exp", "sqrt", "abs", "poly", "offset")

safe_formula <- function(text, cols) {
  if (!is.character(text) || length(text) != 1 || nchar(text) > 400) stop("a formula of up to 400 characters")
  e <- tryCatch(str2lang(text), error = function(err) stop("does not parse: ", conditionMessage(err)))
  walk <- function(x) {
    if (is.symbol(x)) {
      s <- as.character(x)
      if (!(s %in% c(cols, "."))) stop(sprintf("'%s' is not a column of this dataset", s))
    } else if (is.call(x)) {
      f <- x[[1]]
      if (!is.symbol(f) || !(as.character(f) %in% FORMULA_FUNS))
        stop(sprintf("'%s' is not allowed in a formula here", deparse(f)[1]))
      for (a in as.list(x)[-1]) walk(a)
    } else if (!(is.numeric(x) || is.logical(x))) stop("only columns, numbers and TRUE/FALSE")
  }
  walk(e)
  if (!is.call(e) || !identical(e[[1]], as.symbol("~")) || length(e) != 3) stop("write it as response ~ terms")
  stats::as.formula(e, env = FORMULA_ENV)
}

# The formula's environment: base R plus the two stats functions allowed
# above, and nothing of the session's.
FORMULA_ENV <- list2env(list(poly = stats::poly, offset = stats::offset), parent = baseenv())

# ── the fit ──────────────────────────────────────────────────────────────────

# lm for gaussian (summary gives R2 and the F test), glm otherwise. Warnings
# (fitted probabilities of 0 or 1, say) are kept to show, not printed.
fit_model <- function(f, family, data) {
  warns <- character()
  fit <- withCallingHandlers(
    if (family == "gaussian") stats::lm(f, data = data)
    else stats::glm(f, family = get(family, envir = asNamespace("stats"))(), data = data),
    warning = function(w) { warns <<- c(warns, conditionMessage(w)); invokeRestart("muffleWarning") }
  )
  attr(fit, "warns") <- unique(warns)
  fit
}

fam_of <- function(fit) if (inherits(fit, "glm")) stats::family(fit)$family else "gaussian"

# The response as numbers: a two-level factor counts its second level as 1,
# as glm's binomial does.
response_of <- function(mf) {
  y <- stats::model.response(mf)
  if (is.factor(y)) as.numeric(y != levels(y)[1]) else as.numeric(y)
}

fit_stats <- function(fit) {
  k <- sum(!is.na(stats::coef(fit)))
  out <- list(n = stats::nobs(fit), k = k, aic = stats::AIC(fit), bic = stats::BIC(fit))
  if (inherits(fit, "glm")) {
    out$r2 <- 1 - fit$deviance / fit$null.deviance
    out$adj <- NA_real_
    out$sigma <- NA_real_
  } else {
    s <- summary(fit)
    out$r2 <- s$r.squared; out$adj <- s$adj.r.squared; out$sigma <- s$sigma
  }
  out
}

# Estimate, standard error, statistic, p-value and 95% interval per
# coefficient: t-based for lm, Wald for glm. With `exp`, a glm's log-odds or
# log-rates become odds or rate ratios.
coef_table <- function(fit, exp = FALSE) {
  s <- stats::coef(summary(fit))
  ci <- if (inherits(fit, "glm")) stats::confint.default(fit) else stats::confint(fit)
  ci <- ci[rownames(s), , drop = FALSE]
  out <- data.frame(term = rownames(s), estimate = s[, 1], se = s[, 2], stat = s[, 3], p = s[, 4],
                    lo = ci[, 1], hi = ci[, 2], row.names = NULL, check.names = FALSE)
  if (exp && inherits(fit, "glm")) {
    out$estimate <- base::exp(out$estimate); out$lo <- base::exp(out$lo); out$hi <- base::exp(out$hi)
  }
  out
}

# k-fold cross-validated RMSE on the response scale of the formula's left
# side (log(y) is scored on the log scale). Folds are fixed (seed 1, the
# caller's random stream left as it was), so two models meet the same folds.
cv_rmse <- function(f, family, data, k = 10) {
  n <- nrow(data); k <- min(k, n)
  had <- exists(".Random.seed", envir = globalenv())
  if (had) old <- get(".Random.seed", envir = globalenv())
  set.seed(1)
  folds <- sample(rep_len(seq_len(k), n))
  if (had) assign(".Random.seed", old, envir = globalenv()) else rm(".Random.seed", envir = globalenv())
  err <- numeric()
  for (i in seq_len(k)) {
    test <- data[folds == i, , drop = FALSE]
    e <- tryCatch({
      fit <- suppressWarnings(fit_model(f, family, data[folds != i, , drop = FALSE]))
      pred <- stats::predict(fit, newdata = test, type = "response")
      # The left side alone: model.frame() on a fold would also refit poly()
      # to a few rows, which fails when the fold has fewer distinct values.
      y <- eval(f[[2]], test, environment(f))
      (if (is.factor(y)) as.numeric(y != levels(y)[1]) else as.numeric(y)) - pred
    }, error = function(err) {
      box::use(./r/log)
      log$log_debug("cross-validation fold {i} of {k} failed: {conditionMessage(err)}")
      NULL
    })
    if (is.null(e)) return(NA_real_)  # a factor level missing from a fold
    err <- c(err, e)
  }
  sqrt(mean(err^2))
}

# Added-variable plots for lm and glm alike: at a fit's last IRLS step a glm
# is a weighted least-squares fit of its working response, so regressing that
# response and one column on all the others leaves two residual vectors whose
# weighted slope is that column's coefficient, exactly.
av_data <- function(fit, max = 12) {
  X <- stats::model.matrix(fit)
  X <- X[, !is.na(stats::coef(fit)), drop = FALSE]
  w <- if (is.null(fit$weights)) rep(1, nrow(X)) else fit$weights
  z <- if (inherits(fit, "glm")) fit$linear.predictors + fit$residuals else stats::fitted(fit) + stats::residuals(fit)
  cols <- setdiff(colnames(X), "(Intercept)")
  lapply(utils::head(cols, max), function(j) {
    others <- X[, colnames(X) != j, drop = FALSE]
    res <- function(v) if (ncol(others)) stats::lm.wfit(others, v, w)$residuals else v
    rx <- res(X[, j]); ry <- res(z)
    list(term = j, x = rx, y = ry, w = w, slope = sum(w * rx * ry) / sum(w * rx^2))
  })
}

# Is `small` nested in `big`: same rows, same response and family, and every
# column of small's model matrix inside the span of big's. Measured on the
# matrices, so wt is found inside poly(wt, 2) though the labels differ.
nested_in <- function(small, big) {
  if (fam_of(small) != fam_of(big) || stats::nobs(small) != stats::nobs(big)) return(FALSE)
  if (!isTRUE(all.equal(response_of(stats::model.frame(small)), response_of(stats::model.frame(big))))) return(FALSE)
  Xs <- stats::model.matrix(small); Xb <- stats::model.matrix(big)
  qb <- qr(Xb)
  if (qr(Xs)$rank >= qb$rank) return(FALSE)
  max(abs(qr.resid(qb, Xs))) < 1e-7 * max(1, abs(Xs))
}

# The test between two nested models: anova()'s F test for lm, its
# likelihood-ratio (chi-squared) test for glm.
nested_test <- function(a, b) {
  if (nested_in(a, b)) { small <- a; big <- b } else if (nested_in(b, a)) { small <- b; big <- a } else return(NULL)
  if (inherits(big, "glm")) {
    t <- stats::anova(small, big, test = "Chisq")
    list(label = sprintf("LR chi2 = %.2f on %d df", t$Deviance[2], as.integer(t$Df[2])), p = t[["Pr(>Chi)"]][2])
  } else {
    t <- stats::anova(small, big)
    list(label = sprintf("F = %.2f on %d, %d df", t$F[2], as.integer(t$Df[2]), as.integer(t$Res.Df[2])), p = t[["Pr(>F)"]][2])
  }
}

# One row per model, each tested against the row before it.
compare_models <- function(entries) {
  rows <- lapply(seq_along(entries), function(i) {
    e <- entries[[i]]; s <- fit_stats(e$fit)
    tst <- if (i > 1) nested_test(entries[[i - 1]]$fit, e$fit)
    data.frame(model = e$label, formula = e$text, family = e$family, k = s$k, aic = s$aic, bic = s$bic,
               r2 = if (is.na(s$adj)) s$r2 else s$adj, r2_kind = if (is.na(s$adj)) "dev. expl." else "adj. R2",
               cv = e$cv, test = if (is.null(tst)) (if (i > 1) "not nested" else "") else tst$label,
               p = if (is.null(tst)) NA_real_ else tst$p, stringsAsFactors = FALSE)
  })
  do.call(rbind, rows)
}

# ── drawing ──────────────────────────────────────────────────────────────────

ink <- function(mar = c(3.6, 4.4, 3, 1.2)) {
  graphics::par(bg = NA, mar = mar, col.axis = "#c3cad8", col.lab = "#dfe4ee", fg = "#5b6478",
                cex.axis = 0.9, las = 1, mgp = c(2.3, 0.7, 0))
}

title_of <- function(main, sub = NULL) {
  mtext_fit(main, side = 3, line = if (is.null(sub)) 0.6 else 1.5, adj = 0, col = WB$col[["ink"]], cex = 1, font = 2)
  if (!is.null(sub)) mtext_fit(sub, side = 3, line = 0.4, adj = 0, col = "#dfe4ee", cex = 0.85)
}

smooth_line <- function(x, y) {
  ok <- is.finite(x) & is.finite(y)
  if (sum(ok) > 3 && length(unique(x[ok])) > 3) graphics::lines(stats::lowess(x[ok], y[ok]), col = WB$col[["fit"]], lwd = 2)
}

label_top <- function(x, y, score, labels, n = 3) {
  ok <- which(is.finite(score) & is.finite(x) & is.finite(y))
  i <- utils::head(ok[order(-abs(score[ok]))], n)
  if (length(i)) graphics::text(x[i], y[i], labels[i], pos = 4, col = "#dfe4ee", cex = 0.8, xpd = NA)
}

# The four panels of plot.lm (which = c(1, 2, 3, 5)), drawn to match the page.
draw_diagnostics <- function(fit, room = 1.2) {
  graphics::par(mfrow = c(2, 2), oma = c(0, 0, 0.6, room))
  ink(c(3.6, 4.2, 2.6, 1))
  glm <- inherits(fit, "glm")
  yh <- stats::predict(fit)              # the linear predictor, for a glm
  r <- stats::residuals(fit)             # deviance residuals, for a glm
  rs <- stats::rstandard(fit)
  h <- stats::hatvalues(fit)
  cook <- stats::cooks.distance(fit)
  labs <- names(r)
  pt <- function(x, y) graphics::points(x, y, pch = 21, bg = "#5AC8FA55", col = WB$col[["pt"]], cex = 1.1)

  graphics::plot(yh, r, type = "n", xlab = if (glm) "linear predictor" else "fitted", ylab = "residual")
  graphics::abline(h = 0, col = "#ffffff40", lty = 2); pt(yh, r); smooth_line(yh, r); label_top(yh, r, r, labs)
  title_of("Residuals vs fitted", "a curve here is structure the model missed")

  q <- stats::qqnorm(rs, plot.it = FALSE)
  graphics::plot(q$x, q$y, type = "n", xlab = "normal quantile", ylab = "standardised residual")
  graphics::abline(0, 1, col = "#ffffff60", lty = 2); pt(q$x, q$y); label_top(q$x, q$y, rs, labs)
  title_of("Normal Q-Q", "points off the line: tails heavier or lighter")

  sl <- sqrt(abs(rs))
  graphics::plot(yh, sl, type = "n", xlab = if (glm) "linear predictor" else "fitted", ylab = "sqrt |std. residual|")
  pt(yh, sl); smooth_line(yh, sl); label_top(yh, sl, rs, labs)
  title_of("Scale-location", "a rising line: spread grows with the mean")

  rp <- if (glm) stats::rstandard(fit, type = "pearson") else rs
  ok <- is.finite(rp) & h < 1
  p <- sum(!is.na(stats::coef(fit)))
  graphics::plot(h[ok], rp[ok], type = "n", xlim = c(0, max(h[ok]) * 1.08), ylim = range(rp[ok], -2.5, 2.5),
                 xlab = "leverage", ylab = "standardised residual")
  graphics::abline(h = 0, col = "#ffffff40", lty = 2)
  hh <- seq(0.001, max(h[ok]) * 1.08, length.out = 120)
  for (d in c(0.5, 1)) {
    bound <- sqrt(d * p * (1 - hh) / hh)
    graphics::lines(hh, bound, col = WB$col[["bad"]], lty = 3); graphics::lines(hh, -bound, col = WB$col[["bad"]], lty = 3)
  }
  graphics::points(h[ok], rp[ok], pch = 21, cex = 0.8 + 3 * pmin(cook[ok], 1),
                   bg = ifelse(cook[ok] > 0.5, "#FF7A7E88", "#5AC8FA55"), col = ifelse(cook[ok] > 0.5, WB$col[["bad"]], WB$col[["pt"]]))
  label_top(h[ok], rp[ok], cook[ok], labs[ok])
  title_of("Residuals vs leverage", sprintf("dotted: Cook's distance 0.5 and 1; largest %.2f", max(cook[ok])))
}

draw_av <- function(av, room = 1) {
  if (!length(av)) {
    ink(); graphics::plot.new(); title_of("Added-variable plots", "an intercept-only model has no terms to show")
    return(invisible())
  }
  m <- length(av); nc <- ceiling(sqrt(m)); nr <- ceiling(m / nc)
  graphics::par(mfrow = c(nr, nc), oma = c(0, 0, 0, room))
  ink(c(3.2, 3.8, 2.4, 0.8))
  for (a in av) {
    graphics::plot(a$x, a$y, type = "n", xlab = paste(a$term, "| others"), ylab = "response | others")
    graphics::abline(h = 0, v = 0, col = "#ffffff20")
    graphics::points(a$x, a$y, pch = 21, bg = "#5AC8FA55", col = WB$col[["pt"]], cex = 0.9)
    graphics::abline(0, a$slope, col = WB$col[["fit"]], lwd = 2.4)
    title_of(a$term, sprintf("slope %.4g = its coefficient", a$slope))
  }
}

draw_forest <- function(tab, exp = FALSE) {
  tab <- tab[tab$term != "(Intercept)" | nrow(tab) == 1, , drop = FALSE]
  tab <- utils::head(tab, 20)
  m <- nrow(tab); null <- if (exp) 1 else 0
  ink(c(3.4, max(6, min(16, max(nchar(tab$term)) * 0.55)), 2.6, 1.2))
  xl <- range(tab$lo, tab$hi, null, finite = TRUE)
  graphics::plot.new(); graphics::plot.window(xlim = xl, ylim = c(0.5, m + 0.5), log = if (exp) "x" else "")
  graphics::axis(1); graphics::axis(2, at = m:1, labels = tab$term, tick = FALSE, cex.axis = 0.85)
  graphics::abline(v = null, col = "#ffffff60", lty = 2)
  y <- m:1
  sig <- tab$lo > null | tab$hi < null
  graphics::segments(tab$lo, y, tab$hi, y, col = ifelse(sig, WB$col[["good"]], "#c3cad8"), lwd = 2.4)
  graphics::points(tab$estimate, y, pch = 21, cex = 1.5, bg = ifelse(sig, WB$col[["good"]], "#5b6478"), col = "#ffffff")
  title_of(if (exp) "Ratios with 95% intervals" else "Coefficients with 95% intervals",
           "green: the interval leaves out no effect")
}

draw_compare <- function(cmp) {
  graphics::par(mfrow = c(2, 1))
  m <- nrow(cmp)
  dot <- function(v, main, sub, best) {
    ink(c(3, 4.6, 2.8, 1.2))
    ok <- is.finite(v)
    yl <- if (any(ok)) range(v[ok]) + c(-1, 1) * max(diff(range(v[ok])) * 0.12, abs(mean(v[ok])) * 0.01, 1e-6) else c(0, 1)
    graphics::plot.new(); graphics::plot.window(xlim = c(0.5, m + 0.5), ylim = yl)
    graphics::axis(1, at = seq_len(m), labels = cmp$model); graphics::axis(2)
    if (any(ok)) {
      graphics::lines(which(ok), v[ok], col = "#ffffff40", lwd = 1.5)
      b <- which(ok)[best(v[ok])]
      graphics::points(which(ok), v[ok], pch = 21, cex = 2, bg = ifelse(which(ok) == b, WB$col[["good"]], "#5AC8FA88"), col = "#ffffff")
    }
    title_of(main, sub)
  }
  dot(cmp$aic, "AIC", "lower is better; green is the lowest", which.min)
  dot(cmp$cv, "Cross-validated RMSE", "10 folds, the same for every model", which.min)
}

# ── formatting ───────────────────────────────────────────────────────────────

tile <- function(value, label, tone = NULL) {
  box::use(shiny = shiny[tags])
  shiny$tags$div(class = paste("tile", tone), shiny$tags$b(value), shiny$tags$span(label))
}

sig4 <- function(x) ifelse(is.finite(x), formatC(x, digits = 4, format = "fg", flag = "#"), "-")
pval <- function(p) ifelse(is.na(p), "", ifelse(p < 1e-4, "< 0.0001", formatC(p, digits = 3, format = "fg")))

# Lines of right margin that keep a full-window plot clear of the card (380 px plus gaps
# on a desk; a margin line is 19.2 px at res 96).
card_room <- function(session, out) {
  w <- session$clientData[[paste0("output_", session$ns(out), "_width")]]
  if (!is.null(w) && w > 900) 430 / 19.2 else 0.6
}

# ── model ────────────────────────────────────────────────────────────────────

ui_model <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, selectInput, selectizeInput, sliderInput, checkboxInput, textAreaInput,
                         actionButton, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage",
    plotOutput(ns("diag"), height = "100%"),
    tags$details(
      class = "stage-card glass", open = NA,
      tags$summary("Model"),
      tags$div(class = "pair",
        selectInput(ns("data"), "Dataset", names(DATASETS), selected = "mtcars", width = "100%"),
        selectInput(ns("family"), "Family", "gaussian", width = "100%")),
      selectInput(ns("y"), "Response", NULL, width = "100%"),
      selectizeInput(ns("x"), "Predictors", NULL, multiple = TRUE, width = "100%"),
      selectizeInput(ns("logs"), "Log of", NULL, multiple = TRUE, width = "100%"),
      tags$div(class = "pair",
        sliderInput(ns("degree"), "Polynomial degree", 1, 3, 1, step = 1, width = "100%"),
        tags$div(checkboxInput(ns("pairwise"), "Pairwise interactions"), checkboxInput(ns("log_y"), "Log response"))),
      textAreaInput(ns("formula"), "Formula (edit it freely)", "", rows = 2, width = "100%"),
      tags$div(class = "pair",
        actionButton(ns("keep"), "Keep for comparison", class = "btn btn-outline-light btn-sm w-100"),
        actionButton(ns("clear"), "Clear kept", class = "btn btn-outline-light btn-sm w-100")),
      uiOutput(ns("tiles"))
    )
  )
}

# Returns the current model and the kept ones, for the other branches.
server_model <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[moduleServer, reactive, reactiveVal, req, observe, observeEvent, bindEvent, debounce,
                         isolate, validate, need, renderPlot, renderUI, tags, updateSelectInput,
                         updateSelectizeInput, updateTextAreaInput])
  moduleServer(id, function(input, output, session) {
    data <- reactive(load_data(req(input$data)))
    kept <- reactiveVal(list())

    # A new dataset: its response, its default predictors, nothing kept.
    observeEvent(input$data, {
      d <- data(); spec <- DATASETS[[input$data]]
      updateSelectInput(session, "y", choices = responses(d), selected = spec$y)
      updateSelectizeInput(session, "x", choices = setdiff(names(d), spec$y), selected = spec$x)
      kept(list())
    })

    observeEvent(input$y, {
      d <- data(); req(input$y %in% names(d))
      fam <- families_for(d[[input$y]])
      updateSelectInput(session, "family", choices = fam, selected = default_family(input$data, input$y, d[[input$y]]))
      updateSelectizeInput(session, "x", choices = setdiff(names(d), input$y),
                           selected = setdiff(isolate(input$x), input$y))
    })

    observeEvent(list(input$x, input$data), {
      d <- data()
      pos <- Filter(function(v) is.numeric(d[[v]]) && all(d[[v]] > 0), intersect(input$x, names(d)))
      updateSelectizeInput(session, "logs", choices = pos, selected = intersect(isolate(input$logs), pos))
    }, ignoreNULL = FALSE)

    # The builder writes the formula; the text box is what gets fitted, so a
    # hand-typed formula fits too until the builder is touched again.
    observe({
      d <- data(); req(input$y %in% names(d))
      log_y <- isTRUE(input$log_y) && identical(input$family, "gaussian")
      updateTextAreaInput(session, "formula",
                          value = build_formula(d, input$y, input$x, input$logs, input$degree, isTRUE(input$pairwise), log_y))
    })

    formula_text <- debounce(reactive(input$formula), 400)

    current <- reactive({
      d <- data(); fam <- req(input$family); txt <- req(formula_text())
      res <- tryCatch({
        f <- safe_formula(txt, names(d))
        fit <- fit_model(f, fam, d)
        list(fit = fit, f = f, text = txt, family = fam, data = d, name = input$data,
             cv = cv_rmse(f, fam, d), stats = fit_stats(fit))
      }, error = function(e) {
        log$log_error("{txt} ({fam}) did not fit: {conditionMessage(e)}")
        list(error = conditionMessage(e))
      })
      res
    })

    observeEvent(input$keep, {
      m <- current(); req(is.null(m$error))
      k <- kept()
      dup <- vapply(k, function(e) identical(e$text, m$text) && identical(e$family, m$family), TRUE)
      if (!any(dup)) kept(utils::tail(c(k, list(m[c("fit", "text", "family", "cv")])), 8))
    })
    observeEvent(input$clear, kept(list()))

    output$diag <- renderPlot({
      m <- current()
      validate(need(is.null(m$error), m$error))
      draw_diagnostics(m$fit, card_room(session, "diag"))
    }, bg = "transparent", res = 96)

    output$tiles <- renderUI({
      m <- current()
      if (!is.null(m$error)) return(tags$div(class = "tiles", tile("no fit", m$error, "tone-bad")))
      s <- m$stats
      w <- attr(m$fit, "warns")
      tags$div(
        class = "tiles",
        tile(sprintf("%d / %d", s$n, s$k), "rows / parameters"),
        if (is.na(s$adj)) tile(sprintf("%.1f%%", 100 * s$r2), "deviance explained")
        else tile(sprintf("%.4f", s$r2), sprintf("R2, adjusted %.4f", s$adj), if (s$adj > 0.7) "tone-good" else NULL),
        tile(sig4(s$aic), sprintf("AIC   BIC %s", sig4(s$bic))),
        tile(sig4(m$cv), "10-fold CV RMSE"),
        if (length(w)) tags$div(class = "warn", w[1])
      )
    })

    list(current = current, kept = kept)
  })
}

# ── terms ────────────────────────────────────────────────────────────────────

ui_terms <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, plotOutput, uiOutput, checkboxInput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage-duo",
    tags$div(class = "glass split",
      tags$div(class = "split-plot", plotOutput(ns("forest"), height = "100%")),
      tags$div(class = "split-table", checkboxInput(ns("exp"), "glm: show odds or rate ratios (exp)"), uiOutput(ns("table")))),
    tags$div(class = "glass", plotOutput(ns("av"), height = "100%"))
  )
}

server_terms <- function(id, model) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[moduleServer, reactive, renderPlot, renderUI, validate, need, tags])
  moduleServer(id, function(input, output, session) {
    tab <- reactive({
      m <- model$current(); validate(need(is.null(m$error), m$error))
      coef_table(m$fit, isTRUE(input$exp))
    })
    output$forest <- renderPlot(draw_forest(tab(), isTRUE(input$exp) && inherits(model$current()$fit, "glm")),
                                bg = "transparent", res = 96)
    output$table <- renderUI({
      t <- tab(); glm <- inherits(model$current()$fit, "glm")
      tags$table(class = "wb-table",
        tags$thead(tags$tr(lapply(c("term", "estimate", "std. error", if (glm) "z" else "t", "p", "95% interval"), tags$th))),
        tags$tbody(lapply(seq_len(nrow(t)), function(i) tags$tr(
          tags$td(t$term[i]), tags$td(sig4(t$estimate[i])), tags$td(sig4(t$se[i])), tags$td(sprintf("%.2f", t$stat[i])),
          tags$td(class = if (t$p[i] < 0.05) "good", pval(t$p[i])),
          tags$td(sprintf("%s to %s", sig4(t$lo[i]), sig4(t$hi[i])))))))
    })
    output$av <- renderPlot({
      m <- model$current(); validate(need(is.null(m$error), m$error))
      draw_av(av_data(m$fit))
    }, bg = "transparent", res = 96)
  })
}

# ── compare ──────────────────────────────────────────────────────────────────

ui_compare <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, plotOutput, uiOutput])
  ns <- shiny$NS(id)
  tags$section(
    class = "stage-duo",
    tags$div(class = "glass scroll", uiOutput(ns("table"))),
    tags$div(class = "glass", plotOutput(ns("plot"), height = "100%"))
  )
}

server_compare <- function(id, model) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[moduleServer, reactive, renderPlot, renderUI, req, tags])
  moduleServer(id, function(input, output, session) {
    cmp <- reactive({
      k <- model$kept(); m <- model$current()
      entries <- Map(function(e, i) c(e, label = paste0("M", i)), k, seq_along(k))
      if (is.null(m$error) && !any(vapply(k, function(e) identical(e$text, m$text) && identical(e$family, m$family), TRUE)))
        entries <- c(entries, list(c(m[c("fit", "text", "family", "cv")], label = "now")))
      req(length(entries))
      compare_models(entries)
    })
    output$table <- renderUI({
      c <- cmp()
      tags$div(
        tags$h4("Models side by side"),
        tags$table(class = "wb-table",
          tags$thead(tags$tr(lapply(c("", "formula", "family", "k", "AIC", "BIC", "R2", "CV RMSE", "vs the row above", "p"), tags$th))),
          tags$tbody(lapply(seq_len(nrow(c)), function(i) tags$tr(
            tags$td(tags$b(c$model[i])), tags$td(tags$code(c$formula[i])), tags$td(c$family[i]), tags$td(c$k[i]),
            tags$td(sig4(c$aic[i])), tags$td(sig4(c$bic[i])), tags$td(sprintf("%.3f %s", c$r2[i], c$r2_kind[i])),
            tags$td(sig4(c$cv[i])), tags$td(c$test[i]),
            tags$td(class = if (isTRUE(c$p[i] < 0.05)) "good", pval(c$p[i])))))),
        if (nrow(c) < 2) tags$p(class = "hint", "Keep a model, add a term, keep again: each row is tested against the one above it.")
      )
    })
    output$plot <- renderPlot(draw_compare(cmp()), bg = "transparent", res = 96)
  })
}

# ── environment ──────────────────────────────────────────────────────────────

PACKAGES <- c("shiny", "box", "stats", "graphics", "grDevices", "datasets", "utils")

# Read from the running R every time: what serves this page, not what a file
# says should.
env_info <- function() {
  mods <- strsplit(Sys.getenv("LOADEDMODULES"), ":", fixed = TRUE)[[1]]
  list(
    runtime = R.version.string,
    r_home = R.home(),
    module = paste(c(grep("^env/", mods, value = TRUE), grep("^R/", mods, value = TRUE)), collapse = ", "),
    modules = length(mods[nzchar(mods)]),
    packages = vapply(PACKAGES, function(p) tryCatch(as.character(utils::packageVersion(p)), error = function(e) "missing"), ""),
    started = format(Sys.time() - proc.time()[["elapsed"]], "%Y-%m-%d %H:%M:%S %Z"),
    cpus = parallel::detectCores(),
    platform = R.version$platform
  )
}

ui_env <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[NS, tags, uiOutput])
  ns <- shiny$NS(id)
  tags$section(class = "env-wrap",
    tags$details(class = "glass env-card", tags$summary("Environment"), uiOutput(ns("env"))))
}

server_env <- function(id) {
  box::use(./r/log)
  log$log_fn()
  box::use(shiny = shiny[moduleServer, renderUI, tags])
  moduleServer(id, function(input, output, session) {
    output$env <- renderUI({
      e <- env_info()
      row <- function(k, v) tags$tr(tags$th(k), tags$td(v))
      tags$table(class = "wb-table env",
        row("runtime", e$runtime), row("platform", e$platform), row("R home", tags$code(e$r_home)),
        row("stack module", if (nzchar(e$module)) e$module else "none loaded"),
        row("modules loaded", e$modules),
        lapply(names(e$packages), function(p) row(p, e$packages[[p]])),
        row("process started", e$started), row("CPUs", e$cpus))
    })
  })
}

# ── the site ─────────────────────────────────────────────────────────────────

# Full-window stage with a glass card, then two pairs of glass panels side by
# side (stacked on a phone), then the folded Environment card.
STAGE_CSS <- "
.stage { position: relative; height: calc(100vh - 56px); min-height: 560px; overflow: hidden;
  background: radial-gradient(ellipse at 40% 35%, #15233a 0%, #090c16 55%, #04050a 100%); }
.stage > .shiny-plot-output { position: absolute; inset: 0; }
.glass { background: rgba(10, 13, 22, 0.80); border: 1px solid rgba(255, 255, 255, 0.10);
  border-radius: 14px; backdrop-filter: blur(8px); color: #e6e9ef; }
.stage-card { position: absolute; top: 16px; right: 16px; width: 380px; max-height: calc(100% - 32px);
  overflow: auto; padding: 12px 16px; }
.stage-card summary, .env-card summary { font-size: 1.2rem; font-weight: 600; cursor: pointer; }
.stage-card[open] summary { margin-bottom: 8px; }
.stage-card .form-group { margin-bottom: 8px; }
.stage-card textarea { font-family: ui-monospace, monospace; background: rgba(255,255,255,0.06); color: #fff; }
.pair { display: grid; grid-template-columns: 1fr 1fr; gap: 10px; align-items: end; }
.stage-duo { display: grid; grid-template-columns: 1fr 1fr; gap: 16px; padding: 16px;
  height: calc(100vh - 56px); min-height: 560px; background: linear-gradient(180deg, #04050a 0%, #0b1022 100%); }
.stage-duo > .glass { position: relative; padding: 8px; min-height: 0; }
.stage-duo > .glass > .shiny-plot-output { position: absolute; inset: 8px; height: auto !important; }
.split { display: flex; flex-direction: column; }
.split-plot { position: relative; flex: 1 1 55%; min-height: 0; }
.split-plot .shiny-plot-output { position: absolute; inset: 0; height: auto !important; }
.split-table, .scroll { overflow: auto; padding: 6px 10px; }
.split-table { flex: 1 1 45%; }
.wb-table { width: 100%; border-collapse: collapse; font-size: 0.95rem; font-variant-numeric: tabular-nums; }
.wb-table th { color: #c3cad8; font-weight: 600; text-align: left; padding: 4px 8px; border-bottom: 1px solid rgba(255,255,255,0.15); }
.wb-table td { padding: 4px 8px; border-bottom: 1px solid rgba(255,255,255,0.06); color: #e6e9ef; }
.wb-table td.good { color: #3DDC97; font-weight: 600; }
.wb-table code { color: #9fd8ff; background: none; }
.scroll h4 { color: #fff; font-size: 1.1rem; margin: 6px 8px 10px; }
.hint { color: #e6e9ef; margin: 12px 8px; }
.tiles { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 8px; margin-top: 10px; }
.tile { background: rgba(255, 255, 255, 0.06); border-radius: 10px; padding: 7px 10px; }
.tile b { display: block; font-size: 1.3rem; font-weight: 600; color: #fff; font-variant-numeric: tabular-nums; }
.tile span { font-size: 0.9rem; color: #d3d9e6; }
.tile.tone-good b { color: #3DDC97; }
.tile.tone-bad b { color: #FF7A7E; }
.warn { grid-column: 1 / -1; color: #FFB27A; font-size: 0.95rem; }
.env-wrap { padding: 0 16px 16px; background: #0b1022; }
.env-card { padding: 10px 16px; }
.env-card[open] summary { margin-bottom: 8px; }
@media (max-width: 760px) {
  .stage-card { top: auto; bottom: 12px; left: 12px; right: 12px; width: auto; max-height: 52%; }
  .stage:has(.stage-card[open]) > .shiny-plot-output { bottom: 54%; }
  .stage-duo { grid-template-columns: 1fr; height: auto; padding: 10px; gap: 10px; }
  .stage-duo > .glass { height: 420px; }
  .pair { grid-template-columns: 1fr; gap: 0; }
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
      ui_model(ns("model")),
      ui_terms(ns("terms")),
      ui_compare(ns("compare")),
      ui_env(ns("env"))
    )
  )
}

.server_inner <- function(id = "__HOST__", parent_session) {
  box::use(r / shell / shell, ./r/log)
  shell$server_shell(id, parent_session, inner = function(input, output, session) {
    log$log_session(session)
    log$log_info("__HOST__: module server starting")
    model <- server_model("model")
    server_terms("terms", model)
    server_compare("compare", model)
    server_env("env")
  })
}

#' @export
server_module <- local({
  box::use(r / observability / timing[measured])
  wrapped <- measured("__HOST__", .server_inner)
  function(id = "__HOST__", ...) wrapped(id, ...)
})
