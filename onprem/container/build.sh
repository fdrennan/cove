#!/usr/bin/env bash
# Build a Cove project into an Apptainer image, outside the person's account,
# and leave it where they can collect it.
#
#   onprem/container/build.sh <project-dir> [out-root]
#   out-root defaults to /srv/ndexr/containers; the image lands in
#   <out-root>/<owner>/<host>/<stamp>/ with manifest.json, build.log, RUN.txt,
#   and <out-root>/<owner>/<host>/latest points at it.
#
# Why two stages, and why not `apptainer build` with %post (2026-11-01): the
# person's build.sh is their code, and `apptainer build` on this host only
# works as real root (AppArmor's apparmor_restrict_unprivileged_userns=1 stops
# --fakeroot: "mount namespace requires privileges"). So their code never runs
# under root here:
#   1. compile - rootless podman as uid 65534 inside the container, a subuid
#      on the host (not ubuntu), no capabilities, no new privileges, the
#      stack and site-library read-only, only a scratch copy of the source
#      writable;
#   2. package - root runs `apptainer build` from the pinned base and the
#      compiled tree with %files only. Nothing from the project executes.
#   3. smoke - root starts the image as uid 65534 (--security uid:...),
#      --contain, and waits for APP_HEALTH. A failure is recorded and fails
#      the build: an image that does not serve is not delivered.
#
# The image carries no software from the stack. It runs where the stack is
# mounted at exactly /opt/ndexr/stack (read-only), loads the modules the
# manifest names, each at a full version, and stops with a reason when any is
# missing - never a fallback to whatever is on PATH.
#
# The project declares what it needs in .cove/container.env (sourceable):
#   CONTAINER_MODULES  space-separated, full versions (Rust/1.96.0-GCCcore-13.3.0)
# Without that file the language's own modules are used and written to the
# manifest as "declared": "default for <language>".
set -euo pipefail

die() { echo "container: $*" >&2; exit 1; }

project="$(cd "${1:?usage: build.sh <project-dir> [out-root]}" && pwd -P)"
out_root="${2:-/srv/ndexr/containers}"
here="$(cd "$(dirname "$0")" && pwd -P)"

STACK=/opt/ndexr/stack
SITE=/opt/ndexr/site-library
# The base is pinned by digest: the same bytes every build, and the digest is
# what the manifest records. Bumped by hand, never by a tag moving.
BASE_REF="docker.io/library/almalinux:9-minimal"
BASE_DIGEST="sha256:6a245486786d3e92c8b4f97a98e2acd5ffa4d78ef1d5fefbd91a64a9965bcbf0"
BASE="docker.io/library/almalinux@$BASE_DIGEST"

[ -r "$project/.cove/app.env" ] || die "$project has no .cove/app.env - not a Cove project"
[ -x "$project/.cove/run.sh" ] || die "$project has no .cove/run.sh"
[ -r "$STACK/use.sh" ] || die "the ndexr stack is not mounted at $STACK"

# What the project says about itself. Sourced in a subshell so a project's
# app.env cannot set our variables.
read_env() { ( set +u; . "$1" >/dev/null 2>&1; eval "printf '%s' \"\${$2:-}\"" ); }
lang="$(read_env "$project/.cove/app.env" APP_LANGUAGE)"
kind="$(read_env "$project/.cove/app.env" APP_KIND)"
port="$(read_env "$project/.cove/app.env" APP_PORT)"
health="$(read_env "$project/.cove/app.env" APP_HEALTH)"
[ -n "$lang" ] || die ".cove/app.env names no APP_LANGUAGE"
[ "$kind" != module ] || die "APP_KIND=module: a Shiny module is served by the platform's console pool and has no process of its own to put in an image. Give it a port app (APP_KIND=port) to containerise it"
[ -n "$port" ] || die ".cove/app.env names no APP_PORT"
health="${health:-/}"

