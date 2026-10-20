# Frame pacing parity: RetroArch vs slot. for KPA

**Goal:** reproduce the specific RetroArch Android frame pacing that feels
consistently smooth on the owner's KONKR Pocket Advance. **Not** to replace a
known-good beta or copy a generic setting without measuring its active state.

## Inspected source revisions / verified behavior

- RetroArch source: `ef7933e682c1360777e821e4a2e491d33921f6a3`
  (2026-10-20), `runloop.c`, `gfx/video_driver.c`,
  `gfx/drivers_context/android_ctx.c`, `gfx/drivers/gl3.c` and
  `tools/settings_table.reference`.
- slot. beta14 reference: `b95e77cb71a9d968a6f280114f17681b3115af46`,
  `crates/slot-android/src/runtime.rs`, `settings.rs`,
  `android/app/src/main/java/fyi/slot/konkr/MainActivity.kt`.
- **Unknown at this time:** owner-installed RetroArch app/core versions,
  its configured video driver (gl, glcore, vulkan, threaded), active vsync,
  swap interval, video frame delay, audio sync/rate control, and whether
  Run-Ahead/Preemptive Frames or secondary instances are active. Do NOT
  interpret source defaults as the user's configuration.

### RetroArch

1. `runloop.c` uses `runloop_pace_gather`, `runloop_pace_sources`
   and a pacing decision to identify the *actual holding source* for an
   iteration: VSync/presentation, audio buffer backpressure, driver, scanline
   or timer. It avoids stacking two independent throttles where possible
   (notably menu-content-rate + blocking audio).
2. `gfx/drivers_context/android_ctx.c` exposes EGL swap interval and
   buffer swapping through `egl_set_swap_interval` and `egl_swap_buffers`.
   Video-driver code selects/configures swap intervals when supported.
3. Video subsystem manages optional frame delay, swapchain depth,
   audio-rate control and threaded/nonthreaded modes. **Availability varies
   by active Android driver and actual settings.** Not every RetroArch
   latency feature is supported/effective by every renderer.
4. A separate runloop is responsible for applying Run-Ahead/Preemptive Frames,
   input sampling, audio production and presentation without assuming one
   core step always maps to one screen refresh.

### slot. current implementation

1. Active slot game frames are driven by a Java Android `GLSurfaceView`.
   Current settings choose between continuous callbacks and a
   Choreographer requestRender path; Android `GLSurfaceView` owns actual
   EGL swap, **after** the Rust `nativeDrawFrame` call.
2. `Engine::draw` maintains `game_accum` with multiple
   `settings::frame_debt_*` options and a one-real-core-frame budget
   for hybrid SYNC LAB (unless special linked/fast-forward handling).
3. Audio is fed through a bounded native PCM FIFO to an independent
   Android AudioTrack worker. This producer/consumer cadence differs
   from RetroArch's frontend pacing; merely matching frame-debt formulas
   cannot guarantee the same AudioTrack headroom or input delay.
4. Physical beta14/beta15 A/B localized most GL CPU-thread wall
   wait to the first synchronizing GLES call, and skipping glClear moved
   that wait to the final quad. **No verified present-timing gain.**
   The experimental glClear skip is excluded from this research branch.

### Important precision

A GBA core runs near 59.73Hz on a nominal 60Hz display. A mismatch exists,
so perfect one-emulated-frame-per-physical-refresh forever is not a valid
requirement without display refresh matching or some form of resampling or
timing adjustment. Do not make claims about exact 59.728 fps or input-to-photon
from a vendor SurfaceFlinger `averageFPS` field.

## Exact baseline data to obtain

Use the lab's `parse_ra_config.py` allowlisted exporter and
`capture_pair.sh` on the real device, same ROM/core/settings/BIOS and
thermal state; inspect:
- *Video:* actual video driver, vsync, swap interval, max swapchain images,
  threaded video, threaded display pacing, frame delay automatic/manual,
  video refresh and fullscreen, VRR or scanline sync if present.
- *Audio:* audio driver, sync, latency, rate control, timing skew, output rate.
- *Core:* actual mGBA/gpSP version, frameskip, core options, BIOS and shaders.
- *Run-Ahead:* OFF for **first frame-pacing baseline**, then 1/2 and secondary
  instance/Preemptive Frames in a **separate** controlled comparison.
- *Same-device output:* per-layer SurfaceFlinger present2present p50/p95/p99,
  >25ms rate, emulated FPS for slot only, native+audio PCM/AudioTrack counters
  and subjective audio/input. RetroArch FPS is not directly available from
  SurfaceFlinger; retrieve or record separately through its own OSD/logs.
- Repeat **RetroArch -> slot -> RetroArch** (A/B/A), and separately by core,
  to check temperature/drift. Distinguish measured differences from noise.

## Porting design after first A/B/A evidence

1. **Experimental `RetroArch-style pacing` mode, opt-in only.** Preserve
   existing slot settings and stable fallback. Never silently change fps,
   Run-Ahead, rewind, input or sound for existing users.
2. **One primary holding clock per frame.** Use measurements to select
   video-present vs audio-backpressure vs timer, with explicit ownership and
   bounded fractional game-time debt. Do not double-pace the audio producer.
3. **Frame production distinct from video presentation.** Count real core
   frames separately from callback/presents; controlled duplication/skip
   strategy when 59.73/60 drift accumulates. Never run large catch-up bursts
   that increase control latency or mutate SRAM/RA unpredictably.
4. **Potential EGL ownership change is a separate milestone.** Java
   `GLSurfaceView` currently owns presentation. Reproducing RA's precise
   swap-interval/queue/driver behavior might require a custom EGL video
   backend or an RA-host wrapper, not just another formula in Rust. Benchmark
   this *behind a flag* and avoid unnecessary `glFinish`/fences.
5. **Audio and input synchrony are acceptance gates.** Preserve PCM order,
   bounded output depth, release-safe background/lifecycle behavior,
   synchronous real input samples, Runahead transient audio state, RA event
   timeline and battery/RTC safety. A lower GL duration with worse audio
   underruns or worse subjective controls **fails**.
6. **Test and rollback:** repeat mGBA and gpSP, 1x and RunAhead OFF/ON,
   shader ON/OFF, bordered and full-panel, transitions, suspend/resume,
   saves/RTC, RetroAchievements offline/online, Link separate. Measure
   CPU/power and SF present-gap distributions, not just Java callback fps.
   Keep a data-preserving, signed upgrade path to original mode.

**Project direction:** include RetroArch-equivalent frame pacing and Run-Ahead
as *two related, but separately switchable* compatibility features. Neither
can be declared 'perfect' until tested under the same device conditions.

Never merge, tag or release from this research branch without user approval.
