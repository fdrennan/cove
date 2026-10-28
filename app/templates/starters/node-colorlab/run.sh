#!/usr/bin/env bash
# Serve Color Laboratory on 127.0.0.1:$PORT (APP_PORT in app.env), in the
# foreground. up.sh runs this in the background; build.sh checks first.
#
#   ./run.sh
#   PORT=<n> ./run.sh   another port, for this run only
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

exec node server.js
