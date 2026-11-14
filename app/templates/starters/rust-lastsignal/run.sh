#!/usr/bin/env bash
# Serve Last Signal on 127.0.0.1:$PORT (default APP_PORT in app.env), in
# the foreground. up.sh runs this in the background; build.sh compiles first.
#
#   ./run.sh
#   PORT=9999 ./run.sh   another port, for this run only
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

[ -x ./target/release/app ] || { echo "not built yet: run ./build.sh" >&2; exit 1; }
[ -f ./dist/web/game.wasm ] || echo "run: no dist/web/game.wasm yet - the page will say so; run ./build-web.sh" >&2
exec ./target/release/app serve "$PORT"
