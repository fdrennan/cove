#!/usr/bin/env bash
# The browser UI: src/ compiled to wasm, into dist/web/ (what `app serve` hands
# out). Needs rustup's wasm32-unknown-unknown target and wasm-bindgen-cli at the
# version Cargo.toml pins. The stack's Rust module has no wasm target, so this
# uses a rustup toolchain: $APP_RUST (default: Cove's, ~/.local/share/cove-rust).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
rust="${APP_RUST:-$HOME/.local/share/cove-rust}"
if [[ -x "$rust/cargo/bin/cargo" ]]; then
  export RUSTUP_HOME="$rust/rustup" CARGO_HOME="$rust/cargo" PATH="$rust/cargo/bin:$PATH"
elif [[ -x /usr/local/lib/cove/rust/cargo/bin/wasm-bindgen ]]; then
  # Cove's published copy, read-only (onprem/run.sh publish_rust): its
  # toolchain's own bin, not the rustup proxy, and the person's cargo cache,
  # as assets/deploy-app.sh builds. Only the operator has the private one.
  tc="$(ls -1d /usr/local/lib/cove/rust/rustup/toolchains/*/ 2>/dev/null | head -1)"
  export PATH="${tc}bin:/usr/local/lib/cove/rust/cargo/bin:$PATH"
fi
want="0.2.129"
have="$(wasm-bindgen --version 2>/dev/null | awk '{print $2}')"
[[ "$have" == "$want" ]] || { echo "need wasm-bindgen-cli $want (have: ${have:-none}) and the wasm32 target: rustup target add wasm32-unknown-unknown && cargo install wasm-bindgen-cli --version $want --locked" >&2; exit 1; }

echo "▸ building the UI for wasm32"
CARGO_TARGET_DIR="$here/target" cargo build --release --target wasm32-unknown-unknown --manifest-path "$here/Cargo.toml"
out="$here/dist/web"
rm -rf "$out" && mkdir -p "$out"
wasm-bindgen --target web --no-typescript --out-dir "$out" "$here/target/wasm32-unknown-unknown/release/app.wasm"
cp "$here/web/index.html" "$out/"
echo "✓ $out"
