# RETROARCH pacing — opt-in sixth Frame pacing mode

Reproduces the RetroArch defaults that matter for smoothness on a fixed 60Hz
panel: VSync on, "Sync to Exact Content Framerate" off, Dynamic Rate Control on.

## What changes in this mode

- **Video:** the display clocks the core. Each Choreographer VSync runs one
  real core frame (two at most when a refresh was missed), with no fractional
  frame debt. A 59.7275Hz GBA therefore runs at 60Hz (+0.46%) and no picture
  is repeated for clock drift. SYNC LAB and RA CLOCK LAB both still repeat one
  picture every ~3.7s by design.
- **Audio:** core PCM is resampled to 48kHz (48-tap windowed sinc, as original slot. 1.8) and the ratio is
  corrected by at most ±0.5% from the AudioTrack fill level, aiming at a
  half-full buffer, as RetroArch's `audio_rate_control_delta = 0.005`.
  This absorbs the +0.46% speed-up and the drift between the audio and
  display clocks, which is what caused underruns in the other modes.
- **Frame Delay (automatic):** while the display clocks the core, each
  callback waits after VSync before reading the buttons, so the frame shown
  at the next refresh uses fresher input (RetroArch `video_frame_delay_auto`).
  The delay starts at 0, grows by 1ms per second up to 8ms while the slowest
  frame still leaves 4ms before the next refresh, shrinks at once when frames
  get heavier (Run-Ahead, gpSP JIT flushes), and drops 2ms after any missed
  refresh. It is off in menus, fast-forward, rewind and Link sessions.
- **Run-Ahead as Preemptive Frames:** in this mode the Run-Ahead setting
  (1 or 2 frames) uses RetroArch's Preemptive Frames instead of predicting
  the future. The core keeps the state of the last 1-2 real frames; when the
  buttons change, it goes back and replays them with the new buttons, then
  runs the current frame. Every picture shown is real, and the extra work
  happens only on the frame where a button changes, which is cheaper than
  running ahead on every frame (and avoids gpSP's JIT flush on every frame).
  The Run-Ahead guards (Pokémon FLASH1M, SMA4, Link) still apply, and a
  failed rollback quarantines the session exactly like Run-Ahead.
- **Swap-paced loop (experimental):** by default each frame starts on an
  Android Choreographer VSync and is handed to the GL thread. RetroArch
  instead loops on its own: run one frame, draw it, swap, and the blocking
  swap waits for the display. The same APK can run that loop, selected with
  a development launch extra (kept across restarts):
  `adb shell am start -S -n fyi.slot.konkr/.MainActivity --es slot_ra_loop swap`
  (`vsync` selects the Choreographer loop; `swap` is the default). GLSurfaceView then renders continuously, every call
  runs exactly one core frame (a late swap slows the game rather than
  bursting; a swap that did not wait runs nothing), and the refresh rate is
  measured over runs of 60 on-time swaps. Like RetroArch's GPU Hard Sync,
  the loop waits for the GPU (glFinish) before each swap: without it the
  KONKR compositor waited on our unfinished buffer and every frame reached
  the screen one refresh later. Frame Delay is off in this loop,
  since nothing marks the VSync. The diagnostics line reports `ra_loop`.
- **Fallbacks:** if the panel is more than 5% away from the core rate
  (RetroArch's `audio_max_timing_skew`, e.g. 90/120Hz), the mode uses the
  RA CLOCK LAB clock but keeps resampling and rate control. Link sessions keep
  their own pacing. Fast-forward keeps its existing frame budget.

Saves, RTC, achievements and the cores are unchanged.
The 1Hz diagnostics line reports `pacing_mode=retroarch` and
`ra_sync_rate_adjust` (1.00000 = buffer exactly half full) and
`frame_delay_ms` (the automatic Frame Delay in use);
`runahead_preemptive_rollbacks` counts replays (one per button change).

## Acceptance test (hardware required)

Same protocol as RA CLOCK LAB: Wario Land 4, mGBA, 1x, Run-Ahead OFF,
SYNC LAB (120s) → RETROARCH (120s) → SYNC LAB (120s) with the #57 kit.
Pass criteria: fewer SurfaceFlinger gaps >25ms than SYNC LAB, emulated FPS
≈ display FPS, zero AudioTrack underruns after the first second,
`ra_sync_rate_adjust` staying within 0.995..1.005, and no perceived change in
pitch or control response. Then repeat with gpSP and Run-Ahead 1/2.
`frame_delay_ms` should settle between 4 and 8 with Run-Ahead OFF and must
not make the SurfaceFlinger gap count worse than with it pinned at 0.
