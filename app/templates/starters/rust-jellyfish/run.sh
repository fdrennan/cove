#!/usr/bin/env bash
# Serve Jellyfish Drift on 127.0.0.1:$PORT (default APP_PORT in app.env), in
# the foreground. up.sh runs this in the background; build.sh compiles first.
#
#   ./run.sh
#   PORT=9999 ./run.sh   another port, for this run only
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

# Built here when it was not, the first time only: a project made where no
# prebuilt binary could be fetched (Cove's seeds) compiles once before it
# starts, rather than refusing to (operator, 2026-11-27: Start answered "not
# built yet" on a new server).
if [ ! -x ./target/release/app ]; then
  printf '%s INFO    [run] not built yet - building it first; this takes minutes the first time\n' "$(date -u '+%Y-%m-%d %H:%M:%S')"
  ./build.sh || { echo "the build failed - its output is above" >&2; exit 1; }
fi
[ -f ./dist/web/game.wasm ] || echo "run: no dist/web/game.wasm yet - the page will say so; run ./build-web.sh" >&2
exec ./target/release/app serve "$PORT"
