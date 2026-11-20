# __HOST__: a model served as an API with vetiver. The model is fitted when
# the server starts; for a model that takes long to fit, pin it once
# (pins::pin_write) and read it here instead.
#
# /predict takes rows of the predictors as JSON; /metadata, /ping and the
# docs at /__docs__/ come with vetiver.

box::use(./r/log)

fit <- lm(mpg ~ wt + hp + cyl, data = mtcars)
v <- vetiver::vetiver_model(fit, "__HOST__", description = "Fuel economy (mpg) from weight, horsepower and cylinders, fitted on mtcars")
log$log_info("model {v$model_name} loaded: {deparse(formula(fit))} on {nrow(mtcars)} rows of mtcars")

plumber::pr() |>
  vetiver::vetiver_api(v) |>
  plumber::pr_get("/", function(res) {
    res$setHeader("Location", "__docs__/")
    res$status <- 302
    ""
  }, serializer = plumber::serializer_text())
