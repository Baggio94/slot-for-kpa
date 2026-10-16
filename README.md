# slot. for KONKR Pocket Advance

**An unofficial Android port of [slot.](https://github.com/BrandonKowalski/slot) for the KONKR Pocket Advance.**

Originally developed by **Brandon Kowalski** for a different handheld, slot. inspired this Android port, which also incorporates work from [pvaibhav/slot](https://github.com/pvaibhav/slot). This is a community adaptation, **not an official release by those authors** and not affiliated with KONKR/AYN.

## Built for the KONKR

- Android 12, **ARM64 only**, 960 × 640 (3:2) display and controller-first navigation
- Game Boy Advance, Game Boy Color and Game Boy cartridges: **unzipped** `.gba`, `.gbc`, `.gb` files
- Original-inspired cartridge carousel, covers, custom labels, themes and **Cart Studio**
- **mGBA** and **gpSP** libretro cores with save RAM / RTC, save states, rewind, fast-forward and carefully guarded run-ahead
- **Ten GBA shader presets plus Off**; independent GB and GBC screen settings and bezels
- **RetroAchievements**: online and offline earning/sync, notifications and account management
- Link Cable / RFU multiplayer where supported by the game, emulation core and network
- Library indexing, Favorites, optional Recents, custom artwork and preserved controller shortcuts
- Optimized cartridge faces: 160-entry GPU LRU, background prefetch and on-device compressed cache
- Auto-save on eject, per-game settings, RetroArch save compatibility and Cart Studio
- No ROM, BIOS, account credentials or commercial assets from Nintendo are distributed

### Install

1. Download the **signed ARM64 APK** from [Releases](https://github.com/Baggio94/slot-for-konkr/releases).
2. Install it on the **KONKR Pocket Advance**. If updating an existing `fyi.slot.konkr` build, **install over it**: do not uninstall the old version or clear its data.
3. Open slot., press **START**, then select **Library** and choose the folder containing your own unzipped GB/GBC/GBA ROMs.
4. Select your own BIOS folder where applicable.

**Application ID:** `fyi.slot.konkr`. The same permanent certificate as `Baggio94/slot-konkr` must sign every public update. Android `versionCode` increases monotonically (0.2.1 = 106). Changing the certificate would break normal upgrades.

### First-install defaults

Dark theme, no wallpaper, Recents Off, Game Titles Off; LCD3x for GBA/GBC/GB, no color correction; default GB/GBC bezels; Fast Forward 2× with sound, Rewind On, Turbo X/Y, Rumble On, Auto Save on Eject On; Display Sync pacing, 1-frame Run-Ahead, FPS display Off; Wi-Fi, Bluetooth and app status indicators Off. Existing users' saved preferences are never reset by an update.

## Releases and validation

The **public APK contains ARM64 libraries only**; it has no Mac emulator key mapping, Mac discovery bridge or experimental multiplayer sidecar. It is built from the same validated gameplay code maintained in the separate [development and QA repository](https://github.com/Baggio94/slot-konkr).

Each signed production build checks Android identity, `versionCode`, signing certificate, ARM64 mGBA/gpSP/JNI libraries, original Cart Studio WASM and save/graphics regressions. Every public release includes SHA-256 checksums. Visual/performance parity of RetroArch multipass shader ports is not guaranteed pixel-perfect and should be tested on physical hardware.

## Development and source

This is the **production source tree**. Mac emulation, x86_64 builds, diagnostic tools, benchmarks and experimental branches belong in the [development lab](https://github.com/Baggio94/slot-konkr). Features are validated there, then transferred into a reviewable production candidate. The public source includes the native emulation code and scripts needed to rebuild it; do not publish only the APK.

To build locally, use JDK 17, Android SDK API 35, NDK 27.2.12479018, Rust 1.96, cargo-ndk and Gradle 8.7. The CI pipeline rebuilds the pinned native cores and bundled Cart Studio. Production signing requires the original certificate and secure signing secrets; **no keystore or private key is committed**.

## Open source and credits

- **[Brandon Kowalski — slot.](https://github.com/BrandonKowalski/slot)**: original project, user experience and foundational source
- **[pvaibhav — slot fork](https://github.com/pvaibhav/slot)**: inherited improvements
- **mGBA / gpSP / libretro / RetroAchievements contributors**: emulation and achievement components
- Original shader creators and font/graphic authors: notices and licenses retained in [licenses/](licenses/), bundled asset notices and source headers

The Slot-derived code remains under the repository's **GNU GPL v3** terms; relevant third-party components retain their own licenses. Attribution, copyright notices and corresponding source are required even though this GitHub repository is **not marked as a fork**.

Please report device issues with the ROM platform, core, screen mode, Android version and steps to reproduce, without uploading commercial ROMs or copyrighted BIOS files.
