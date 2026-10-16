# slot. for KONKR Pocket Advance — v1.2.0-beta2

**Android 12 / ARM64 (KONKR Pocket Advance)** — signed pre-release.

This is a **beta**, not a replacement for the current stable v1.1.0 release.

## What's new

- **Automatic cartridge labels.** Uses the original Slot Cart Studio's official art catalog and exact ROM CRC32 identification. Safely skips unmatched games, never overwrites manually imported/Cart Studio labels, and runs in small low-priority batches only while Slot is in the foreground and no game is playing.
- **Corrected image fitting.** Fits the complete official label image to each GBA/GB/GBC cartridge label well, matching the original Cart Studio center-cover behavior. Fixes an issue in beta1 where valid official images were downloaded but rejected.
- **Progressive indexing.** Continues to later games instead of repeatedly stopping at the first 16 ROMs without matching art; diagnostic messages are available with the Android log tag `SlotAutoLabels`.
- **Smart ROM prewarming (unchanged).** After a cartridge has remained selected on the idle carousel for about 900 ms, asynchronously prepares the ROM in the private Android cache; already-cached ROMs are not copied again. The existing 64 MiB per-ROM and approximately 256 MiB cache budgeting, cancellation, and foreground-only conditions are unchanged.
- Existing emulator cores, RetroAchievements, save/state handling, Cart Studio, and multiplayer behavior have not been intentionally changed by beta2.

## Validation and limits

- On an Android 12 Mac emulator, an actual 247-ROM library resulted in **236 downloaded/saved labels** (the remaining 11 had no matching official artwork in that test). Visual cartridge presentation was confirmed by the tester.
- The same test confirmed that a new 8 MiB ROM was warmed **before launch**, and Android-side launch preparation then took **5 ms**. This excludes emulator startup and graphics initialization.
- A full battery-life/standby and performance validation on a physical KONKR Pocket Advance is **still required**.
- Official artwork coverage depends on matching CRC32 entries in the upstream catalog; no approximate filename matching is performed.

## Installation

Download `slot-for-konkr-1.2.0-beta2-arm64.apk`. Install it **over** an existing signed release (do not uninstall) to preserve app data and selected folders. Package: `fyi.slot.konkr`. Version code: **110**. Architecture: **arm64-v8a only**.

A `SHA256SUMS.txt` file is attached. The CI verifies the persistent app-signing certificate and ARM64 libretro/native libraries.

Source: branch [`beta/1.2.0`](https://github.com/Baggio94/slot-for-kpa/tree/beta/1.2.0). Stable `main` remains at v1.1.0.