declared="project"
modules=""
if [ -r "$project/.cove/container.env" ]; then
  modules="$(read_env "$project/.cove/container.env" CONTAINER_MODULES)"
  [ -n "$modules" ] || die ".cove/container.env sets no CONTAINER_MODULES"
else
  declared="default for $lang"
  case "$lang" in
    rust)   modules="Rust/1.96.0-GCCcore-13.3.0" ;;
    python) modules="env/Python/3.13.15" ;;
    julia)  modules="env/Julia/1.11.7" ;;
    node)   modules="nodejs/22.22.2-GCCcore-13.3.0" ;;
    r)      modules="env/r4.6_py3.13-release" ;;
    *)      die "no default modules for APP_LANGUAGE=$lang: declare them in .cove/container.env (CONTAINER_MODULES=...)" ;;
  esac
fi
for m in $modules; do
  case "$m" in */*) ;; *) die "module '$m' has no version: name it in full (ml -t avail $m) so the image always loads the same one" ;; esac
done

case "$(uname -m)" in
  x86_64|amd64)  triple=x86_64-pc-linux-gnu ;;
  aarch64|arm64) triple=aarch64-unknown-linux-gnu ;;
  *) die "no stack for $(uname -m)" ;;
esac

host="$(basename "$project")"
owner="$(basename "$(dirname "$project")")"
case "$owner" in domains) owner=platform ;; esac
stamp="${CONTAINER_STAMP:-$(date -u +%Y%m%dT%H%M%SZ)}"
dest="$out_root/$owner/$host/$stamp"
work="${CONTAINER_WORK:-$HOME/.cache/cove-container}/$owner-$host-$stamp"
mkdir -p "$dest" "$work"
log="$dest/build.log"
exec > >(tee -a "$log") 2>&1
echo "container: $owner/$host -> $dest"

apptainer_bin() {
  ( set +u; . "$STACK/use.sh" >/dev/null 2>&1; ml Apptainer >/dev/null 2>&1; command -v apptainer )
}
APPTAINER="$(apptainer_bin)" || die "ml Apptainer failed"
[ -x "$APPTAINER" ] || die "no apptainer in the stack"

# The work tree has three owners: ubuntu, the build container's subuid, and
# root (apptainer's cache and tmp). Each removes its own.
cleanup() { sudo -n rm -rf "$work/cache" "$work/build-temp-"* 2>/dev/null; podman unshare rm -rf "$work"; }
trap cleanup EXIT

# --- source: what the project tracks, never its build output or runtime state.
src="$work/app"
mkdir -p "$src"
commit=""
if git -C "$project" rev-parse --git-dir >/dev/null 2>&1 \
   && [ "$(git -C "$project" rev-parse --show-toplevel)" = "$project" ]; then
  commit="$(git -C "$project" rev-parse HEAD)"
  dirty="$(git -C "$project" status --porcelain | wc -l)"
  # Tracked plus untracked-but-not-ignored: what the person sees as their
  # project, uncommitted edits included (the manifest says how many).
  ( cd "$project" && git ls-files -co --exclude-standard -z | tar --null -T - -cf - ) | tar -xf - -C "$src"
else
  dirty=-1
  tar -C "$project" --exclude=./target --exclude=./node_modules --exclude=./.cove/run \
      --exclude=./.git --exclude=./dist --exclude=./data -cf - . | tar -xf - -C "$src"
fi
echo "container: source $(find "$src" -type f | wc -l) files${commit:+, commit ${commit:0:12}}"

# --- 1. compile, unprivileged.
podman image exists "$BASE" || podman pull "$BASE" >/dev/null
podman unshare chown -R 65534:65534 "$src"
binds=(-v "$STACK:$STACK:ro")
[ -d "$SITE" ] && binds+=(-v "$SITE:$SITE:ro")
podman run --rm --user 65534:65534 --cap-drop=all --security-opt no-new-privileges \
  --pids-limit 4096 --memory "${CONTAINER_MEMORY:-8g}" \
  "${binds[@]}" -v "$src:/app" -w /app -e HOME=/tmp/home -e MODULES="$modules" \
  -e COMPAT="$STACK/$triple/prefix/$triple/usr/bin" \
  "$BASE" bash -c '
    set -euo pipefail
    mkdir -p "$HOME"
    set +u; . /opt/ndexr/stack/use.sh >/dev/null
    for m in $MODULES; do ml "$m" || { echo "container: ml $m failed in the build" >&2; exit 1; }; done
    # Lmod reads unset variables, so set -u waits until it is done.
    loaded="$(ml -t list 2>&1)"
    set -u
    for m in $MODULES; do grep -qx "$m" <<< "$loaded" || { echo "container: $m is not loaded after ml $m" >&2; exit 1; }; done
    grep -v ":$" <<< "$loaded" > /app/.cove/.container-modules
    # The base image is minimal on purpose; the linker, tar, make and git a
    # build needs come from the stack'"'"'s compat layer, as at run time.
    export PATH="$PATH:$COMPAT"
    [ -x .cove/build.sh ] && .cove/build.sh
    rm -rf /app/.cove/run
  ' || die "the build failed inside the build container (log: $log)"
resolved="$(cat "$src/.cove/.container-modules")"
podman unshare rm -f "$src/.cove/.container-modules"
# Build caches are not part of the app: a Rust target/ keeps only the binary.
if [ -d "$src/target" ]; then
  podman unshare find "$src/target" -mindepth 1 -maxdepth 1 ! -name release -exec rm -rf {} +
  podman unshare find "$src/target/release" -mindepth 1 -maxdepth 1 \( -type d -o -name '*.d' -o -name '*.rlib' \) -exec rm -rf {} +
fi
echo "container: compiled; $(podman unshare du -sh "$src" | cut -f1) to package"

# --- 2. package, as root, from files only.
podman save --format oci-archive -o "$work/base.tar" "$BASE" >/dev/null
cat > "$work/entry.sh" <<EOF
#!/bin/bash
# Written by Cove's container builder for $owner/$host ($stamp).
set -euo pipefail
die() { echo "\$*" >&2; exit 78; }
[ -r /opt/ndexr/stack/use.sh ] || die "this image needs the ndexr stack at /opt/ndexr/stack: apptainer run --bind /opt/ndexr/stack:/opt/ndexr/stack:ro ..."
case "\$(uname -m)" in x86_64|amd64) [ "$triple" = x86_64-pc-linux-gnu ] ;; aarch64|arm64) [ "$triple" = aarch64-unknown-linux-gnu ] ;; *) false ;; esac \
  || die "this image was built for $triple and this machine is \$(uname -m)"
set +u; . /opt/ndexr/stack/use.sh >/dev/null
for m in $modules; do ml "\$m" >/dev/null 2>&1 || die "module \$m is not in the stack mounted here (built against revision $(getfattr --only-values -n user.revision "$STACK" 2>/dev/null))"; done
set -u
export PATH="\$PATH:$STACK/$triple/prefix/$triple/usr/bin"
cd /app
exec /app/.cove/run.sh "\$@"
EOF
cat > "$work/image.def" <<EOF
Bootstrap: oci-archive
From: $work/base.tar

%files
  $src /app
  $work/entry.sh /ndexr/entry.sh

%post
  mkdir -p /opt/ndexr/stack
  chown -R 0:0 /app
  chmod 0755 /ndexr/entry.sh
  chmod -R a+rX /app

%environment
  export PORT="\${PORT:-$port}"

%runscript
  exec /ndexr/entry.sh "\$@"

%labels
  org.ndexr.project $owner/$host
  org.ndexr.built $stamp
  org.ndexr.commit ${commit:-none}
  org.ndexr.stack.revision $(getfattr --only-values -n user.revision "$STACK" 2>/dev/null)
  org.ndexr.modules $modules
EOF
# The %post above is the builder's own chown and chmod over files it wrote; the
# project's code is data at this point and nothing in it is run.
sif="$dest/$host.sif"
sudo -n env PATH="$PATH" APPTAINER_TMPDIR="$work" APPTAINER_CACHEDIR="$work/cache" \
  "$APPTAINER" build --force "$sif" "$work/image.def" >/dev/null || die "apptainer build failed (log: $log)"
sudo -n chown "$(id -u):$(id -g)" "$sif"

# --- 3. smoke: run it as nobody and wait for APP_HEALTH.
smoke_port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
sudo -n env PATH="$PATH" "$APPTAINER" run --security uid:65534,gid:65534 --contain --writable-tmpfs \
  --bind "$STACK:$STACK:ro" --env PORT="$smoke_port" "$sif" > "$work/smoke.log" 2>&1 &
smoke_pid=$!
smoke=failed
for _ in $(seq 1 "${SMOKE_WAIT:-120}"); do
  if curl -fsS -o /dev/null "http://127.0.0.1:$smoke_port$health" 2>/dev/null; then smoke=passed; break; fi
  kill -0 "$smoke_pid" 2>/dev/null || break
  sleep 1
done
sudo -n pkill -TERM -P "$smoke_pid" 2>/dev/null || true
sudo -n kill -TERM "$smoke_pid" 2>/dev/null || true
wait "$smoke_pid" 2>/dev/null || true
sed 's/^/smoke: /' "$work/smoke.log"
echo "container: smoke $smoke (GET $health on a fresh port)"

# --- record.
sha="$(sha256sum "$sif" | cut -d' ' -f1)"
python3 - "$dest/manifest.json" <<EOF
import json, sys
json.dump({
  "project": "$owner/$host",
  "built_at": "$stamp",
  "builder": "cove onprem/container/build.sh, as $(id -un) on $(hostname -s), outside the person's account",
  "source": {"commit": "$commit" or None, "uncommitted_files": $dirty},
  "app": {"language": "$lang", "kind": "${kind:-port}", "port": int("$port"), "health": "$health",
          "entry_point": "/ndexr/entry.sh -> /app/.cove/run.sh"},
  "architecture": "$triple",
  "base_image": {"ref": "$BASE_REF", "digest": "$BASE_DIGEST"},
  "stack": {"path": "$STACK", "mount": "read-only bind, required at run time",
            "revision": "$(getfattr --only-values -n user.revision "$STACK" 2>/dev/null)",
            "root_hash": "$(getfattr --only-values -n user.root_hash "$STACK" 2>/dev/null)"},
  "modules": {"declared": "$declared", "requested": "$modules".split(),
              "resolved": """$resolved""".split()},
  "image": {"file": "$host.sif", "sha256": "$sha", "bytes": $(stat -c %s "$sif")},
  "smoke": "$smoke",
  "run": "apptainer run --writable-tmpfs --bind /opt/ndexr/stack:/opt/ndexr/stack:ro $host.sif",
}, open(sys.argv[1], "w"), indent=2)
EOF
cat > "$dest/RUN.txt" <<EOF
$owner/$host, built $stamp from ${commit:-an unversioned folder}

Needs Apptainer and the ndexr stack mounted read-only at /opt/ndexr/stack
(stack revision $(getfattr --only-values -n user.revision "$STACK" 2>/dev/null) or later; modules: $modules).

  sha256sum -c <<< "$sha  $host.sif"
  apptainer run --writable-tmpfs --bind /opt/ndexr/stack:/opt/ndexr/stack:ro $host.sif
  # then open http://127.0.0.1:$port/   (PORT=9999 apptainer run ... for another port)
EOF
[ "$smoke" = passed ] || die "the image built but did not answer $health - not delivered (log: $log)"
ln -sfn "$stamp" "$out_root/$owner/$host/latest"
echo "container: done - $sif ($sha)"
