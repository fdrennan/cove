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

# Where the set's packages are, one rule (operator, 2026-11-20: "the local
# version of cove not knowing where to find packages"). The env module names
# the set's library (NDEXR_ENV_SITE_LIBRARY) and where it is published
# (NDEXR_ENV_REPO). On ndexr's servers the library is there; on a laptop the
# stack comes over CernVM-FS but packages never do, so the library is a link
# to nothing - and the old check, "is JULIA_LOAD_PATH set", passed while Julia
# found nothing: "Package HTTP not found" (a Gentoo laptop, 2026-11-20). Now:
# the machine's library if it is there, else this person's own copy, fetched
# once from the published set (sha256 checked) - no sudo, no ndexr CLI.
julia_set() {
  local set="$1" triple repo here cache tmp sum want
  case "$(uname -m)" in
    aarch64|arm64) triple=aarch64-unknown-linux-gnu ;;
    *)             triple=x86_64-pc-linux-gnu ;;
  esac
  here="${NDEXR_ENV_SITE_LIBRARY:-/opt/ndexr/site-library/$triple/$set}/julia"
  if [ -f "$here/Project.toml" ]; then echo "$here"; return 0; fi
  here="/opt/ndexr/site-library/$triple/$set/julia"
  if [ -f "$here/Project.toml" ]; then echo "$here"; return 0; fi
  cache="${XDG_DATA_HOME:-$HOME/.local/share}/ndexr/site-library/$triple/$set/julia"
  if [ -f "$cache/Project.toml" ]; then echo "$cache"; return 0; fi
  repo="${NDEXR_ENV_REPO:-https://repo.ndexr.io/env/$triple/$set/julia}"
  mkdir -p "$(dirname "$cache")"
  exec 8>"$(dirname "$cache")/.fetch.lock"
  flock 8
  if [ -f "$cache/Project.toml" ]; then echo "$cache"; return 0; fi
  log() { printf '%s INFO    [env] %s\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$*" >&2; }
  want="$(curl -fsSL --max-time 30 "${repo%/julia}/current.json" | tr -d ' \n' | sed -n 's/.*"julia_archive":{[^}]*"sha256":"\([0-9a-f]\{64\}\)".*/\1/p')"
  [ -n "$want" ] || { echo "env: could not read $set's published record (${repo%/julia}/current.json) - is this computer online?" >&2; return 1; }
  tmp="$(mktemp -d "$(dirname "$cache")/.fetch.XXXXXX")"
  log "fetching the Julia package set $set ($triple) once, about 2 GB, into $cache"
  if ! curl -fsSL --retry 3 --max-time 3600 -o "$tmp/set.tar.gz" "$repo/set.tar.gz"; then
    rm -rf "$tmp"; echo "env: downloading $repo/set.tar.gz failed" >&2; return 1
  fi
  sum="$(sha256sum "$tmp/set.tar.gz" | cut -d' ' -f1)"
  if [ "$sum" != "$want" ]; then
    rm -rf "$tmp"; echo "env: $set's archive does not match its published sha256 ($sum, expected $want) - not used" >&2; return 1
  fi
  mkdir -p "$tmp/julia" && tar -xzf "$tmp/set.tar.gz" -C "$tmp/julia" && rm -f "$tmp/set.tar.gz" &&
    rm -rf "$cache" && mv "$tmp/julia" "$cache" && rm -rf "$tmp" ||
    { rm -rf "$tmp"; echo "env: unpacking $set failed" >&2; return 1; }
  log "the Julia package set $set is ready in $cache"
  echo "$cache"
}

if stack_here; then
  set +u
  . /opt/ndexr/stack/use.sh >/dev/null 2>&1
  avail="$(ml -t avail env/Julia/1.11.7 2>&1 || true)"
  if grep -q 'env/Julia/1.11.7' <<<"$avail"; then
    stack_ml env/Julia/1.11.7
  else
    stack_ml Julia/1.11.7
  fi
  SITE="$(julia_set Julia/1.11.7)" || { echo "env: the Julia package set Julia/1.11.7 is not on this computer and could not be fetched" >&2; exit 127; }
  export JULIA_DEPOT_PATH="$HOME/.julia:$SITE/depot:"
  export JULIA_LOAD_PATH="@:@v#.#:$SITE:@stdlib"
  set -u
fi
command -v julia >/dev/null || { echo "no julia: run where /opt/ndexr/stack is mounted" >&2; exit 127; }
export JULIA_CONDAPKG_BACKEND=Null
export PORT="${PORT:-$APP_PORT}"
# One Julia thread per core.
export JULIA_NUM_THREADS="${JULIA_NUM_THREADS:-auto}"
