#!/usr/bin/env bash
# Build: render report.Rmd once with its default parameters, so up fails
# here, with a reason, rather than on the first visit.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
Rscript -e 'invisible(parse("server.R")); rmarkdown::render("report.Rmd", output_dir = commandArgs(TRUE)[1], intermediates_dir = commandArgs(TRUE)[1], quiet = TRUE)' "$work"
echo "build: report.Rmd renders, server.R parses"
