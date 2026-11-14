#!/usr/bin/env bash
# Build: this project as the type .cove/app.env names (APP_LANGUAGE), by that
# type's own .cove/<language>/build.sh. Every type's scripts are in .cove/;
# Cove's Type menu (Settings › Info), or APP_LANGUAGE, picks which runs.
#
# What it prints is kept in .cove/run/build.log as well - the last build,
# however it was started (Cove's Build, an agent, a terminal) - beside the
# running app's .cove/run/app.log. Cove shows both under Logs.
#
#   .cove/build.sh
set -euo pipefail
cove="$(cd "$(dirname "$0")" && pwd)"
. "$cove/app.env"
script="$cove/${APP_LANGUAGE:-}/build.sh"
[ -x "$script" ] || { echo "build: no $script - APP_LANGUAGE in .cove/app.env names no type with a build.sh" >&2; exit 1; }
mkdir -p "$cove/run"
{ echo "== build $(date -u +%FT%TZ) · ${APP_LANGUAGE}"; "$script" "$@"; } 2>&1 | tee "$cove/run/build.log"
