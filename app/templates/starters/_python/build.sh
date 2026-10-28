#!/usr/bin/env bash
# Build: byte-compile the app and check the stack has what it imports, so up
# fails here, with a reason, rather than inside its health wait.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

start=$SECONDS
python3 -m compileall -q .
python3 -c 'import app' >/dev/null
echo "build: done in $((SECONDS - start)) s ($(python3 --version))"
