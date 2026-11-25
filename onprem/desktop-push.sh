#!/usr/bin/env bash
# Keep the desktop Coves on the test machines current: every release run.sh
# publishes also goes, as the desktop app, to `ssh laptop` and `ssh neo` when
# they answer (operator, 2026-10-18: "those machines need to be able to have
# the desktop version updated because we have to keep testing the desktop
# versions of those").
#
#   onprem/desktop-push.sh all <release-id>     what run.sh fires after a publish
#   onprem/desktop-push.sh laptop|neo|cell [id] one machine, by hand
#   onprem/desktop-push.sh status               where each machine is
#
# laptop  x86_64 Linux (Gentoo since 2026-11-19). Built HERE (build-linux.sh:
#         glibc 2.28 via zigbuild, with this host's WebKitGTK for the App panel
#         - the laptop has neither zig nor the dev libs). Installed per user
#         (~/.local, the tarball's install.sh): one build per architecture,
#         no .deb (operator, 2026-11-19: "dont need ubuntu specific version
#         instead we just build by arch").
# cell    every account's cell: the WEBVIEW=0 server build (a cell has no
#         GTK/WebKit), at app/dist-cell/linux-x86_64/cove.tar.gz. ood-sync-users
#         (each minute, user-provision.sh ndexr_cell_tools_ensure) publishes it
#         to the cells' /opt/ndexr/cove on change; a cell starts it at boot.
#         Without it no cell had a Cove server and the desktop app could not
#         connect (2026-11-02).
# neo     macOS arm64. Built THERE (neo.svbt.ndexr.io/neo/build-desktop.sh,
#         Apple silicon only) and installed as /Applications/Cove.app.
#
# Releases come every save, so each machine has one runner (flock) and a
# "wanted" release: a burst of saves is one build, of the newest. A machine
# that does not answer is skipped quietly; the next release tries again. A
# running Cove is never quit - the new one is on disk for its next launch.
set -uo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
app="$here/app"
domains="$(cd "$here/.." && pwd)"
state="${XDG_STATE_HOME:-$HOME/.local/state}/cove-desktop"
mkdir -p "$state"
SETTLE="${SETTLE:-120}"

log() { echo "$(date -u +%FT%TZ) $*" >> "$state/$host.log"; }

status() {
  for h in laptop neo cell; do
    if [[ -f "$state/$h.json" ]]; then cat "$state/$h.json"; echo; else echo "{\"host\":\"$h\",\"state\":\"never built\"}"; fi
  done
}

# {host, release, state, at, note} - what Cove or a person reads.
mark() {
  printf '{"host":"%s","release":"%s","state":"%s","at":"%s","note":"%s"}\n' \
    "$host" "$1" "$2" "$(date -u +%FT%TZ)" "${3//\"/\'}" > "$state/$host.json.part" &&
    mv -f "$state/$host.json.part" "$state/$host.json"
}

reachable() { [[ "$host" == cell ]] || ssh -o BatchMode=yes -o ConnectTimeout=5 "$host" true 2>/dev/null; }

# Build on this host, in a clean env: build_linux [then-command].
build_linux() {
  # Cove's own toolchain (Rust 1.96) on a clean PATH: without it build-linux.sh
  # loads the stack's Rust, whose PKG_CONFIG_PATH hides the system GTK/atk the
  # webview links against ("you may need to install atk", 2026-10-18).
  local tc="${APP_RUST:-$HOME/.local/share/cove-rust}" i
  # This host is busy: the release profile's fat-LTO link wants 2+ GB in one
  # rustc, and earlyoom killed it at 2 GB free (2026-10-18). The laptop cannot
  # build it itself (no GTK/WebKit dev packages, Rust 1.88, no root), so: wait
  # for 8 GB, then thin LTO over 4 jobs - a desktop test build.
  # One build at a time: the cell's and the laptop's start on the same
  # release, and two fat links at once is the earlyoom kill above.
  exec 8> "$state/build.lock"; flock 8
  for i in $(seq 1 60); do
    (( $(awk '/MemAvailable/ {print int($2/1048576)}' /proc/meminfo) >= 8 )) && break
    sleep 30
  done
  ( cd "$app" && env -i WEBVIEW="${WEBVIEW:-1}" HOME="$HOME" USER="$USER" TMPDIR="$HOME/tmp" RUSTUP_HOME="$tc/rustup" CARGO_HOME="$tc/cargo" \
      CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_BUILD_JOBS=4 \
      PATH="$tc/cargo/bin:/usr/local/bin:/usr/bin:/bin" bash -c "nice -n 10 ./build-linux.sh${1:+ && $1}" ) >> "$state/$host.log" 2>&1
}

