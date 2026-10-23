#!/usr/bin/env bash
# Read-only same-device RetroArch vs slot Android present timing.
# Usage: bash capture_pair.sh ra mgba 120 [adb-serial]
#        bash capture_pair.sh slot mgba 120 [adb-serial]
# Start the SAME game on the selected app before invoking this script.
set -euo pipefail

target="${1:-}"
core="${2:-}"
duration="${3:-120}"
serial="${4:-${ANDROID_SERIAL:-}}"
[[ "$target" == ra || "$target" == slot ]] || {
  echo "Usage: $0 {ra|slot} {mgba|gpsp} 120 [adb-serial]" >&2; exit 2;
}
[[ "$core" == mgba || "$core" == gpsp ]] || {
  echo "Core must be mgba or gpsp" >&2; exit 2;
}
[[ "$duration" =~ ^[0-9]+$ ]] && (( duration >= 60 && duration <= 900 )) || {
  echo "Duration must be 60..900 seconds" >&2; exit 2;
}
if command -v adb >/dev/null 2>&1; then
  ADB=( "$(command -v adb)" )
elif [[ -x "$HOME/Library/Android/sdk/platform-tools/adb" ]]; then
  ADB=( "$HOME/Library/Android/sdk/platform-tools/adb" )
else
  echo "ADB/platform-tools not found" >&2; exit 3
fi
if [[ -n "$serial" ]]; then ADB+=( -s "$serial" ); fi
[[ "$("${ADB[@]}" get-state 2>/dev/null)" == device ]] || {
  echo "KONKR ADB not ready (or more than one device: specify serial)" >&2; exit 3;
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
  [[ -n "$package" ]] || {
    echo "No supported RetroArch package found. Use RA installed on KONKR." >&2; exit 4;
  }
fi
pid="$("${ADB[@]}" shell pidof "$package" 2>/dev/null | tr -d '\r\n' || true)"
[[ -n "$pid" ]] || {
  echo "$package is not running. Open the GBA game BEFORE capture." >&2; exit 4;
}
# Verifying the foreground package avoids capturing the Android launcher
# accidentally. Some OEMs omit this field: refuse if a different foreground
# package is positively identified; warn when none can be determined.
active="$("${ADB[@]}" shell dumpsys activity activities 2>/dev/null |
  grep -E 'topResumedActivity|mResumedActivity|ResumedActivity' | head -n 1 || true)"
if [[ -n "$active" && "$active" != *"$package"* ]]; then
  echo "Foreground may not be $package. $active" >&2
  echo "Open the game and retry; no files or app data were changed." >&2
  exit 4
fi

stamp="$(date -u +%Y%m%dT%H%M%SZ)"
output="parity-results/$stamp-$target-$core"
mkdir -p "$output"
cat >"$output/metadata.txt" <<EOF
app=$target
package=$package
core_requested=$core
duration_seconds=$duration
utc_start=$stamp
note=Core/game not verified automatically. Keep the same ROM/BIOS/settings.
EOF
sf="$output/surfaceflinger-raw.txt"
summary="$output/present-summary.json"
logfile="$output/slot-emulation.log"
recorder=""
cleanup() {
  if [[ -n "$recorder" ]]; then
    kill "$recorder" 2>/dev/null || true
    wait "$recorder" 2>/dev/null || true
  fi
  "${ADB[@]}" shell dumpsys SurfaceFlinger --timestats -disable >/dev/null 2>&1 || true
}
trap cleanup EXIT
echo "Measuring $target/$core for $duration seconds. Do not leave game or change options."
# Optional slot-only diagnostic; never read ROM/save/audio data.
if [[ "$target" == slot ]]; then
  "${ADB[@]}" logcat -v time -T 1 -s SlotEmuDiag:I SlotKONKR:I SlotKonkr:I '*:S' >"$logfile" &
  recorder=$!
fi
"${ADB[@]}" shell dumpsys SurfaceFlinger --timestats -clear -enable \
  >"$output/surfaceflinger-enable.txt" 2>&1 || {
  echo "SurfaceFlinger timestats could not be enabled; inconclusive" >&2
  exit 5
}
sleep "$duration"
"${ADB[@]}" shell dumpsys SurfaceFlinger --timestats -dump >"$sf"
python3 "$(dirname "$0")/surface_parity.py" "$sf" --target "$target" --output "$summary"
if [[ "$target" == slot ]]; then
  python3 "$(dirname "$0")/../emulation-lab/report.py" "$logfile" \
    --expected-seconds "$duration" --json >"$output/slot-emulation-summary.json"
fi
echo "Results: $output"
echo "Share present-summary.json (and slot-emulation-summary.json for slot)."
echo "RAW logs are for local troubleshooting only; do not upload private RetroArch configs."
