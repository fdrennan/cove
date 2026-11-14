#!/usr/bin/env bash
# RStudio's source as a git repository in src/, laid out so you can read how
# Inlet came to be and then work in it directly - no patch files by hand:
#
#   upstream   the RStudio release Inlet builds from: the exact tarball
#              EasyBuild uses (checked against the easyconfig's sha256)
#   inlet      upstream + Inlet's patches, one commit per patch, in the
#              easyconfig's order - `git log upstream..inlet`, `git show`
#   work       where you edit: branched from inlet, checked out
#
# Then ./recipe.sh turns your work into patches and ./rebuild.sh builds it.
#
#   ./source.sh      once (192 MB download); an existing src/ is left alone
set -euo pipefail
cd "$(dirname "$0")"
. .cove/r/env.sh
[ -d src/.git ] && { echo "source: src/ is already here - left as it is"; exit 0; }

ec="$(ls "$EBROOTINLET"/easybuild/Inlet-*.eb | head -1)"
ver="$(sed -n "s/^version = '\(.*\)'/\1/p" "$ec")"
tarball="v$ver.tar.gz"
sum="$(grep -oE "'v%\(version\)s\.tar\.gz': '[0-9a-f]{64}'" "$ec" | grep -oE '[0-9a-f]{64}')"
# Where EasyBuild looks for it too (rebuild.sh's --sourcepath), so the build
# never downloads it again.
cache="${COVE_EB_SOURCES:-$HOME/.local/easybuild/sources}/i/Inlet"
mkdir -p "$cache"
if ! echo "$sum  $cache/$tarball" | sha256sum -c --quiet - 2>/dev/null; then
  echo "source: fetching RStudio $ver (192 MB) from ndexr's source mirror"
  curl -fsSL --retry 3 -o "$cache/$tarball.part" \
    "https://ndexr-stack-public.s3.us-east-2.amazonaws.com/stack/sources/i/Inlet/v${ver/+/%2B}.tar.gz"
  mv "$cache/$tarball.part" "$cache/$tarball"
  echo "$sum  $cache/$tarball" | sha256sum -c --quiet - || { echo "source: $tarball does not match the easyconfig's sha256" >&2; exit 1; }
fi

mkdir -p src
tar -xzf "$cache/$tarball" -C src --strip-components=1
g() { git -C src -c user.name="${GIT_AUTHOR_NAME:-$(id -un)}" -c user.email="${GIT_AUTHOR_EMAIL:-$(id -un)@ndexr.io}" "$@"; }
g init -q -b upstream
g add -A
g commit -q -m "RStudio $ver (upstream release, $tarball)"
g tag upstream
g checkout -q -b inlet
# The easyconfig's `patches` list is the order.
n=0
for p in $(grep -oE "'[^']+\.patch'" "$ec" | tr -d "'" | awk '!seen[$0]++'); do
  (cd src && patch -p1 --no-backup-if-mismatch -s <"$EBROOTINLET/easybuild/$p") || { echo "source: $p does not apply" >&2; exit 1; }
  g add -A
  g commit -q -m "${p%.patch}" -m "Inlet's patch $p, from the stack's recipe ($EBROOTINLET/easybuild)"
  n=$((n + 1))
done
g checkout -q -b work
echo "source: src/ - upstream, $n Inlet patches as commits on inlet, and your branch work checked out"
echo "        git -C src log --oneline upstream..inlet    every Inlet patch"
