# RetroArch reference parity lab — slot. for KPA

**Research stage 1, not a new emulator backend.** Runs on the real KONKR without
installing an APK, changing user settings, exporting ROMs, modifying SRAM/RTC,
uninstalling slot. or changing the public v1.2.2 release.

Source pins (2026-10-21):
- slot baseline: `b95e77cb71a9d968a6f280114f17681b3115af46` (beta14)
- RetroArch source inspected: `ef7933e682c1360777e821e4a2e491d33921f6a3`, `runahead.c`
- slot. pinned Android mGBA libretro: `7a12d6d4b9acb14c0ae62c9166b6a2f3d08007f6` **plus project patches**
- slot. pinned Android gpSP libretro: `5819380c2ffb0900219d700a382ee68c464ebb99` **plus project patches**
- The versions of mGBA and gpSP actually installed in *your RetroArch* are
  **unknown** until verified on the KONKR, and cannot be inferred from this repo.

## What the code comparison proves

**Same core interface:** Both frontends call libretro's `retro_run`,
`retro_serialize` / `retro_unserialize` and offer mGBA/gpSP. Same interface
does not ensure matching core commit, options, audio buffering or video pacing.

**Run-Ahead single instance:**
RetroArch `runahead_run()` in `runahead.c` executes the actual frame,
serializes the state after the actual frame, runs N more frames with predicted
last input, and restores state. Its A/V frontend suspends selected callbacks
during prediction. Slot `GameSession::prepare_runahead()` runs the real frame
in `advance()`, snapshots core + transient core audio, renders 1–2 speculative
frames, discards predicted audio, restores both state/audio, and quarantines the
session if restoration fails. These are *related algorithms*, not exact
equivalence in input sampling, audio outputs or scheduler timing.

**RetroArch has other paths:**
1. Secondary core: distinct libretro instance + save-state handoff when input
   becomes dirty; game video predicted on the secondary core. It includes
   copying/loading the libretro core binary in a distinct loader context and
   filtering side-effectful environment calls. Simply calling dlopen twice on
   Android would NOT safely create two independent instances.
2. Preemptive Frames: records a ring of prior states and replays historical
   input after changes. Distinct from Run-Ahead; not in slot today.

**Do not remove slot's protections:** current mGBA Flash/Gen3 Pokémon and
Super Mario Advance 4 Run-Ahead guards, speculative PCM sidecars, link/session
guards, rollback quarantine, RetroAchievements timeline, native SRAM/RTC
integrity and audio-only callbacks all protect functionality.

**Main remaining unknown:** which exact RA core/config/Run-Ahead path and
audio/video driver yield the user's excellent subjective results on their
KONKR. We must measure this before porting a whole frontend.

## No-data-loss KONKR baseline procedure

Prerequisites: macOS or Linux, `adb`, Python 3.9+, one connected KONKR,
RetroArch installed, slot. installed. ROM/BIOS must be *the same legal copies*
in both apps. The shell script does not start an app or alter settings: **open
the game manually before each capture**.

1. Choose a stable game such as Wario Land 4. **For the initial reference
   comparison set Run-Ahead OFF in both apps**, Fast Forward OFF, same mGBA
   core, shader/filter OFF in both if possible; use the same ROM language and
   BIOS, brightness and thermal conditions. Observe which slot pacing mode is
   selected; don't silently change it if testing that mode.
2. Open the game in RetroArch, leave its screen visible and run:
   ```bash
   bash tools/retroarch-parity/capture_pair.sh ra mgba 120
   ```
3. Open the *same game* in slot, leave gameplay visible, then:
   ```bash
   bash tools/retroarch-parity/capture_pair.sh slot mgba 120
   ```
4. Repeat RetroArch once more as **A/B/A** (or B/A/B) to catch thermal and
   background-process drift. For gpSP repeat separately with `gpsp`.
5. Each capture writes `parity-results/<UTC>-<app>-<core>/present-summary.json`.
   Compare using the resulting actual paths:
   ```bash
   python3 tools/retroarch-parity/compare.py \
     parity-results/<RA-DIRECTORY>/present-summary.json \
     parity-results/<SLOT-DIRECTORY>/present-summary.json
   ```
   These per-layer SurfaceFlinger histograms show presentation intervals,
   not true emulated FPS, GPU execution, audio delay or input-to-photon delay.
   Some RetroArch releases/Android versions omit compatible layer metrics;
   the report then explicitly says **inconclusive**.
6. RetroArch configuration (optional): export `retroarch.cfg` and the selected
   `mgba_libretro.opt` or `gpsp_libretro.opt` using your own device's RA
   Configuration/Directories UI or a file manager. Their paths depend on
   Android package/scoped storage; **do not assume ADB can access private
   Android/data**. **Do not upload raw configs** (they can contain usernames,
   tokens, paths). Generate the sanitized allowlisted JSON locally:
   ```bash
   python3 tools/retroarch-parity/parse_ra_config.py \
      --core mgba --retroarch-cfg /your/local/retroarch.cfg \
      --core-options /your/local/mgba_libretro.opt \
      --output mgba-retroarch-safe-settings.json
   ```
   A `--core-options` path is optional. Repeat for gpSP.
   Missing key means **unknown**, not `false` or a default.
7. Record manually in a local note: RetroArch version, mGBA/gpSP core version
   as displayed under Core Information, shader, Run-Ahead (OFF/1/2), secondary
   core ON/OFF, preemptive ON/OFF, audio driver, whether the video looked
   smooth, subjective controls and audio. The capture script **cannot**
   verify active ROM/core from `pidof`.

**Share only `present-summary.json`, slot's
`slot-emulation-summary.json` and sanitized options JSON.** Raw SF/logcat
and original RA configuration should remain local unless explicitly examined.

## Gates for implementing parity backends

- **Milestone 1 (this PR):** capture actual RA reference core/settings and
  paired RA/slot present histograms; diagnose differences without reinstall.
- **Milestone 2:** isolated optional parity pacing/audio adapter in slot;
  A/B with preserved original mode, full F7/F9/F10/RA/save regression tests,
  recoverable crash behavior, signed APK and explicit owner acceptance.
- **Milestone 3:** actual secondary-instance Run-Ahead behind a separate
  experimental option: distinct dynamic loader identity, safe libretro
  callbacks and VFS, state compatibility checks, side-effect isolation and
  headless lifecycle tests; cannot be a blind copy of `runahead.c`.
- **Milestone 4:** optional Preemptive Frames only after the reliable
  secondary/single-instance modes are validated on real hardware.

Do not merge, tag or release anything from this research branch without
explicit approval. Public main is v1.2.2. GPLv3 and third-party notice
obligations apply if RetroArch source is actually incorporated later.
