#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

# The browser's websocket comes from the site's address through nginx, which
# decides who reaches this server; the server itself listens on 127.0.0.1.
printf '%s INFO    [app] __HOST__: Panel app starting on 127.0.0.1:%s\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$PORT"
exec python3 -m panel serve app.py --address 127.0.0.1 --port "$PORT" \
  --allow-websocket-origin '*' --index app --liveness --dev
