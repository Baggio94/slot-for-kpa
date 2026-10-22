#!/usr/bin/env bash
# RetroArch vs slot. A/B capture that needs only adb (no Python on the Mac).
# Saves the raw SurfaceFlinger present histograms, plus slot.'s 5s
# diagnostics, to ~/Desktop/slot-parity/; parse them later with
# surface_parity.py and compare.py.
# Usage: bash capture_raw.sh {ra|slot} [seconds] [adb-serial]
# Open the game in the app BEFORE running it, and leave it playing.
set -euo pipefail

target="${1:-}"
duration="${2:-120}"
serial="${3:-${ANDROID_SERIAL:-}}"
[[ "$target" == ra || "$target" == slot ]] || {
  echo "Usage: $0 {ra|slot} [seconds] [adb-serial]" >&2; exit 2;
}
[[ "$duration" =~ ^[0-9]+$ ]] && (( duration >= 30 && duration <= 900 )) || {
  echo "Duration must be 30..900 seconds" >&2; exit 2;
}
if command -v adb >/dev/null 2>&1; then
  ADB=( "$(command -v adb)" )
elif [[ -x "$HOME/platform-tools/adb" ]]; then
  ADB=( "$HOME/platform-tools/adb" )
elif [[ -x "$HOME/Library/Android/sdk/platform-tools/adb" ]]; then
  ADB=( "$HOME/Library/Android/sdk/platform-tools/adb" )
else
  echo "adb not found" >&2; exit 3
fi
if [[ -n "$serial" ]]; then ADB+=( -s "$serial" ); fi
[[ "$("${ADB[@]}" get-state 2>/dev/null)" == device ]] || {
  echo "KONKR not connected over adb (or several devices: give the serial)" >&2; exit 3;
}

package=fyi.slot.konkr
if [[ "$target" == ra ]]; then
  package=""
  installed="$("${ADB[@]}" shell pm list packages | tr -d '\r')"
  for candidate in com.retroarch.aarch64 com.retroarch com.retroarch.ra32; do
    if grep -Fqx "package:$candidate" <<<"$installed"; then
      package="$candidate"
      break
    fi
  done
  [[ -n "$package" ]] || { echo "RetroArch is not installed on the KONKR" >&2; exit 4; }
fi
"${ADB[@]}" shell pidof "$package" >/dev/null 2>&1 || {
  echo "$package is not running: open the game first" >&2; exit 4;
}

output="$HOME/Desktop/slot-parity/$(date +%Y%m%d-%H%M%S)-$target"
mkdir -p "$output"
version="$("${ADB[@]}" shell dumpsys package "$package" | tr -d '\r' |
  grep -m1 versionName | sed 's/.*versionName=//' || true)"
cat >"$output/$target-metadata.txt" <<EOF
app=$target
package=$package
version=$version
duration_seconds=$duration
start=$(date -u +%Y-%m-%dT%H:%M:%SZ)
EOF

recorder=""
cleanup() {
  if [[ -n "$recorder" ]]; then
    kill "$recorder" 2>/dev/null || true
    wait "$recorder" 2>/dev/null || true
  fi
  "${ADB[@]}" shell dumpsys SurfaceFlinger --timestats -disable >/dev/null 2>&1 || true
}
trap cleanup EXIT

if [[ "$target" == slot ]]; then
  "${ADB[@]}" logcat -c
  "${ADB[@]}" logcat -v time -s SlotEmuDiag:I >"$output/slot-diagnostics.txt" &
  recorder=$!
fi
"${ADB[@]}" shell dumpsys SurfaceFlinger --timestats -clear -enable >/dev/null 2>&1 || {
  echo "SurfaceFlinger timestats could not be enabled" >&2; exit 5;
}
echo "Measuring $target for $duration s. Keep playing; do not open a menu."
sleep "$duration"
"${ADB[@]}" shell dumpsys SurfaceFlinger --timestats -dump >"$output/$target-surfaceflinger.txt"
echo "Done. Send the files in: $output"
