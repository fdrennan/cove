#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

# Started again whenever server.jl changes.
stamp() { stat -c %Y server.jl 2>/dev/null || true; }
child=
trap '[ -n "$child" ] && kill "$child" 2>/dev/null; exit 0' TERM INT
while :; do
  seen="$(stamp)"
  julia --startup-file=no server.jl &
  child=$!
  while [ "$(stamp)" = "$seen" ] && kill -0 "$child" 2>/dev/null; do sleep 1; done
  kill "$child" 2>/dev/null || true
  wait "$child" 2>/dev/null || true
  # Stopped by itself, with nothing changed: wait for the save that fixes it.
  while [ "$(stamp)" = "$seen" ]; do sleep 1; done
done
