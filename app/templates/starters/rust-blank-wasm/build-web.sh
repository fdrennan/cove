#!/usr/bin/env bash
# Compile src/lib.rs for the browser, to dist/web/app.wasm. build.sh runs this.
set -euo pipefail
cd "$(dirname "$0")"
. ./env.sh

cargo build --release --lib --target wasm32-unknown-unknown
mkdir -p dist/web
cp target/wasm32-unknown-unknown/release/page.wasm dist/web/app.wasm.tmp
mv dist/web/app.wasm.tmp dist/web/app.wasm
echo "build-web: dist/web/app.wasm, $(wc -c <dist/web/app.wasm) bytes"
