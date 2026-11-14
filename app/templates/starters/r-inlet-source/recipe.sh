#!/usr/bin/env bash
# Your work in src/ as an EasyBuild recipe in recipe/: each commit on `work`
# since `inlet` becomes one numbered patch (git format-patch), and what is not
# committed yet one more; then Inlet's own easyconfig, with those patches added
# after Inlet's. recipe/ is saved with the project - it is your work, small
# enough to keep; src/ is not.
#
#   ./recipe.sh      after editing in src/ (rebuild.sh runs it first)
set -euo pipefail
cd "$(dirname "$0")"
. .cove/r/env.sh
[ -d src/.git ] || { echo "recipe: no src/ - run ./source.sh first" >&2; exit 1; }

rm -rf recipe
mkdir -p recipe
cp "$EBROOTINLET"/easybuild/*.patch recipe/
ec="$(basename "$(ls "$EBROOTINLET"/easybuild/Inlet-*.eb | head -1)")"
cp "$EBROOTINLET/easybuild/$ec" "recipe/$ec"
chmod u+w recipe/*

mine=()
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
git -C src format-patch -q --no-signature --zero-commit -o "$tmp" inlet..work
for f in "$tmp"/*.patch; do
  [ -e "$f" ] || continue
  name="Inlet-mine-$(basename "$f")"
  cp "$f" "recipe/$name"
  mine+=("$name")
done
if ! git -C src diff --quiet HEAD; then
  name="Inlet-mine-9999-not-committed.patch"
  git -C src diff HEAD >"recipe/$name"
  mine+=("$name")
fi
if [ ${#mine[@]} -gt 0 ]; then
  {
    echo
    echo "# Your work in src/ (recipe.sh): applied after Inlet's own patches, in order."
    echo "patches += ["
    for p in "${mine[@]}"; do echo "    '$p',"; done
    echo "]"
  } >>"recipe/$ec"
fi
echo "recipe: recipe/$ec - Inlet's patches and ${#mine[@]} of yours: ${mine[*]:-none yet}"
