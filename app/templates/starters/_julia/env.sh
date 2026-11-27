# Sourced by run.sh and build.sh: app.env, and Julia from the ndexr stack in
# this one shell (a module loaded in one shell is gone in the next). Packages
# come from the stack's Julia set, piece by piece; what it lacks, from Pkg.
. ./app.env

# The stack this project's modules come from - the ndexr stack, EESSI or
# Compute Canada's - and stack_here, stack_enter and stack_ml (.cove/stack.sh).
. ./stack.sh

# Where the set's packages are, one rule (operator, 2026-11-20: "the local
# version of cove not knowing where to find packages"). The env module names
# the set's library (NDEXR_ENV_SITE_LIBRARY) and where it is published
# (NDEXR_ENV_REPO). On ndexr's servers the library is there. On a laptop the
# stack comes over CernVM-FS but packages never do, so this person gets their
# own copy - and only of what the project declares, the way `ml` brings one
# module over CernVM-FS and not the stack (operator, 2026-11-20: "pulling all
# 2gb when its unlikely we use all those packages ... declared packages not
# available are always pulled or built but nothing more"). The set is
# published in pieces, each named for its sha256: a package with its compiled
# cache, a JLL's binaries, the environment itself. julia/chunks.tsv gives each
# package's line - its own piece and every piece of its dependency closure -
# so the fetch is the union of the declared packages' lines, each piece once,
# checked, kept for good. julia-api's six packages were 2.3 GB as the whole set.
julia_set() {
  local set="$1" triple repo here cache pieces idx want names list sha size n=0 mb=0
  case "$(uname -m)" in
    aarch64|arm64) triple=aarch64-unknown-linux-gnu ;;
    *)             triple=x86_64-pc-linux-gnu ;;
  esac
  here="${NDEXR_ENV_SITE_LIBRARY:-/opt/ndexr/site-library/$triple/$set}/julia"
  if [ -f "$here/Project.toml" ]; then echo "$here"; return 0; fi
  here="/opt/ndexr/site-library/$triple/$set/julia"
  if [ -f "$here/Project.toml" ]; then echo "$here"; return 0; fi
  cache="${XDG_DATA_HOME:-$HOME/.local/share}/ndexr/site-library/$triple/$set/julia"
  pieces="$cache/.pieces"
  # The whole set, fetched by this script before it fetched pieces: in use.
  if [ -f "$cache/Project.toml" ] && [ ! -d "$pieces" ]; then echo "$cache"; return 0; fi
  repo="${NDEXR_ENV_REPO:-https://repo.ndexr.io/env/$triple/$set/julia}"
  mkdir -p "$pieces"
  exec 8>"$pieces/.lock"
  flock 8
  # A whole-set download the earlier version of this script left half done.
  rm -rf "$(dirname "$cache")"/.part.* "$(dirname "$cache")"/.fetch.* "$(dirname "$cache")"/.resume-*.tar.gz
  log() { printf '%s INFO    [env] %s\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$*" >&2; }
  idx="$pieces/chunks.tsv"
  # The index is the published one when this computer is online; offline,
  # the last one fetched serves what it already holds.
  want="$(curl -fsSL --max-time 30 "${repo%/julia}/current.json" 2>/dev/null | tr -d ' \n' | sed -n 's/.*"julia_chunks":{[^}]*"sha256":"\([0-9a-f]\{64\}\)".*/\1/p')"
  if [ -n "$want" ] && [ "$(sha256sum "$idx" 2>/dev/null | cut -d' ' -f1)" != "$want" ]; then
    curl -fsSL --retry 3 -o "$idx.new" "$repo/chunks.tsv" && [ "$(sha256sum "$idx.new" | cut -d' ' -f1)" = "$want" ] && mv "$idx.new" "$idx" ||
      { rm -f "$idx.new"; echo "env: $set's piece index does not match its published sha256" >&2; }
  fi
  [ -s "$idx" ] || { echo "env: could not read $set's piece index (${repo%/julia}/current.json) - is this computer online?" >&2; return 1; }
  names=" @base $(julia_needs | tr "\n" " ") "
  list="$(awk -F'\t' -v w="$names" 'index(w, " " $1 " ") { print $2 }' "$idx" | tr ' ' '\n' | grep . | sort -u)"
  for p in $list; do
    sha="${p%%:*}"; size="${p##*:}"
    [ -e "$pieces/$sha" ] && continue
    n=$((n + 1)); mb=$((mb + size / 1048576))
  done
  [ "$n" -eq 0 ] || log "fetching $n pieces of the Julia package set $set ($triple), $mb MB, for:$(julia_needs | tr '\n' ' ' | sed 's/^/ /')"
  for p in $list; do
    sha="${p%%:*}"
    [ -e "$pieces/$sha" ] && continue
    # A piece that stopped halfway resumes at the next start (curl -C -).
    curl -fsSL --retry 3 -C - -o "$pieces/.dl-$sha" "$repo/chunks/$sha.tar.gz" ||
      { rm -f "$pieces/.dl-$sha"; curl -fsSL --retry 3 -o "$pieces/.dl-$sha" "$repo/chunks/$sha.tar.gz"; } ||
      { echo "env: downloading $repo/chunks/$sha.tar.gz stopped; the next start resumes it" >&2; return 1; }
    [ "$(sha256sum "$pieces/.dl-$sha" | cut -d' ' -f1)" = "$sha" ] ||
      { rm -f "$pieces/.dl-$sha"; echo "env: piece $sha of $set does not match its sha256 - not used" >&2; return 1; }
    tar -xzf "$pieces/.dl-$sha" -C "$cache" && rm -f "$pieces/.dl-$sha" && : > "$pieces/$sha" ||
      { echo "env: unpacking piece $sha of $set failed" >&2; return 1; }
  done
  [ "$n" -eq 0 ] || log "the Julia packages are ready in $cache"
  echo "$cache"
}

