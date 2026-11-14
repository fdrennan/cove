#!/usr/bin/env bash
# Run: this project as the type .cove/app.env names (APP_LANGUAGE), by that
# type's own .cove/<language>/run.sh. Every type's scripts are in .cove/;
# Cove's Type menu (Settings › Info), or APP_LANGUAGE, picks which runs.
#
#   .cove/run.sh
set -euo pipefail
cove="$(cd "$(dirname "$0")" && pwd)"
. "$cove/app.env"
script="$cove/${APP_LANGUAGE:-}/run.sh"
[ -x "$script" ] || { echo "run: no $script - APP_LANGUAGE in .cove/app.env names no type with a run.sh" >&2; exit 1; }
exec "$script" "$@"
