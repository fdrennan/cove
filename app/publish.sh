#!/usr/bin/env bash
# Ship whatever the build scripts left in dist/<platform>/ to repo.ndexr.io —
# the one place ndexr software is delivered, beside the CLI at /ndexr/<arch>/.
#
#   ./build-dmg.sh   (Mac)    → dist/darwin/cove.{dmg,tar.gz}
#   ./build-linux.sh (Linux)  → dist/linux-<arch>/cove.tar.gz
#   ./build-deb.sh   (Linux)  → dist/deb/cove_<version>_<arch>.deb
#   ./build-windows.sh (Linux, cross) → dist/windows-<arch>/cove.{zip,tar.gz}
#   ./publish.sh [--dry-run] [platform...]     e.g. linux-x86_64 deb; none = every dist/ folder
#
# Name the platforms that were built for this version: dist/ keeps every
# platform's last build, and publishing all of it would label an old Mac or
# Windows build with the new version (2026-11-14: the feed sat at 0.1.8 from
# 2026-11-03, so a new laptop installed a Cove eleven days old).
#
# Layout (s3://ndexr/ndexr/cove/ is served at https://repo.ndexr.io/ndexr/cove/):
#   <platform>/<version>/<file>, <file>.sha256    immutable, every release kept
#   <platform>/latest/<file>,    <file>.sha256    stable wget URLs (a .deb drops
#                                                its version: cove_<arch>.deb)
#   <platform>/latest/release.json               the updater's pointer (src/update.rs)
#   manifest.json                                every platform's release.json, for the site
#
# Order matters: versioned copies, then latest files, then release.json, then
# manifest.json — a reader never sees a pointer to something not yet uploaded.
set -euo pipefail
# Never while a build writes dist/: onprem/desktop-push.sh builds the Linux
# tarballs and .debs under this lock, and a publish beside one uploaded a
# half-written .deb (2026-11-02).
lockdir="${XDG_STATE_HOME:-$HOME/.local/state}/cove-desktop"; mkdir -p "$lockdir"
exec 8> "$lockdir/build.lock"
flock -n 8 || { echo "▸ a build is writing dist/ - waiting for it"; flock 8; }

here="$(cd "$(dirname "$0")" && pwd)"
bucket="${NDEXR_DIST_BUCKET:-s3://ndexr/ndexr/cove}"
public="${NDEXR_DIST_URL:-https://repo.ndexr.io/ndexr/cove}"
version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$here/Cargo.toml" | head -1)"
dry=""; [[ "${1:-}" == --dry-run ]] && { dry="--dryrun"; shift; }

sha() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
bytes() { wc -c < "$1" | tr -d ' '; }
put() { aws s3 cp $dry --only-show-errors "$@"; }

shopt -s nullglob
if (( $# )); then
  platforms=()
  for p in "$@"; do
    [[ -d "$here/dist/$p" ]] || { echo "no dist/$p - build it first" >&2; exit 1; }
    platforms+=("$here/dist/$p/")
  done
else
  platforms=("$here"/dist/*/)
fi
(( ${#platforms[@]} )) || { echo "nothing in dist/ — run build-dmg.sh or build-linux.sh first" >&2; exit 1; }

for dir in "${platforms[@]}"; do
  p="$(basename "$dir")"
  # A platform ships its tarball, or (dist/deb) its .debs — which the updater
  # never reads, so release.json's "file" names the first download there.
  files=("$dir"cove.* "$dir"cove_*.deb)
  main="$dir/cove.tar.gz"
  # `|| true`: no .deb (dist/web, a darwin build) is not an error - under
  # set -e + pipefail the failed grep ended the whole publish there, before
  # manifest.json (2026-10-13).
  [[ -f "$main" ]] || main="$(printf '%s\n' "${files[@]}" | grep '\.deb$' | head -1 || true)"
  [[ -n "$main" && -f "$main" ]] || { echo "skip $p: no cove.tar.gz or .deb" >&2; continue; }
  echo "▸ $p v$version"

  latest_name() { local n; n="$(basename "$1")"; [[ "$n" == *.deb ]] && n="$(sed "s/_${version}_/_/" <<<"$n")"; echo "$n"; }

  downloads=()
  for f in "${files[@]}"; do
    [[ "$f" == *.sha256 ]] && continue
    n="$(basename "$f")"; s="$(sha "$f")"
    echo "$s  $n" > "$f.sha256"
    put "$f"        "$bucket/$p/$version/$n"
    put "$f.sha256" "$bucket/$p/$version/$n.sha256"
    downloads+=("{\"name\":\"$(latest_name "$f")\",\"sha256\":\"$s\",\"bytes\":$(bytes "$f"),\"url\":\"$public/$p/latest/$(latest_name "$f")\"}")
  done
  for f in "${files[@]}"; do
    [[ "$f" == *.sha256 ]] && continue
    l="$(latest_name "$f")"
    put "$f" "$bucket/$p/latest/$l"
    echo "$(sha "$f")  $l" | put - "$bucket/$p/latest/$l.sha256"
  done

  rel="$dir/release.json"
  printf '{"version":"%s","platform":"%s","file":"%s","sha256":"%s","published_at":"%s","downloads":[%s]}\n' \
    "$version" "$p" "$(latest_name "$main")" "$(sha "$main")" "$(date -u +%FT%TZ)" "$(IFS=,; echo "${downloads[*]}")" > "$rel"
  put "$rel" "$bucket/$p/latest/release.json" --content-type application/json --cache-control no-cache
  echo "  ✓ $public/$p/latest/"
done

# manifest.json: rebuilt from every platform's live pointer, so a Mac publish
# and a Linux publish never clobber each other's entry.
entries=()
for p in $(aws s3 ls "$bucket/" | awk '/PRE/ {sub("/","",$2); print $2}'); do
  # A read that fails is retried, and a platform still unread keeps the old
  # manifest: skipping it dropped darwin from the download page once (a Linux
  # publish, 2026-11-19) while its files were all there.
  r="$(aws s3 cp "$bucket/$p/latest/release.json" - 2>/dev/null || { sleep 2; aws s3 cp "$bucket/$p/latest/release.json" - 2>/dev/null; })" || {
    aws s3 ls "$bucket/$p/latest/release.json" >/dev/null 2>&1 || continue
    echo "✗ could not read $p/latest/release.json - manifest.json left as it was" >&2; exit 1
  }
  entries+=("\"$p\":$r")
done
manifest="$(mktemp)"
printf '{"product":"cove","base":"%s","platforms":{%s}}\n' "$public" "$(IFS=,; echo "${entries[*]}")" > "$manifest"
put "$manifest" "$bucket/manifest.json" --content-type application/json --cache-control no-cache
rm -f "$manifest"
echo "✓ $public/manifest.json"
