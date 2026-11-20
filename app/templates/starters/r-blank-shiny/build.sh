#!/usr/bin/env bash
# Build: check app.R parses, so up fails here, with a reason, rather than on
# the first visit.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

Rscript -e 'invisible(parse("app.R"))'
echo "build: app.R parses"
