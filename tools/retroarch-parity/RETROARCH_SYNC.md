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
- **Fallbacks:** if the panel is more than 5% away from the core rate
  (RetroArch's `audio_max_timing_skew`, e.g. 90/120Hz), the mode uses the
  RA CLOCK LAB clock but keeps resampling and rate control. Link sessions keep
  their own pacing. Fast-forward keeps its existing frame budget.

Run-Ahead, saves, RTC, achievements and the cores are unchanged.
The 1Hz diagnostics line reports `pacing_mode=retroarch` and
`ra_sync_rate_adjust` (1.00000 = buffer exactly half full).

## Acceptance test (hardware required)

Same protocol as RA CLOCK LAB: Wario Land 4, mGBA, 1x, Run-Ahead OFF,
SYNC LAB (120s) → RETROARCH (120s) → SYNC LAB (120s) with the #57 kit.
Pass criteria: fewer SurfaceFlinger gaps >25ms than SYNC LAB, emulated FPS
≈ display FPS, zero AudioTrack underruns after the first second,
`ra_sync_rate_adjust` staying within 0.995..1.005, and no perceived change in
pitch or control response. Then repeat with gpSP and Run-Ahead 1/2.
