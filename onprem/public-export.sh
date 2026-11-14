#!/usr/bin/env bash
# The public copy of Cove, https://github.com/fdrennan/cove (app.rs SOURCE_URL):
# this working tree's files, less what is private to the platform (operator,
# 2026-10-28: "check cove repo for any problems before open sourcing";
# 2026-11-14: "the gitlab repo for cove.ndexr.io should be public and
# accessible").
#
#   onprem/public-export.sh [dest] [message]    default ~/scratch/cove-public
#
# A new dest is a new repository with one commit. A dest that is already the
# public repository (a clone of it) gets the files as they are now in one more
# commit - nothing when nothing changed - so its history is the releases'.
# This repository's own history keeps phone photos, a work address on six
# commits and Chrome profiles, so the public one never carries it. Nothing is
# pushed here: the push is the publisher's (`git -C dest push`).
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
dest="${1:-$HOME/scratch/cove-public}"
message="${2:-Cove $(date -u +%Y-%m-%dT%H:%MZ)}"
author="${COVE_PUBLIC_AUTHOR:-Freddy Drennan <fdrennan@ndexr.io>}"

# Private to the platform: the agents' working notes and their HTML renders,
# the operator's screenshots, the vhost, and build requests to another domain.
private='^(CLAUDE\.(md|html)|CONTEXT\.(md|html)|nginx\.conf|supporting_docs/|stack-requests/|app/ud-[^/]*/)'

if [[ -d "$dest/.git" ]]; then
  # Every tracked file out, then today's in: a file deleted here goes there too.
  git -C "$dest" rm -rq --ignore-unmatch . >/dev/null
elif [[ -e "$dest" ]]; then
  echo "$dest exists and is not a git repository - remove it or name another" >&2
  exit 1
else
  mkdir -p "$dest"
  git -C "$dest" init -q -b main
  message="Cove: initial public release"
fi
git -C "$here" ls-files -z | grep -zEv "$private" | (cd "$here" && xargs -0 cp --parents -t "$dest")

git -C "$dest" add -A
if git -C "$dest" diff --cached --quiet && git -C "$dest" rev-parse -q --verify HEAD >/dev/null; then
  echo "✓ $dest: nothing changed since $(git -C "$dest" log -1 --format=%s)"
  exit 0
fi
git -C "$dest" -c user.name="${author% <*}" -c user.email="$(sed 's/.*<\(.*\)>/\1/' <<<"$author")" \
  commit -q -m "$message"
echo "✓ $dest: $(git -C "$dest" ls-files | wc -l) files, $(git -C "$dest" rev-list --count HEAD) commit(s), $(du -sh "$dest/.git" | cut -f1) of history"
