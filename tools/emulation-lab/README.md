## Fast path from beta10 onward — one command

For routine tests, **stop doing a full core × Run-Ahead matrix at each build**.
Run the Rust/Android/parser safety checks in CI, then reserve **one short physical
spot-check per modified subsystem**. Extend only when output headroom or regressions
require more evidence.

The read-only `quickcheck.py` automates the existing `capture.sh` script,
extracts native metrics and **actual AudioTrack queue-depth history**, and
writes `quickcheck.md` + `quickcheck.json` alongside the raw reports.
It does **not** modify an installed APK, selected emulator, Run-Ahead,
settings, ROMs, game state or saves. **Start a game first.**

From the repository root (gpSP + RA1 selected in the app already):

    python3 tools/emulation-lab/quickcheck.py --seconds 180 --expect-core gpsp --expect-ra 1

Or review a previous capture, without any device:

    python3 tools/emulation-lab/quickcheck.py --existing emulation-lab-results/TIMESTAMP-scenario

The known KONKR serial is the default (override with `--serial`).
If using an **older beta3 test kit**, download the single standalone
`tools/emulation-lab/quickcheck.py` from this branch into the kit root:

    python3 quickcheck.py --seconds 180 --expect-core gpsp --expect-ra 1

**Results:** `OK PROVISOIRE` indicates no *measured* hardware underruns or RA
failures and no near-empty sampled AudioTrack output queue in this capture.
`SURVEILLER` flags <1ms queue headroom, invalid EGL timestamps, or missing
measurements; `A REVOIR` flags underruns, incompatible settings or failures.
Even OK does NOT establish audible quality, permanent stability, real
presentation timing or input-to-photon latency. Real-device acceptance
still requires listening, visual review, and representative game coverage.
The known EGL probe loses some timestamps on KONKR; do not interpret its
~57 sampled present FPS as physical displayed FPS. `audio_empty_total` is
NOT a count of AudioTrack underruns.

---

# slot. for KONKR — Emulation Lab (1.2.2-beta3)

An opt-in diagnostics build for the established **Mac Apple Silicon ARM64
Android 12L** AVD `Slot_KONKR_API32` (960×640 / 3:2), and a KONKR ARM64
test build. The debug package is **`fyi.slot.konkr.diag`**, side-by-side
with `fyi.slot.konkr`; its internal saves and preferences are isolated.
External RetroArch save/state folders are disabled in the debug build.
It never changes the installed verified v1.2.1.

## Running the Mac AVD

```sh
"$HOME/Library/Android/sdk/emulator/emulator" -avd Slot_KONKR_API32 -gpu auto -no-boot-anim
"$HOME/Library/Android/sdk/platform-tools/adb" devices
"$HOME/Library/Android/sdk/platform-tools/adb" -s emulator-5556 shell getprop ro.product.cpu.abi
# This test APK requires: arm64-v8a
"$HOME/Library/Android/sdk/platform-tools/adb" -s emulator-5556 install -r slot-for-konkr-1.2.2-beta3-mac-avd-debug.apk
"$HOME/Library/Android/sdk/platform-tools/adb" -s emulator-5556 shell am start -n fyi.slot.konkr.diag/fyi.slot.konkr.MainActivity
```

After granting SAF read access to your ROM folder, use your own legally acquired
ROMs and keep comparisons on the same game/settings/device. No ROM, BIOS,
audio content or credentials are included in or uploaded by this project.

**Mac keyboard:** arrows=D-pad, Z/X=A/B, A/S=X/Y,
Q/E=L1/R1, 1/2=L2/R2, Enter=START, Space=SELECT, Tab=MODE/HOME.
Android Studio/AVD must have hardware keyboard input enabled. The same
Android controller events from a connected gamepad work unchanged.

## First comparison matrix

| Scene | Core | Run-Ahead | Auto Save | What to observe |
|---|---|---|---|---|
| Drill Dozer language/start menu | mGBA | 0 | OFF | START works? |
| Same fresh launch | mGBA | 1 | OFF | Does START stop working? |
| Same fresh launch | mGBA | 1 | ON | Does Auto Save affect it? |
| Same fresh launch | gpSP | 1 | OFF | Control baseline |
| WarioWare audio gameplay | mGBA | 0 | unchanged | Listen for crackle |
| Same replay | mGBA | 1 | unchanged | Crackle introduced by rollback? |
| Same replay | gpSP | 1 | unchanged | Compare source vs Android output |
| GB and GBC varied gameplay | mGBA | 0 then 1 | unchanged | Audio/video/pacing baseline |
| GBA heavy title | both | 0 then 1 | unchanged | Cost, missed frames, underruns |

To isolate the start-state problem, **eject and relaunch** between tests, don't
resume a prior savestate accidentally. When feasible, repeat each 45s trial
twice. Link/MultiPlayer should be tested separately and never be treated as
a 60 FPS single-console benchmark.

## Capturing a run

With a game running, from the repository root:

```sh
bash tools/emulation-lab/capture.sh emulator-5556 drill_mgba_ra0 45
bash tools/emulation-lab/capture.sh emulator-5556 drill_mgba_ra1 45
bash tools/emulation-lab/capture.sh emulator-5556 drill_gpsp_ra1 45
# Real KONKR (test APK, if installed), separate from emulator:
bash tools/emulation-lab/capture.sh BW0308N250009576 warioware_mgba_ra1 60
```

Each capture produces `emulation-lab-results/<stamp>-<scene>/{logcat.txt,report.txt,report.json,metadata.txt}`.
Share only the report files if you prefer; logs also include the stock
`SlotKonkr` diagnostic messages and may mention cartridge titles. No save
payloads, ROM files, raw PCM or screenshots are captured.

Useful quick live logs:

```sh
adb -s emulator-5556 logcat -v time -s SlotEmuDiag:I SlotKONKR:I SlotKonkr:I '*:S'
```

## Measurements and limitations

* `emu_fps` measures **real** libretro steps, not speculative frames or screen refresh;
  the nominal GBA target is approximately **59.7275** frames/s.
* `gl_callback_fps`, callback gap bins and render-duration logs are
  **not** actual image presentation timestamps. Swap and screen scanout follow.
* `runahead_snapshot_us`, `runahead_speculative_us`, and
  `runahead_restore_us` isolate the cost of speculative emulation.
  Nonzero `runahead_failures` means rollback has been disabled for safety.
* `audiotrack_underruns` and `audio_queued_ms` identify Android playback
  starvation. `audio_jump_candidates` is only a heuristic; intentional
  loud transients may trigger it. Audio quality still needs listening tests.
* `input_next_gl_mean_us` is **Android key dispatch → next GL callback**,
  *not* controller-to-photon latency. Real latency requires an external
  high-speed camera or synchronized hardware probe.
* Mac AVD uses virtualized input/audio/GPU and its frame rate **cannot**
  establish physical KONKR results. Test unchanged scenarios on the device too.
* No automatic CPU governors, audio buffer changes, input prediction, frame
  skipping, OpenGL readbacks, runahead disable/enable, save-state rewrites,
  BIOS selection changes or core patches are applied by the Lab.

For safety/performance, future fixes should be based on paired baseline + lab
measurements, be behind a targeted game/core guard where needed, and receive
independent save-state, RetroAchievements, link, audio and ZIP regression tests.
