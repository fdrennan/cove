#!/bin/sh
# Install or update Cove, on any machine it ships for. One line:
#
#   curl -fsSL https://ndexr.io/install.sh | sh
#
# ndexr.io serves this file as it is here (its server.js reads it from
# disk), so the page and the installer cannot drift apart.
#
#   Linux                      the build for this machine's architecture
#                              (x86_64 or aarch64) into ~/.local, no sudo; the
#                              one command for WebKitGTK is printed if missing
#   macOS                      Cove.app in /Applications
#
# On Linux it then asks whether to mount the ndexr software stack at
# /opt/ndexr/stack (CernVM-FS, sudo once); COVE_MOUNT_STACK=yes|no answers it
# without a terminal.
#
# One Linux build per architecture, for every distribution (operator,
# 2026-11-19: "dont need ubuntu specific version instead we just build by
# arch"). --user, which once meant "not the .deb", is still accepted.
#
# Every download is checked against its published SHA-256 before it is used.
# Operator, 2026-11-14: "cove.ndexr.io installs should look and feel cleaner"
# - before this, a .deb fetched into $HOME made apt print "_apt ... Permission
# denied", and the tarball's installer ended in a paragraph of options.
set -eu

base="${COVE_FEED:-https://repo.ndexr.io/ndexr/cove}"

if [ -t 1 ]; then b="$(printf '\033[1m')"; d="$(printf '\033[2m')"; g="$(printf '\033[32m')"; r="$(printf '\033[31m')"; n="$(printf '\033[0m')"
else b=""; d=""; g=""; r=""; n=""; fi
step() { printf '  %s›%s %s\n' "$d" "$n" "$*"; }
fail() { printf '  %s✗%s %s\n' "$r" "$n" "$*" >&2; exit 1; }

fetch() { # fetch <url> <dest>
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1" -o "$2"
  else wget -qO "$2" "$1"; fi || fail "could not download $1"
}
sha() { if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }
checked() { # checked <url> <dest>: download it and its .sha256, and compare
  fetch "$1" "$2"
  fetch "$1.sha256" "$2.sha256"
  [ "$(sha "$2")" = "$(cut -d' ' -f1 "$2.sha256")" ] || fail "checksum mismatch for $1 - nothing was installed"
}

case "$(uname -s)-$(uname -m)" in
  Darwin-*)                  platform=darwin ;;
  Linux-x86_64|Linux-amd64)  platform=linux-x86_64 ;;
  Linux-aarch64|Linux-arm64) platform=linux-aarch64 ;;
  *) fail "Cove is not built for $(uname -s) $(uname -m) - it runs in your browser: https://cove.ndexr.io" ;;
esac

printf '\n  %sCove%s installer\n\n' "$b" "$n"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

if [ "$platform" = darwin ]; then
  step "downloading Cove for macOS"
  checked "$base/darwin/latest/cove.tar.gz" "$tmp/cove.tar.gz"
  tar -xzf "$tmp/cove.tar.gz" -C "$tmp"
  dest=/Applications
  [ -w "$dest" ] || { dest="$HOME/Applications"; mkdir -p "$dest"; }
  xattr -dr com.apple.quarantine "$tmp/Cove.app" 2>/dev/null || true
  rm -rf "$dest/Cove.app.old"
  [ -d "$dest/Cove.app" ] && mv "$dest/Cove.app" "$dest/Cove.app.old"
  mv "$tmp/Cove.app" "$dest/Cove.app"
  rm -rf "$dest/Cove.app.old"
  where="$dest/Cove.app"; start="open Cove from Launchpad"
else
  step "downloading Cove for Linux ($(uname -m))"
  checked "$base/$platform/latest/cove.tar.gz" "$tmp/cove.tar.gz"
  tar -xzf "$tmp/cove.tar.gz" -C "$tmp"
  set -- "$tmp"/cove-*/install.sh
  [ -x "$1" ] || fail "the download has no install.sh"
  step "installing into ~/.local"
  # Its own messages go to a log: a tarball published before 2026-11-14 says
  # them at length, and this installer says what matters below.
  COVE_QUIET=1 "$1" > "$tmp/install.log" 2>&1 || { tail -n 15 "$tmp/install.log" >&2; fail "the tarball's installer failed (above)"; }
  where="${PREFIX:-$HOME/.local}/bin/cove"; start="or open Cove from your applications"
  # The App panel's webview is WebKitGTK 4.1: without it cove does not start.
  if ldd "$where" 2>/dev/null | grep -q "not found"; then
    if command -v dnf >/dev/null 2>&1; then lacks="sudo dnf install webkit2gtk4.1"
    elif command -v pacman >/dev/null 2>&1; then lacks="sudo pacman -S webkit2gtk-4.1"
    elif command -v zypper >/dev/null 2>&1; then lacks="sudo zypper install libwebkit2gtk-4_1-0"
    elif command -v apt-get >/dev/null 2>&1; then lacks="sudo apt install libwebkit2gtk-4.1-0"
    elif command -v emerge >/dev/null 2>&1; then lacks="sudo emerge --ask net-libs/webkit-gtk:4.1"
    else lacks="install WebKitGTK 4.1 (webkit2gtk-4.1) from your distribution"; fi
  fi
