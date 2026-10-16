# slot. for KONKR Pocket Advance — 0.2.1

First independent KONKR ARM64-only production release candidate, promoted from the tested `slot-konkr` QA105 feature set.

## Included

- New-user onboarding in original Slot keycap style: press START, select Library, add unzipped ROMs only
- First-install preferences: Dark / no wallpaper, LCD3x GBA/GB/GBC, Display Sync, one-frame Run-Ahead and opt-in system indicators
- Persistent compressed cartridge artwork cache, 160 GPU faces and smooth scrolling prioritization
- Ten GBA screen shader presets plus Off, including Dot defaults and LCD3x
- Cart Studio, RetroAchievements online/offline, per-game profiles, mGBA/gpSP, compatible save states, Link/RFU where supported
- **ARM64-v8a APK only**, no Mac AVD keyboard code or x86_64 binaries

## Upgrade

Android package `fyi.slot.konkr`; `versionName 0.2.1`, `versionCode 106`. The APK is signed with the established permanent certificate. Install over 0.2.0 / QA105 to preserve data, never uninstall to update.

The GPL-3.0 corresponding source, original Slot credits, core and shader attributions are included in this repository. No ROMs or BIOS are bundled.

**Before public publishing:** validate this exact ARM64 build on a physical KONKR, confirm the signing certificate, and review the changelog and GitHub release assets. Work-in-progress code in the development lab is not a public release.
