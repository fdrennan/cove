#!/usr/bin/env bash
# VS Code's own source of truth into recipe/: the easyconfig the stack built
# it from and any patches, copied from the installed module. Add a patch (and
# its name to the easyconfig's patches = [...]) and ./rebuild.sh builds your
# VS Code with EasyBuild, as the stack does.
#
#   ./recipe.sh          copy them once; an existing recipe/ is left alone
set -euo pipefail
cd "$(dirname "$0")"
. .cove/node/env.sh
[ -d recipe ] && { echo "recipe: recipe/ is already here - left as it is"; exit 0; }
mkdir recipe
cp "$EBROOTCODEMINSERVER"/easybuild/*.eb recipe/
cp "$EBROOTCODEMINSERVER"/easybuild/*.patch recipe/ 2>/dev/null || true
chmod u+w recipe/*
echo "recipe: $(ls recipe) from $EBROOTCODEMINSERVER"
