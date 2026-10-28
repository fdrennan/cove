#!/usr/bin/env bash
# Build the universal (Apple silicon + Intel) Cove.app on a Mac and drop it in
# dist/darwin/ under stable names; ./publish.sh ships it to repo.ndexr.io.
#
#   ./build-dmg.sh                       # app + bundled ndexr CLI, if the crate builds
#   NDEXR_CRATE= ./build-dmg.sh          # skip bundling the CLI
#   ./build-dmg.sh && ./publish.sh
#
#   dist/darwin/cove.dmg        first install: drag to Applications
#   dist/darwin/cove.tar.gz     Cove.app — what in-place updates swap in
#
# Needs: rustup (stable ≥ 1.95), Xcode command-line tools (lipo, codesign, hdiutil).
# The ndexr CLI is not published for darwin, so it is built here and shipped in
# Contents/Resources/bin; the app puts that dir at the END of PATH, so an ndexr
# you install yourself always wins.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
# TARGETS="aarch64-apple-darwin" for a quick Apple-silicon-only build.
read -r -a targets <<< "${TARGETS:-aarch64-apple-darwin x86_64-apple-darwin}"
version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$here/Cargo.toml" | head -1)"
crate="${NDEXR_CRATE-$here/../../../ndexr.ndexr.io/crate}"
work="$here/target/bundle"
app="$work/Cove.app"
installed="Cove.app"
out="$here/dist/darwin"

[[ "$(uname -s)" == Darwin ]] || { echo "build-dmg.sh runs on macOS only" >&2; exit 1; }
rustup target add "${targets[@]}" >/dev/null

# universal <manifest> <bin> <dest> [features]: build every target, lipo them into one.
universal() {
  local slices=()
  for t in "${targets[@]}"; do
    cargo build --release --target "$t" --manifest-path "$1" --bin "$2" ${4:+--features "$4"}
    slices+=("$(dirname "$1")/target/$t/release/$2")
  done
  lipo -create -output "$3" "${slices[@]}"
}

rm -rf "$work" "$out" && mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/bin" "$out"

echo "▸ building cove $version (${targets[*]})"
# webview: the App panel's embedded WKWebView (webview.rs) - the desktop shows
# the live site inside the IDE, never in another browser.
universal "$here/Cargo.toml" cove "$app/Contents/MacOS/cove" webview

if [[ -n "$crate" && -f "$crate/Cargo.toml" ]]; then
  echo "▸ building ndexr CLI from $crate (universal)"
  universal "$crate/Cargo.toml" ndexr "$app/Contents/Resources/bin/ndexr" ||
    { echo "▸ ndexr CLI did not build for darwin — shipping without it" >&2; rm -f "$app/Contents/Resources/bin/ndexr"; }
else
  echo "▸ no ndexr crate at '$crate' — shipping without a bundled CLI"
fi

# Finder/Dock icon. packaging/macos/ndexr.icns is built from cove-icon-1024.png
# (2026-10-15: the app's star-trails backdrop, assets/backdrops/, cropped to the
# tent under the circling sky on Apple's 824-in-1024 grid) with sips + iconutil
# on a Mac. packaging/linux/io.ndexr.cove.svg embeds the same crop.
cp "$here/packaging/macos/ndexr.icns" "$app/Contents/Resources/ndexr.icns"

# The browser UI, so `cove serve` works from the bundle on a Mac
# (server.rs looks in Contents/Resources/web). Built by build-web.sh first.
if [[ -f "$here/dist/web/index.html" ]]; then
  cp -R "$here/dist/web" "$app/Contents/Resources/web"
else
  echo "▸ no dist/web — the app will not be able to serve a browser (run ./build-web.sh first)" >&2
fi

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Cove</string>
  <key>CFBundleDisplayName</key><string>Cove</string>
  <key>CFBundleIdentifier</key><string>io.ndexr.cove</string>
  <key>CFBundleExecutable</key><string>cove</string>
  <key>CFBundleIconFile</key><string>ndexr</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>LSApplicationCategoryType</key><string>public.app-category.developer-tools</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSAppleEventsUsageDescription</key><string>ndexr opens Terminal to run interactive agents.</string>
</dict>
</plist>
PLIST

# Ad-hoc signature: required for arm64 binaries to launch at all. Set
# SIGN_ID="Developer ID Application: …" to sign for distribution instead.
codesign --force --deep --options runtime --sign "${SIGN_ID:--}" "$app"

# Installed as "Cove.app", never ndexr.app: dmg.ndexr.io's desktop app is
# /Applications/ndexr.app, and dragging the ide in replaced it (2026-10-13).
# src/update.rs extracts stage/Cove.app from the tarball and swaps it onto
# whatever bundle is running, so a copy installed under another name updates
# in place too.
stage="$(mktemp -d)"
cp -R "$app" "$stage/$installed"
ln -s /Applications "$stage/Applications"
hdiutil create -volname "Cove" -srcfolder "$stage" -fs APFS -format ULFO -ov "$out/cove.dmg" >/dev/null
rm -rf "$stage"
tar -C "$work" -czf "$out/cove.tar.gz" Cove.app

echo "✓ v$version → $out  ($(lipo -archs "$app/Contents/MacOS/cove"))"
echo "  next: ./publish.sh"
