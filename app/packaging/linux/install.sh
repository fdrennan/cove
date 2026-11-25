#!/usr/bin/env bash
# Install cove from an unpacked release tarball.
#
#   ./install.sh                     # per-user: ~/.local (no sudo)
#   sudo PREFIX=/usr/local ./install.sh
#   ./install.sh --uninstall
#
# Layout:  $PREFIX/bin/cove
#          $PREFIX/lib/cove/bin/ndexr          (bundled CLI, if shipped)
#          $PREFIX/lib/cove/bin/cove-desk-stack  (App panel, the stack's WebKitGTK)
#          $PREFIX/lib/cove/bin/cove-desk      (App panel, the system's WebKitGTK)
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
for f in ndexr cove-desk cove-desk-stack; do
  if [[ -f "$here/lib/cove/bin/$f" ]]; then
    install -Dm755 "$here/lib/cove/bin/$f" "$lib/bin/$f"
  fi
done
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

# bin/cove links no WebKit or GTK, so it starts on any desktop. The App
# panel's embedded browser comes from a sibling build (src/desktop/desk.rs): the
# ndexr stack's WebKitGTK when the stack is mounted, else the system's when it
# has one. Nothing is installed for it (operator, 2026-11-24: "build it into
# the stack so when cvmfs is used its there"); without either, the panel
# opens sites in the browser.
# (ldd exits 0 even when a library is missing: its "not found" is the test.)
desk_ok=0
[[ -f "$lib/bin/cove-desk" ]] && ! ldd "$lib/bin/cove-desk" 2>/dev/null | grep -q "not found" && desk_ok=1
if [[ -z "${COVE_QUIET:-}" && ! -r /opt/ndexr/stack/use.sh && "$desk_ok" == 0 ]]; then
  printf '\n  The App panel shows your site inside Cove once the ndexr stack is mounted\n  (cove setup-stack); until then it opens sites in your browser.\n\n'
fi

# Run by the one-line installer (packaging/install.sh), which says the rest.
[[ -n "${COVE_QUIET:-}" ]] && exit 0
echo "✓ Cove is installed: $bin"
echo "  Start it: cove   (or open Cove from your applications)"
case ":$PATH:" in *":$prefix/bin:"*) ;; *) echo "  $prefix/bin is not on your PATH yet - open a new terminal, or use the app menu" ;; esac
