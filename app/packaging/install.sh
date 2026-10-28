#!/bin/sh
# Install or update ndexr desktop from repo.ndexr.io, on any platform it ships for.
# Published as https://repo.ndexr.io/ndexr/cove/install.sh by app/publish.sh.
#
#   curl -fsSL https://repo.ndexr.io/ndexr/cove/install.sh | sh
#   wget -qO- https://repo.ndexr.io/ndexr/cove/install.sh | sh
#   … | PREFIX=/usr/local sudo -E sh          # Linux: system-wide instead of ~/.local
#
# macOS  → Cove.app in /Applications (or ~/Applications if that is not writable)
# Linux  → the tarball's own install.sh: ~/.local/bin/cove + launcher
set -eu

base="${COVE_FEED:-https://repo.ndexr.io/ndexr/cove}"

case "$(uname -s)-$(uname -m)" in
  Darwin-*)                      platform=darwin ;;
  Linux-x86_64|Linux-amd64)      platform=linux-x86_64 ;;
  Linux-aarch64|Linux-arm64)     platform=linux-aarch64 ;;
  *) echo "ndexr desktop is not published for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

fetch() { # fetch <url> <dest>
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
  else wget -qO "$2" "$1"; fi
}
sha() {
  if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
url="$base/$platform/latest/cove.tar.gz"
echo "▸ $url"
fetch "$url" "$tmp/cove.tar.gz"
fetch "$url.sha256" "$tmp/cove.tar.gz.sha256"
[ "$(sha "$tmp/cove.tar.gz")" = "$(cut -d' ' -f1 "$tmp/cove.tar.gz.sha256")" ] ||
  { echo "sha256 mismatch for $url" >&2; exit 1; }
tar -xzf "$tmp/cove.tar.gz" -C "$tmp"

if [ "$platform" = darwin ]; then
  dest=/Applications
  [ -w "$dest" ] || { dest="$HOME/Applications"; mkdir -p "$dest"; }
  xattr -dr com.apple.quarantine "$tmp/Cove.app" 2>/dev/null || true
  rm -rf "$dest/Cove.app.old"
  [ -d "$dest/Cove.app" ] && mv "$dest/Cove.app" "$dest/Cove.app.old"
  mv "$tmp/Cove.app" "$dest/Cove.app"
  rm -rf "$dest/Cove.app.old"
  echo "✓ $dest/Cove.app — open it from Launchpad, or: open '$dest/Cove.app'"
else
  for d in "$tmp"/cove-*/; do
    [ -x "$d/install.sh" ] && exec "$d/install.sh"
  done
  echo "tarball has no install.sh" >&2
  exit 1
fi
