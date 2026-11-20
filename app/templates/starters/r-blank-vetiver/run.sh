#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background. Restarts whenever an .R file changes.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

stamp() { find . -name '*.R' -not -path './.run/*' -printf '%T@ %p\n' 2>/dev/null | sort | md5sum; }
child=
trap '[ -n "$child" ] && kill "$child" 2>/dev/null; exit 0' TERM INT
while :; do
  seen="$(stamp)"
  Rscript -e 'box::use(./r/log); log$log_info("__HOST__: vetiver API starting on 127.0.0.1:{Sys.getenv(\"PORT\")}"); plumber::pr_run(source("model.R")$value, host = "127.0.0.1", port = as.integer(Sys.getenv("PORT")), docs = TRUE, quiet = TRUE)' &
  child=$!
  while [ "$(stamp)" = "$seen" ] && kill -0 "$child" 2>/dev/null; do sleep 1; done
  kill "$child" 2>/dev/null || true
  wait "$child" 2>/dev/null || true
  while [ "$(stamp)" = "$seen" ]; do sleep 1; done
done
