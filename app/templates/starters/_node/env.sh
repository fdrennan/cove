# Sourced by run.sh and build.sh: app.env, and Node.js from the ndexr stack in
# this one shell (a module loaded in one shell is gone in the next). The server
# uses Node's standard library only; the browser libraries are vendored in
# static/vendor/ (see its README). Nothing here installs anything.
. ./app.env

if [ -r /opt/ndexr/stack/use.sh ]; then
  set +u
  . /opt/ndexr/stack/use.sh >/dev/null 2>&1
  ml nodejs/22.22.2-GCCcore-13.3.0 >/dev/null 2>&1 || ml nodejs >/dev/null 2>&1 || true
  set -u
fi
command -v node >/dev/null || { echo "no node: run where /opt/ndexr/stack is mounted (ml nodejs)" >&2; exit 127; }
export PORT="${PORT:-$APP_PORT}"
