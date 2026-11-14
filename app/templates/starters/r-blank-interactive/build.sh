#!/usr/bin/env bash
# Build: check index.Rmd's code parses, so up fails here, with a reason,
# rather than on the first visit.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
Rscript -e 'invisible(knitr::purl("index.Rmd", output = file.path(commandArgs(TRUE)[1], "index.R"), quiet = TRUE)); invisible(parse(file.path(commandArgs(TRUE)[1], "index.R")))' "$work"
echo "build: index.Rmd parses"
