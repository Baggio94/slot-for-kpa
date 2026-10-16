# slot. for KONKR Pocket Advance — 0.2.2

Battery and resource-efficiency candidate for the dedicated KONKR ARM64 build.

## Changes since 0.2.1

- Adaptive idle carousel rendering: the original smooth, continuous Slot animation and full-speed scrolling stay unchanged. Only a stationary, menu-free shelf with no queued cartridge artwork uses a lightweight 4 Hz refresh; every controller input immediately restores continuous rendering.
- Unchanged cartridge-face priority, two raster workers, prefetch radius and compressed LZ4 disk cache. Games and Display Sync never use the idle-carousel throttle.
- Android foreground-only 100ms UI pulse: Android suspends this monitoring when backgrounded, resuming immediately with the activity.
- Preserve suspend-time SRAM, RTC and RetroArch exports by draining queued save notifications after the final native suspend.
- RetroAchievements online/offline cache fetching continues through its existing independent network worker; background SAF fingerprinting parks without repeated wakeups while the activity is paused and resumes promptly.
- The achievement evaluator reduces idle timer wakeups on the shelf and when Android stops supplying game frames, while retaining its 5 ms gameplay path and the 8-frame queue.
- Run-Ahead remains exactly as before; default 1 frame and all existing game/core guards are unchanged.

## Upgrade and verification

Package: `fyi.slot.konkr` · Version: `0.2.2` · Android `versionCode 107` · ARM64-v8a only.
The established persistent signing certificate must remain unchanged; use `adb -d install -r` over 0.2.1. Never uninstall to upgrade.

Before publishing a public release, verify the GitHub Actions build and certificate, test a physical KONKR (scrolling, fast system changes, artwork availability, RA background caching, game audio, save/export on suspend, deep sleep and immediate D-pad wake), and compare battery/CPU measurements to 0.2.1. This candidate is not a claim of measured battery improvement.

No ROMs or BIOS are bundled. Credits and GPL-3.0 corresponding sources remain in the repository.
