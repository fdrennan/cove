#!/usr/bin/env bash
# Build the UI for the browser — what `cove serve` hands out — into
# dist/web/: index.html, cove.js, cove_bg.wasm, icon.svg.
#
#   ./build-web.sh
#
# Needs: rustup's wasm32-unknown-unknown target, and wasm-bindgen-cli at the
# exact version Cargo.lock pins (checked below). wasm-opt is used if present.
# The bundle is the same on every OS and arch: build it once (neo does, in
# neo.svbt.ndexr.io/neo/build-desktop.sh) and ship it in every package.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
out="$here/dist/web"
want="$(awk '/^name = "wasm-bindgen"$/{getline; gsub(/version = |"/,""); print; exit}' "$here/Cargo.lock")"
have="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')"
[[ "$have" == "$want" ]] || { echo "need wasm-bindgen-cli $want (have: ${have:-none}): cargo install wasm-bindgen-cli --version $want --locked" >&2; exit 1; }
rustup target add wasm32-unknown-unknown >/dev/null

echo "▸ building the UI for wasm32"
cargo build --release --target wasm32-unknown-unknown --manifest-path "$here/Cargo.toml"
rm -rf "$out" && mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" \
  "$here/target/wasm32-unknown-unknown/release/cove.wasm"
if command -v wasm-opt >/dev/null; then
  wasm-opt -O3 -o "$out/cove_bg.wasm" "$out/cove_bg.wasm"
fi
cp "$here/web/index.html" "$out/"
cp "$here/packaging/linux/io.ndexr.cove.svg" "$out/icon.svg"
# A web app on a phone's Home Screen opens full screen: its manifest and icon.
cp "$here/web/manifest.webmanifest" "$out/"
cp "$here/packaging/macos/cove-icon-1024.png" "$out/icon.png"
# The loading page's photograph - the one backdrop.rs paints behind the app.
cp "$here/assets/backdrops/star-trails-1920.webp" "$out/splash.webp"
echo "✓ $out ($(du -sh "$out" | cut -f1))"
