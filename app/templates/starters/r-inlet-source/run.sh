#!/usr/bin/env bash
# Serve __HOST__ on 127.0.0.1:$PORT (APP_PORT in app.env), in the foreground:
# Inlet, as you, at the site's root. up.sh runs this in the background.
#
# Your Inlet is config/: preferences, themes, keybindings and snippets. Inlet
# reads them from here (RSTUDIO_CONFIG_HOME) and writes what you change in its
# own Global Options back here, so your IDE is versioned with the project.
# Its state - sessions, history, the database - is .run/inlet, never committed.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

here="$(pwd -P)"
st="$here/.run/inlet"
# rserver's data folder holds its unix sockets, and a socket path over 108
# bytes stops it at start ("File name too long"): a project's .run/ is too
# deep (test.drennanfreddy.ndexr.io, 2026-11-07). It goes in a short folder
# named for this project; everything else stays in .run/inlet.
data="$HOME/.local/state/ndexr-inlet/$(printf '%s' "$here" | md5sum | cut -c1-10)"
mkdir -p "$st/db" "$st/home-data" "$data"
chmod 700 "$st" "$data"
printf 'provider=sqlite\ndirectory=%s/db\n' "$st" >"$st/db.conf"
printf 'session-default-working-dir=%s\nsession-default-new-project-dir=%s\nsession-timeout-minutes=0\n' "$here" "$here" >"$st/rsession.conf"
export RSTUDIO_CONFIG_HOME="$here/config"
export RSTUDIO_DATA_HOME="$st/home-data"
unset RSTUDIO_WHICH_R

printf '%s INFO    [app] __HOST__: Inlet (rserver) starting on 127.0.0.1:%s\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$PORT" >&2
exec rserver \
  --server-daemonize=0 \
  --rsession-which-r="$EBROOTR/bin/R" \
  --www-address=127.0.0.1 \
  --www-port="$PORT" \
  --server-user="$(id -un)" \
  --auth-none=1 \
  --rsession-config-file="$st/rsession.conf" \
  --database-config-file="$st/db.conf" \
  --server-data-dir="$data" \
  --server-pid-file="$st/rserver.pid" \
  --secure-cookie-key-file="$st/cookie-key"
