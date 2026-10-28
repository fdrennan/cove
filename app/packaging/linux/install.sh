#!/usr/bin/env bash
# Install cove from an unpacked release tarball.
#
#   ./install.sh                     # per-user: ~/.local (no sudo)
#   sudo PREFIX=/usr/local ./install.sh
#   ./install.sh --uninstall
#
# Layout:  $PREFIX/bin/cove
#          $PREFIX/lib/cove/bin/ndexr          (bundled CLI, if shipped)
#          $PREFIX/share/applications/io.ndexr.cove.desktop
#          $PREFIX/share/icons/hicolor/scalable/apps/io.ndexr.cove.svg
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
if [[ -n "${PREFIX:-}" ]]; then prefix="$PREFIX"
elif [[ $EUID -eq 0 ]]; then prefix=/usr/local
else prefix="$HOME/.local"
fi

bin="$prefix/bin/cove"
lib="$prefix/lib/cove"
desktop="$prefix/share/applications/io.ndexr.cove.desktop"
icon="$prefix/share/icons/hicolor/scalable/apps/io.ndexr.cove.svg"

refresh() {
  command -v update-desktop-database >/dev/null && update-desktop-database -q "$prefix/share/applications" 2>/dev/null || true
  command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true
}

if [[ "${1:-}" == --uninstall ]]; then
  rm -rf "$bin" "$lib" "$desktop" "$icon"
  refresh
  echo "✓ removed cove from $prefix"
  exit 0
fi

install -Dm755 "$here/bin/cove" "$bin"
# The App panel's webview links WebKitGTK 4.1, so cove does not start without
# it - and a stock desktop install often lacks it (Pop!_OS 22.04 laptop,
# 2026-10-15: "error while loading shared libraries: libwebkit2gtk-4.1.so.0").
missing="$(ldd "$bin" 2>/dev/null | awk '/not found/ {print $1}' | tr '\n' ' ')"
if [[ -n "$missing" ]]; then
  echo "  ! cove needs libraries this machine lacks: $missing"
  if command -v apt-get >/dev/null; then echo "    sudo apt install libwebkit2gtk-4.1-0"
  elif command -v dnf >/dev/null; then echo "    sudo dnf install webkit2gtk4.1"
  elif command -v pacman >/dev/null; then echo "    sudo pacman -S webkit2gtk-4.1"
  else echo "    install WebKitGTK 4.1 (webkit2gtk-4.1) from your distribution"
  fi
fi
if [[ -f "$here/lib/cove/bin/ndexr" ]]; then
  install -Dm755 "$here/lib/cove/bin/ndexr" "$lib/bin/ndexr"
fi
# Absolute Exec= so the launcher works even when $PREFIX/bin is not on the
# session PATH (common for ~/.local/bin under GNOME).
sed "s|^Exec=.*|Exec=$bin|" "$here/share/applications/io.ndexr.cove.desktop" |
  install -Dm644 /dev/stdin "$desktop"
install -Dm644 "$here/share/icons/hicolor/scalable/apps/io.ndexr.cove.svg" "$icon"
refresh

echo "✓ installed $bin"
case ":$PATH:" in *":$prefix/bin:"*) ;; *) echo "  note: $prefix/bin is not on PATH — launch from the app menu or add it" ;; esac

# Signed in, Cove runs R, Python and the shell on your ndexr cell (remote
# first, every machine). Linux on an arch the stack is built for can also
# build the environments here, from source (host.rs STACK_ARCHES).
case "$(uname -m)" in
  x86_64|aarch64)
    echo "  Linux $(uname -m) is supported: sign in to run on your ndexr cell, or build the"
    echo "  environments on this machine from source in Cove › Software › Install"
    echo "  (the bundled ndexr CLI does the build; Tools › Global Options › Environment mounts the prebuilt stack instead)." ;;
  *)
    echo "  Linux $(uname -m) has no ndexr stack build: sign in and Cove runs on your ndexr cell." ;;
esac
