#!/usr/bin/env bash
# Reproducible libretro Gambatte build for Android arm64 (GB/GBC). No BIOS/ROMs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NDK="${ANDROID_NDK_HOME:-${ANDROID_SDK_ROOT:?Set ANDROID_NDK_HOME or ANDROID_SDK_ROOT}/ndk/27.2.12479018}"
[ -x "$NDK/ndk-build" ] || { echo "Android NDK ndk-build not found in $NDK" >&2; exit 1; }
LLVM="$(echo "$NDK"/toolchains/llvm/prebuilt/*/bin)"
# Pinned upstream libretro gambatte-libretro commit (2026-08-21).
PIN="d9d6cd06382d1ced30de34d56d3609452323dab1"
OUT="$ROOT/.build/gambatte"
SRC="$OUT/source"
mkdir -p "$OUT"
if [ ! -d "$SRC/.git" ]; then
  git init -q "$SRC"
  git -C "$SRC" remote add origin https://github.com/libretro/gambatte-libretro.git
fi
if [ "$(git -C "$SRC" rev-parse HEAD 2>/dev/null || true)" != "$PIN" ]; then
  git -C "$SRC" fetch --depth=1 origin "$PIN"
  git -C "$SRC" checkout --detach -q FETCH_HEAD
fi
# Game Link network fixes: a server that missed its client retries on wall
# time, sockets cannot freeze the emulation thread, and SB receives the right
# bits when a game reads it mid-transfer. Apply strictly, never half-patched.
for patch in netlink-fixes.patch; do
  patch_file="$ROOT/cores/gambatte/$patch"
  if git -C "$SRC" apply --reverse --check "$patch_file" >/dev/null 2>&1; then
    echo "Already applied: $patch"
  elif git -C "$SRC" apply --check "$patch_file"; then
    git -C "$SRC" apply "$patch_file"
    echo "Applied: $patch"
  else
    echo "Patch mismatch (refusing to use unsafe Gambatte source): $patch" >&2
    exit 1
  fi
done
# Upstream's own Android recipe (jni/Android.mk): RGB565 video, static libc++,
# 16 KiB pages. Only the KONKR ABI is built.
"$NDK/ndk-build" -C "$SRC/libgambatte/libretro/jni" -j4 \
  APP_ABI=arm64-v8a APP_PLATFORM=android-31 \
  NDK_PROJECT_PATH="$SRC/libgambatte/libretro" \
  NDK_OUT="$OUT/obj" NDK_LIBS_OUT="$OUT/libs"
CORE="$OUT/libs/arm64-v8a/libretro.so"
[ -s "$CORE" ] || { echo "Missing Gambatte libretro library" >&2; exit 1; }
SYMBOLS="$("$LLVM/llvm-nm" -D --defined-only "$CORE")"
for symbol in retro_run retro_serialize retro_get_memory_data; do
  grep -q " $symbol$" <<< "$SYMBOLS" || {
    echo "Missing libretro symbol in Gambatte: $symbol" >&2
    exit 1
  }
done
if "$LLVM/llvm-readelf" -d "$CORE" | grep -F 'libc++_shared.so'; then
  echo "Gambatte must link libc++ statically" >&2
  exit 1
fi
DEST="$ROOT/android/app/src/main/jniLibs/arm64-v8a"
mkdir -p "$DEST"
cp "$CORE" "$DEST/libgambatte_libretro.so"
mkdir -p "$ROOT/android/app/src/main/assets/licenses"
cp "$SRC/COPYING" "$ROOT/android/app/src/main/assets/licenses/gambatte-COPYING.txt"
cat > "$ROOT/android/app/src/main/assets/core-build-gambatte.json" <<JSON
{"core":"gambatte","revision":"$PIN","patches":["netlink-fixes.patch"]}
JSON
echo "Gambatte arm64 core: $DEST/libgambatte_libretro.so"
