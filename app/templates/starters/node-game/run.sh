#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

# A save in src/ is stripped to static/js/ again (milliseconds), and the open
# page reloads itself on the change (static/js/live.js); a save of server.js
# restarts the server.
stamp() { find server.js src -type f -printf '%T@\n' 2>/dev/null | sort -n | tail -1; }
child=
trap '[ -n "$child" ] && kill "$child" 2>/dev/null; exit 0' TERM INT
while :; do
  seen="$(stamp)"
  node --no-warnings scripts/strip-types.mjs src static/js || true
  node server.js &
  child=$!
  while [ "$(stamp)" = "$seen" ] && kill -0 "$child" 2>/dev/null; do sleep 1; done
  kill "$child" 2>/dev/null || true
  wait "$child" 2>/dev/null || true
  while [ "$(stamp)" = "$seen" ] && ! kill -0 "$child" 2>/dev/null; do sleep 1; done
done
