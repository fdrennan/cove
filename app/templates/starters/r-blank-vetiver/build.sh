#!/usr/bin/env bash
# Build: fit the model and build the API once, so up fails here, with a
# reason, rather than inside its health wait.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

Rscript -e 'invisible(source("model.R")$value); cat("build: model fitted, API built\n")'
