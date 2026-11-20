#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background. quarto preview renders index.qmd again
# on every save and reloads the open page.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

printf '%s INFO    [app] __HOST__: quarto preview starting on 127.0.0.1:%s\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$PORT" >&2
exec quarto preview index.qmd --host 127.0.0.1 --port "$PORT" --no-browser
