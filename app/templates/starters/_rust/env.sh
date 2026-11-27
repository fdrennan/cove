# Sourced by run.sh and build.sh: app.env, and cargo - Rust 1.95 or newer
# (egui 0.36), from PATH when it is that new, else from the ndexr stack in
# this one shell. A session's PATH has the host's /usr/bin/cargo, 1.93, and
# the starters failed on it with "epaint_default_fonts@0.36.2 requires rustc
# 1.95" (2026-10-25) while it was taken as good enough.
. ./app.env

new_cargo() { cargo --version 2>/dev/null | grep -qE ' 1\.(9[5-9]|[1-9][0-9]{2})\.'; }
# The stack this project's modules come from - the ndexr stack, EESSI or
# Compute Canada's - and stack_here, stack_enter and stack_ml (.cove/stack.sh).
. ./stack.sh

if stack_here; then
  set +u
  stack_enter
  stack_ml Rust/1.96.0-GCCcore-13.3.0
  set -u
fi
new_cargo || { echo "need Rust 1.95 or newer (have: $(cargo --version 2>&1)): install it, or run where $COVE_STACK_LABEL is mounted" >&2; exit 127; }

# Crates from the stack's vendored Rust set, a read-only directory every
# account shares, when it is here: no download into each person's ~/.cargo,
# and the same sources at the same paths for everyone, which is what lets a
# new project start from Cove's prebuilt target/ (onprem/run.sh
# publish_seeds). A crate the set does not vendor fails to resolve:
# APP_VENDOR=0 ./build.sh builds from crates.io instead.
case "$(uname -m)" in
  aarch64|arm64) triple=aarch64-unknown-linux-gnu ;;
  *)             triple=x86_64-pc-linux-gnu ;;
esac
vendor="/opt/ndexr/site-library/$triple/Rust/1.96.0/rust/vendor"
# Cargo takes a source replacement from config, never from the environment
# (CARGO_SOURCE_* was ignored: 445 crates downloaded, 2026-10-25), so cargo is
# wrapped here, and exported for build-web.sh.
if [ "${APP_VENDOR:-1}" = 1 ] && [ -d "$vendor" ]; then
  export APP_CARGO_VENDOR="$vendor"
  cargo() { command cargo --config "source.crates-io.replace-with='ndexr-vendor'" --config "source.ndexr-vendor.directory='$APP_CARGO_VENDOR'" "$@"; }
  export -f cargo
fi
export PORT="${PORT:-$APP_PORT}"
# The app's name, read when it runs (host_name() in its main.rs or app.rs):
# the project's folder, which Cove names after it. A binary compiled ahead of
# time from the starter (a seed, built as seed.ndexr.io) then names this project.
export APP_HOST="${APP_HOST:-$(basename "$PWD")}"
