#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

exec python3 -m uvicorn app:app --host 127.0.0.1 --port "$PORT" --reload --reload-include "*.py"
