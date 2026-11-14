#!/usr/bin/env bash
# Inlet's own source of truth into recipe/: the easyconfig the stack built it
# from and every patch it applies to RStudio, copied from the installed module.
# Change a patch or add one, and that recipe builds your variant with EasyBuild
# (hours: it compiles RStudio). Most changes need none of this - config/ first.
#
#   ./recipe.sh          copy them once; an existing recipe/ is left alone
set -euo pipefail
cd "$(dirname "$0")"
. .cove/r/env.sh
[ -d recipe ] && { echo "recipe: recipe/ is already here - left as it is"; exit 0; }
mkdir recipe
cp "$EBROOTINLET"/easybuild/*.eb "$EBROOTINLET"/easybuild/*.patch recipe/
echo "recipe: $(ls recipe/*.patch | wc -l) patches and $(ls recipe/*.eb) from $EBROOTINLET"
