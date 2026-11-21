#!/usr/bin/env bash
# cove.ndexr.io, on-prem: the app served from this host's own working tree,
# rebuilt and restarted the moment its source changes — as svbt.ndexr.io
# reloads when svbt's VERSION moves, but for a compiled app.
#
#   onprem/run.sh                 foreground (what the user unit runs)
#   PORT=8796 onprem/run.sh       the loopback port nginx proxies to (default 8796;
#                                 8788 and 8790 are the laptop and neo tunnels)
#
# Loop: build (profile `onprem`: release speed, no fat LTO) → serve → every
# 2 s, if anything under app/src, app/assets, app/templates or the Cargo files is newer than
# the running build, rebuild; only a build that succeeds replaces the running
# server, so a half-typed edit never takes the site down. The browser UI,
# app/dist/web, is rebuilt after every server build (build_web) and served
# from disk, so a page reload picks it up.
set -uo pipefail

here="$(cd "$(dirname "$0")/.." && pwd)"
app="$here/app"
port="${PORT:-8796}"
state="${XDG_STATE_HOME:-$HOME/.local/state}/cove-onprem"
target="${CARGO_TARGET_DIR:-$state/target}"
bin="$state/cove"
mkdir -p "$state"

log() { echo "$(date +%T) onprem: $*"; }

# The browser UI is the IDE here, so it rebuilds with the server. The stack's
# Rust has no wasm32 target: build-web.sh runs on a private rustup toolchain
# (1.96.0 + wasm32-unknown-unknown + wasm-bindgen-cli 0.2.129) when one exists.
web_rust="$HOME/.local/share/cove-rust"

# Rust ≥ 1.95 (egui 0.36): the host's rustc is older; the stack's is not.
# (Lmod's init reads unset variables, so -u is off while it loads.)
set +u
# shellcheck disable=SC1091
. /opt/ndexr/stack/use.sh >/dev/null 2>&1 && ml Rust/1.96.0-GCCcore-13.3.0 >/dev/null 2>&1
set -u
# ...but a CVMFS publish takes the triple's tree away for about an hour, and
# with it the stack's cargo: every build in that window fell back to the
# host's rustc 1.93 and failed (2026-10-13). The private toolchain above is
# 1.96 too, so the server builds on it whenever the stack's cargo is not there.
cargo_() {
  if cargo --version 2>/dev/null | grep -q ' 1\.9[5-9]\.'; then
    cargo "$@"
  elif [[ -x "$web_rust/cargo/bin/cargo" ]]; then
    RUSTUP_HOME="$web_rust/rustup" CARGO_HOME="$web_rust/cargo" PATH="$web_rust/cargo/bin:$PATH" cargo "$@"
  else
    echo "onprem: need Rust ≥ 1.95 (ml Rust/1.96.0-GCCcore-13.3.0, or $web_rust), have: $(cargo --version 2>&1)" >&2
    return 1
  fi
}
build_web() {
  [[ -x "$web_rust/cargo/bin/wasm-bindgen" ]] || { log "no wasm toolchain in $web_rust — serving the last app/dist/web"; return 0; }
  local t0=$SECONDS
  if RUSTUP_HOME="$web_rust/rustup" CARGO_HOME="$web_rust/cargo" PATH="$web_rust/cargo/bin:$PATH" \
     "$app/build-web.sh" >"$state/web.log" 2>&1; then
    log "browser UI built in $((SECONDS - t0))s"
  else
    log "browser UI build failed — see $state/web.log; serving the last app/dist/web"
  fi
}

# The atlas's shape is a promise (operator, 2026-10-21: "focus on making
# sure were not reverting back on our visual improvements"): when anything
# the atlas is drawn from changed, its tests run before the build becomes a
# release, and a build that fails them is held - the running release stays
# and every session keeps what it has. Several sessions edit the atlas at
# once; one whole-file copy from a stale buffer undid another's work.
guard() {
  [[ -f "$state/guard.stamp" && -z "$(find "$app/src/atlas.rs" "$app/src/atlas" -newer "$state/guard.stamp" -print -quit 2>/dev/null)" ]] && return 0
  local t0=$SECONDS
  if CARGO_TARGET_DIR="$state/test-target" cargo_ test --manifest-path "$app/Cargo.toml" --bin cove atlas:: >"$state/guard.log" 2>&1; then
    touch -r "$state/build-started" "$state/guard.stamp"
    log "atlas tests passed in $((SECONDS - t0))s"
    return 0
  fi
  log "atlas tests failed - release held, the running build stays; see $state/guard.log"
  grep -E "^(error|thread '.*' .*panicked)" -A2 "$state/guard.log" | head -12
  return 1
}

