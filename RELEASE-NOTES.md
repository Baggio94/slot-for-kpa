# slot. for KONKR Pocket Advance — v1.2.2

This update focuses on smoother, more reliable emulation, ZIP cartridge support, improved artwork handling, and optional SleepManager status integration.

## What's New

- **SYNC LAB emulation pacing** — A new frame-pacing option tuned and tested on the KONKR Pocket Advance. **New installations start with SYNC LAB and Run-Ahead OFF** for consistent controls and audio. Your existing emulation preferences and per-game profiles are preserved on upgrade.
- **ZIP cartridges** — Open `.zip` files containing GB, GBC or GBA games directly from your existing ROM folders. Supported games are detected automatically; archives remain unchanged. Save and RetroAchievements handling use the extracted game's identity.
- **More reliable cartridge artwork** — Retry interrupted or temporary download failures automatically while slot. is open, without overwriting artwork you've customized.
- **GB/GBC cartridge polish** — Improved original-style cartridge shell selection and Cart Studio compatibility, including games stored in ZIP archives.
- **Improved mGBA compatibility** — A targeted fix for **Drill Dozer** avoids an incorrect Game Boy Player detection path, without changing other games.
- **SleepManager 0.7.3 status support** — When you enable integration icons in slot., its SleepManager indicator reads the new read-only status API to distinguish an enabled-and-running service from a stopped one. SleepManager 0.7.2 remains supported through a best-effort legacy fallback.
- **Emulation stability and diagnostics** — Refined audio startup, protected frame pacing from multi-frame catch-up regressions, strengthened ZIP/save compatibility checks, and added safeguards against missing native Android/JNI methods.

## Emulation Defaults & Known Limitations

**New installs:** SYNC LAB / Run-Ahead OFF. **Existing installs:** all saved choices remain unchanged.

Run-Ahead remains available as an optional per-game setting. Audio behavior with Run-Ahead, particularly mGBA + Run-Ahead 1 on some titles, still needs further optimization; that work is planned for **v1.2.3**. This release does not promise zero audio underruns for every game and configuration.

## Installation

Download **slot-for-konkr-1.2.2-arm64.apk** from the assets below and install it over your existing slot. for KONKR Pocket Advance app. **Do not uninstall the previous version or clear app data.** The package name and signing certificate are unchanged, so your settings, ROM access permissions, save files and RetroAchievements configuration are preserved on a normal update.

The built-in updater will discover this verified GitHub release once it has been published.

For setup instructions and controls, see the [README](https://github.com/Baggio94/slot-for-kpa#readme).

## Credits

An unofficial Android port of the original **[slot.](https://slot-cfw.fyi/)** by **[Brandon Kowalski](https://github.com/BrandonKowalski/slot)** and its contributors.

Thanks to **[Its-TroyM](https://github.com/Its-TroyM)** for the 12-hour / 24-hour clock option introduced in v1.2.1.

Distributed under the **GPL-3.0 license**.
