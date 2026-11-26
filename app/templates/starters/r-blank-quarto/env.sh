# Sourced by run.sh and build.sh: app.env, and R from the ndexr stack in this
# one shell (a module loaded in one shell is gone in the next). Packages come
# from the stack's R set, built on the n-boxes; see APP_NEEDS below.
. ./app.env

# The stack this project's modules come from - the ndexr stack, EESSI or
# Compute Canada's - and stack_here, stack_enter and stack_ml (.cove/stack.sh).
. ./stack.sh

if stack_here; then
  set +u
  stack_enter
  stack_ml env/r4.6_py3.13-release
  set -u
fi
command -v Rscript >/dev/null || { echo "no Rscript: run where $COVE_STACK_LABEL is mounted (env/r4.6_py3.13-release)" >&2; exit 127; }

# Every package APP_NEEDS names as r:<name>, and nothing more. A box holds
# only part of the set in its site library (the records marked SiteLibrary);
# what it lacks comes from ndexr's own repository for the set - the binaries
# the n-boxes built, one version each - into the person's R library for this R
# (R_LIBS_USER, from the module). A declared package ndexr has not built comes
# from CRAN and compiles here (operator, 2026-11-20: "declared packages not
# available are always pulled or built but nothing more"); for one ndexr has,
# CRAN's row is dropped even when CRAN's version is newer.
cove_triple="$(printf '%s' "${NDEXR_ENV_SITE_LIBRARY:-}" | sed -n 's|.*/stack/\([^/]*\)/eb/.*|\1|p')"
cove_repo=""
if [ -n "$cove_triple" ] && [ -n "${EBVERSIONR:-}" ] && [ -n "${NDEXR_BIOC_VERSION:-}" ]; then
  cove_repo="${NDEXR_PKGS_URL:-https://repo.ndexr.io/env}/$cove_triple/R/$EBVERSIONR-bioc$NDEXR_BIOC_VERSION/R"
fi
# On EESSI or Compute Canada's stack the person's packages for it go in a
# library of their own for that stack and R version (stack.sh stack_enter).
if [ "${COVE_STACK_NAME:-ndexr}" != ndexr ]; then
  export R_LIBS_USER="$HOME/R/$COVE_STACK_NAME/$(Rscript -e 'cat(as.character(getRversion()))')"
fi
[ -n "${R_LIBS_USER:-}" ] && mkdir -p "$R_LIBS_USER"
Rscript - "$cove_repo" ${APP_NEEDS:-} <<'R' || exit 127
a <- commandArgs(TRUE)
repo <- a[1]
want <- sub("^r:", "", grep("^r:", a[-1], value = TRUE))
gone <- function() want[!vapply(want, requireNamespace, TRUE, quietly = TRUE)]
m <- gone()
lib <- Sys.getenv("R_LIBS_USER")
if (length(m) && nzchar(lib)) {
  message("env: ", paste(m, collapse = " "), " not on this box; fetching from ", if (nzchar(repo)) repo else "CRAN")
  cran <- getOption("repos")[["CRAN"]]
  if (is.null(cran) || !nzchar(cran) || cran == "@CRAN@") cran <- "https://cloud.r-project.org"
  ndexr_first <- function(db) {
    ours <- startsWith(db[, "Repository"], repo)
    db[ours | !(db[, "Package"] %in% db[ours, "Package"]), , drop = FALSE]
  }
  options(available_packages_filters = list("R_version", "OS_type", "subarch", ndexr_first, "duplicates"))
  tryCatch(utils::install.packages(m, lib = lib, repos = if (nzchar(repo)) c(ndexr = repo, CRAN = cran) else c(CRAN = cran), type = "source", quiet = TRUE),
           error = function(e) message("env: could not fetch from ", repo, ": ", conditionMessage(e)))
  m <- gone()
}
if (length(m)) {
  message("could not install: ", paste(m, collapse = ", "), " - neither ndexr's R set nor CRAN had it for this R")
  quit(status = 127)
}
R
export PORT="${PORT:-$APP_PORT}"
