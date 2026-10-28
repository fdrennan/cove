# Sourced by run.sh and build.sh: app.env, and Julia from the ndexr stack in
# this one shell (a module loaded in one shell is gone in the next). Packages
# come from the stack's Julia set; nothing here installs any.
. ./app.env

if [ -r /opt/ndexr/stack/use.sh ]; then
  set +u
  . /opt/ndexr/stack/use.sh >/dev/null 2>&1
  avail="$(ml -t avail env/Julia/1.11.7 2>&1 || true)"
  if grep -q 'env/Julia/1.11.7' <<<"$avail"; then
    ml env/Julia/1.11.7
  else
    ml Julia/1.11.7
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
