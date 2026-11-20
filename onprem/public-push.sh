#!/usr/bin/env bash
# The public copy follows the releases: after run.sh publishes one, this puts
# the same source on https://github.com/fdrennan/cove (operator, 2026-11-20:
# "the fact this is on the front page and also not updated with the latest is
# a problem" - it was a week behind, pushed by hand).
#
#   onprem/public-push.sh [release id]
#
# At most once an hour (a release can land every few minutes). The export is
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

[[ -n "$(find "$stamp" -mmin -60 2>/dev/null)" ]] && exit 0
exec 9>"$state/public.lock"
flock -n 9 || exit 0

if [[ ! -d "$dest/.git" ]]; then
  git clone -q "$remote" "$dest" || { say "clone of $remote failed"; exit 1; }
fi
git -C "$dest" fetch -q origin main && git -C "$dest" reset -q --hard origin/main ||
  { say "fetch from $remote failed"; exit 1; }
before="$(git -C "$dest" rev-parse HEAD)"
"$here/onprem/public-export.sh" "$dest" "Cove: release $id" >>"$log" 2>&1
if [[ "$(git -C "$dest" rev-parse HEAD)" == "$before" ]]; then
  touch "$stamp"
  exit 0
fi

# The gate. Credential shapes in the lines this commit adds, and paths the
# export must never carry - either one holds the push.
hits="$(git -C "$dest" diff "$before" HEAD | grep -E '^\+' | grep -cE \
  'AKIA[0-9A-Z]{16}|-----BEGIN [A-Z ]*PRIVATE KEY|sk-ant-[A-Za-z0-9_-]{20,}|ndxr_[0-9a-f]{12}_[0-9a-f]{48}|gh[pousr]_[A-Za-z0-9]{30,}|xox[baprs]-[A-Za-z0-9-]{10,}' || true)"
private="$(git -C "$dest" diff --name-only "$before" HEAD | grep -cE '^(CLAUDE|CONTEXT)\.|(^|/)nginx\.conf$|supporting_docs/|stack-requests/|/ud-|\.env$|\.pem$|\.key$' || true)"
if [[ "$hits" != 0 || "$private" != 0 ]]; then
  say "HELD $id: $hits credential-shaped line(s), $private private path(s) - nothing pushed; see git -C $dest show HEAD"
  git -C "$dest" reset -q --hard "$before"
  exit 1
fi
git -C "$dest" push -q origin HEAD:main || { say "push of $id failed"; exit 1; }
touch "$stamp"
say "pushed $id as $(git -C "$dest" rev-parse --short HEAD)"
