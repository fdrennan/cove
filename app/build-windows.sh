#!/usr/bin/env bash
# Build app/dist/windows-<arch>/cove.{zip,tar.gz}, cross-compiled on Linux.
# Stable names, nothing else in that dir: publish.sh uploads both to
# repo.ndexr.io/ndexr/cove/windows-<arch>/ and writes .sha256 / release.json.
#
#   ./build-windows.sh                   # x86_64-pc-windows-gnu via cargo-zigbuild
#
#   cove.zip      for people: unzip, run install.ps1 (per-user, no admin)
#   cove.tar.gz   for the updater (src/update.rs): the same <name>/bin/cove.exe
#                      layout as linux-<arch>; Windows 10+ tar.exe reads it
#
# Windows is a remote front end (operator, 2026-10-13: "a remote ide going
# forward"): the app signs in to ndexr and runs R, Python and the shell in the
# person's cell. No ndexr CLI is bundled - there is no Windows build of it.
# Needs: Rust >= 1.95 with the x86_64-pc-windows-gnu target, cargo-zigbuild +
# zig (the same ones build-linux.sh uses, under ~/.local/share/ndexr-rustup).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$here/Cargo.toml" | head -1)"
arch="${ARCH:-x86_64}"
target="$arch-pc-windows-gnu"
name="cove-$version-windows-$arch"
out="$here/dist/windows-$arch"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/$name"
rustup_home="$HOME/.local/share/ndexr-rustup"

rust_ok() { command -v rustc >/dev/null && [[ "$(rustc --version | awk '{print $2}' | cut -d. -f2)" -ge 95 ]]; }
rust_ok || { echo "need rustc ≥ 1.95 (have: $(rustc --version 2>/dev/null || echo none))" >&2; exit 1; }
export PATH="$PATH:$rustup_home/zig:$rustup_home/cargo/bin"
command -v cargo-zigbuild >/dev/null && command -v zig >/dev/null ||
  { echo "need cargo-zigbuild + zig (expected under $rustup_home)" >&2; exit 1; }
[[ -d "$(rustc --print target-libdir --target "$target" 2>/dev/null)" ]] ||
  { echo "rustc has no std for $target - rustup target add $target" >&2; exit 1; }

echo "▸ building cove $version ($target)"
cargo zigbuild --release --target "$target" --manifest-path "$here/Cargo.toml"

mkdir -p "$stage/bin"
cp "$here/target/$target/release/cove.exe" "$stage/bin/"
# Cove's licence and the notices of every crate in it travel with the binary.
cp "$here/../LICENSE" "$here/THIRD-PARTY-NOTICES.txt" "$stage/"
cp "$here/packaging/windows/install.ps1" "$stage/"
file "$stage/bin/cove.exe" | grep -q 'PE32+' || { echo "not a Windows PE32+ binary" >&2; exit 1; }

# Written beside, then renamed: publish.sh never sees a half-written file.
mkdir -p "$out"
rm -f "$out"/cove.*
python3 - "$work" "$name" "$out/.cove.zip.part" <<'PY'
import os, sys, zipfile
work, name, dest = sys.argv[1:]
with zipfile.ZipFile(dest, "w", zipfile.ZIP_DEFLATED) as z:
    for d, _, files in os.walk(os.path.join(work, name)):
        for f in files:
            p = os.path.join(d, f)
            z.write(p, os.path.relpath(p, work))
PY
tar -C "$work" -czf "$out/.cove.tar.gz.part" "$name"
mv -f "$out/.cove.zip.part" "$out/cove.zip"
mv -f "$out/.cove.tar.gz.part" "$out/cove.tar.gz"
echo "✓ $out/cove.zip + cove.tar.gz  ($name, $(du -h "$out/cove.zip" | cut -f1))"
