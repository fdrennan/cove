#!/usr/bin/env bash
# Build: a Shiny module is served by the platform's console pool, so there is
# nothing to compile or start - this checks module.r parses, which is what a
# broken save would otherwise hide (the router keeps the last good copy).
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"

if [ -r /opt/ndexr/stack/use.sh ]; then
  set +u
  . /opt/ndexr/stack/use.sh >/dev/null 2>&1
  ml env/r4.6_py3.13-release >/dev/null 2>&1 || ml R/4.6.1-foss-2024a >/dev/null 2>&1 || true
  set -u
fi
command -v Rscript >/dev/null || { echo "no Rscript: run where /opt/ndexr/stack is mounted" >&2; exit 127; }
Rscript -e 'invisible(parse("module.r"))'
echo "build: module.r parses"
