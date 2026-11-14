#!/usr/bin/env bash
# Build: Inlet comes built from the stack (ml Inlet), so this checks what is
# yours - every JSON file in config/ parses (a broken rstudio-prefs.json makes
# Inlet ignore all of it) - and that rserver is there.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh
for f in $(find config -name '*.json'); do
  python3 -c 'import json, sys; json.load(open(sys.argv[1]))' "$f" || { echo "build: $f is not valid JSON" >&2; exit 1; }
done
echo "build: config/ parses; $(rserver --version 2>/dev/null | head -1 || echo rserver) from $EBROOTINLET"
