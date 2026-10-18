#!/usr/bin/env bash
# Linux/macOS KONKR native build plus debug APK; CI also assembles signed Release.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
# About uses the Android app versionName rather than a frozen DEV13 label.
SLOT_KONKR_VERSION="$(sed -nE 's/^[[:space:]]*versionName[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/p' android/app/build.gradle.kts | head -n 1)"
if [ -z "$SLOT_KONKR_VERSION" ]; then
  echo "Cannot read Android versionName" >&2
  exit 1
fi
export SLOT_KONKR_VERSION
rustup target add aarch64-linux-android
if [ ! -f android/app/src/main/jniLibs/arm64-v8a/libmgba_libretro.so ]; then
  bash scripts/build-mgba-android.sh
fi
if [ ! -f android/app/src/main/jniLibs/arm64-v8a/libgpsp_libretro.so ]; then
  bash scripts/build-gpsp-android.sh
fi
# Even local builds must use the exact upstream Slot keycaps, not the
# generic Android font. The same exporter is verified by CI.
cargo run --locked -p slot-ui --example export_apps_legend -- \
  android/app/src/main/assets/apps_controls
cargo ndk -t arm64-v8a -o android/app/src/main/jniLibs build --release -p slot-android

# Keep the validated mGBA/gpSP/Slot machine code and all exported JNI/libretro
# symbols unchanged. The preview APK previously shipped .symtab/.strtab and
# (in mGBA) multi-megabyte .debug_* sections that Android never uses.
# Strip only nonessential symbol/debug tables before Gradle packages the .so.
# Set SLOT_NATIVE_STRIP=0 to produce an unstripped diagnostic build.
if [ "${SLOT_NATIVE_STRIP:-1}" = "1" ]; then
  NDK_ROOT="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-}}"
  if [ -z "$NDK_ROOT" ]; then
    SDK_ROOT="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
    NDK_ROOT="$SDK_ROOT/ndk/27.2.12479018"
  fi
  STRIP=""
  for candidate in "$NDK_ROOT"/toolchains/llvm/prebuilt/*/bin/llvm-strip; do
    if [ -x "$candidate" ]; then STRIP="$candidate"; break; fi
  done
  if [ -z "$STRIP" ]; then
    echo "Could not locate Android NDK llvm-strip at $NDK_ROOT" >&2
    exit 1
  fi
  for lib in slot_android mgba_libretro gpsp_libretro; do
    native="android/app/src/main/jniLibs/arm64-v8a/lib${lib}.so"
    [ -s "$native" ] || { echo "Native library missing: $native" >&2; exit 1; }
    before="$(wc -c < "$native" | tr -d ' ')"
    "$STRIP" --strip-unneeded "$native"
    after="$(wc -c < "$native" | tr -d ' ')"
    echo "Trimmed $lib metadata: $before -> $after bytes"
  done
fi
(cd android && gradle --no-daemon assembleDebug)
echo "APK: android/app/build/outputs/apk/debug/app-debug.apk"
