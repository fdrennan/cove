#!/bin/sh
# Build and install ndexr desktop on a Mac from source — the path until
# repo.ndexr.io/ndexr/cove/darwin/ has a published build (then use install.sh).
#
#   curl -fsSL https://cove.ndexr.io/domains/drennanfreddy/cove.ndexr.io/images/bootstrap/bootstrap-mac.sh | sh
#
# Installs, if missing: Xcode command-line tools (prompts once), rustup + stable.
# Builds Apple-silicon only (UNIVERSAL=1 for arm64 + x86_64), installs
# Cove.app into /Applications (or ~/Applications), and opens it.
set -eu

src="https://cove.ndexr.io/domains/drennanfreddy/cove.ndexr.io/images/bootstrap"
work="$HOME/.ndexr/cove-src"

[ "$(uname -s)" = Darwin ] || { echo "bootstrap-mac.sh is for macOS" >&2; exit 1; }

if ! xcode-select -p >/dev/null 2>&1; then
  xcode-select --install || true
  echo "▸ Finish the Xcode command-line tools install, then run this again." >&2
  exit 1
fi

[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
if ! command -v rustup >/dev/null 2>&1; then
  echo "▸ installing rustup"
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable
  . "$HOME/.cargo/env"
fi
rustup update stable >/dev/null 2>&1 || true
rustup default stable >/dev/null

echo "▸ fetching source"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$src/cove-src.tar.gz" -o "$tmp/src.tar.gz"
curl -fsSL "$src/cove-src.tar.gz.sha256" -o "$tmp/src.sha256"
[ "$(shasum -a 256 "$tmp/src.tar.gz" | cut -d' ' -f1)" = "$(cut -d' ' -f1 "$tmp/src.sha256")" ] ||
  { echo "sha256 mismatch for source tarball" >&2; exit 1; }
rm -rf "$work" && mkdir -p "$work"
tar -xzf "$tmp/src.tar.gz" -C "$work"

targets="aarch64-apple-darwin"
[ -n "${UNIVERSAL:-}" ] && targets="aarch64-apple-darwin x86_64-apple-darwin"
echo "▸ building ($targets) — a few minutes on first run"
TARGETS="$targets" NDEXR_CRATE= "$work/build-dmg.sh"

dest=/Applications
[ -w "$dest" ] || { dest="$HOME/Applications"; mkdir -p "$dest"; }
tar -xzf "$work/dist/darwin/cove.tar.gz" -C "$tmp"
rm -rf "$dest/Cove.app"
mv "$tmp/Cove.app" "$dest/Cove.app"
echo "✓ $dest/Cove.app"

if ! command -v ndexr >/dev/null 2>&1; then
  echo "  note: no ndexr CLI on PATH yet — there is no published macOS build of it."
  echo "        The app runs; Console/Workspace actions need ndexr once it is installed."
fi
open "$dest/Cove.app"
