#!/usr/bin/env sh
# Builds the WebAssembly demo into web/pkg. Needs the wasm32 target and a
# wasm-bindgen CLI matching the crate version (see Cargo.lock).
set -eu
cd "$(dirname "$0")/.."
cargo build --release -p web-demo --target wasm32-unknown-unknown
wasm-bindgen --target web --no-typescript --out-dir web/pkg target/wasm32-unknown-unknown/release/web_demo.wasm
sh web/stamp.sh
echo "serve with: python -m http.server -d web 8765"