# LICENCES ARE A RELEASE CONDITION TOO (2026-10-29, operator: "make a
# compliance plan or ability to prove that compliance"). Cove ships binaries,
# so the notices of every crate in it must ship with them (the server embeds
# THIRD-PARTY-NOTICES.txt, regenerated here whenever Cargo.lock moves), no
# crate may need a copyleft licence, and no line of RStudio's or Inlet's code
# may be in Cove. onprem/compliance.sh --gate checks all of it in ~10 s; a
# failure holds the release exactly as a failed atlas test does. The full
# report, with Inlet's live checks, is the daily cove-compliance.timer.
notices() {
  [[ "$app/THIRD-PARTY-NOTICES.txt" -nt "$app/Cargo.lock" ]] && return 0
  "$here/onprem/compliance.sh" --notices >"$state/notices.log" 2>&1 \
    && log "third-party notices regenerated for the new Cargo.lock" \
    || log "third-party notices could not be regenerated - see $state/notices.log"
}
licence_gate() {
  "$here/onprem/compliance.sh" --gate --out "$state/compliance" >"$state/compliance.log" 2>&1 && return 0
  log "licence gate failed - release held, the running build stays; see $state/compliance.log"
  grep -E '^FAIL' "$state/compliance.log" | head -6
  return 1
}

build() {
  notices
  # What this build compiles, named once, at its start: the commit, and
  # "+dirty" when app/ held uncommitted edits (the working tree is what is
  # compiled). publish() names the release by it. It used to read HEAD when
  # the build ENDED, so a commit landing mid-build named a release that did
  # not contain it: 20261120T231025Z-f7692fb was dc293ce's code, and a test
  # of f7692fb's fix against it failed (2026-11-20).
  ( cd "$app" && printf '%s%s' "$(git rev-parse --short HEAD 2>/dev/null || echo tree)" "$([[ -n "$(git status --porcelain -- . 2>/dev/null)" ]] && echo +dirty)" ) > "$state/build-commit.new" && mv -f "$state/build-commit.new" "$state/build-commit"
  log "building $(cat "$state/build-commit")…"
  local t0=$SECONDS
  # The stamp is when the build STARTED: an edit saved mid-build is newer than
  # it and builds again. (Stamped at the end, an edit landing during a 17 s
  # build was never compiled — the plot theme's R driver, 2026-10-13.)
  touch "$state/build-started"
  # SIX JOBS, NOT ONE PER CORE (2026-10-29). Uncapped, LLVM ran up to 24 of
  # the profile's 16 codegen units at once and rustc passed 2.8 GiB while the
  # host's account cells held most of its 30 GiB; earlyoom killed the build
  # three times running and no release shipped for half an hour. Codegen takes
  # jobserver tokens, so the cap bounds the peak. COVE_BUILD_JOBS overrides.
  CARGO_BUILD_JOBS="${COVE_BUILD_JOBS:-6}" CARGO_TARGET_DIR="$target" cargo_ build --profile onprem --manifest-path "$app/Cargo.toml" >"$state/build.log" 2>&1
  local rc=$?
  grep -E '^(error|warning: unused)' -A5 "$state/build.log"
  local out="$target/onprem/cove"
  # Held before it replaces anything, so nothing can publish it later.
  if [[ $rc -eq 0 && -x "$out" ]] && { ! guard || ! licence_gate; }; then
    log "build held after $((SECONDS - t0))s — keeping the running server"
    return 1
  fi
  if [[ $rc -eq 0 && -x "$out" ]]; then
    cp "$out" "$bin.new" && mv "$bin.new" "$bin" && touch -r "$state/build-started" "$bin.stamp"
    log "built in $((SECONDS - t0))s"
    build_web
    return 0
  fi
  log "build failed after $((SECONDS - t0))s — keeping the running server"
  return 1
}

