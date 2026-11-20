# __HOST__: a blank Plumber API. run.sh restarts it on every save; the
# interactive docs are at /__docs__/.

#* @apiTitle __HOST__
#* @apiDescription A blank Plumber API, running.

# r/log.r: one log format across Cove's languages (LOG_LEVEL=DEBUG for more).
box::use(./r/log)

#* Every request, one line: method and path.
#* @filter log
function(req) {
  log$log_info("{req$REQUEST_METHOD} {req$PATH_INFO}")
  plumber::forward()
}

#* The page at the root: what this is and where the docs are.
#* @get /
#* @serializer html
function() {
  paste0(
    "<!doctype html><title>__HOST__</title>",
    "<main style='max-width:40rem;margin:3rem auto;font-family:system-ui,sans-serif'>",
    "<h1>__HOST__</h1><p>A blank Plumber API, running. Save a file and it restarts.</p>",
    "<p><a href='__docs__/'>The API's docs</a> · <a href='summary?x=1,2,3,4,10'>summary?x=1,2,3,4,10</a></p></main>"
  )
}

#* Liveness, for Up's wait.
#* @get /health
#* @serializer unboxedJSON
function() list(ok = TRUE)

#* Summary statistics of a comma-separated list of numbers.
#* @param x:str Numbers, comma-separated, e.g. 1,2,3
#* @get /summary
#* @serializer unboxedJSON
function(x = "1,2,3") {
  v <- as.numeric(strsplit(x, ",", fixed = TRUE)[[1]])
  if (anyNA(v)) stop("x must be numbers separated by commas")
  list(n = length(v), mean = mean(v), sd = stats::sd(v), median = stats::median(v))
}
