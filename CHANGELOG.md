# Changelog — slot. for KONKR Pocket Advance

## 0.2.1 — KONKR ARM64 production extraction (candidate)

- Created a standalone, non-fork, KONKR-focused source distribution from the tested QA105 Android port.
- Kept the original Android app ID `fyi.slot.konkr` and permanent update certificate; incremented Android versionCode to 106.
- Removed the Mac AVD keyboard adapter, emulator input latch, Mac discovery bridge, x86_64 native build and experimental multiplayer sidecar.
- Removed the old Linux CFW website/assets, experimental test harnesses and historical development workflow files while retaining all source, assets, licenses and functional tests required to rebuild and validate the Android app.
- Retained all QA105 functionality: two-line first-run prompts, new-user defaults, RetroAchievements, Cart Studio, mGBA/gpSP, GBA/GB/GBC shaders, persistent artwork cache, save interoperability and supported multiplayer.
- Introduced a dedicated production-only ARM64 signing, test, package, ABI assertion and checksum release workflow.
- This production candidate must pass CI and physical KONKR testing before tag `konkr-v0.2.1` is published in the independent repository.

## 0.2.0 — prior universal Android release

- Released in [Baggio94/slot-konkr](https://github.com/Baggio94/slot-konkr/releases/tag/konkr-v0.2.0).
- First publicly distributed Android build with ARM64 + x86_64 and production features; its update-compatible certificate is preserved here.

Original Slot credits: [Brandon Kowalski](https://github.com/BrandonKowalski/slot), [pvaibhav](https://github.com/pvaibhav/slot). Third-party licenses and copyright notices remain in `licenses/` and the source files.
