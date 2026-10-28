#!/usr/bin/env bash
# Build: syntax-check the server and every browser script, and confirm the
# vendored libraries are the ones static/vendor/README.md names, so up fails
# here, with a reason, rather than inside its health wait.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

start=$SECONDS
n=0
while IFS= read -r f; do
  node --check "$f"
  n=$((n + 1))
done < <(find . -name '*.js' -not -path './static/vendor/*' -not -path './.run/*' -not -path './node_modules/*')
if [ -f static/vendor/README.md ]; then
  (cd static/vendor && sed -n '/^```$/,/^```$/p' README.md | grep -E '^[0-9a-f]{64} ' | sha256sum -c --quiet -)
fi
echo "build: $n scripts checked, vendor verified in $((SECONDS - start)) s ($(node --version))"
