#!/usr/bin/env bash
# Serve Orbital on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground.
# up.sh runs this in the background; build.sh checks the scripts first.
#
#   ./run.sh
#   PORT=9999 ./run.sh   another port, for this run only
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

exec node server.js
