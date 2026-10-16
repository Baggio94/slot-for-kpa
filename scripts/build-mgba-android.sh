#!/usr/bin/env bash
# Build a reproducible arm64 mGBA libretro binary for the Android KONKR shell.
# Requires git, cmake, Android NDK 27+; no commercial ROMs or BIOS files.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# Accept an explicitly selected NDK or the most recent SDK-installed version.
# macOS/Apple Silicon uses prebuilt/darwin-x86_64 in the Android NDK.
if [ -n "${ANDROID_NDK_HOME:-}" ]; then
  NDK="$ANDROID_NDK_HOME"
else
  SDK="${ANDROID_SDK_ROOT:-${ANDROID_HOME:-$HOME/Library/Android/sdk}}"
  NDK=""
  for candidate in "$SDK"/ndk/*; do
    if [ -f "$candidate/build/cmake/android.toolchain.cmake" ]; then
      NDK="$candidate"
    fi
  done
fi
[ -n "$NDK" ] && [ -f "$NDK/build/cmake/android.toolchain.cmake" ] || {
  echo "Android NDK not found; set ANDROID_NDK_HOME" >&2
  exit 1
}
PIN="7a12d6d4b9acb14c0ae62c9166b6a2f3d08007f6"
OUT="$ROOT/.build/mgba"
SRC="$OUT/source"
BUILD="$OUT/android-arm64"
mkdir -p "$OUT"
if [ ! -d "$SRC/.git" ]; then
  git init -q "$SRC"
  git -C "$SRC" remote add origin https://github.com/libretro/mgba.git
fi
if [ "$(git -C "$SRC" rev-parse HEAD 2>/dev/null || true)" != "$PIN" ]; then
  git -C "$SRC" fetch --depth=1 origin "$PIN"
  git -C "$SRC" checkout --detach -q FETCH_HEAD
fi
# Apply the seven exact original Slot mGBA patches proven to coexist
# with the pinned libretro revision on the KONKR macOS NDK audit.
# Re-running the build must not delete or silently reset local sources.
for patch in \
  685023e05-fifo-dma-source-offset.patch \
  gb-audio-capacitor.patch \
  gb-audio-stopped-channel.patch \
  link-mode.patch \
  zz-gb-link-mode.patch \
  zz-link-peer-frameskip.patch \
  zz-link-peer-silence.patch
do
  patch_file="$ROOT/cores/mgba/$patch"
  if git -C "$SRC" apply --reverse --check "$patch_file" >/dev/null 2>&1; then
    echo "Already applied: $patch"
  elif git -C "$SRC" apply --check "$patch_file"; then
    git -C "$SRC" apply "$patch_file"
    echo "Applied: $patch"
  else
    echo "Patch mismatch (refusing to reset source): $patch" >&2
    exit 1
  fi
done
# Keep the transient audio rollback ABI on top of the Multiplayer patch.
# Persistent RetroArch state format is unchanged.
python3 "$ROOT/scripts/patch-mgba-speculative-audio.py" "$SRC"
cmake -S "$SRC" -B "$BUILD" \
  -DCMAKE_TOOLCHAIN_FILE="$NDK/build/cmake/android.toolchain.cmake" \
  -DANDROID_ABI=arm64-v8a -DANDROID_PLATFORM=android-31 \
  -DCMAKE_BUILD_TYPE=Release \
  -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
  -DBUILD_LIBRETRO=ON -DSKIP_LIBRARY=ON \
  -DBUILD_QT=OFF -DBUILD_SDL=OFF -DBUILD_GL=OFF \
  -DBUILD_GLES2=OFF -DBUILD_GLES3=OFF \
  -DBUILD_SHARED=OFF -DBUILD_STATIC=OFF \
  -DUSE_FFMPEG=OFF -DUSE_PNG=OFF -DUSE_SQLITE3=OFF \
  -DUSE_LUA=OFF -DUSE_JSON_C=OFF -DUSE_LIBZIP=OFF \
  -DUSE_LZMA=OFF -DENABLE_SCRIPTING=OFF \
  -DBUILD_LTO=OFF -DENABLE_DEBUGGERS=OFF \
  -DCMAKE_POSITION_INDEPENDENT_CODE=ON
cmake --build "$BUILD" --target mgba_libretro -j 4
CORE="$(find "$BUILD" -type f -name '*libretro*.so' -print -quit)"
[ -n "$CORE" ] && [ -s "$CORE" ] || { echo "mGBA libretro core missing" >&2; exit 1; }
case "$(uname -s)" in
  Darwin) NDK_HOST="darwin-x86_64" ;;
  Linux) NDK_HOST="linux-x86_64" ;;
  *) echo "Unsupported build host: $(uname -s)" >&2; exit 1 ;;
esac
NM="$NDK/toolchains/llvm/prebuilt/$NDK_HOST/bin/llvm-nm"
[ -x "$NM" ] || { echo "Android NDK llvm-nm missing: $NM" >&2; exit 1; }
SYMBOLS="$("$NM" -D --defined-only "$CORE")"
for symbol in retro_init retro_load_game retro_run retro_serialize retro_unserialize slot_mgba_audio_sidecar slot_mgba_audio_sidecar_size; do
  grep -q " $symbol$" <<< "$SYMBOLS" || {
    echo "mGBA ABI symbol missing: $symbol" >&2
    exit 1
  }
done
# Do not use 'strings | grep -q' here: pipefail may produce false negatives
# when grep exits before strings drains the binary (observed on macOS).
python3 - "$CORE" <<'PY'
from pathlib import Path
import sys
core = Path(sys.argv[1]).read_bytes()
for value in (b"mgba_link\0", b"mgba_link_player\0", b"SLK1"):
    if value not in core:
        raise SystemExit(f"mGBA linked-core marker missing: {value!r}")
    print(f"Verified linked-core marker: {value!r}")
PY
DEST="$ROOT/android/app/src/main/jniLibs/arm64-v8a"
mkdir -p "$DEST"
cp "$CORE" "$DEST/libmgba_libretro.so"
mkdir -p "$ROOT/android/app/src/main/assets/licenses"
if [ -f "$SRC/LICENSE" ]; then
  cp "$SRC/LICENSE" "$ROOT/android/app/src/main/assets/licenses/mgba-LICENSE.txt"
fi
echo "mGBA arm64 libretro: $DEST/libmgba_libretro.so"
