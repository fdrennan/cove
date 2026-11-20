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
  rm -rf "$bin" "$lib" "$desktop" "$icon" "$prefix/share/cove"
  refresh
  echo "✓ removed cove from $prefix"
  exit 0
fi

install -Dm755 "$here/bin/cove" "$bin"
if [[ -f "$here/lib/cove/bin/ndexr" ]]; then
  install -Dm755 "$here/lib/cove/bin/ndexr" "$lib/bin/ndexr"
fi
# The browser UI `cove serve` hands out (<prefix>/share/cove/web, as the .deb
# puts it at /usr/share/cove/web): Cove in a browser, here or on a server.
if [[ -f "$here/share/cove/web/index.html" ]]; then
  rm -rf "$prefix/share/cove/web"
  mkdir -p "$prefix/share/cove"
  cp -r "$here/share/cove/web" "$prefix/share/cove/web"
fi
# Absolute Exec= so the launcher works even when $PREFIX/bin is not on the
# session PATH (common for ~/.local/bin under GNOME).
sed "s|^Exec=.*|Exec=$bin|" "$here/share/applications/io.ndexr.cove.desktop" |
  install -Dm644 /dev/stdin "$desktop"
install -Dm644 "$here/share/icons/hicolor/scalable/apps/io.ndexr.cove.svg" "$icon"
refresh

# The App panel's webview links WebKitGTK 4.1, so cove does not start without
# it - and a stock desktop install often lacks it (Pop!_OS 22.04 laptop,
# 2026-10-15: "error while loading shared libraries: libwebkit2gtk-4.1.so.0").
# One command to fix it, not a list of sonames (operator, 2026-11-14: installs
# "should look and feel cleaner").
if ldd "$bin" 2>/dev/null | grep -q "not found"; then
  if command -v apt-get >/dev/null; then fix="sudo apt install libwebkit2gtk-4.1-0"
  elif command -v dnf >/dev/null; then fix="sudo dnf install webkit2gtk4.1"
  elif command -v pacman >/dev/null; then fix="sudo pacman -S webkit2gtk-4.1"
  elif command -v zypper >/dev/null; then fix="sudo zypper install libwebkit2gtk-4_1-0"
  else fix="install WebKitGTK 4.1 (webkit2gtk-4.1) from your distribution"
  fi
  printf '\n  ! Cove needs WebKitGTK 4.1, which this machine does not have yet:\n\n      %s\n\n' "$fix"
fi

# Run by the one-line installer (packaging/install.sh), which says the rest.
[[ -n "${COVE_QUIET:-}" ]] && exit 0
echo "✓ Cove is installed: $bin"
echo "  Start it: cove   (or open Cove from your applications)"
case ":$PATH:" in *":$prefix/bin:"*) ;; *) echo "  $prefix/bin is not on your PATH yet - open a new terminal, or use the app menu" ;; esac
