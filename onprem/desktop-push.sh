#!/usr/bin/env bash
# Keep the desktop Coves on the test machines current: every release run.sh
# publishes also goes, as the desktop app, to `ssh laptop` and `ssh neo` when
# they answer (operator, 2026-10-18: "those machines need to be able to have
# the desktop version updated because we have to keep testing the desktop
# versions of those").
#
#   onprem/desktop-push.sh all <release-id>     what run.sh fires after a publish
#   onprem/desktop-push.sh laptop|neo [id]      one machine, by hand
#   onprem/desktop-push.sh status               where each machine is
#
# laptop  Pop!_OS 22.04, x86_64. Built HERE (build-linux.sh: glibc 2.28 via
#         zigbuild, with this host's WebKitGTK for the App panel - the laptop
#         has neither zig nor the dev libs), then build-deb.sh. Installed per
#         user (~/.local, the tarball's install.sh): the laptop has no
#         password-less sudo. With one, the .deb goes in through apt instead.
#         The .deb is always left at ~/Downloads/cove_latest_amd64.deb.
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
  for h in laptop neo; do
    if [[ -f "$state/$h.json" ]]; then cat "$state/$h.json"; echo; else echo "{\"host\":\"$h\",\"state\":\"never built\"}"; fi
  done
}

# {host, release, state, at, note} - what Cove or a person reads.
mark() {
  printf '{"host":"%s","release":"%s","state":"%s","at":"%s","note":"%s"}\n' \
    "$host" "$1" "$2" "$(date -u +%FT%TZ)" "${3//\"/\'}" > "$state/$host.json.part" &&
    mv -f "$state/$host.json.part" "$state/$host.json"
}

reachable() { ssh -o BatchMode=yes -o ConnectTimeout=5 "$host" true 2>/dev/null; }

laptop() {
  local arch=x86_64 tgz deb
  # Cove's own toolchain (Rust 1.96) on a clean PATH: without it build-linux.sh
  # loads the stack's Rust, whose PKG_CONFIG_PATH hides the system GTK/atk the
  # webview links against ("you may need to install atk", 2026-10-18).
  local tc="${APP_RUST:-$HOME/.local/share/cove-rust}" i
  # This host is busy: the release profile's fat-LTO link wants 2+ GB in one
  # rustc, and earlyoom killed it at 2 GB free (2026-10-18). The laptop cannot
  # build it itself (no GTK/WebKit dev packages, Rust 1.88, no root), so: wait
  # for 8 GB, then thin LTO over 4 jobs - a desktop test build.
  for i in $(seq 1 60); do
    (( $(awk '/MemAvailable/ {print int($2/1048576)}' /proc/meminfo) >= 8 )) && break
    sleep 30
  done
  ( cd "$app" && env -i HOME="$HOME" USER="$USER" TMPDIR="$HOME/tmp" RUSTUP_HOME="$tc/rustup" CARGO_HOME="$tc/cargo" \
      CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 CARGO_BUILD_JOBS=4 \
      PATH="$tc/cargo/bin:/usr/local/bin:/usr/bin:/bin" bash -c 'nice -n 10 ./build-linux.sh && ./build-deb.sh' ) >> "$state/$host.log" 2>&1 ||
    { echo "the Linux build failed"; return 1; }
  tgz="$app/dist/linux-$arch/cove.tar.gz"
  deb="$(ls -t "$app"/dist/deb/cove_*_amd64.deb 2>/dev/null | head -1)"
  ssh "$host" 'mkdir -p ~/.cache/cove-desktop ~/Downloads' &&
    scp -q "$tgz" "$host:.cache/cove-desktop/cove.tar.gz" &&
    { [[ -z "$deb" ]] || scp -q "$deb" "$host:Downloads/cove_latest_amd64.deb"; } || { echo "copy to the laptop failed"; return 1; }
  # shellcheck disable=SC2016
  ssh "$host" 'bash -s' >> "$state/$host.log" 2>&1 <<'SH' || { echo "install on the laptop failed"; return 1; }
set -e
cd ~/.cache/cove-desktop && rm -rf cove-*-linux-* && tar xzf cove.tar.gz
if sudo -n true 2>/dev/null; then
  sudo -n apt-get install -y ~/Downloads/cove_latest_amd64.deb
else
  ./cove-*-linux-*/install.sh
fi
SH
  # Installed is not the same as able to start: say what it lacks.
  local missing
  missing="$(ssh "$host" 'b=$(command -v cove || echo ~/.local/bin/cove); ldd "$b" 2>/dev/null | awk "/not found/ {print \$1}" | tr "\n" " "')"
  if [[ -n "$missing" ]]; then
    echo "installed, but it cannot start until the laptop has: $missing- run once on the laptop: sudo apt install ~/Downloads/cove_latest_amd64.deb (pulls libwebkit2gtk-4.1-0)"
    return 2
  fi
  echo "installed in ~/.local/bin/cove"
}

neo() {
  NEO_APP=cove.ndexr.io TARGETS=aarch64-apple-darwin NEO_HOST="$host" \
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
  laptop|neo) SETTLE="${SETTLE_ONE:-0}" run_one "$1" "${2:-}" ;;
  all)
    # Each in a transient unit of its own: started from run.sh, a plain child
    # would die with cove-onprem on its next restart, mid-build.
    for h in laptop neo; do
      systemd-run --user --quiet --collect --unit="cove-desktop-$h-$(date +%s%N)" \
        --setenv=SETTLE="$SETTLE" "$0" "$h-queued" "${2:-}" 2>/dev/null ||
        setsid "$0" "$h-queued" "${2:-}" < /dev/null > /dev/null 2>&1 &
    done ;;
  laptop-queued|neo-queued) run_one "${1%-queued}" "${2:-}" ;;
  *) echo "usage: desktop-push.sh all <release> | laptop|neo [release] | status" >&2; exit 2 ;;
esac
