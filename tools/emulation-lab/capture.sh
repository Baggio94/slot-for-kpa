#!/usr/bin/env bash
# Record a reproducible 45s game scenario without modifying any ROM/save.
# Usage: bash tools/emulation-lab/capture.sh emulator-5556 mgba_runahead1
set -euo pipefail
serial="${1:-emulator-5556}"
scenario="${2:-unnamed}"
seconds="${3:-45}"
[[ "$scenario" =~ ^[a-zA-Z0-9._-]+$ ]] || { echo "Unsafe scenario label" >&2; exit 2; }
[[ "$seconds" =~ ^[0-9]+$ ]] && (( seconds >= 10 && seconds <= 600 )) ||
  { echo "Duration 10..600 seconds required" >&2; exit 2; }
if command -v adb >/dev/null 2>&1; then ADB="$(command -v adb)"
else ADB="$HOME/Library/Android/sdk/platform-tools/adb"; fi
"$ADB" -s "$serial" get-state >/dev/null
abi="$("$ADB" -s "$serial" shell getprop ro.product.cpu.abi | tr -d '\r')"
echo "Device $serial ($abi), scenario $scenario. Start the GAME before sampling."
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
dir="emulation-lab-results/$timestamp-$scenario"
mkdir -p "$dir"
printf 'scenario=%s\nserial=%s\nabi=%s\nduration_s=%s\n' "$scenario" "$serial" "$abi" "$seconds" >"$dir/metadata.txt"
# Capture only development performance tags. No ROM bytes, credentials,
# save-state data, screenshots or audio PCM is gathered.
# logcat otherwise replays buffered samples from PREVIOUS trials, which
# would silently corrupt A/B comparisons. Clear only the transient Android
# log buffers; this does NOT touch APK data, ROMs or saves.
"$ADB" -s "$serial" logcat -c
"$ADB" -s "$serial" logcat -v time -s SlotEmuDiag:I SlotKONKR:I SlotKonkr:I '*:S' >"$dir/logcat.txt" &
recorder=$!
trap 'kill "$recorder" 2>/dev/null || true' EXIT
sleep "$seconds"
kill "$recorder" 2>/dev/null || true
wait "$recorder" 2>/dev/null || true
trap - EXIT
python3 "$(dirname "$0")/report.py" "$dir/logcat.txt" --expected-seconds "$seconds" > "$dir/report.txt"
python3 "$(dirname "$0")/report.py" "$dir/logcat.txt" --expected-seconds "$seconds" --json > "$dir/report.json"
echo "Saved $dir/report.txt and $dir/report.json"
cat "$dir/report.txt"
