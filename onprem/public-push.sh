#!/usr/bin/env bash
# The public copy follows the releases: after run.sh publishes one, this puts
# the same source on https://github.com/fdrennan/cove (operator, 2026-11-20:
# "the fact this is on the front page and also not updated with the latest is
# a problem" - it was a week behind, pushed by hand).
#
#   onprem/public-push.sh [release id]
#
# Every release (operator, 2026-11-26: "cove builds going forward must a)
# always be updated in github repo b) github repo branch must always equal
# current version of software for ALL platforms"): `main` and the version's
# own branch, v<Cargo.toml version>, both move to the source of the release
# just published - the source cove.ndexr.io serves and the feed's installers
# were built from (onprem/platforms.sh checks they agree). Until 2026-11-26 it
# ran at most once an hour, so the newest releases of an hour never reached
# GitHub. Releases queue on the lock rather than skip: each carries the one
# before it, and the last one is always pushed. The export is
# public-export.sh's, into a clone of the public repo kept here, so the public
# history is one commit per push. Nothing is pushed when the new commit adds a
# credential-shaped line or a file the export should have left out: that is
# logged and held for a person, never published.
set -euo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
state="$HOME/.local/state/cove-onprem"
dest="$state/public"
stamp="$state/public.stamp"
log="$state/public-push.log"
id="${1:-$(git -C "$here" rev-parse --short HEAD)}"
remote="${COVE_PUBLIC_REMOTE:-git@github.com:fdrennan/cove.git}"
mkdir -p "$state"
say() { echo "$(date -u +%FT%TZ) $*" >>"$log"; }

version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$here/app/Cargo.toml" | head -1)"
branch="v$version"
exec 9>"$state/public.lock"
flock -w 1800 9 || { say "waited 30 min for the push before $id - skipped"; exit 1; }

if [[ ! -d "$dest/.git" ]]; then
  git clone -q "$remote" "$dest" || { say "clone of $remote failed"; exit 1; }
fi
git -C "$dest" fetch -q origin main && git -C "$dest" reset -q --hard origin/main ||
  { say "fetch from $remote failed"; exit 1; }
before="$(git -C "$dest" rev-parse HEAD)"
"$here/onprem/public-export.sh" "$dest" "Cove $version: release $id" >>"$log" 2>&1
if [[ "$(git -C "$dest" rev-parse HEAD)" == "$before" ]]; then
  # Same source as main: the version's branch still has to point at it.
  git -C "$dest" push -q origin "HEAD:refs/heads/$branch" || { say "push of $branch failed"; exit 1; }
  touch "$stamp"
  exit 0
fi

# The gate. Credential shapes in the lines this commit adds, and paths the
# export must never carry - either one holds the push.
hits="$(git -C "$dest" diff "$before" HEAD | grep -E '^\+' | grep -cE \
  'AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY|sk-ant-[A-Za-z0-9_-]{20,}|ndxr_[0-9a-f]{12}_[0-9a-f]{48}|gh[pousr]_[A-Za-z0-9]{30,}|xox[baprs]-[A-Za-z0-9-]{10,}' || true)"
# Dotenv files only: `\.env$` also caught every starter's app.env, the
# project's own settings (port, language), and held every push from
# 2026-11-27 02:04 to 15:40 UTC as "9 private paths" while GitHub sat at 0.1.32
# and ndexr.io's installer differed from GitHub's (operator: "make sure this
# doesnt happen again").
private="$(git -C "$dest" diff --name-only "$before" HEAD | grep -cE '^(CLAUDE|CONTEXT)\.|(^|/)nginx\.conf$|supporting_docs/|stack-requests/|/ud-|(^|/)\.env(\.[^/]*)?$|\.pem$|\.key$' || true)"
if [[ "$hits" != 0 || "$private" != 0 ]]; then
  say "HELD $id: $hits credential-shaped line(s), $private private path(s) - nothing pushed; see git -C $dest show HEAD"
  git -C "$dest" reset -q --hard "$before"
  exit 1
fi
git -C "$dest" push -q origin HEAD:main "HEAD:refs/heads/$branch" || { say "push of $id failed"; exit 1; }
touch "$stamp"
say "pushed $id as $(git -C "$dest" rev-parse --short HEAD) to main and $branch"
# Every platform against this version, for whoever looks next
# (onprem/platforms.sh; Cove's admin bar and ops read the file).
"$here/onprem/platforms.sh" --json > "$state/platforms.json.part" 2>/dev/null; mv -f "$state/platforms.json.part" "$state/platforms.json"
