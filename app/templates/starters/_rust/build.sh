#!/usr/bin/env bash
# Build: the server, and the browser UI where the project has one. up never
# compiles; it starts what this built.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

start=$SECONDS
cargo build --release
if [ -x ./build-web.sh ]; then
  ./build-web.sh || echo "build: the browser UI was not rebuilt - the last dist/web, if any, is served" >&2
fi
echo "build: done in $((SECONDS - start)) s"
