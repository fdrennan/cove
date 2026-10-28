#!/usr/bin/env bash
# The RStudio source cove.ndexr.io compares against and builds out from:
# .rstudio-src/, a clone of rstudio/rstudio at the tag the stack's easyconfigs
# build (RStudio-Server and Inlet, 2026.04.0+526 = 57f8932a), with
# two refs to compare:
#
#   v2026.04.0+526   upstream, as RStudio-Server-*.eb builds it (bare)
#   inlet            that tag + ndexr.ndexr.io's i/Inlet/ patch series,
#                    one commit per patch, in the easyconfig's order
#
#   git -C .rstudio-src diff v2026.04.0+526..inlet     everything Inlet adds
#   git -C .rstudio-src log --oneline inlet            one line per patch
#
# The patches live in ndexr.ndexr.io and are never copied here. Re-run this
# script when they change: it rebuilds `inlet` from the tag and fails if a patch
# no longer applies.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
src="$here/.rstudio-src"
tag='v2026.04.0+526'
ec="$here/../../ndexr.ndexr.io/crate/assets/stack-kit/easybuild-suite/easyconfigs/i/Inlet"
eb="$ec/Inlet-2026.04.0+526-foss-2024a-R-4.6.1.eb"

[[ -d "$src/.git" ]] || git clone -q --depth 1 --branch "$tag" https://github.com/rstudio/rstudio.git "$src"
git -C "$src" reset -q --hard && git -C "$src" clean -qfd
git -C "$src" checkout -q --detach "$tag"
git -C "$src" checkout -q -B inlet

# The easyconfig's `patches` list is the order, not the file names on disk.
mapfile -t patches < <(grep -oE "'[^']+\.patch'" "$eb" | tr -d "'" | awk '!seen[$0]++')
for p in "${patches[@]}"; do
  f="$ec/$p"
  [[ -f "$f" ]] || f="$(find "$ec/.." -name "$p" -print -quit)"
  [[ -f "$f" ]] || { echo "missing patch: $p" >&2; exit 1; }
  (cd "$src" && patch -p1 --no-backup-if-mismatch -s < "$f") || { echo "does not apply: $p" >&2; exit 1; }
  git -C "$src" add -A
  git -C "$src" -c user.name=ndexr -c user.email=ndexr@ndexr.io \
    commit -q -m "${p%.patch}" -m "From ndexr.ndexr.io easyconfigs/i/Inlet/$p"
  echo "applied ${p%.patch}"
done
git -C "$src" diff --stat "$tag"..inlet | tail -1