# What this project needs: julia:<Name> in APP_NEEDS, the [deps] of its own
# Project.toml when it has one, and every package its code loads - a `using`
# or `import` in its .jl files. Declared and loaded drifted apart in every
# Julia starter (JSON declared, JSON3 loaded), and on a server of its own,
# which fetches only what is named, the app died at its first `using`
# (operator, 2026-11-27, Optimizer Race on ndexrtesting.com: "Package JSON3
# ... is required but does not seem to be installed"). A `using` an agent
# adds is fetched the same way. One name per line; Base, Core and Main are
# Julia itself.
julia_needs() {
  for w in ${APP_NEEDS:-}; do case "$w" in julia:*) echo "${w#julia:}" ;; esac; done
  [ -f Project.toml ] && sed -n '/^\[deps\]/,/^\[/{s/^\([A-Za-z_][A-Za-z0-9_]*\)[[:space:]]*=.*/\1/p}' Project.toml
  find . -maxdepth 3 -name '*.jl' -not -path './.cove/*' -not -path './depot/*' -print0 2>/dev/null |
    xargs -0 -r sed -nE 's/^[[:space:]]*(using|import)[[:space:]]+([A-Z][A-Za-z0-9_]*([[:space:]]*,[[:space:]]*[A-Z][A-Za-z0-9_]*)*).*/\2/p' |
    tr ',' '\n' | tr -d ' \t' | grep -vxE 'Base|Core|Main' | grep .
  return 0
}

if stack_here; then
  set +u
  stack_enter
  avail="$(ml -t avail env/Julia/1.11.7 2>&1 || true)"
  if grep -q 'env/Julia/1.11.7' <<<"$avail"; then
    stack_ml env/Julia/1.11.7
  else
    stack_ml Julia/1.11.7
  fi
  SITE="$(julia_set Julia/1.11.7)" || { echo "env: the Julia package set Julia/1.11.7 is not on this computer and could not be fetched" >&2; exit 127; }
  export JULIA_DEPOT_PATH="$HOME/.julia:$SITE/depot:"
  export JULIA_LOAD_PATH="@:@v#.#:$SITE:@stdlib"
  # A declared package the set holds only as another's dependency (JSON, under
  # HTTP) is not on the load path; Pkg adds it to this person's default
  # environment at the version already installed from the set's pieces
  # (PRESERVE_TIERED_INSTALLED), not the newest. One ndexr has not built at
  # all comes from the General registry and compiles here.
  export JULIA_PKG_PRESERVE_TIERED_INSTALLED=true
  missing="$(julia --startup-file=no -e 'for n in ARGS; Base.find_package(n) === nothing && println(n); end' $(julia_needs | sort -u) 2>/dev/null || true)"
  if [ -n "$missing" ]; then
    printf '%s INFO    [env] not in ndexr'"'"'s Julia set, adding from the General registry: %s\n' "$(date -u '+%Y-%m-%d %H:%M:%S')" "$(echo $missing)" >&2
    julia --startup-file=no -e 'using Pkg; Pkg.add(ARGS)' $missing >&2 || { echo "env: could not add $(echo $missing)" >&2; exit 127; }
  fi
  set -u
fi
command -v julia >/dev/null || { echo "no julia: run where $COVE_STACK_LABEL is mounted" >&2; exit 127; }
export JULIA_CONDAPKG_BACKEND=Null
export PORT="${PORT:-$APP_PORT}"
# One Julia thread per core.
export JULIA_NUM_THREADS="${JULIA_NUM_THREADS:-auto}"
