# Logging for __HOST__, base R only (no package to install). Every line goes
# to stderr, unbuffered, in the one format every Cove starter writes:
#
#   2026-11-18 12:04:31 INFO    [app] serving on 127.0.0.1:8834
#
# so .cove/run/app.log (Cove's Logs tab) reads the same in R, Python, Node,
# Julia and Rust. Times are UTC. Inside a Shiny module the line is indented by
# the module's depth and names it, so the log reads as the tree the app is
# built in (after box.biomarker.ndexr.io's r/log.r).
#
# Levels, shown when at or above LOG_LEVEL (default INFO):
#   TRACE 0  DEBUG 1  INFO 2  SUCCESS 2  WARN 3  ERROR 4
# WARN and ERROR only print: they never signal a condition, so logging inside
# a tryCatch handler cannot change what the code does. Use stop() for that.
#
# Text takes {expr}, evaluated where the call is:
#   box::use(r / log)                    # box apps (box.path = the project)
#   box::use(./r/log)                    # a module.r beside r/
#   source("r/log.r", local = log <- new.env())   # plain scripts
#   log$log_info("{nrow(df)} rows from {table}")
#
# Every ui_* and server_* calls log_fn() first:
#   server_plot <- function(id) { log$log_fn(); shiny::moduleServer(...) }

.LEVELS <- c(TRACE = 0L, DEBUG = 1L, INFO = 2L, SUCCESS = 2L, WARN = 3L, ERROR = 4L)

.threshold <- function() {
  lvl <- toupper(Sys.getenv("LOG_LEVEL", "INFO"))
  if (lvl %in% names(.LEVELS)) .LEVELS[[lvl]] else .LEVELS[["INFO"]]
}

# {expr} -> its value, in the caller's frame.
.glue <- function(text, envir) {
  m <- gregexpr("\\{[^{}]+\\}", text)
  parts <- regmatches(text, m)[[1]]
  if (!length(parts)) {
    return(text)
  }
  vals <- vapply(parts, function(p) {
    v <- tryCatch(eval(parse(text = substr(p, 2, nchar(p) - 1)), envir), error = function(e) paste0("<", conditionMessage(e), ">"))
    paste(format(v), collapse = ", ")
  }, "")
  regmatches(text, m) <- list(vals)
  text
}

# The Shiny module a line belongs to, as c("app", "plot"), or empty outside
# a session (startup, scripts).
.path_of <- function(full_id = NULL) {
  if (is.null(full_id)) {
    d <- if ("shiny" %in% loadedNamespaces()) shiny::getDefaultReactiveDomain() else NULL
    full_id <- if (is.null(d)) "" else sub("-$", "", d$ns(""))
  }
  if (!nzchar(full_id)) {
    return(character(0))
  }
  parts <- strsplit(full_id, "-", fixed = TRUE)[[1]]
  # A module app's root id is its hostname, which may hold "-" itself
  # (dinosaur-ridge.drennanfreddy.ndexr.io): ids have no dots, so everything
  # up to the last part with a dot is the root.
  dotted <- which(grepl(".", parts, fixed = TRUE))
  if (length(dotted)) {
    k <- max(dotted)
    parts <- c(paste(parts[seq_len(k)], collapse = "-"), parts[-seq_len(k)])
  }
  parts
}

.emit <- function(level, text, envir, path = .path_of()) {
  if (.LEVELS[[level]] < .threshold()) {
    return(invisible())
  }
  where <- if (length(path)) path[length(path)] else Sys.getenv("LOG_NAME", "app")
  line <- sprintf(
    "%s %-7s %s[%s] %s",
    format(Sys.time(), "%Y-%m-%d %H:%M:%S", tz = "UTC"), level,
    strrep("  ", max(length(path) - 1L, 0L)), where, .glue(text, envir)
  )
  cat(line, "\n", sep = "", file = stderr())
  invisible()
}

#' @export
log_trace <- function(text) .emit("TRACE", text, parent.frame())
#' @export
log_debug <- function(text) .emit("DEBUG", text, parent.frame())
#' @export
log_info <- function(text) .emit("INFO", text, parent.frame())
#' @export
log_success <- function(text) .emit("SUCCESS", text, parent.frame())
#' @export
log_warn <- function(text) .emit("WARN", text, parent.frame())
#' @export
log_error <- function(text) .emit("ERROR", text, parent.frame())

#' A module's entry, at INFO: the calling function's name, indented by where
#' it sits in the tree (`id` in the caller, namespaced for server*).
#' @export
log_fn <- function() {
  caller <- parent.frame()
  # page$server_x, server_x, or no name at all (do.call, testServer given
  # the function): a log line must never be what stops the server.
  f <- sys.call(-1)[[1]]
  name <- if (is.symbol(f) || is.call(f) && identical(f[[1]], as.name("$"))) sub(".*\\$", "", paste(deparse(f), collapse = "")) else "(module)"
  id <- get0("id", envir = caller, inherits = FALSE)
  full <- if (!is.character(id) || length(id) != 1L) {
    ""
  } else if (startsWith(name, "server") && "shiny" %in% loadedNamespaces() && !is.null(d <- shiny::getDefaultReactiveDomain())) {
    d$ns(id)
  } else {
    id
  }
  .emit("INFO", name, caller, .path_of(full))
}

#' One line per Shiny session: opened, and closed with how long it lasted.
#' Call it first in the server function.
#' @export
log_session <- function(session) {
  started <- Sys.time()
  sid <- substr(session$token, 1, 6)
  agent <- session$request$HTTP_USER_AGENT
  .emit("INFO", paste0("session ", sid, " opened", if (length(agent) && nzchar(agent)) paste0(" (", sub(" .*", "", agent), ")") else ""), environment(), character(0))
  session$onSessionEnded(function() {
    .emit("INFO", sprintf("session %s closed after %.0f s", sid, as.numeric(difftime(Sys.time(), started, units = "secs"))), environment(), character(0))
  })
  invisible()
}
