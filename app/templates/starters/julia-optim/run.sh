#!/usr/bin/env bash
# Serve Optimizer Race on 127.0.0.1:$PORT (default APP_PORT in app.env), in the
# foreground. up.sh runs this in the background; build.sh compiles first.
#
#   ./run.sh
#   PORT=9999 ./run.sh   another port, for this run only
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

printf '%s INFO    [run] Optimizer Race: starting Julia on 127.0.0.1:%s (the first start compiles packages and can take a few minutes)\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$PORT"
exec julia --startup-file=no server.jl
