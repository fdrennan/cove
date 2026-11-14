# Sourced by run.sh, build.sh and recipe.sh: app.env, and Inlet (with its R)
# from the ndexr stack, in this one shell.
. ./app.env

# An Inlet you rebuilt yourself (rebuild.sh) is in your own module tree for
# this project, which comes first, so `ml Inlet` loads yours once it exists.
if [ -r /opt/ndexr/stack/use.sh ]; then
  set +u
  . /opt/ndexr/stack/use.sh >/dev/null 2>&1
  mine="${COVE_EB_PREFIX:-$HOME/.local/easybuild/$(basename "$(pwd -P)")}/modules/all"
  [ -d "$mine" ] && module use "$mine"
  ml Inlet >/dev/null 2>&1 || true
  set -u
fi
command -v rserver >/dev/null || { echo "no rserver: run where /opt/ndexr/stack is mounted (ml Inlet)" >&2; exit 127; }
[ -n "${EBROOTR:-}" ] && [ -x "$EBROOTR/bin/R" ] || { echo "no R came with Inlet (EBROOTR)" >&2; exit 127; }
export PORT="${PORT:-$APP_PORT}"
