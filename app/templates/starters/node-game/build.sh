#!/usr/bin/env bash
# Build: the game's TypeScript (src/) to the browser's JavaScript (static/js/)
# by Node's own type stripping, every script then checked by Node, and the
# vendored libraries checked against their recorded sha256. No npm, no
# network. run.sh does the same stripping again whenever src/ changes.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

start=$SECONDS
node --no-warnings scripts/strip-types.mjs src static/js
n=0
while IFS= read -r f; do
  node --check "$f"
  n=$((n + 1))
done < <(find server.js scripts static/js -name '*.js' -o -name '*.mjs')
(cd static/vendor && sed -n '/^```$/,/^```$/p' README.md | grep -E '^[0-9a-f]{64} ' | sha256sum -c --quiet -)
echo "build: $n scripts checked, vendor verified in $((SECONDS - start)) s ($(node --version))"