cell() {
  WEBVIEW=0 build_linux || { echo "the cell build failed"; return 1; }
  echo "built app/dist-cell/linux-x86_64/cove.tar.gz; cells take it within a minute, running ones at their next start"
}

laptop() {
  local arch=x86_64 tgz
  build_linux || { echo "the Linux build failed"; return 1; }
  tgz="$app/dist/linux-$arch/cove.tar.gz"
  ssh "$host" 'mkdir -p ~/.cache/cove-desktop' &&
    scp -q "$tgz" "$host:.cache/cove-desktop/cove.tar.gz" || { echo "copy to the laptop failed"; return 1; }
  # shellcheck disable=SC2016
  ssh "$host" 'bash -s' >> "$state/$host.log" 2>&1 <<'SH' || { echo "install on the laptop failed"; return 1; }
set -e
cd ~/.cache/cove-desktop && rm -rf cove-*-linux-* && tar xzf cove.tar.gz
./cove-*-linux-*/install.sh
SH
  # Installed is not the same as able to start: say what it lacks.
  local missing
  missing="$(ssh "$host" 'b=$(command -v cove || echo ~/.local/bin/cove); ldd "$b" 2>/dev/null | awk "/not found/ {print \$1}" | tr "\n" " "')"
  if [[ -n "$missing" ]]; then
    echo "installed, but it cannot start until the laptop has: $missing- install WebKitGTK 4.1 there once (Gentoo: net-libs/webkit-gtk:4.1; Debian/Ubuntu: libwebkit2gtk-4.1-0)"
    return 2
  fi
  echo "installed in ~/.local/bin/cove"
}

neo() {
  # Universal (Apple silicon and Intel): this dist/darwin is also what
  # publish.sh ships to the download page, and an arm64-only one went there
  # once a push built after a universal one (2026-11-02).
  NEO_APP=cove.ndexr.io NEO_HOST="$host" \
    nice -n 10 "$domains/neo.svbt.ndexr.io/neo/build-desktop.sh" --install >> "$state/$host.log" 2>&1 ||
    { echo "the macOS build or install failed"; return 1; }
  echo "installed /Applications/Cove.app"
}

run_one() {
  host="$1"
  [[ -n "${2:-}" ]] && echo "$2" > "$state/$host.want"
  [[ -f "$state/$host.want" ]] || echo "by-hand-$(date -u +%Y%m%dT%H%M%SZ)" > "$state/$host.want"
  exec 9> "$state/$host.lock"
  flock -n 9 || exit 0          # a runner is going; it picks up the newer want
  while :; do
    sleep "$SETTLE"             # let a burst of saves settle into one build
    local want done note rc
    want="$(cat "$state/$host.want")"
    done="$(cat "$state/$host.done" 2>/dev/null || true)"
    [[ "$want" == "$done" ]] && break
    if ! reachable; then
      log "skip $want: $host does not answer"
      mark "$done" "unreachable" "$host did not answer for $want; the next release tries again"
      break
    fi
    log "build $want"
    mark "$done" "building" "building $want"
    note="$("$host")"; rc=$?
    log "rc=$rc $note"
    case $rc in
      0) echo "$want" > "$state/$host.done"; mark "$want" "ok" "$note" ;;
      2) echo "$want" > "$state/$host.done"; mark "$want" "needs-action" "$note" ;;
      *) mark "$done" "failed" "$want: $note - see $state/$host.log"; break ;;
    esac
    # Keep the log to its last 4000 lines.
    tail -n 4000 "$state/$host.log" > "$state/$host.log.part" && mv -f "$state/$host.log.part" "$state/$host.log"
  done
}

case "${1:-}" in
  status) status ;;
  laptop|neo|cell) SETTLE="${SETTLE_ONE:-0}" run_one "$1" "${2:-}" ;;
  all)
    # Each in a transient unit of its own: started from run.sh, a plain child
    # would die with cove-onprem on its next restart, mid-build.
    # Not the laptop (operator, 2026-11-20: "you cant touch the laptop outside
    # of me updating it"): it takes each release from the published feed
    # (src/update.rs), like anyone's. `desktop-push.sh laptop` by hand still works.
    for h in neo cell; do
      systemd-run --user --quiet --collect --unit="cove-desktop-$h-$(date +%s%N)" \
        --setenv=SETTLE="$SETTLE" "$0" "$h-queued" "${2:-}" 2>/dev/null ||
        setsid "$0" "$h-queued" "${2:-}" < /dev/null > /dev/null 2>&1 &
    done ;;
  laptop-queued|neo-queued|cell-queued) run_one "${1%-queued}" "${2:-}" ;;
  *) echo "usage: desktop-push.sh all <release> | laptop|neo|cell [release] | status" >&2; exit 2 ;;
esac
