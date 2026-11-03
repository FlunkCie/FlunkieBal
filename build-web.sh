#!/bin/sh
# Build the web version into dist/ (ES module page, also what the Android app ships)
# and dist-art/ (one self-contained page with the glue inlined, plus the .wasm).
#
# Needs: rustup target add wasm32-unknown-unknown
#        cargo install wasm-bindgen-cli --version 0.2.130   (must match wasm-bindgen in Cargo.lock)
# BUILD_STD=1 builds the standard library from source instead (nightly features via
# RUSTC_BOOTSTRAP, needs rust-src) for machines where the wasm32 target can't be installed.
set -e
cd "$(dirname "$0")"
if [ "${BUILD_STD:-0}" = "1" ]; then
  RUSTC_BOOTSTRAP=1 cargo build --release --lib -Zbuild-std=std,panic_abort --target wasm32-unknown-unknown
else
  cargo build --release --lib --target wasm32-unknown-unknown
fi
WASM=target/wasm32-unknown-unknown/release/flunkiebal.wasm
wasm-bindgen --target web --no-typescript --out-dir dist "$WASM"
# the page asks for this build's .js and .wasm by name, so no cache hands it another build's
V=$(sha256sum dist/flunkiebal_bg.wasm | cut -c1-12)
sed "s/__BUILD__/$V/g" web/index.html > dist/index.html
wasm-bindgen --target no-modules --no-typescript --out-dir dist-art "$WASM"
python3 web/make_page.py dist-art/flunkiebal.js dist-art/flunkiebal.html
ls -la dist dist-art
