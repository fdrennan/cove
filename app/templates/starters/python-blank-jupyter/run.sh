#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background. A save of notebook.ipynb runs it again;
# a save of app.py restarts the server.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

child=
trap '[ -n "$child" ] && kill "$child" 2>/dev/null; exit 0' TERM INT
while :; do
  seen="$(stat -c %Y app.py)"
  python3 app.py &
  child=$!
  while [ "$(stat -c %Y app.py)" = "$seen" ] && kill -0 "$child" 2>/dev/null; do sleep 1; done
  kill "$child" 2>/dev/null || true
  wait "$child" 2>/dev/null || true
  while [ "$(stat -c %Y app.py)" = "$seen" ]; do sleep 1; done
done
