#!/usr/bin/env bash
# Build the Android APK: cross-compile the cdylib with the NDK, then package and
# sign it with the SDK's own tools. No Gradle, because there is no Java in the
# app: a NativeActivity loads the .so and calls android_main.
#
# Needs ANDROID_NDK_HOME and ANDROID_HOME, the aarch64-linux-android Rust target,
# and Java on PATH for keytool/apksigner.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
here="$root/android"
: "${ANDROID_NDK_HOME:?set ANDROID_NDK_HOME to an installed NDK}"
: "${ANDROID_HOME:?set ANDROID_HOME to an installed SDK}"

build_tools="$(ls -d "$ANDROID_HOME"/build-tools/* | sort -V | tail -1)"
platform="$(ls -d "$ANDROID_HOME"/platforms/android-* | sort -V | tail -1)"

version_name="${VERSION_NAME:-0.0.0}"
version_code="${VERSION_CODE:-1}"
out="$root/target/android"
rm -rf "$out"
mkdir -p "$out/apk/lib/arm64-v8a"

toolchain="$ANDROID_NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64"
clang="$toolchain/bin/aarch64-linux-android${ANDROID_API:-26}-clang"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$clang"
export CC_aarch64_linux_android="$clang"
export AR_aarch64_linux_android="$toolchain/bin/llvm-ar"

cd "$root"
cargo build --release -p nopads_android --target aarch64-linux-android
so="$root/target/aarch64-linux-android/release/libnopads_android.so"

"$build_tools/aapt2" link \
    -o "$out/base.apk" \
    -I "$platform/android.jar" \
    --manifest "$here/AndroidManifest.xml" \
    --min-sdk-version "${ANDROID_API:-26}" \
    --target-sdk-version 35 \
    --version-code "$version_code" \
    --version-name "$version_name"

cp "$so" "$out/apk/lib/arm64-v8a/"
(cd "$out/apk" && zip -q -r "$out/base.apk" lib)
"$build_tools/zipalign" -f -p 4 "$out/base.apk" "$out/aligned.apk"

ks="$out/debug.keystore"
keytool -genkeypair -keystore "$ks" -storepass android -keypass android \
    -alias androiddebugkey -keyalg RSA -keysize 2048 -validity 10000 \
    -dname "CN=Android Debug,O=Android,C=US"
"$build_tools/apksigner" sign --ks "$ks" --ks-pass pass:android \
    --key-pass pass:android --out "$out/7on7-${version_name}-android-arm64.apk" \
    "$out/aligned.apk"
"$build_tools/aapt2" dump badging "$out/7on7-${version_name}-android-arm64.apk" | head -4
echo "wrote $out/7on7-${version_name}-android-arm64.apk"