pid=""
# Every ndexr user (2026-10-13): once onprem/install-multiuser.sh has put the
# root unit in place, the server is `cove multiuser` under it, and this
# loop only builds and restarts it - a user unit cannot start a root process
# it could then watch or signal.
mu_unit=cove-onprem-multiuser
multiuser() { [[ -f "/etc/systemd/system/$mu_unit.service" ]]; }
alive() { if multiuser; then systemctl is-active --quiet "$mu_unit"; else kill -0 "$pid" 2>/dev/null; fi; }
# What the root unit runs, and what every person's session runs as them: a
# root-owned copy outside this home. Sessions start from the multiuser
# server's own binary and serve its web dir, and /home/ubuntu (0750) and
# ~/.local (0700) are closed to everyone else - so every account but the
# operator's got "could not start a session: Permission denied (os error 13)"
# (fdrennan, 2026-10-15).
shared=/usr/local/lib/cove
# EACH BUILD IS A RELEASE, AND NO ONE IS MOVED ONTO IT (operator, 2026-10-18:
# "we can't be kicking people out of their sessions whenever we restart").
# A build goes to $shared/releases/<stamp>/{cove,web}, and $shared/current
# points at the newest; $shared/cove and $shared/web are links through it, for
# the unit and anything else that names them. A running session keeps the
# build it started on - its binary and its browser UI - and the page offers
# the new one (release.rs); new sessions start on `current`
# (multiuser.rs current_release).
publish() {
  local id rel
  id="$(date -u +%Y%m%dT%H%M%SZ)-$(cat "$state/build-commit" 2>/dev/null || echo tree)"
  rel="$shared/releases/$id"
  sudo -n install -d -m 755 "$shared" "$shared/releases" "$rel" "$rel/web" &&
    sudo -n install -m 755 -o root -g root "$bin" "$rel/cove" &&
    sudo -n rsync -a --chown=root:root --chmod=D755,F644 "$app/dist/web/" "$rel/web/" ||
    { log "could not publish to $rel"; return 1; }
  # The old flat layout (a file and a folder) gives way to the links once.
  [[ -L "$shared/web" || ! -e "$shared/web" ]] || sudo -n mv "$shared/web" "$shared/web.flat-$id"
  [[ -L "$shared/cove" || ! -e "$shared/cove" ]] || sudo -n mv "$shared/cove" "$shared/cove.flat-$id"
  sudo -n ln -sfn "releases/$id" "$shared/current.new" && sudo -n mv -Tf "$shared/current.new" "$shared/current" &&
    sudo -n ln -sfn current/cove "$shared/cove" && sudo -n ln -sfn current/web "$shared/web" ||
    { log "could not point $shared/current at $id"; return 1; }
  log "published release $id"
  # The desktop app follows every release onto the test machines (laptop,
  # neo) that answer - detached, one build per machine at a time.
  setsid "$here/onprem/desktop-push.sh" all "$id" </dev/null >/dev/null 2>&1 &
  # The public source follows too, at most hourly, behind a secrets gate
  # (onprem/public-push.sh): github.com/fdrennan/cove went a week stale when
  # it was pushed by hand (operator, 2026-11-20).
  setsid "$here/onprem/public-push.sh" "$id" </dev/null >/dev/null 2>&1 &
  prune_releases
  publish_claude
  publish_rust
  publish_seeds </dev/null >/dev/null 2>&1 &
}
# Releases no process runs and that are not among the newest five go; one a
# session still runs stays however old it is.
prune_releases() {
  local inuse keep r
  inuse="$(sudo -n find /proc -maxdepth 2 -name exe -printf '%l\n' 2>/dev/null | sed -n "s#^$shared/releases/\([^/]*\)/cove.*#\1#p" | sort -u)"
  # The server itself always runs a release, so nothing seen means the look
  # failed, not that nothing runs. It failed during a crash loop on a box at
  # load 86 (2026-10-25), and the releases of two running sessions were
  # deleted under them: their pages said "not found".
  [[ -n "$inuse" ]] || { log "prune: could not see which releases are running - pruning nothing"; return 0; }
  keep="$(ls -1 "$shared/releases" 2>/dev/null | sort | tail -n 5)"
  for r in $(ls -1 "$shared/releases" 2>/dev/null); do
    printf '%s\n%s\n' "$inuse" "$keep" | grep -qxF "$r" && continue
    [[ "$(readlink "$shared/current")" == "releases/$r" ]] && continue
    sudo -n rm -rf "$shared/releases/$r" && log "pruned release $r"
  done
}
# The root server is restarted only when its own code changed: everything a
# session runs comes from its release, so a UI or session-side build needs no
# restart at all. When it is restarted the sessions carry on (KillMode=process,
# multiuser.rs adopt) - a few seconds of failed polls, nobody signed out.
server_changed() {
  [[ ! -f "$state/server.stamp" ]] && return 0
  [[ -n "$(find "$app/src/multiuser.rs" "$app/src/server.rs" "$app/src/host.rs" "$app/src/api.rs" "$app/src/storage.rs" "$app/src/kernel.rs" "$app/src/envs.rs" "$app/assets" "$app/templates" "$app/assets/route-app.sh" "$app/Cargo.toml" "$app/Cargo.lock" "$app/THIRD-PARTY-NOTICES.txt" -newer "$state/server.stamp" -print -quit 2>/dev/null)" ]]
}
# Claude Code for every account's Terminal (operator, 2026-10-16: "not working
# claude" - fdrennan got "claude: command not found"; the only copy was in
# ~ubuntu, which no other account can read). This machine's current claude,
# copied beside cove when it changes; the Terminal puts $shared/bin on PATH
# behind the person's own ~/.local/bin, so an install of their own wins.
publish_claude() {
  local src
  src="$(readlink -f "$HOME/.local/bin/claude" 2>/dev/null)"
  [[ -x "$src" ]] || return 0
  sudo -n cmp -s "$src" "$shared/bin/claude" && return 0
  sudo -n install -d -m 755 "$shared/bin" &&
    sudo -n install -m 755 -o root -g root "$src" "$shared/bin/claude.new" &&
    sudo -n mv -f "$shared/bin/claude.new" "$shared/bin/claude" &&
    log "published claude $("$src" --version 2>/dev/null | head -1) to $shared/bin"
}
# Cove's Rust toolchain (wasm32 target + wasm-bindgen) for the person's own
# Rust apps (app/assets/deploy-app.sh builds them AS the person): ~ubuntu is unreadable to
# them, so a root-owned, read-only copy beside cove, refreshed when it changes.
publish_rust() {
  local src="$HOME/.local/share/cove-rust" stamp
  [[ -x "$src/cargo/bin/wasm-bindgen" ]] || return 0
  stamp="$(ls -1 "$src/rustup/toolchains" 2>/dev/null | tr '\n' ' ')$("$src/cargo/bin/wasm-bindgen" --version 2>/dev/null)"
  [[ "$(sudo -n cat "$shared/rust/.stamp" 2>/dev/null)" == "$stamp" ]] && return 0
  sudo -n install -d -m 755 "$shared/rust" &&
    sudo -n rsync -a --delete --chown=root:root --chmod=Dgo+rx,Fgo+r --exclude 'registry/' --exclude 'git/' "$src/" "$shared/rust/" &&
    sudo -n find "$shared/rust/rustup/toolchains" "$shared/rust/cargo/bin" -type f -perm -u+x -exec chmod go+x {} + &&
    printf '%s' "$stamp" | sudo -n tee "$shared/rust/.stamp" >/dev/null &&
    log "published the Rust toolchain to $shared/rust ($stamp)"
}
# Compiled starters, built once for everyone (operator, 2026-10-25: "make sure
# we have a user ready project already compiled ... so that it can start right
# up"; app/src/host.rs seeds). Each starter that compiles is laid out by this
# build (`cove seed lay`), built with its own build.sh, and its outputs -
# target/, dist/, Cargo.lock - published read-only to $shared/seeds/<starter>
# with the stamp of the templates they came from; a new project copies them
# in. Only a starter whose stamp moved is rebuilt. In the background, one at
# a time: the Rust app takes 15 min cold on a busy box.
publish_seeds() {
  exec 9>"$state/seeds.lock"
  flock -n 9 || return 0
  local s stamp work t0 p
  mkdir -p "$state/seeds"
  for s in $("$bin" seed list 2>/dev/null); do
    stamp="$("$bin" seed stamp "$s" 2>/dev/null)" || continue
    [[ "$(cat "$shared/seeds/$s/.stamp" 2>/dev/null)" == "$stamp" ]] && continue
    work="$state/seeds/$s"
    rm -rf "$work"
    "$bin" seed lay "$s" "$work" >/dev/null || { log "seed $s: could not lay it out"; continue; }
    t0=$SECONDS
    # Lowest priority, four jobs: an unbounded seed build (24 rustc beside
    # Cove's own build and others) took the load to 86 and every console
    # replica stopped answering (ndexr.io down, 2026-10-25).
    if ! (cd "$work" && CARGO_BUILD_JOBS=4 nice -n 19 .cove/build.sh) >"$state/seeds/$s.log" 2>&1; then
      log "seed $s: build failed after $((SECONDS - t0))s - see $state/seeds/$s.log"
      continue
    fi
    sudo -n install -d -m 755 "$shared/seeds" && sudo -n rm -rf "$shared/seeds/$s.new" && sudo -n install -d -m 755 "$shared/seeds/$s.new" || continue
    for p in target dist Cargo.lock; do
      [[ -e "$work/$p" ]] && sudo -n cp -a "$work/$p" "$shared/seeds/$s.new/"
    done
    printf '%s\n' "$stamp" | sudo -n tee "$shared/seeds/$s.new/.stamp" >/dev/null &&
      sudo -n chown -R root:root "$shared/seeds/$s.new" && sudo -n chmod -R go+rX,go-w "$shared/seeds/$s.new" &&
      sudo -n rm -rf "$shared/seeds/$s" && sudo -n mv "$shared/seeds/$s.new" "$shared/seeds/$s" &&
      log "seed $s: built in $((SECONDS - t0))s, published to $shared/seeds/$s ($stamp)"
  done
}
serve() {
  if multiuser; then
    [[ -n "$pid" ]] && kill "$pid" 2>/dev/null && wait "$pid" 2>/dev/null
    pid=""
    publish || return
    if ! systemctl is-active --quiet "$mu_unit" || server_changed; then
      touch -r "$state/build-started" "$state/server.stamp" 2>/dev/null || touch "$state/server.stamp"
      sudo -n systemctl restart "$mu_unit" && log "restarted $mu_unit (cove multiuser) on 127.0.0.1:$port - sessions carry on"
    else
      log "server unchanged - not restarted; open sessions are offered the new build"
    fi
    return
  fi
  [[ -n "$pid" ]] && kill "$pid" 2>/dev/null && wait "$pid" 2>/dev/null
  # Never self-update: this server tracks the working tree, not releases.
  COVE_UPDATE_SECS=0 \
  COVE_TRUST_EMAILS="${COVE_TRUST_EMAILS:-$(cat "$state/trust-emails" 2>/dev/null)}" \
    "$bin" serve --listen "127.0.0.1:$port" --web-dir "$app/dist/web" &
  pid=$!
  log "serving pid $pid on 127.0.0.1:$port"
}

