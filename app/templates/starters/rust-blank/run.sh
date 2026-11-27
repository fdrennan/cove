#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

# Built here when it was not, the first time only: a project made where no
# prebuilt binary could be fetched (Cove's seeds) compiles once before it
# starts, rather than refusing to (operator, 2026-11-27: Start answered "not
# built yet" on a new server).
if [ ! -x ./target/release/app ]; then
  printf '%s INFO    [run] not built yet - building it first; this takes minutes the first time\n' "$(date -u '+%Y-%m-%d %H:%M:%S')"
  ./build.sh || { echo "the build failed - its output is above" >&2; exit 1; }
fi
# Started again whenever ./build.sh makes a new binary changes.
stamp() { stat -c %Y target/release/app 2>/dev/null || true; }
child=
trap '[ -n "$child" ] && kill "$child" 2>/dev/null; exit 0' TERM INT
while :; do
  seen="$(stamp)"
  ./target/release/app serve "$PORT" &
  child=$!
  while [ "$(stamp)" = "$seen" ] && kill -0 "$child" 2>/dev/null; do sleep 1; done
  kill "$child" 2>/dev/null || true
  wait "$child" 2>/dev/null || true
  # Stopped by itself, with nothing changed: wait for the save that fixes it.
  while [ "$(stamp)" = "$seen" ]; do sleep 1; done
  printf '%s INFO    [run] %s changed - restarting\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" target/release/app
done
