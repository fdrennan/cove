#!/usr/bin/env bash
# Build and install ndexr desktop on this Mac from source, in one line:
#
#   curl -fsSL https://cove.ndexr.io/domains/drennanfreddy/cove.ndexr.io/images/src/install-mac.sh | bash
#
# Until a signed .dmg is published to repo.ndexr.io/ndexr/cove/darwin/, this
# is the install path. It needs the Xcode command-line tools (it asks for them),
# installs Rust via rustup if missing, builds for this Mac's arch (UNIVERSAL=1
# for arm64 + x86_64), puts Cove.app in /Applications (or ~/Applications), and
# opens it. Re-run it to rebuild from the latest source.
set -euo pipefail

src_url="${COVE_SRC:-https://cove.ndexr.io/domains/drennanfreddy/cove.ndexr.io/images/src/cove-src.tar.gz}"
work="$HOME/.ndexr/cove-src"

say() { printf '\033[1;34m▸\033[0m %s\n' "$*"; }
die() { printf '\033[1;31m✗\033[0m %s\n' "$*" >&2; exit 1; }

[[ "$(uname -s)" == Darwin ]] || die "this installer is for macOS"

if ! xcode-select -p >/dev/null 2>&1; then
  xcode-select --install 2>/dev/null || true
  die "install the Xcode command-line tools from the dialog, then re-run this command"
fi

# Rust ≥ 1.95 (egui 0.36).
[[ -f "$HOME/.cargo/env" ]] && . "$HOME/.cargo/env"
minor() { rustc --version 2>/dev/null | awk '{print $2}' | cut -d. -f2; }
if ! command -v rustup >/dev/null; then
  say "installing Rust (rustup, minimal profile)"
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path
  . "$HOME/.cargo/env"
elif [[ "$(minor)" -lt 95 ]]; then
  say "updating Rust to stable"
  rustup update stable
fi

say "fetching source"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "$src_url" -o "$tmp/src.tar.gz"
curl -fsSL "$src_url.sha256" -o "$tmp/src.sha256"
[[ "$(shasum -a 256 "$tmp/src.tar.gz" | cut -d' ' -f1)" == "$(cut -d' ' -f1 "$tmp/src.sha256")" ]] ||
  die "source checksum mismatch — try again in a minute"
rm -rf "$work" && mkdir -p "$work"
tar -xzf "$tmp/src.tar.gz" -C "$work"

case "$(uname -m)" in
  arm64)  native=aarch64-apple-darwin ;;
  x86_64) native=x86_64-apple-darwin ;;
  *)      die "unsupported arch $(uname -m)" ;;
esac
targets="$native"
[[ "${UNIVERSAL:-0}" == 1 ]] && targets="aarch64-apple-darwin x86_64-apple-darwin"

say "building ($targets) — first build takes a few minutes"
NDEXR_CRATE="${NDEXR_CRATE-}" TARGETS="$targets" bash "$work/build-dmg.sh"

dest=/Applications
[[ -w "$dest" ]] || { dest="$HOME/Applications"; mkdir -p "$dest"; }
say "installing to $dest/Cove.app"
osascript -e 'tell application "ndexr" to quit' >/dev/null 2>&1 || true
rm -rf "$dest/Cove.app"
ditto "$work/target/bundle/Cove.app" "$dest/Cove.app"

printf '\033[1;32m✓\033[0m ndexr %s installed — opening it\n' \
  "$(/usr/libexec/PlistBuddy -c 'Print CFBundleShortVersionString' "$dest/Cove.app/Contents/Info.plist")"
open "$dest/Cove.app"
