#!/usr/bin/env bash
# Prints the RetroArch settings that change smoothness and button response
# (run-ahead, frame delay, hard sync, swapchain, audio latency, video driver)
# and the mGBA core options, so slot. can be compared on equal terms.
# Usage: bash ra_settings.sh [adb-serial]
set -euo pipefail

serial="${1:-${ANDROID_SERIAL:-}}"
if command -v adb >/dev/null 2>&1; then
  ADB=( "$(command -v adb)" )
elif [[ -x "$HOME/platform-tools/adb" ]]; then
  ADB=( "$HOME/platform-tools/adb" )
else
  echo "adb not found" >&2; exit 3
fi
if [[ -n "$serial" ]]; then ADB+=( -s "$serial" ); fi

keys='^(video_driver|video_threaded|video_vsync|video_swap_interval|video_hard_sync|video_hard_sync_frames|video_max_swapchain_images|video_frame_delay|video_frame_delay_auto|video_waitable_swapchains|video_max_frame_latency|video_refresh_rate|video_black_frame_insertion|video_shader_enable|video_smooth|vrr_runloop_enable|run_ahead_enabled|run_ahead_frames|run_ahead_secondary_instance|preemptive_frames_enable|input_poll_type_behavior|input_block_timeout|audio_driver|audio_latency|audio_out_rate|audio_sync|audio_rate_control|audio_rate_control_delta|audio_resampler|audio_resampler_quality|rewind_enable|fastforward_frameskip|sustained_performance_mode|frontend_log_level) = '

found=0
for root in /storage/emulated/0/Android/data/com.retroarch.aarch64/files \
            /storage/emulated/0/Android/data/com.retroarch/files \
            /storage/emulated/0/RetroArch; do
  cfg="$root/retroarch.cfg"
  "${ADB[@]}" shell "test -f '$cfg'" 2>/dev/null || continue
  found=1
  echo "== $cfg"
  "${ADB[@]}" shell "cat '$cfg'" | tr -d '\r' | grep -E "$keys" || true
  for extra in "$root/config/mGBA/mGBA.cfg" "$root/config/mGBA/mGBA.opt" \
               "$root/retroarch-core-options.cfg"; do
    "${ADB[@]}" shell "test -f '$extra'" 2>/dev/null || continue
    echo "== $extra"
    "${ADB[@]}" shell "cat '$extra'" | tr -d '\r' | grep -E "$keys|^mgba_" || true
  done
done
(( found )) || { echo "retroarch.cfg not found on the device" >&2; exit 4; }
