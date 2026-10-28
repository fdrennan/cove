#!/usr/bin/env bash
# Serve the site on 127.0.0.1:$PORT (default APP_PORT in app.env), in the
# foreground: media/ holds what you add, out/ what the app makes. build.sh
# compiles the server and the browser UI; up.sh runs this in the background.
#
#   ./run.sh
#   PORT=8810 ./run.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

[ -x ./target/release/app ] || { echo "not built yet: run ./build.sh" >&2; exit 1; }
mkdir -p media out
exec ./target/release/app serve "$PORT"
