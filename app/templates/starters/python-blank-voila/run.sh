#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background. Voilà runs app.ipynb afresh for each
# visit, so a save shows on the next reload of the page.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

printf '%s INFO    [app] __HOST__: Voila app starting on 127.0.0.1:%s\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$PORT"
exec python3 -m voila app.ipynb --no-browser --port="$PORT" --Voila.ip=127.0.0.1 \
  --Voila.tornado_settings="{'allow_origin': '*'}"
