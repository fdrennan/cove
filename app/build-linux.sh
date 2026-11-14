#!/usr/bin/env bash
# Build app/dist/linux-<arch>/cove.tar.gz. Stable name, nothing else in
# that dir: publish.sh uploads it to repo.ndexr.io/ndexr/cove/linux-<arch>/
# and writes the .sha256 / release.json there.
#
#   ./build-linux.sh                     # host arch, bundled static ndexr CLI
#   ARCH=aarch64 ./build-linux.sh        # cross-build via zig
#   NDEXR_CLI= ./build-linux.sh          # ship without the bundled CLI
#   NDEXR_CLI=/path/to/ndexr ./build-linux.sh
#   WEBVIEW=0 ./build-linux.sh           # no WebKitGTK: dist-cell/linux-<arch>/, for cells
#
# WEBVIEW=0 is the cell's build (user-provision.sh publishes it to
# /opt/ndexr/cove). A cell only runs `cove serve` and has no GTK or WebKit, so
# the desktop tarball's binary could not load there and no cell had a Cove
# server: the desktop app could not connect (2026-11-02, "connect ECONNREFUSED
# 0.0.0.0:8002"). Its own target dir, so it never clobbers a desktop build,
# and its own dist-cell/, so publish.sh (every dist/*/) never offers it as a
# download.
#
# Linked against glibc 2.28 with cargo-zigbuild, so one binary runs on Rocky 8+,
# Ubuntu 20.04+, Debian 10+. The bundled CLI is the static musl ndexr. Install:
#   wget -qO- https://repo.ndexr.io/ndexr/cove/linux-$(uname -m)/latest/cove.tar.gz | tar xz
#   ./cove-*-linux-*/install.sh
# The tarball's top dir <name>/{bin,lib/cove/bin} is a contract with
# src/update.rs, which swaps those two binaries in place.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$here/Cargo.toml" | head -1)"
arch="${ARCH:-$(uname -m)}"
target="$arch-unknown-linux-gnu"
name="cove-$version-linux-$arch"
out="$here/dist/linux-$arch"
tdir="$here/target"
if [[ "${WEBVIEW:-1}" == 0 ]]; then out="$here/dist-cell/linux-$arch"; tdir="$here/target-cell"; fi
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
stage="$work/$name"
rustup_home="$HOME/.local/share/ndexr-rustup"
musl_cli="$here/../../../ndexr.ndexr.io/crate/target/$arch-unknown-linux-musl/release/ndexr"

[[ "$(uname -s)" == Linux ]] || { echo "build-linux.sh runs on Linux only" >&2; exit 1; }

# ── toolchain: Rust ≥ 1.95 (egui 0.36) — the system rustc on ndexr hosts is older
rust_ok() { command -v rustc >/dev/null && [[ "$(rustc --version | awk '{print $2}' | cut -d. -f2)" -ge 95 ]]; }
if ! rust_ok; then
  hpc="$here/../../../hpc.ndexr.io/scripts/hpc-env.sh"
  # shellcheck disable=SC1090
  [[ -f "$hpc" ]] && { source "$hpc" >/dev/null 2>&1; ml Rust/1.96.0-GCCcore-13.3.0 >/dev/null 2>&1 || true; }
  rust_ok || { echo "need rustc ≥ 1.95 (have: $(rustc --version 2>/dev/null || echo none))" >&2; exit 1; }
fi
# Appended, not prepended: cargo/bin also holds rustup shims that would shadow
# the module's rustc with an unconfigured proxy. Only cargo-zigbuild is wanted.
export PATH="$PATH:$rustup_home/zig:$rustup_home/cargo/bin"
command -v cargo-zigbuild >/dev/null && command -v zig >/dev/null ||
  { echo "need cargo-zigbuild + zig (expected under $rustup_home)" >&2; exit 1; }
rustc --print target-libdir --target "$target" >/dev/null 2>&1 && [[ -d "$(rustc --print target-libdir --target "$target")" ]] ||
  { echo "rustc $(rustc --version | awk '{print $2}') has no std for $target" >&2; exit 1; }

