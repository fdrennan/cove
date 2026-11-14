#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background. The page reloads itself (static/live.js).
#
#   ./run.sh
#   PORT=<n> ./run.sh   another port, for this run only
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

# Started again whenever server.js is saved. (node --watch does the same, for 30 MB
# more per site, and every person's site is one of these.)
stamp() { stat -c %Y server.js 2>/dev/null || true; }
child=
trap '[ -n "$child" ] && kill "$child" 2>/dev/null; exit 0' TERM INT
while :; do
  seen="$(stamp)"
  node server.js &
  child=$!
  while [ "$(stamp)" = "$seen" ] && kill -0 "$child" 2>/dev/null; do sleep 1; done
  kill "$child" 2>/dev/null || true
  wait "$child" 2>/dev/null || true
  # Stopped by itself, with nothing changed: wait for the save that fixes it.
  while [ "$(stamp)" = "$seen" ]; do sleep 1; done
done
