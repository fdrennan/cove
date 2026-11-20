#!/usr/bin/env bash
# Build: check app.R and every module under app/ parse, so up fails here, with
# a reason, rather than on the first visit.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

Rscript -e 'for (f in c("app.R", list.files("app", "[.]R$", recursive = TRUE, full.names = TRUE))) invisible(parse(f)); cat("build:", length(list.files("app", "[.]R$", recursive = TRUE)) + 1L, "files parse\n")'
