#!/usr/bin/env bash
# Rebuild: this IDE from recipe/ with the stack's own EasyBuild, exactly as the
# stack builds (its activate.sh: prefix toolchain, eb.env's filter-deps, hooks,
# sysroot, RPATH). Every dependency is already in CVMFS, so only the IDE is
# compiled. It installs into your own folder, never the stack, and your build
# comes before the stack's in the module path from then on.
#
#   ./rebuild.sh             build recipe/*.eb (copy it there first: ./recipe.sh)
set -euo pipefail
cd "$(dirname "$0")"
. .cove/app.env
here="$(pwd -P)"

triple="$(uname -m)-pc-linux-gnu"
[ "$triple" = aarch64-pc-linux-gnu ] && triple=aarch64-unknown-linux-gnu
stack="/opt/ndexr/stack/$triple"
[ -r "$stack/activate.sh" ] || { echo "rebuild: no $stack/activate.sh - run where the ndexr stack is mounted" >&2; exit 127; }
ec="$(ls recipe/*.eb 2>/dev/null | head -1)"
[ -n "$ec" ] || { echo "rebuild: no recipe/*.eb - run ./recipe.sh first" >&2; exit 1; }

prefix="${COVE_EB_PREFIX:-$HOME/.local/easybuild/${APP_HOST:-$(basename "$here")}}"
sources="${COVE_EB_SOURCES:-$HOME/.local/easybuild/sources}"
mkdir -p "$prefix" "$sources"

# activate.sh is written for an interactive shell: no set -u, and it reads $1.
set +u
. "$stack/activate.sh" "" >/dev/null
set -u
command -v eb >/dev/null || { echo "rebuild: the stack's EasyBuild did not activate" >&2; exit 127; }
# GWT's compiler watches the source tree with inotify; a host short of
# watches stopped it ("User limit of inotify watches reached", 2026-11-07).
# A one-off compile needs no watching.
export JAVA_TOOL_OPTIONS="${JAVA_TOOL_OPTIONS:+$JAVA_TOOL_OPTIONS }-Dgwt.watchFileChanges=false"

echo "rebuild: $ec -> $prefix (EasyBuild $(eb --version | sed -n 's/.*EasyBuild \([0-9.]*\).*/\1/p'), every dependency from $stack)"
# --rebuild builds the recipe even though the stack has a module of that name;
# its dependencies resolve from CVMFS and are never rebuilt. Sources come from
# the recipe's own URLs, else ndexr's public source mirror, and must match the
# sha256 the recipe carries; a patch of yours needs no checksum.
eb "$here/$ec" --robot --rebuild --disable-enforce-checksums \
  --robot-paths="$here/recipe:$stack/easybuild-suite/easyconfigs:" \
  --installpath="$prefix" --sourcepath="$sources" \
  --extra-source-urls="https://ndexr-stack-public.s3.us-east-2.amazonaws.com/stack/sources"
echo "rebuild: done - Down then Up runs your build ($prefix/modules/all)"
