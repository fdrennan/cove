#!/usr/bin/env bash
# One RStudio session for `cove multiuser` (2026-10-13, operator: "modify
# rstudio so it works with users like cove.ndexr.io does ... multiple live
# sessions ... traverse projects through the url ... get rstudio into lenses").
#
# Installed to /usr/local/lib/cove/rstudio-session by install-multiuser.sh
# and started by multiuser (multiuser.conf `rstudio-launcher`) already AS THE
# PERSON - uid, gid, groups, HOME - never as root.
#
#   rstudio-session <port> <id> <folder> [<module>]
#
# One session = one Inlet `rserver` (ndexr's modified RStudio Server) from the CVMFS stack, single-user
# (--auth-none) on a loopback port, mounted at /r/<id>/. Nothing reaches the
# port but nginx, and nginx only after multiuser's /__rstudio_gate has matched
# the session's capability cookie (nginx/conf.d/rstudio_sessions.conf).
#
# Open-source RStudio gives a person ONE session. Several at once work because
# each gets its own server data dir, database, RSTUDIO_DATA_HOME (suspended
# state, history) and RSTUDIO_CONFIG_HOME - nothing of one session is shared
# with another. Proven 2026-10-13: two at once for the same account, each R in
# its own folder.
set -eu
port=$1 id=$2 folder=$3 module=${4:-}

[[ $port =~ ^[0-9]+$ && $id =~ ^[0-9a-f]{10}$ ]] || { echo "rstudio-session: bad port or id" >&2; exit 2; }
[[ -d $folder ]] || folder=$HOME

# SHORT on purpose: rserver puts unix sockets under its data dir, and a path
# over 108 bytes dies at start with "File name too long" (a scratchpad path
# did, 2026-10-13).
base="$HOME/.local/state/ndexr-rstudio"
st="$base/$id"
mkdir -p "$st/data" "$st/db" "$st/home-data" "$st/config"
chmod 700 "$base" "$st"
# Sessions that ended a week ago leave only history behind.
find "$base" -mindepth 1 -maxdepth 1 -type d -mtime +7 ! -name "$id" -exec rm -rf {} + 2>/dev/null || true

printf 'provider=sqlite\ndirectory=%s/db\n' "$st" >"$st/db.conf"
printf 'session-default-working-dir=%s\nsession-default-new-project-dir=%s\nsession-timeout-minutes=0\n' "$folder" "$folder" >"$st/rsession.conf"

export RSTUDIO_DATA_HOME="$st/home-data"
export RSTUDIO_CONFIG_HOME="$st/config"
# The folder the URL named is an RStudio PREFERENCE (initial_working_directory),
# and a preference outranks rsession.conf's session-default-working-dir: with
# only the latter, R started in ~. So: the person's own prefs, with the folder
# set, as this session's config home.
python3 - "${XDG_CONFIG_HOME:-$HOME/.config}/rstudio/rstudio-prefs.json" "$st/config/rstudio-prefs.json" "$folder" <<'PY'
import json, os, sys
src, dst, folder = sys.argv[1:]
prefs = {}
if os.path.isfile(src):
    try:
        prefs = json.load(open(src))
    except Exception:
        prefs = {}
prefs["initial_working_directory"] = folder
with open(dst, "w") as f:
    json.dump(prefs, f, indent=2)
PY

# A folder holding an RStudio project opens AS that project (2026-10-13,
# operator: "go to the project based on the URL"). Open-source RStudio Server
# has no ?project= URL (Workbench's), but every RStudio reopens the last
# project at start when restore_last_project is on: this session's own data
# home names the folder's .Rproj as that last project. Several .Rproj files:
# the one named for the folder, else the first by name.
rproj=""
if [[ -f "$folder/$(basename "$folder").Rproj" ]]; then
  rproj="$folder/$(basename "$folder").Rproj"
else
  rproj=$(find "$folder" -maxdepth 1 -name '*.Rproj' -type f 2>/dev/null | sort | head -1)
fi
if [[ -n $rproj ]]; then
  mkdir -p "$st/home-data/projects_settings"
  printf '%s' "$rproj" >"$st/home-data/projects_settings/last-project-path"
  python3 - "$st/config/rstudio-prefs.json" <<'PY'
import json, sys
p = sys.argv[1]
prefs = json.load(open(p))
prefs["restore_last_project"] = True
json.dump(prefs, open(p, "w"), indent=2)
PY
fi

# Lmod's init and module files are not written for `set -u`.
set +u
. /opt/ndexr/stack/use.sh
# Inlet was pimp-my-rstudio until 2026-10-28. Where the stack has not
# published Inlet yet, the old build starts - and says so in the session log,
# because it still shows the old name and Posit's logo (inlet.ndexr.io, #713).
ml Inlet 2>/dev/null || { echo "rstudio-session: no Inlet module on this host; starting pimp-my-rstudio" >&2; ml pimp-my-rstudio; }
# The environment picked at New: its R becomes the session's R, and its PATH
# (Python, the site library) is what rsession inherits.
[[ -n $module ]] && ml "$module"
set -u
command -v rserver >/dev/null || { echo "rstudio-session: no rserver after ml Inlet" >&2; exit 1; }
# rserver reads R_HOME_DIR / R_DOC_DIR out of the real R front-end script, and
# RSTUDIO_WHICH_R outranks --rsession-which-r (see scripts/rstudio/pimp-rstudio.sh).
[[ -n ${EBROOTR:-} && -x $EBROOTR/bin/R ]] || { echo "rstudio-session: no R loaded" >&2; exit 1; }
unset RSTUDIO_WHICH_R

cd "$folder"
exec rserver \
  --server-daemonize=0 \
  --rsession-which-r="$EBROOTR/bin/R" \
  --www-address=127.0.0.1 \
  --www-port="$port" \
  --www-root-path="/r/$id/" \
  --server-user="$(id -un)" \
  --auth-none=1 \
  --rsession-config-file="$st/rsession.conf" \
  --database-config-file="$st/db.conf" \
  --server-data-dir="$st/data" \
  --server-pid-file="$st/rserver.pid" \
  --secure-cookie-key-file="$st/cookie-key"
