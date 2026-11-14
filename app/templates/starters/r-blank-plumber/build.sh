#!/usr/bin/env bash
# Build: check plumber.R parses and Plumber can read its routes, so up fails
# here, with a reason, rather than inside its health wait.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

Rscript -e 'p <- plumber::pr("plumber.R"); cat("build: plumber.R has", length(p$endpoints[[1]]), "routes\n")'
