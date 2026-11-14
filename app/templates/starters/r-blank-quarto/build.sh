#!/usr/bin/env bash
# Build: render index.qmd once, so up fails here, with a reason, rather than
# inside its health wait.
#
#   ./build.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

command -v quarto >/dev/null || { echo "no quarto: the stack's env set loads Quarto" >&2; exit 127; }
quarto render index.qmd --quiet
echo "build: index.qmd renders ($(quarto --version))"
