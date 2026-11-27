#!/bin/sh
# Install or update Cove, on any machine it ships for. One line:
#
#   curl -fsSL https://ndexr.io/install.sh | sh
#
# ndexr.io serves this file as it is here (its server.js reads it from
# disk), so the page and the installer cannot drift apart.
#
#   Linux                      the build for this machine's architecture
#                              (x86_64 or aarch64) into ~/.local, no sudo;
#                              nothing else to install - the App panel's
#                              WebKitGTK comes from the ndexr stack
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
#   … | sh -s -- --stack       also mount the ndexr stack (CernVM-FS), no question
#   … | sh -s -- --no-stack    never ask
#   … | sh -s -- --stack --front   a server: the stack, then HTTPS in front of
#                              Cove from the stack's Caddy (cove setup-front)
#   … | sh -s -- --server [--front] [--name cove.example.org] [--single]
#                              a server with NOTHING installed: mount the
#                              stack and run Cove from it (`ml cove`). The
#                              organisation's by default - people sign in with
#                              their own accounts, each gets their own sessions
#                              (cove setup-server); --single: one person's.
#                              A stack update is a Cove update. People connect
#                              to it from the desktop app (Connect to a server)
#                              or a browser (operator, 2026-11-24: "no install
#                              path ... server uses the stack ... people will be
#                              able to use the app to connect to their stack")
#
# Every download is checked against its published SHA-256 before it is used.
# Operator, 2026-11-14: "cove.ndexr.io installs should look and feel cleaner"
# - before this, a .deb fetched into $HOME made apt print "_apt ... Permission
# denied", and the tarball's installer ended in a paragraph of options.
set -eu

base="${COVE_FEED:-https://repo.ndexr.io/ndexr/cove}"

# The flags, read once, here: further down `set --` reuses the positional
# parameters for the tarball's installer, and the --stack read after it found
# that path instead, so `sh -s -- --stack` never mounted anything
# (ndexrtesting.com, 2026-11-21; COVE_MOUNT_STACK=yes was the workaround).
flag_stack=""
flag_front=""
flag_server=""
flag_single=""
flag_name=""
prev=""
for a in "$@"; do
  [ "$prev" = --name ] && flag_name="$a"
  case "$a" in --stack|--yes|-y) flag_stack=yes ;; --no-stack) flag_stack=no ;; --front) flag_front=yes ;; --server) flag_server=yes ;; --single) flag_single=yes ;; esac
  prev="$a"
done

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

