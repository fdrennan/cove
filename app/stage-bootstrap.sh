#!/usr/bin/env bash
# Refresh images/bootstrap/ — the source tarball + bootstrap-mac.sh that a Mac
# builds from until repo.ndexr.io carries a darwin release. Re-run after any
# change under app/; nginx serves the directory as soon as the files land.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="$here/../images/bootstrap"
mkdir -p "$out"

tar -C "$here" -czf "$out/cove-src.tar.gz" \
  --exclude=./target --exclude=./dist --exclude=./.gitignore .
(cd "$out" && sha256sum cove-src.tar.gz > cove-src.tar.gz.sha256)
install -m 644 "$here/packaging/bootstrap-mac.sh" "$out/bootstrap-mac.sh"

echo "✓ curl -fsSL https://cove.ndexr.io/domains/drennanfreddy/cove.ndexr.io/images/bootstrap/bootstrap-mac.sh | sh"
