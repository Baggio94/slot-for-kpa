#!/usr/bin/env bash
# Read-only authoritative SurfaceFlinger present-to-present capture for Android.
# Assumes Slot's GBA game is already RUNNING and visible on the KONKR.
set -euo pipefail

serial="${1:-BW0308N250009576}"
seconds="${2:-120}"
out_dir="${3:-./surfaceflinger-results}"
if ! [[ "$seconds" =~ ^[0-9]+$ ]] || (( seconds < 30 || seconds > 900 )); then
  echo "Capture duration must be 30-900 seconds, got: $seconds" >&2
  exit 2
fi
if command -v adb >/dev/null 2>&1; then
  adb_bin="$(command -v adb)"
elif [[ -x "$HOME/Library/Android/sdk/platform-tools/adb" ]]; then
  adb_bin="$HOME/Library/Android/sdk/platform-tools/adb"
else
  echo "Android SDK platform-tools/adb is required" >&2
  exit 2
fi
"$adb_bin" -s "$serial" get-state | grep -Fx device >/dev/null
mkdir -p "$out_dir"
stamp="$(date -u +%Y%m%dT%H%M%SZ)"
raw="$out_dir/$stamp-surfaceflinger-timestats.txt"
summary="$out_dir/$stamp-surfaceflinger-summary.json"
cleanup() {
  "$adb_bin" -s "$serial" shell dumpsys SurfaceFlinger --timestats -disable >/dev/null 2>&1 || true
}
trap cleanup EXIT
echo "Sampling the game's SurfaceFlinger presentation on $serial for $seconds seconds."
echo "Play in Slot during capture; do not open the home menu or switch apps."
if ! "$adb_bin" -s "$serial" shell dumpsys SurfaceFlinger --timestats -clear -enable >"$out_dir/$stamp-enable.txt" 2>&1; then
  echo "This device does not permit SurfaceFlinger timestats. No FPS inferred." >&2
  exit 3
fi
sleep "$seconds"
"$adb_bin" -s "$serial" shell dumpsys SurfaceFlinger --timestats -dump >"$raw"
script_dir="$(cd "$(dirname "$0")" && pwd)"
python3 "$script_dir/surfaceflinger_timestats.py" "$raw" --json > "$summary"
echo "Raw SurfaceFlinger: $raw"
echo "Parsed summary:      $summary"
python3 "$script_dir/surfaceflinger_timestats.py" "$raw"
