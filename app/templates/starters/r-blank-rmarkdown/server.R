# __HOST__: report.Rmd published as a page, with its parameters from the
# address (?points=1000&seed=7 - Posit calls this a parameterized report). A
# render is kept per set of parameters until report.Rmd changes; the page
# reloads itself when it does. A report that fails shows its error.

box::use(./r/log)

out <- file.path(getwd(), ".run", "html")
dir.create(out, recursive = TRUE, showWarnings = FALSE)
stamp <- function() as.character(file.mtime("report.Rmd"))
defaults <- rmarkdown::yaml_front_matter("report.Rmd")$params

live <- "<script>(()=>{let s=null;setInterval(async()=>{if(document.hidden)return;try{const v=await(await fetch('__live',{cache:'no-store'})).text();if(s!==null&&v!==s)location.reload();s=v}catch{}},1000)})()</script>"

render <- function(query) {
  q <- shiny::parseQueryString(query)
  p <- defaults
  for (k in intersect(names(q), names(p))) p[[k]] <- methods::as(q[[k]], class(p[[k]])[1])
  key <- paste(stamp(), paste(names(p), unlist(p), sep = "=", collapse = "&"))
  file <- file.path(out, paste0(substr(digest_of(key), 1, 16), ".html"))
  if (!file.exists(file)) {
    log$log_info("rendering report.Rmd with {paste(names(p), unlist(p), sep = '=', collapse = ' ')}")
    work <- tempfile("render-")
    dir.create(work)
    on.exit(unlink(work, recursive = TRUE), add = TRUE)
    rmarkdown::render("report.Rmd", output_file = basename(file), output_dir = out, intermediates_dir = work,
                      params = p, envir = new.env(parent = globalenv()), quiet = TRUE)
  }
  sub("</body>", paste0(live, "</body>"), paste(readLines(file, warn = FALSE), collapse = "\n"), fixed = TRUE)
}
digest_of <- function(s) {
  f <- tempfile()
  on.exit(unlink(f))
  writeLines(s, f)
  unname(tools::md5sum(f))
}

reply <- function(status, type, body) list(status = status, headers = list(`Content-Type` = type, `Cache-Control` = "no-store"), body = body)

app <- list(call = function(req) {
  path <- req$PATH_INFO
  if (path == "/health") return(reply(200L, "application/json", "{\"ok\":true}"))
  if (path == "/__live") return(reply(200L, "text/plain", stamp()))
  if (path != "/") return(reply(404L, "text/plain", "not found"))
  tryCatch(
    reply(200L, "text/html; charset=utf-8", render(req$QUERY_STRING)),
    error = function(e) {
      log$log_error("report.Rmd did not render: {conditionMessage(e)}")
      reply(500L, "text/html; charset=utf-8",
        paste0("<h1>report.Rmd did not render</h1><pre>", htmltools::htmlEscape(conditionMessage(e)), "</pre>", live))
    })
})

port <- as.integer(Sys.getenv("PORT"))
s <- httpuv::startServer("127.0.0.1", port, app)
log$log_info("__HOST__: report server starting on 127.0.0.1:{port}")
while (TRUE) httpuv::service(1000)