fi

# The ndexr software stack: asked once, here, not only by the desktop app at
# its first start - a server has no desktop to ask from, and stayed without
# its software (operator, 2026-11-21: "the cove installer when its running on
# a server somewhere or just generally should ask the user if they would like
# to mount the stack"). It is the published configurator `ndexr install cvmfs`
# and Cove's own Set up run (install/cvmfs.sh, its sha256 checked): the
# CernVM-FS client, an fstab line, /opt/ndexr/stack mounted. Root, once.
# `curl | sh` reads this script on stdin, so the question goes to the terminal;
# with none (a provisioning script), COVE_MOUNT_STACK=yes|no answers it, else
# the one command is printed and nothing waits.
stack=/opt/ndexr/stack
stack_said=""
if [ "$platform" != darwin ]; then
  if [ -r "$stack/use.sh" ]; then
    stack_said="mounted at $stack"
  else
    answer="${COVE_MOUNT_STACK:-}"
    # Opened, not only present: without a controlling terminal /dev/tty
    # passes -r and -w and then fails to open, and under set -e that ended the
    # installer before its summary (measured with setsid, 2026-11-21). In a
    # subshell: a failed redirection on `:`, a special built-in, exits a
    # POSIX sh outright, even inside an if.
    if [ -z "$answer" ] && ( : > /dev/tty ) 2>/dev/null; then
      printf '\n  %sMount the ndexr software stack?%s  R, Python, Julia, Rust and thousands of packages,\n' "$b" "$n" > /dev/tty
      printf '  read-only at %s over CernVM-FS. It needs sudo once.  [Y/n] ' "$stack" > /dev/tty
      read -r answer < /dev/tty || answer=n
      [ -n "$answer" ] || answer=y
    fi
    case "$answer" in
      y|Y|yes|YES|Yes)
        step "mounting the ndexr stack at $stack (sudo)"
        checked "https://repo.ndexr.io/ndexr/install/cvmfs.sh" "$tmp/cvmfs.sh"
        # Gentoo's client comes from its own tree; the configurator installs
        # it everywhere else (as Cove's Set up does, stack.rs setup_script).
        if command -v emerge >/dev/null 2>&1 && ! command -v cvmfs2 >/dev/null 2>&1; then
          sudo emerge --noreplace --quiet-build=y net-fs/cvmfs > "$tmp/stack.log" 2>&1 || true
        fi
        sudo env NDEXR_CVMFS_REPO=stack.ndexr.io NDEXR_CVMFS_HOST=cvmfs.ndexr.io bash "$tmp/cvmfs.sh" >> "$tmp/stack.log" 2>&1 || true
        if [ -r "$stack/use.sh" ]; then
          stack_said="mounted at $stack"
        else
          mkdir -p "$HOME/.cache/cove" && cp "$tmp/stack.log" "$HOME/.cache/cove/stack-install.log"
          tail -n 8 "$tmp/stack.log" >&2
          stack_said="not mounted - the log is ~/.cache/cove/stack-install.log; Cove offers Set up again at its start"
        fi
        ;;
      *)
        stack_said="not mounted - later: curl -fsSL https://ndexr.io/install.sh | COVE_MOUNT_STACK=yes sh"
        ;;
    esac
  fi
fi

version="$( (curl -fsSL "$base/$platform/latest/release.json" || wget -qO- "$base/$platform/latest/release.json") 2>/dev/null | sed -n 's/.*"version":"\([^"]*\)".*/\1/p')" || true

printf '\n  %s✓ Cove %s is installed%s  %s%s%s\n\n' "$g$b" "$version" "$n" "$d" "$where" "$n"
if [ -n "${lacks:-}" ]; then
  printf '    %sOne more step%s  Cove needs WebKitGTK 4.1:  %s\n\n' "$b" "$n" "$lacks"
fi
printf '    %sStart it%s       cove   %s(%s)%s\n' "$b" "$n" "$d" "$start" "$n"
[ -n "$stack_said" ] && printf '    %sThe stack%s      %s\n' "$b" "$n" "$stack_said"
printf '    %sIn a browser%s   https://cove.ndexr.io\n' "$b" "$n"
printf '    %sSource%s         https://github.com/fdrennan/cove\n\n' "$b" "$n"
# A Cove from the old .deb stays in /usr/bin, and nothing updates it now.
if [ "$platform" != darwin ] && command -v dpkg-query >/dev/null 2>&1 && dpkg-query -W -f='${Status}' cove 2>/dev/null | grep -q ' installed$'; then
  printf '    %sOld package%s    the Debian package of Cove is still installed and no longer updates:  sudo apt remove cove\n\n' "$b" "$n"
fi
if [ "$platform" != darwin ]; then
  case ":$PATH:" in *":$(dirname "$where"):"*) ;; *) printf '    %s%s is not on your PATH yet - open a new terminal, or use the app menu%s\n\n' "$d" "$(dirname "$where")" "$n" ;; esac
fi
