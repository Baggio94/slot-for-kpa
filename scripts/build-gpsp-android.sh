#!/usr/bin/env bash
# Reproducible libretro gpSP build for Android arm64. No copyrighted BIOS/ROMs.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
NDK="${ANDROID_NDK_HOME:-${ANDROID_SDK_ROOT:?Set ANDROID_NDK_HOME or ANDROID_SDK_ROOT}/ndk/27.2.12479018}"
LLVM="$NDK/toolchains/llvm/prebuilt/linux-x86_64/bin"
[ -x "$LLVM/aarch64-linux-android31-clang" ] || {
  echo "Android NDK 27 arm64 compiler not found in $LLVM" >&2; exit 1;
}
# Pinned upstream libretro gpSP commit (2026-09-19).
PIN="5819380c2ffb0900219d700a382ee68c464ebb99"
OUT="$ROOT/.build/gpsp"
SRC="$OUT/source"
mkdir -p "$OUT"
if [ ! -d "$SRC/.git" ]; then
  git init -q "$SRC"
  git -C "$SRC" remote add origin https://github.com/libretro/gpsp.git
fi
if [ "$(git -C "$SRC" rev-parse HEAD 2>/dev/null || true)" != "$PIN" ]; then
  git -C "$SRC" fetch --depth=1 origin "$PIN"
  git -C "$SRC" checkout --detach -q FETCH_HEAD
fi
# An Advance Wars host->join transition may leave a stale serial queue.
# The tracked patch exists in KPA but v1.2.2 never compiled it into gpSP.
# Apply safely/idempotently or stop; never silently ship a vulnerable core.
AW_PATCH="$ROOT/cores/gpsp/aw-session-reset.patch"
if git -C "$SRC" apply --reverse --check "$AW_PATCH" >/dev/null 2>&1; then
  echo "Already applied: aw-session-reset.patch"
elif git -C "$SRC" apply --check "$AW_PATCH"; then
  git -C "$SRC" apply "$AW_PATCH"
  echo "Applied: aw-session-reset.patch"
else
  echo "gpSP Advance Wars safety patch mismatch; aborting" >&2
  exit 1
fi
# Mandatory native regression on the very same patched gpSP source that will
# be cross-compiled into the APK. ASAN validates queue-length safety.
HOST_CC="${CC_HOST:-cc}"
# The standalone AW harness intentionally supplies a small test/common.h,
# which conflicts with the production source/common.h if compiled in-place.
# Copy the *already-patched* serial_proto.c into an isolated include root,
# so both the harness and included protocol use the same test stubs.
mkdir -p "$OUT/aw-harness"
cp "$SRC/serial_proto.c" "$OUT/aw-harness/serial_proto.c"
"$HOST_CC" -std=gnu11 -O1 -g -fno-omit-frame-pointer \
  -fstack-protector-strong -fsanitize=address \
  -I "$OUT/aw-harness" -I "$ROOT/cores/gpsp/test" \
  "$ROOT/cores/gpsp/test/awtest.c" -o "$OUT/awtest-asan"
ASAN_OPTIONS=detect_leaks=0 "$OUT/awtest-asan"

# Preserve only transient mixer data for speculative rollbacks; persistent
# libretro/RetroArch save-state format stays exactly as pinned upstream.
python3 "$ROOT/scripts/patch-gpsp-speculative-audio.py" "$SRC"
make -C "$SRC" -j4 platform=arm64 \
  CC="$LLVM/aarch64-linux-android31-clang" \
  CXX="$LLVM/aarch64-linux-android31-clang++" \
  AR="$LLVM/llvm-ar"
CORE="$SRC/gpsp_libretro.so"
[ -s "$CORE" ] || { echo "Missing gpSP libretro library" >&2; exit 1; }
# The sidecar must actually be exported through gpSP's version script.
SYMBOLS="$("$LLVM/llvm-nm" -D --defined-only "$CORE")"
for symbol in slot_gpsp_audio_sidecar slot_gpsp_audio_sidecar_size; do
  grep -q " $symbol$" <<< "$SYMBOLS" || {
    echo "Missing required gpSP speculative-audio symbol: $symbol" >&2
    exit 1
  }
done
DEST="$ROOT/android/app/src/main/jniLibs/arm64-v8a"
mkdir -p "$DEST"
cp "$CORE" "$DEST/libgpsp_libretro.so"
mkdir -p "$ROOT/android/app/src/main/assets/licenses"
for license in COPYING LICENSE LICENSE.txt; do
  if [ -f "$SRC/$license" ]; then
    cp "$SRC/$license" "$ROOT/android/app/src/main/assets/licenses/gpsp-${license}.txt"
  fi
done
# F11: ship pinned-core and safety-patch provenance in the release APK.
cat > "$ROOT/android/app/src/main/assets/core-build-gpsp.json" <<EOF
{"core":"gpsp","revision":"$PIN","patches":["aw-session-reset.patch","patch-gpsp-speculative-audio.py"],"native_aw_harness":"asan-passed"}
EOF
echo "gpSP arm64 core: $DEST/libgpsp_libretro.so"
