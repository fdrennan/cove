#!/usr/bin/env bash
# One Positron session for `cove multiuser` (2026-10-14, operator: "like
# rstudio.ndexr.io and ide.ndexr.io we need positron.ndexr.io to take and create
# a session for the logged in user" - and "should use ml positron for it and
# nothing else").
#
# Installed to /usr/local/lib/cove/positron-session by install-multiuser.sh and
# started by multiuser (multiuser.conf `positron-launcher`) already AS THE
# PERSON - uid, gid, groups, HOME - never as root.
#
#   positron-session <port> <id> <folder> [<module>]
#
# One session = one `positron-server` from the stack's Positron-Server module,
# on a loopback port, mounted at /p/<id>/. nginx reaches it only after
# multiuser's /__rstudio_gate has matched the session's capability cookie
# (nginx/conf.d/positron_sessions.conf); the server ALSO wants its connection
# token, which is the same session secret (COVE_TOKEN, from multiuser), handed
# over in a 600 file - never on the command line, where `ps` shows it.
set -eu
port=$1 id=$2 folder=$3 module=${4:-}

[[ $port =~ ^[0-9]+$ && $id =~ ^[0-9a-f]{10}$ ]] || { echo "positron-session: bad port or id" >&2; exit 2; }
[[ -n ${COVE_TOKEN:-} ]] || { echo "positron-session: no COVE_TOKEN" >&2; exit 2; }
[[ -d $folder ]] || folder=$HOME

# Per session: its own server data (logs, state, workspace storage). Shared
# per person: extensions and the User settings, so a setting made in one
# Positron is there in the next.
base="$HOME/.local/state/ndexr-positron"
st="$base/$id"
shared="$HOME/.positron-server"
mkdir -p "$st/data" "$shared/extensions" "$shared/data/User"
chmod 700 "$base" "$st"
ln -sfn "$shared/data/User" "$st/data/User"
# Sessions that ended a week ago leave nothing worth keeping.
find "$base" -mindepth 1 -maxdepth 1 -type d -mtime +7 ! -name "$id" -exec rm -rf {} + 2>/dev/null || true

( umask 077; printf '%s' "$COVE_TOKEN" >"$st/token" )
unset COVE_TOKEN COVE_SEED

# Lmod's init and module files are not written for `set -u`.
set +u
. /opt/ndexr/stack/use.sh
ml Positron-Server
# The environment picked at New: its R and Python are what Positron's
# consoles find on PATH.
[[ -n $module ]] && ml "$module"
set -u
command -v positron-server >/dev/null || { echo "positron-session: no positron-server after ml Positron-Server" >&2; exit 1; }

# Posit's Positron Server will not serve without a license key "in a hosted
# environment" - proven 2026-10-14 with the stack's 2026.10.0-297: it binds,
# then exits with "No license key provided". The key is the operator's to
# install, root-owned and readable by group ndexr (every session's group).
lic=/etc/cove/positron-license.key
[[ -r $lic ]] || { echo "positron-session: no Positron license key at $lic (Posit requires one to host Positron Server)" >&2; exit 1; }

# The folder reaches the browser as ?folder= (multiuser's enter_direct): this
# server has no --default-folder.
cd "$folder"
exec positron-server \
  --host 127.0.0.1 \
  --port "$port" \
  --server-base-path "/p/$id" \
  --connection-token-file "$st/token" \
  --accept-server-license-terms \
  --telemetry-level off \
  --server-data-dir "$st" \
  --extensions-dir "$shared/extensions" \
  --license-key-file "$lic"
