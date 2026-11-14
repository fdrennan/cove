#!/usr/bin/env bash
# Build dist/deb/cove_<version>_<amd64|arm64>.deb — the desktop app
# and `cove serve` in one package, installed the way rstudio-server is:
#
#   sudo gdebi cove_0.1.0_amd64.deb        (or: sudo apt install ./…deb)
#   sudo systemctl enable --now cove@$USER
#
# Built from what the other scripts leave behind, so run them first:
#   ./build-linux.sh   → dist/linux-<arch>/cove.tar.gz (binary + bundled ndexr CLI)
#   ./build-web.sh     → dist/web/ (the UI as WebAssembly; any OS builds it, neo does)
#
#   ARCH=aarch64 ./build-deb.sh
#
# Layout: /usr/bin/cove, /usr/lib/cove/bin/ndexr (runner.rs
# finds it there), /usr/share/cove/web (server.rs's default),
# /lib/systemd/system/cove@.service and cove-multiuser.service,
# /etc/pam.d/cove + /etc/cove/multiuser.conf (conffiles), desktop entry + icon.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
version="$(sed -n 's/^version *= *"\(.*\)"/\1/p' "$here/Cargo.toml" | head -1)"
arch="${ARCH:-$(uname -m)}"
case "$arch" in
  x86_64) deb_arch=amd64 ;;
  aarch64) deb_arch=arm64 ;;
  *) echo "unsupported arch $arch" >&2; exit 1 ;;
esac
tarball="$here/dist/linux-$arch/cove.tar.gz"
web="$here/dist/web"
[[ -f "$tarball" ]] || { echo "no $tarball — run ./build-linux.sh first" >&2; exit 1; }
[[ -f "$web/index.html" ]] || { echo "no $web — run ./build-web.sh (on any machine with wasm-bindgen) first" >&2; exit 1; }

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tar -C "$work" -xzf "$tarball"
src="$(find "$work" -mindepth 1 -maxdepth 1 -type d -name 'cove-*' | head -1)"
root="$work/root"

install -Dm755 "$src/bin/cove" "$root/usr/bin/cove"
# Cove's licence and the notices of every crate in it travel with the binary.
install -Dm644 "$here/../LICENSE" "$root/usr/share/doc/cove/LICENSE"
install -Dm644 "$here/THIRD-PARTY-NOTICES.txt" "$root/usr/share/doc/cove/THIRD-PARTY-NOTICES.txt"
[[ -x "$src/lib/cove/bin/ndexr" ]] && install -Dm755 "$src/lib/cove/bin/ndexr" "$root/usr/lib/cove/bin/ndexr"
mkdir -p "$root/usr/share/cove"
cp -r "$web" "$root/usr/share/cove/web"
install -Dm644 "$src/share/applications/io.ndexr.cove.desktop" "$root/usr/share/applications/io.ndexr.cove.desktop"
install -Dm644 "$src/share/icons/hicolor/scalable/apps/io.ndexr.cove.svg" "$root/usr/share/icons/hicolor/scalable/apps/io.ndexr.cove.svg"
install -Dm644 "$here/packaging/linux/cove@.service" "$root/lib/systemd/system/cove@.service"
install -Dm644 "$here/packaging/linux/cove-multiuser.service" "$root/lib/systemd/system/cove-multiuser.service"
install -Dm644 "$here/packaging/linux/pam.d-cove" "$root/etc/pam.d/cove"
install -Dm644 "$here/packaging/linux/multiuser.conf" "$root/etc/cove/multiuser.conf"

mkdir -p "$root/DEBIAN"
# WebKitGTK only when the binary links it (build-linux.sh drops the webview
# where it cannot cross-link it, e.g. aarch64 from this host).
webkit_dep=""
objdump -p "$src/bin/cove" | grep -q 'NEEDED.*libwebkit2gtk-4.1' && webkit_dep=", libwebkit2gtk-4.1-0"
# Kept across upgrades: the site's own sign-in rules and policy.
printf '/etc/pam.d/cove\n/etc/cove/multiuser.conf\n' > "$root/DEBIAN/conffiles"
cat > "$root/DEBIAN/control" <<CONTROL
Package: cove
Version: $version
Architecture: $deb_arch
Maintainer: ndexr <support@ndexr.io>
Section: devel
Priority: optional
Homepage: https://cove.ndexr.io
Depends: libc6 (>= 2.28), libpam0g${webkit_dep}
Recommends: libgl1, libxkbcommon0
Description: Cove — scientific software at any scale, kernel to user, immutable over time
 The ndexr app: every package a stack builds (Gentoo Prefix, EasyBuild,
 R, Python), live; the stack installed into /opt or built from source;
 the ndexr CLI and Claude Code. Runs as a desktop app, or as a per-user
 server - for one person (cove@<user>.service) or for everyone on the
 machine, signing in with their own accounts (cove-multiuser.service).
CONTROL
cat > "$root/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
if [ -d /run/systemd/system ]; then
  systemctl daemon-reload >/dev/null 2>&1 || true
  # Upgrades: restart the instances that were running, onto the new binary.
  for u in $(systemctl list-units --plain --no-legend 'cove@*.service' 'cove-multiuser.service' 2>/dev/null | awk '{print $1}'); do
    systemctl try-restart "$u" >/dev/null 2>&1 || true
  done
fi
cat <<MSG
cove installed.
  desktop:  cove
  server, one person:  sudo systemctl enable --now cove@<user>
            token in ~<user>/.config/ndexr/cove-token, then http://127.0.0.1:8788/login
  server, everyone:    sudo systemctl enable --now cove-multiuser
            sign in with your own account at http://127.0.0.1:8787/login;
            /admin for sessions; policy in /etc/cove/multiuser.conf
  environments: signed in, Cove runs on your ndexr cell. This machine's arch
            is one the stack is built for, so you can also build the
            environments here from source: Cove › Software › Install.
MSG
POSTINST
cat > "$root/DEBIAN/prerm" <<'PRERM'
#!/bin/sh
set -e
if [ "$1" = remove ] && [ -d /run/systemd/system ]; then
  for u in $(systemctl list-units --plain --no-legend 'cove@*.service' 'cove-multiuser.service' 2>/dev/null | awk '{print $1}'); do
    systemctl stop "$u" >/dev/null 2>&1 || true
  done
fi
PRERM
chmod 755 "$root/DEBIAN/postinst" "$root/DEBIAN/prerm"

mkdir -p "$here/dist/deb"
deb="$here/dist/deb/cove_${version}_${deb_arch}.deb"
# Built beside, then renamed: a publish (or a push) reading dist/deb mid-build
# shipped a 1,282-byte cove_amd64.deb (2026-11-02).
dpkg-deb --root-owner-group --build "$root" "$deb.part" >/dev/null
mv -f "$deb.part" "$deb"
echo "✓ $deb ($(du -h "$deb" | cut -f1))"