trap '[[ -n "$pid" ]] && kill "$pid" 2>/dev/null; exit 0' INT TERM

changed() {
  # app/templates too: host.rs include_str!s the new-site starters, and an
  # edit there never rebuilt (2026-10-25, the trimmed shiny-app template sat
  # unreleased for an hour). app/web too: build_web copies it into each
  # release, so an edit to web/index.html alone never shipped (2026-10-29, the
  # phone-keyboard fix sat waiting for an unrelated Rust change).
  # THIRD-PARTY-NOTICES.txt too: the server embeds it (server.rs
  # notices_response), and a regenerated file outside these folders sat
  # unreleased for 7 hours (2026-10-30, licenses.ndexr.io cove.notices-served).
  [[ -n "$(find "$app/src" "$app/assets" "$app/templates" "$app/web" "$app/Cargo.toml" "$app/Cargo.lock" "$app/THIRD-PARTY-NOTICES.txt" -newer "$bin.stamp" -print -quit 2>/dev/null)" ]]
}

touch -d '1970-01-01' "$bin.stamp" 2>/dev/null || touch "$bin.stamp"
build || [[ -x "$bin" ]] || { log "no binary to serve"; exit 1; }
serve
while sleep 2; do
  if ! alive; then
    log "server exited — restarting"
    serve
  elif changed; then
    # A failed build waits for the next change instead of retrying every 2 s
    # (it spun at 0 s a try while the stack's cargo was gone, 2026-10-13).
    if build; then serve; else touch -r "$state/build-started" "$bin.stamp"; fi
  fi
done
