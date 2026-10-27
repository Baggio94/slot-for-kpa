# Changelog — slot. for KONKR Pocket Advance

This changelog covers the public releases of the dedicated KONKR Pocket Advance Android port. Earlier development experiments are preserved in Git history.

## v1.3.4 — 27 October 2026

- Analog stick: the first centred stick event reaches Android again, so its stick-to-D-pad conversion releases the direction (#68).
- Libretro sensor interface: accelerometer and gyroscope from Android, in RetroArch's Android units and rotation; MBC7 tilt axes inverted as mGBA expects (#67).
- Boktai 1/2/3: per-game Sun setting (0..8 / 0..10, one gauge bar per level; SENSOR with a light sensor), handed to mGBA through slot_mgba_set_sun so resumed games keep it; L3/R3 and SELECT+L2/R2 adjust it; starts at 0 so the game calibrates in the dark (#69).
- Sensor cartridges start on mGBA instead of gpSP; MBC7 cartridges on mGBA instead of Gambatte.
- Core options are available in retro_init: Gambatte now shows the GB/GBC boot ROM (#71). Gambatte rumble level 10.
- An empty save state is refused instead of crashing mGBA.

## v1.3.3 — 26 October 2026

- GB/GBC multiplayer over Wi-Fi (SELECT + HOME, Host/Join). mGBA: GB lockstep link, cartridge identity for GB/GBC, peer ROM .gb/.gbc, Pokémon Gold/Silver/Crystal clock kept during the link. Gambatte: Game Link (netlink) driven by HOST/JOIN, with reconnect, timeouts and a serial shift fix.
- In-app update: the Android installer starts in its own task, so "App installed" no longer gets stuck when the old slot. task is killed.

## v1.3.2 — 26 October 2026

- RetroAchievements on GBA: achievement RAM is exposed when Run-Ahead saves a state before the first frame (Super Mario Advance "Core does not expose achievement RAM").
- RetroAchievements: no "tracking interrupted" after activating a large achievement set (Pokémon Emerald); larger evaluator frame queue and logcat diagnostics.
- No "Couldn't import BIOS files" toast after a reboot with slot. as the home app; the silent startup restore retries quietly. Shorter "BIOS ready." toast.
- Controls no longer frozen for ~2 s after waking the console: Wi-Fi and Bluetooth status are read off the UI thread.

## v1.3.1 — 25 October 2026

- Fixed "Couldn't save" on Save and Eject for Pokémon Gen III games on mGBA that had not been saved in-game yet. The Flash guard now only refuses an image that would replace a complete save on disk.

## v1.3.0 — 24 October 2026

- **Gambatte** as a second core for Game Boy and Game Boy Color, picked on a drawn GB pak board; battery saves carried between mGBA and Gambatte.
- Settings → Systems: one page per system with a **Default Core** row; a cart's own core choice still wins.
- GB/GBC screen sizes on L1/R1: Integer, Original Ratio (10:9, sharp-bilinear when no shader is picked) and Stretch.
- Watomsk's Game Boy, Game Boy Pocket and Game Boy Color overlays (with permission), adapted to 3:2; a toast explains when a bezel cannot be shown.
- Live GB Palette row and button-art legends, as slot. 1.8.1.
- Visuals drawn at the KONKR's native 960x640 density; redrawn status bar icons.
- Controls: direct, unbuffered hat D-pad; quick taps kept between two core frames; NATIVE runs one core frame per swap as RetroArch; low-latency audio output and display priority for the emulator thread.
- Adaptive Link Cable input delay (mGBA Cable).
- Clearer messages and menu labels; Status Bar settings page.
- Faster launch after idling on the shelf; smooth first scroll after ejecting a game.
- New first-install defaults: Fast Forward 3x, Rewind 3x, X/Y Buttons A/B, GB Palette GBC Green, no GB/GBC bezel. Existing settings are kept.
- Removed: the original Linux Slot app, retired pacing modes and beta experiments (controlled EGL presenter, latch wait).

## v1.2.3 — 23 October 2026

- New **Native** Frame Pacing, the default: the display clocks the core as in RetroArch, with a 48 kHz windowed-sinc resampler, dynamic audio rate control and GPU hard sync. Measured on KONKR as smooth as RetroArch (2 late frames in 2 minutes) at lower latency.
- **Responsive** Frame Pacing for the lowest latency (VSync callback loop with automatic Frame Delay).
- All previous pacing modes (SYNC LAB, Standard, Display Sync and lab modes), global and per game, migrate to Native. Link keeps its own lockstep pacing.
- RetroArch-style Run-Ahead with a second instance and Preemptive Frames; Run-Ahead stays OFF by default.
- Game Boy / Game Boy Color audio played at its real 131072 Hz rate.
- Rewind speed 1x/2x/3x/5x, per ROM and core; 2x default as in original slot.
- Fast Forward can be set to Off (from original slot. 1.8.1).
- Save State Switcher asks before deleting a state (from original slot. 1.8.0).
- Durable settings, profiles, favorites and recents; battery saves written off the graphics thread with barriers before rewind, state restore, suspend, eject and Link.
- Event-driven RetroAchievements evaluation.
- Upstream mGBA MBC2 save RAM, FLASH1M bank and DMG palette fixes; gpSP serial session fix.
- Smoother game start and fewer per-frame allocations in audio and rendering.

## v1.2.2 — 20 October 2026

- **SYNC LAB** emulation pacing; new installs default to SYNC LAB with Run-Ahead OFF. Existing settings and per-game overrides remain unchanged.
- ZIP ROM loading for GBA/GB/GBC with bounded, read-only extraction, stable game identity and RetroArch saves/RetroAchievements compatibility.
- Retry logic for transient official cartridge label download failures, without overwriting user labels.
- Improved GB/GBC Cart Studio and shelf cartridge material matching, including ZIP ROMs.
- Targeted mGBA **Drill Dozer** Game Boy Player mode compatibility fix.
- SleepManager **0.7.3 Status API v1** support for the shelf indicator, read-only and queried off the graphics thread, with legacy fallback.
- Better audio startup, additional pacing/audio diagnostics and QA regression coverage; native JNI export validation in signed APK CI.
- Preserves single-real-frame SYNC LAB display cadence after an experimental double-frame recovery was rejected for its perceived control/visual regression.
- Run-Ahead audio optimization intentionally deferred to **v1.2.3**.

## v1.2.1 — 18 October 2026

- Optional saved **12-hour / 24-hour clock** setting in START → Settings → Interface, based on a contribution by [Its-TroyM](https://github.com/Its-TroyM) ([PR #2](https://github.com/Baggio94/slot-for-kpa/pull/2)).
- Improved About, Software Update and RetroAchievements spacing and alignment.
- More consistent white Wi-Fi, Bluetooth and app integration icons; transparent, automatically stacked background progress indicators.
- Automatic label scanning uses the validated v1.2.0 downloading logic, with clearer progress and diagnostics.
- All v1.2.0 features and in-place upgrade compatibility preserved.

## v1.2.0 — 18 October 2026

- Automatic official Cart Studio artwork downloads for missing cartridge labels.
- Better matching of translated, patched and regional ROMs.
- Low-priority ROM preloading to reduce launch time.
- Built-in software updater with optional automatic checks.
- Browsing, performance, navigation and stability improvements.

## v1.1.0 — 16 October 2026

- KONKR Pocket Advance-focused ARM64 port of original [slot.](https://slot-cfw.fyi/) v1.7.0.
- RetroAchievements with offline unlocks, native Cart Studio, and Android app launcher.
- Link Cable and Wireless RFU multiplayer for compatible games.
- RetroArch/Syncthing-oriented saves and save states.
- Screen customization, shaders, controller integration and performance improvements.

Older pre-release development milestones remain accessible through commit history and prior tags.

Original slot. by [Brandon Kowalski](https://github.com/BrandonKowalski/slot) and contributors. Unofficial Android port distributed under the GPL-3.0 license.
