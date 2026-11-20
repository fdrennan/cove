# Sourced by run.sh and build.sh: app.env, and Node.js from the ndexr stack in
# this one shell (a module loaded in one shell is gone in the next). The server
# uses Node's standard library only; the browser libraries are vendored in
# static/vendor/ (see its README). Nothing here installs anything.
. ./app.env

# The app runs in its stack environment or not at all - never on whatever the
# host happens to have (operator, 2026-11-19: "the application must absolutely
# run with its proper ml environment"). stack_ml loads the first of its
# modules that loads and says what is loaded, in the log's own format.
stack_here() {
  [ -r /opt/ndexr/stack/use.sh ] || { echo "env: the ndexr stack is not mounted at /opt/ndexr/stack - set it up in Cove (Stack in the status bar)" >&2; exit 127; }
}
stack_ml() {
  for m in "$@"; do
    if ml "$m" >/dev/null 2>&1; then
      printf '%s INFO    [env] %s from the ndexr stack\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$(ml -t list 2>&1 | grep "^${m%%/*}/" | tail -1)"
      return 0
    fi
  done
  echo "env: none of $* loads from the ndexr stack - try: ml spider ${1%%/*}" >&2
  exit 127
}

if stack_here; then
  set +u
  . /opt/ndexr/stack/use.sh >/dev/null 2>&1
  stack_ml nodejs/22.22.2-GCCcore-13.3.0 nodejs
  set -u
fi
command -v node >/dev/null || { echo "no node: run where /opt/ndexr/stack is mounted (ml nodejs)" >&2; exit 127; }
export PORT="${PORT:-$APP_PORT}"
