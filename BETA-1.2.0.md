# slot. for KONKR Pocket Advance — 1.2.0-beta1

This is a **test build**, not a stable/public replacement for 1.1.0.

## Included in beta1

- **Conservative automatic cartridge artwork:** while foregrounded, on a
  validated connection, a low-priority worker checks the official Slot Cart
  Studio CRC32 artwork index and loads full physical-cartridge scans when the
  checksum and platform match. No filename guesses, ROM modifications or
  overwrite of existing custom/Cart Studio labels. Eight new labels at most
  per pass; unsupported/ambiguous scans are left to the Studio. New labels
  are shown only when the carousel is idle to avoid disrupting scroll.
- **Warm the selected ROM:** after leaving a cartridge selected for ~900 ms,
  a separate low-priority cancellable Android worker prepares a bounded
  cached ROM, reusing the **same** path as the existing launcher. Moving,
  opening menus, inserting a cart, playing or backgrounding cancels the
  optional read. The launch worker, emulator cores, saves and RetroArch
  import flow are unchanged.
- **No changes** to one-frame Run-Ahead, mGBA/gpSP, shaders, animations,
  controller key assignments, save/state format or RetroAchievements.

## Test checklist

- Fast left/right scrolling and L/R system changes: same smoothness and
  immediate input, no art placeholder regressions.
- An unchanged selected ROM: first load time vs second load time; check
  `SlotKonkr` log for `ROM cache` launch measurements.
- New library with missing artwork: correct label only on CRC32-matched carts;
  manual/Cart Studio labels preserved; working without Wi-Fi.
- RAM, CPU, storage and battery impact over extended browsing.
- Offline RA, save/share, sleep/wake and multiplayer smoke tests.

## Updating

Package `fyi.slot.konkr`; `versionName 1.2.0-beta1`;
`versionCode 109`. The original persistent signing certificate is required
for `adb install -r` upgrades. **Never uninstall** a previous version.
Return to the stable 1.1.0 only with a data-preserving higher-version release;
Android normally blocks downgrading to lower versionCode.

Configuration backup/restore, custom KONKR panel calibration and LCD
ghosting remain **proposals**, not included in beta1. They're deliberately
postponed to avoid changes to the in-game UI and save logic in one beta.
