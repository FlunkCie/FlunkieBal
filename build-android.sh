#!/bin/sh
# Build the game's native libraries for Android into the given directory (<dir>/<abi>/libflunkiebal.so).
# Gradle runs this; `cd android && gradle assembleDebug` builds the whole app.
#
# Needs: the NDK (Android Studio > SDK Manager), and
#        rustup target add aarch64-linux-android x86_64-linux-android
# FLUNKI_ABIS picks what to build, e.g. FLUNKI_ABIS=x86_64 for a quicker emulator build.
set -e
cd "$(dirname "$0")"
OUT=${1:-android/app/build/rustLibs}
ABIS=${FLUNKI_ABIS:-arm64-v8a x86_64}
API=26
SDK=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Android/Sdk}}
NDK=${ANDROID_NDK_HOME:-$(ls -d "$SDK"/ndk/* 2>/dev/null | sort -V | tail -1)}
[ -d "$NDK" ] || { echo "No Android NDK found: install one in Android Studio's SDK Manager" >&2; exit 1; }
BIN=$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin
# rustup's cargo: that is the one with the Android targets
export PATH="$HOME/.cargo/bin:$BIN:$PATH"
for abi in $ABIS; do
  case $abi in
    arm64-v8a) t=aarch64-linux-android ;;
    x86_64) t=x86_64-linux-android ;;
    *) echo "unknown ABI $abi" >&2; exit 1 ;;
  esac
  T=$(echo "$t" | tr 'a-z-' 'A-Z_')
  u=$(echo "$t" | tr '-' '_')
  export "CARGO_TARGET_${T}_LINKER=$BIN/$t$API-clang"
  # phones with 16 KB memory pages (and Google Play) need this
  export "CARGO_TARGET_${T}_RUSTFLAGS=-C link-arg=-Wl,-z,max-page-size=16384"
  export "CC_$u=$BIN/$t$API-clang" "AR_$u=$BIN/llvm-ar" "RANLIB_$u=$BIN/llvm-ranlib"
  cargo build --release --lib --target "$t"
  mkdir -p "$OUT/$abi"
  cp "target/$t/release/libflunkiebal.so" "$OUT/$abi/"
done
