# Model Workbench · __HOST__
A Shiny site, started from Cove's template: regression models built term by
term on R's own datasets (mtcars, airquality, ToothGrowth, iris, trees,
swiss, warpbreaks, InsectSprays). Pick a response and predictors, add logs,
polynomials and interactions, and R writes the formula, fits it, checks it
and compares it with the models before it. Change first: `DATASETS` - add a
data frame of your own with its default response and predictors, and every
view models it.

R's strength on show: the formula interface and model objects. One formula
drives `lm`/`glm`, the model matrix, `predict()` in cross-validation,
`anova()` between models and every diagnostic; nothing re-implements a fit.

## Files
- module.r - the whole site, a Shiny module (the ndexr module contract:
  `ui_module` / `server_module`, wrapped in `measured`). Four branches, each a
  `ui_`/`server_` pair:
  - `model` - the formula builder (or type one), the fit (lm for gaussian,
    glm for binomial and poisson), the four plot.lm diagnostics (residuals vs
    fitted, normal Q-Q, scale-location, leverage with Cook's distance), and
    R2, AIC, BIC and 10-fold cross-validated RMSE
  - `terms` - every coefficient with its 95% interval (t for lm, Wald for
    glm; odds or rate ratios on request) as a table and a forest plot, and an
    added-variable plot per model-matrix column, whose slope is the coefficient
  - `compare` - models kept as you go: AIC, BIC, adjusted R2 or deviance
    explained, CV RMSE, and the F test (lm) or likelihood-ratio test (glm) of
    each against the row above when one is nested in the other. Nesting is
    measured on the model matrices, so `mpg ~ wt` is found inside
    `mpg ~ poly(wt, 2)`.
  - `env` - a folded Environment card read live from the running R: version,
    R home, the stack module from LOADEDMODULES, each package's version, the
    process start time and the CPU count
  The diagnostics fill the window under the site's header, the controls in a
  glass card that folds to its title (a `<details>`; on a phone it starts
  folded). Save module.r and reload: the router picks the change up, no deploy.
- app.env, build.sh - a module app (no port of its own); build.sh checks that
  module.r parses

A typed formula is code, and it runs in the shared R process: `safe_formula()`
lets through only the dataset's columns, numbers and arithmetic, `I`, `log`,
`exp`, `sqrt`, `abs`, `poly` and `offset`. Each dataset is cut to complete
cases, so all its models see the same rows and AIC and `anova()` compare like
with like. Cross-validation folds are fixed (seed 1) so every model meets the
same ones; a log response is scored on the log scale.

Checks: `lm(mpg ~ wt, mtcars)` gives slope -5.3445 and R2 0.7528;
`glm(am ~ wt, binomial)` gives 12.040 and -4.024.

Logging: r/log.r (log$log_info, log_fn in every ui_/server_), one format across languages; LOG_LEVEL=DEBUG for more.

## Built on
Base R (stats, graphics, datasets) and Shiny only, from the stack's R; no
download, key or database.
Chambers, J. M. and Hastie, T. J. (1992), "Statistical Models in S" - the
formula language and model objects R inherited.
Wilkinson, G. N. and Rogers, C. E. (1973), "Symbolic Description of Factorial
Models for Analysis of Variance", Applied Statistics 22(3).
Nelder, J. A. and Wedderburn, R. W. M. (1972), "Generalized Linear Models",
J. R. Stat. Soc. A 135(3).
Cook, R. D. (1977), "Detection of Influential Observation in Linear
Regression", Technometrics 19(1).
Akaike, H. (1974), "A New Look at the Statistical Model Identification",
IEEE Trans. Automatic Control 19(6).

## Auth
You and the people you share it with (Project › Access sets who; a new site is never public).

## Open
- (none logged)
