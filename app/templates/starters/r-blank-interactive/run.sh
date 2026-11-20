#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background. rmarkdown::run renders index.Rmd again
# when it changes.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

exec Rscript -e 'box::use(./r/log); log$log_info("__HOST__: interactive document starting on 127.0.0.1:{Sys.getenv(\"PORT\")}"); rmarkdown::run("index.Rmd", shiny_args = list(host = "127.0.0.1", port = as.integer(Sys.getenv("PORT")), launch.browser = FALSE))'
