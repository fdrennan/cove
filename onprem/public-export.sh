#!/usr/bin/env bash
# The public copy of Cove: this working tree's files, less what is private to
# the platform, as a new repository with one commit and no history (operator,
# 2026-10-28: "check cove repo for any problems before open sourcing").
#
#   onprem/public-export.sh [dest]        default ~/scratch/cove-public
#
# This repository's history keeps phone photos, a work address on six
# commits and Chrome profiles, so the public one starts fresh rather
# than rewriting this one. Nothing is pushed: add a remote to dest and push
# it yourself.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
dest="${1:-$HOME/scratch/cove-public}"
author="${COVE_PUBLIC_AUTHOR:-Freddy Drennan <fdrennan@ndexr.io>}"

# Private to the platform: the agents' working notes and their HTML renders,
# the operator's screenshots, the vhost, and build requests to another domain.
private='^(CLAUDE\.(md|html)|CONTEXT\.(md|html)|nginx\.conf|supporting_docs/|stack-requests/|app/ud-[^/]*/)'

[[ -e "$dest" ]] && { echo "$dest exists - remove it or name another" >&2; exit 1; }
mkdir -p "$dest"
git -C "$here" ls-files -z | grep -zEv "$private" | (cd "$here" && xargs -0 cp --parents -t "$dest")

git -C "$dest" init -q -b main
git -C "$dest" add -A
git -C "$dest" -c user.name="${author% <*}" -c user.email="$(sed 's/.*<\(.*\)>/\1/' <<<"$author")" \
  commit -q -m "Cove: initial public release"
echo "✓ $dest: $(git -C "$dest" ls-files | wc -l) files, $(du -sh "$dest/.git" | cut -f1) of history"
