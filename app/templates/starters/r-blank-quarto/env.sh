# Sourced by run.sh and build.sh: app.env, and R from the ndexr stack in this
# one shell (a module loaded in one shell is gone in the next). Packages come
# from the stack's R set, built on the n-boxes; see APP_NEEDS below.
. ./app.env

if [ -r /opt/ndexr/stack/use.sh ]; then
  set +u
  . /opt/ndexr/stack/use.sh >/dev/null 2>&1
  ml env/r4.6_py3.13-release >/dev/null 2>&1 || true
  set -u
fi
command -v Rscript >/dev/null || { echo "no Rscript: run where /opt/ndexr/stack is mounted (env/r4.6_py3.13-release)" >&2; exit 127; }

# Every package APP_NEEDS names as r:<name>. A box holds only part of the set
# in its site library (the records marked SiteLibrary); what it lacks comes
# from ndexr's own repository for the set - the binaries the n-boxes built,
# one version each - into the person's R library for this R (R_LIBS_USER,
# from the module). Never CRAN or another mirror.
cove_triple="$(printf '%s' "${NDEXR_ENV_SITE_LIBRARY:-}" | sed -n 's|.*/stack/\([^/]*\)/eb/.*|\1|p')"
cove_repo=""
if [ -n "$cove_triple" ] && [ -n "${EBVERSIONR:-}" ] && [ -n "${NDEXR_BIOC_VERSION:-}" ]; then
  cove_repo="${NDEXR_PKGS_URL:-https://repo.ndexr.io/env}/$cove_triple/R/$EBVERSIONR-bioc$NDEXR_BIOC_VERSION/R"
fi
[ -n "${R_LIBS_USER:-}" ] && mkdir -p "$R_LIBS_USER"
Rscript - "$cove_repo" ${APP_NEEDS:-} <<'R' || exit 127
a <- commandArgs(TRUE)
repo <- a[1]
want <- sub("^r:", "", grep("^r:", a[-1], value = TRUE))
gone <- function() want[!vapply(want, requireNamespace, TRUE, quietly = TRUE)]
m <- gone()
lib <- Sys.getenv("R_LIBS_USER")
if (length(m) && nzchar(repo) && nzchar(lib)) {
  message("env: ", paste(m, collapse = " "), " not on this box; fetching from ", repo)
  tryCatch(utils::install.packages(m, lib = lib, repos = repo, type = "source", quiet = TRUE),
           error = function(e) message("env: could not fetch from ", repo, ": ", conditionMessage(e)))
  m <- gone()
}
if (length(m)) {
  message("not in the stack's R set: ", paste(m, collapse = ", "), " - ndexr builds none of it for this box")
  quit(status = 127)
}
R
export PORT="${PORT:-$APP_PORT}"
