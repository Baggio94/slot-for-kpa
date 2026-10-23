# RA CLOCK LAB — optional beta16 experiment

Based on research PR #57, 657186a0ca1e7606cbb742c78bc7b7da7196522b. **This is not a Vulkan
renderer, not a RetroArch runloop port and not a replacement for Run-Ahead.**

A fifth Frame pacing option, **RA CLOCK LAB**, is available after SYNC LAB.
On a fresh install the unchanged default is SYNC LAB; historical profiles
deserialize with the new boolean OFF. From RA CLOCK LAB press Left once to
return to SYNC LAB; no uninstall or data clearing.

The experiment passes Choreographer's latest VSync frame timestamp to the
native render callback, and only the opt-in mode derives fractional libretro
time from consecutive VSync stamps rather than GL callback arrival jitter.
Repeat/missing/backwards/long-gap timestamps cannot trigger unbounded frame
steps. It retains a one-committed-frame-per-callback budget. Link, rewind and
fast forward keep the existing scheduling paths. The Android EGL swap still
occurs inside GLSurfaceView after the callback; PCM/AudioTrack, speculative
audio sidecars, saves, Link, RTC, achievements and the actual cores are
unchanged.

## Acceptance test (hardware required)

Use the same **signed versionCode 154** APK for all arms; do not downgrade or
uninstall. Wario Land 4, mGBA, 1x, Run-Ahead OFF:
SYNC LAB (120s) → RA CLOCK LAB (120s) → SYNC LAB (120s).
Use the #57 parity kit to compare true per-layer SF >25ms gaps, p99,
emulated FPS and audio/PCM, as well as controls and visual feel. Repeat gpSP
before independent Run-Ahead OFF/1/2 tests.

Reference RA Vulkan baseline was 3 → 22 → 1 SF gaps >25ms (RA/slot/RA),
p99 18 → 21 → 18ms. No gain is claimed for this change before new
physical evidence. If it does not improve SF pacing, disable it and
investigate custom EGL ownership / opt-in Vulkan swapchain separately.

Never merge, tag, release or change main without explicit owner approval.
