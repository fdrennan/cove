#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background. A save of report.Rmd renders on the next
# visit; a save of server.R restarts the server.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

stamp() { stat -c %Y server.R; }
child=
trap '[ -n "$child" ] && kill "$child" 2>/dev/null; exit 0' TERM INT
while :; do
  seen="$(stamp)"
  Rscript server.R &
  child=$!
  while [ "$(stamp)" = "$seen" ] && kill -0 "$child" 2>/dev/null; do sleep 1; done
  kill "$child" 2>/dev/null || true
  wait "$child" 2>/dev/null || true
  while [ "$(stamp)" = "$seen" ]; do sleep 1; done
done
