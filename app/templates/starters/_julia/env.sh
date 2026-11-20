# Sourced by run.sh and build.sh: app.env, and Julia from the ndexr stack in
# this one shell (a module loaded in one shell is gone in the next). Packages
# come from the stack's Julia set; nothing here installs any.
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
  avail="$(ml -t avail env/Julia/1.11.7 2>&1 || true)"
  if grep -q 'env/Julia/1.11.7' <<<"$avail"; then
    stack_ml env/Julia/1.11.7
    # The module is CernVM-FS's; the set's packages are not (they come with
    # `ndexr pkgs sync`, as on ndexr's servers). Without them every `using`
    # fails as "Package HTTP not found" (a fresh laptop, 2026-11-19).
    if [ -z "${JULIA_LOAD_PATH:-}" ] && [ -z "${NDEXR_ENV_SITE_LIBRARY:-}" ]; then
      echo "env: the Julia package set (Julia/1.11.7) is not on this computer - Cove gets it (Get the Julia packages), or: sudo ndexr pkgs sync Julia/1.11.7" >&2
      exit 127
    fi
  else
    stack_ml Julia/1.11.7
    case "$(uname -m)" in
      aarch64|arm64) triple=aarch64-unknown-linux-gnu ;;
      *)             triple=x86_64-pc-linux-gnu ;;
    esac
    SITE=/opt/ndexr/site-library/$triple/Julia/1.11.7/julia
    if [ -e "$SITE" ]; then
      export JULIA_DEPOT_PATH="$HOME/.julia:$SITE/depot:"
      export JULIA_LOAD_PATH="@:@v#.#:$SITE:@stdlib"
    fi
  fi
  set -u
fi
command -v julia >/dev/null || { echo "no julia: run where /opt/ndexr/stack is mounted" >&2; exit 127; }
export JULIA_CONDAPKG_BACKEND=Null
export PORT="${PORT:-$APP_PORT}"
# One Julia thread per core.
export JULIA_NUM_THREADS="${JULIA_NUM_THREADS:-auto}"
