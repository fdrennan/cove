#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background. shiny::runApp finds app.R here.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

exec Rscript -e 'shiny::runApp(".", host = "127.0.0.1", port = as.integer(Sys.getenv("PORT")), launch.browser = FALSE)'
