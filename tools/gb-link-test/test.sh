#!/bin/bash
# Game Boy Link test for slot.'s mGBA core, on the build host.
#
# Builds mGBA's libretro core for the host from an already patched source
# (scripts/build-mgba-android.sh leaves it in .build/mgba/source), then plays
# the test ROMs of roms.py as Host and as Join, the way slot. links two
# consoles. Fails when a byte goes wrong, a slave gets nothing, the two
# consoles' pairs differ, or a session start (state save + restore) stops the
# link.
#
# usage: tools/gb-link-test/test.sh [PATCHED_MGBA_SOURCE]
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
HERE="$ROOT/tools/gb-link-test"
SRC="${1:-$ROOT/.build/mgba/source}"
WORK="$ROOT/.build/gb-link-test"
mkdir -p "$WORK"
[ -f "$SRC/src/platform/libretro/libretro.c" ] || { echo "No patched mGBA source in $SRC" >&2; exit 1; }
grep -q SLOT_GB_DIRECT_CABLE_V1 "$SRC/src/platform/libretro/libretro.c" ||
  { echo "mGBA source lacks the direct Game Boy cable" >&2; exit 1; }

cmake -S "$SRC" -B "$WORK/host" -DCMAKE_BUILD_TYPE=Release -DCMAKE_POLICY_VERSION_MINIMUM=3.5 \
  -DBUILD_LIBRETRO=ON -DSKIP_LIBRARY=ON -DBUILD_QT=OFF -DBUILD_SDL=OFF -DBUILD_GL=OFF \
  -DBUILD_GLES2=OFF -DBUILD_GLES3=OFF -DBUILD_SHARED=OFF -DBUILD_STATIC=OFF \
  -DUSE_FFMPEG=OFF -DUSE_PNG=OFF -DUSE_SQLITE3=OFF -DUSE_LUA=OFF -DUSE_JSON_C=OFF \
  -DUSE_LIBZIP=OFF -DUSE_LZMA=OFF -DENABLE_SCRIPTING=OFF -DBUILD_LTO=OFF \
  -DENABLE_DEBUGGERS=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON > "$WORK/build.log" 2>&1 ||
  { cat "$WORK/build.log"; exit 1; }
cmake --build "$WORK/host" --target mgba_libretro -j"$(nproc)" >> "$WORK/build.log" 2>&1 ||
  { cat "$WORK/build.log"; exit 1; }
CORE="$WORK/host/mgba_libretro.so"
cc -O2 -std=gnu11 -I"$SRC/src/platform/libretro" "$HERE/harness.c" -ldl -o "$WORK/harness"
python3 "$HERE/roms.py" "$WORK"

failed=0
# play NAME FRAMES [env...]: Host runs GB0 (master), Join runs GB1 (slave).
play() {
  local name=$1 frames=$2; shift 2
  env "$@" "$WORK/harness" "$CORE" "$WORK/$name-master.gb" "$WORK/$name-slave.gb" 0 "$frames" \
    > "$WORK/host.txt" 2> "$WORK/host.err"
  env "$@" "$WORK/harness" "$CORE" "$WORK/$name-slave.gb" "$WORK/$name-master.gb" 1 "$frames" \
    > "$WORK/join.txt" 2> "$WORK/join.err"
}
# check LABEL: every printed frame, the master counted no wrong byte and its
# count moved, the slave's count moved, and both consoles hold the same pair.
check() {
  local label=$1
  local result
  result="$(python3 - "$WORK/host.txt" "$WORK/join.txt" <<'PY'
import sys
host = [dict(kv.split("=") for kv in line.split()) for line in open(sys.argv[1])]
join = [dict(kv.split("=") for kv in line.split()) for line in open(sys.argv[2])]
problems = []
if not host or len(host) != len(join):
    problems.append("missing frames")
for h, j in zip(host, join):
    if h["hash"] != j["hash"]:
        problems.append(f"frame {h['frame']}: the two consoles' pairs differ")
        break
    if h["err"] != "0":
        problems.append(f"frame {h['frame']}: master got {h['err']} wrong bytes")
        break
for a, b in zip(host, host[1:]):
    if a["ok"] == b["ok"]:
        problems.append(f"frames {a['frame']}-{b['frame']}: master stopped receiving")
        break
for a, b in zip(join, join[1:]):
    if a["ok"] == b["ok"]:
        problems.append(f"frames {a['frame']}-{b['frame']}: slave stopped receiving")
        break
print("; ".join(problems) or "ok")
PY
)"
  if [ "$result" = ok ]; then
    echo "PASS $label"
  else
    echo "FAIL $label: $result"
    failed=1
  fi
}

for name in poll irq; do
  play "$name" 120 EVERY=10; check "$name DMG"
  for reload in 3 7 11 50; do
    play "$name" 120 EVERY=10 RELOAD=$reload; check "$name DMG, session start at frame $reload"
  done
done
play cgb 120 EVERY=10 "MODEL=Game Boy Color"; check "cgb double speed, fast clock"
play cgb 120 EVERY=10 "MODEL=Game Boy Color" RELOAD=9; check "cgb double speed, session start at frame 9"
exit "$failed"