# ── --server: Cove from the stack, nothing installed ─────────────────────────
# The stack is mounted with the ndexr CLI fetched into $tmp (checksum-checked,
# gone when this exits), then a cove-stack@<user> service runs the stack's own
# Cove: `ml cove` is the app, its App panel build, its browser UI and the
# ndexr CLI, all from /opt/ndexr/stack. Root once (sudo) for the mount and the
# unit; Cove itself runs as the user.
if [ "$flag_server" = yes ]; then
  [ "$platform" = darwin ] && fail "--server is for a Linux server; on a Mac install the app and connect to the server"
  user="${SUDO_USER:-$(id -un)}"
  as_root() { if [ "$(id -u)" = 0 ]; then "$@"; else sudo "$@"; fi; }
  if [ ! -r /opt/ndexr/stack/use.sh ]; then
    step "mounting the ndexr stack at /opt/ndexr/stack (CernVM-FS, root once)"
    checked "https://repo.ndexr.io/ndexr/$(uname -m)/latest/ndexr" "$tmp/ndexr"
    chmod +x "$tmp/ndexr"
    as_root "$tmp/ndexr" install cvmfs --yes || fail "the stack was not mounted (above)"
  fi
  [ -r /opt/ndexr/stack/use.sh ] || fail "/opt/ndexr/stack has no use.sh after the mount"
  # The newest Cove sets the server up: the stack's, or the feed's while the
  # stack's module trails it (operator, 2026-11-27: a new server on the
  # stack's cove/0.1.30, with the feed at 0.1.33, showed every new person
  # bugs already fixed). The server keeps choosing the newest after this
  # (cove-newest, cove-feed-update: `cove setup-server`).
  vnum() { grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1; }
  stack_v="$(bash -c 'set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1 && ml cove >/dev/null 2>&1 && cove --version' 2>/dev/null | vnum || true)"
  feed_v="$( (curl -fsSL "$base/$platform/latest/release.json" || wget -qO- "$base/$platform/latest/release.json") 2>/dev/null | sed -n 's/.*"version":"\([^"]*\)".*/\1/p')" || true
  cove_bin=cove
  if [ -n "$feed_v" ] && { [ -z "$stack_v" ] || { [ "$feed_v" != "$stack_v" ] && [ "$(printf '%s\n%s\n' "$stack_v" "$feed_v" | sort -V | tail -1)" = "$feed_v" ]; }; }; then
    step "Cove $feed_v from the feed (the stack has ${stack_v:-none})"
    checked "$base/$platform/latest/cove.tar.gz" "$tmp/cove.tar.gz"
    mkdir -p "$tmp/x" && tar -xzf "$tmp/cove.tar.gz" -C "$tmp/x"
    set -- "$tmp"/x/cove-*/bin/cove
    [ -x "$1" ] || fail "the download has no bin/cove"
    as_root mkdir -p /var/lib/cove/feed
    as_root rm -rf "/var/lib/cove/feed/$feed_v"
    as_root cp -a "$(dirname "$(dirname "$1")")" "/var/lib/cove/feed/$feed_v"
    as_root ln -sfn "$feed_v" /var/lib/cove/feed/current
    cove_bin="/var/lib/cove/feed/$feed_v/bin/cove"
  elif [ -n "$stack_v" ]; then
    step "Cove $stack_v from the stack"
  else
    fail "neither the stack nor the feed ($base) has a Cove for $platform"
  fi
  # The server itself is `cove setup-server` (stack.rs), the one place its
  # units, PAM file and settings are written: the organisation's by default
  # (cove-multiuser - people sign in with their accounts, each gets their own
  # sessions), one person's with --single.
  stack_cove() { as_root env COVE_BIN="$cove_bin" bash -c 'set +u; . /opt/ndexr/stack/use.sh >/dev/null 2>&1; ml cove >/dev/null 2>&1; exec "$COVE_BIN" "$@"' cove "$@"; }
  if [ "$flag_single" = yes ]; then set -- --user "$user"; else set --; fi
  [ -n "$flag_name" ] && set -- "$@" --name "$flag_name"
  stack_cove setup-server --yes "$@" || fail "the server was not set up (above)"
  if [ "$flag_front" = yes ]; then
    stack_cove setup-front --yes ||
      printf '    %sHTTPS was not set up%s - run: sudo bash -c ". /opt/ndexr/stack/use.sh && ml cove && cove setup-front --yes"\n' "$r" "$n"
  fi
  printf '\n  %s✓ Cove runs on this server%s  %sthe newest of the stack'"'"'s and the feed'"'"'s, checked every hour%s\n\n' "$g$b" "$n" "$d" "$n"
  printf '    %sFrom a desktop%s the Cove app: account menu, Connect to a server\n\n' "$b" "$n"
  exit 0
fi

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
  # No WebKitGTK to install (operator, 2026-11-24): bin/cove links none, and
  # the App panel's embedded browser is the stack's (src/desktop/desk.rs).
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
    answer="${COVE_MOUNT_STACK:-$flag_stack}"
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
# The ndexr stack on this machine (Linux): agreed with --stack (or --yes), or
# asked at the terminal when there is one. A server is set up in one go:
#   curl -fsSL https://ndexr.io/install.sh | sh -s -- --stack
if [ "$platform" != darwin ] && [ ! -r /opt/ndexr/stack/use.sh ]; then
  want="$flag_stack"
  if [ -z "$want" ] && [ -r /dev/tty ] && [ -t 1 ]; then
    printf '    %sThe ndexr stack%s  R, Python, Julia and their libraries, mounted read-only over\n' "$b" "$n"
    printf '                     CernVM-FS (needs sudo once). Mount it now? [y/N] '
    read -r ans </dev/tty || ans=""
    case "$ans" in y|Y|yes|YES) want=yes ;; *) want=no ;; esac
    echo
  fi
  if [ "$want" = yes ]; then
    "$where" setup-stack --yes || printf '    %sThe stack was not mounted%s - Cove asks again when it starts, or run: cove setup-stack\n' "$r" "$n"
    echo
  fi
fi
# HTTPS in front of this Cove, from the stack's Caddy: asked for with --front,
# on a server whose stack is mounted (cove setup-front, root once).
if [ "$flag_front" = yes ] && [ "$platform" != darwin ]; then
  if [ -r /opt/ndexr/stack/use.sh ]; then
    "$where" setup-front --yes || printf '    %sHTTPS was not set up%s - run: sudo cove setup-front --yes\n' "$r" "$n"
  else
    printf '    %sHTTPS was not set up%s - the ndexr stack is not mounted (add --stack)\n' "$r" "$n"
  fi
  echo
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
