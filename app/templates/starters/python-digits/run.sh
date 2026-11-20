#!/usr/bin/env bash
# Serve Digit Lab on 127.0.0.1:$PORT (default APP_PORT in app.env), in the
# foreground. up.sh runs this in the background; build.sh compiles first.
#
#   ./run.sh
#   PORT=8810 ./run.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

exec python3 -m uvicorn app:app --host 127.0.0.1 --port "$PORT"