echo "▸ building cove $version ($target, glibc 2.28, rustc $(rustc --version | awk '{print $2}'))"
# webview: the App panel's embedded WebKitGTK 4.1 (webview.rs). Linked against
# this host's libwebkit2gtk-4.1 / gtk-3 (libwebkit2gtk-4.1-dev); at run time the
# machine needs libwebkit2gtk-4.1-0, which a stock install can lack (install.sh checks).
export PKG_CONFIG_ALLOW_CROSS=1
multi="/usr/lib/$arch-linux-gnu"
# The webview needs that arch's GTK/WebKit dev libs (multiarch). This host has
# none for aarch64 (2026-10-15: link failed, "unable to find dynamic system
# library 'webkit2gtk-4.1'"), so that build ships without the embedded App
# panel - it opens the site in the browser instead, as Windows does.
features="--features webview"
if [[ "${WEBVIEW:-1}" == 0 ]]; then
  features=""
  echo "▸ WEBVIEW=0 - building without the embedded webview"
elif [[ ! -e "$multi/libwebkit2gtk-4.1.so" ]]; then
  features=""
  echo "▸ no $multi/libwebkit2gtk-4.1.so here - building without the embedded webview"
fi
RUSTFLAGS="${RUSTFLAGS:-} -L native=$multi -C link-arg=-Wl,--allow-shlib-undefined" \
CARGO_TARGET_DIR="$tdir" cargo zigbuild --release --target "$target.2.28" --manifest-path "$here/Cargo.toml" $features

mkdir -p "$stage/bin"
cp "$tdir/$target/release/cove" "$stage/bin/"
# Cove's licence and the notices of every crate in it travel with the binary.
install -Dm644 "$here/../LICENSE" "$stage/share/doc/cove/LICENSE"
install -Dm644 "$here/THIRD-PARTY-NOTICES.txt" "$stage/share/doc/cove/THIRD-PARTY-NOTICES.txt"

# ── bundled CLI: static musl ndexr, local build first, else repo.ndexr.io
cli="${NDEXR_CLI-auto}"
if [[ "$cli" == auto ]]; then
  if [[ -x "$musl_cli" ]]; then cli="$musl_cli"
  else
    url="https://repo.ndexr.io/ndexr/$arch/latest/ndexr"
    cli="$work/ndexr-$arch"
    echo "▸ fetching $url"
    curl -fsSL "$url" -o "$cli" && curl -fsSL "$url.sha256" -o "$cli.sha256"
    [[ "$(sha256sum "$cli" | cut -d' ' -f1)" == "$(cut -d' ' -f1 "$cli.sha256")" ]] ||
      { echo "sha256 mismatch for $url" >&2; exit 1; }
  fi
fi
if [[ -n "$cli" ]]; then
  echo "▸ bundling ndexr CLI from $cli"
  install -Dm755 "$cli" "$stage/lib/cove/bin/ndexr"
else
  echo "▸ shipping without a bundled CLI"
fi

install -Dm644 "$here/packaging/linux/io.ndexr.cove.desktop" "$stage/share/applications/io.ndexr.cove.desktop"
install -Dm644 "$here/packaging/linux/io.ndexr.cove.svg" "$stage/share/icons/hicolor/scalable/apps/io.ndexr.cove.svg"
install -Dm755 "$here/packaging/linux/install.sh" "$stage/install.sh"

glibc="$(objdump -T "$stage/bin/cove" | grep -o 'GLIBC_[0-9.]*' | sort -Vu | tail -1)"
[[ "$(printf '%s\n' "$glibc" GLIBC_2.28 | sort -V | tail -1)" == GLIBC_2.28 ]] ||
  { echo "binary needs $glibc (> 2.28) — refusing to ship" >&2; exit 1; }

# Written beside, then renamed: publish.sh never sees a half-written tarball.
mkdir -p "$out"
tar -C "$work" -czf "$out/.cove.tar.gz.part" "$name"
mv -f "$out/.cove.tar.gz.part" "$out/cove.tar.gz"
echo "✓ $out/cove.tar.gz  ($name, needs $glibc, $(du -h "$out/cove.tar.gz" | cut -f1))"
