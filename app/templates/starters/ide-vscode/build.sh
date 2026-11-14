#!/usr/bin/env bash
# Build: VS Code comes built from the stack (ml code-server) or from your own
# rebuild (rebuild.sh), so this checks what is yours - config/'s JSON parses
# (VS Code ignores a settings file it cannot read) - and which build runs.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh
for f in $(find config -name '*.json'); do
  # VS Code's JSON allows // comments; whole-line ones are dropped first.
  grep -vE '^\s*//' "$f" | python3 -c 'import json, sys; json.load(sys.stdin)' || { echo "build: $f is not valid JSON" >&2; exit 1; }
done
echo "build: config/ parses; code-server $(code-server --version 2>/dev/null | head -1 | cut -d' ' -f1) from $EBROOTCODEMINSERVER"
