#!/usr/bin/env bash
# The game to WebAssembly: src/game.rs for wasm32-unknown-unknown, copied to
# dist/web/game.wasm, which the server hands the page. build.sh runs this after
# the server; run it alone after editing src/game.rs, then reload the page (the
# server reads the module from disk on every request, so no restart).
#
#   ./build-web.sh
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

if ! rustc --print target-libdir --target wasm32-unknown-unknown >/dev/null 2>&1 \
   || [ ! -d "$(rustc --print target-libdir --target wasm32-unknown-unknown)" ]; then
  echo "build-web: this Rust has no wasm32-unknown-unknown target (the stack's Rust/1.96.0 does)" >&2
  exit 1
fi
cargo build --release --lib --target wasm32-unknown-unknown
mkdir -p dist/web
cp target/wasm32-unknown-unknown/release/game.wasm dist/web/game.wasm.tmp
mv dist/web/game.wasm.tmp dist/web/game.wasm
echo "build-web: dist/web/game.wasm, $(wc -c <dist/web/game.wasm) bytes"
