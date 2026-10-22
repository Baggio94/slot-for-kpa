# Controlled EGL presenter — development experiment

This is the first owned-EGL prototype. It retains the GLES compositor, core,
Run-Ahead policy and audio policy. It does not yet use Swappy, presentation
deadlines or GPU fences, and does not claim RetroArch parity.

## Selecting the two paths in the same build

The default remains `glsurfaceview`. The development launch extra selects and
persists the backend at Activity creation. Close the current session normally
before restarting; never force-stop with an unsaved game running.

```sh
adb shell am start -S -n fyi.slot.konkr/.MainActivity --es slot_presenter controlled-egl
adb shell am start -S -n fyi.slot.konkr/.MainActivity --es slot_presenter glsurfaceview
```

For a debug build use `fyi.slot.konkr.diag/fyi.slot.konkr.MainActivity`.
No versionCode or release/signing policy is changed by this prototype.

## Ownership and timing

- A `HandlerThread` owns both EGL and every native session callback. A 1x1 pbuffer
  parks the context between window surfaces; ordered account/suspend commands can
  still run with a current context. The thread lives until Activity destruction.
- An owner-thread Choreographer is the only frame scheduler on this path. The UI
  Choreographer is disabled. Repeated render requests coalesce; native input and
  lifecycle commands are not coalesced. At most one frame callback is outstanding.
- The existing continuous/dirty shelf policy remains: settled shelves request a
  heartbeat, animated UI/gameplay requests display callbacks. VSync interval is 1.
- `nativeDrawFrame` uses the same core cadence logic and options. Resume/reattach
  clears elapsed time debt and queued PCM, without reloading a ROM or state.
- `surfaceDestroyed` synchronously waits for the owner to stop using the window,
  as required by Android. A blocked driver could therefore block that lifecycle
  callback; returning early and using the destroyed window would be unsafe.
- An EGL error stops frame scheduling, abandons old native graphics names and
  destroys EGL, retaining the session. Surface replacement or resume retries
  reconstruction. No automatic busy retry or silent switch to the other backend.
- Pause stops frames before the existing suspend checkpoint. Final shutdown drops
  the engine on its owner before EGL teardown. Activity/process recreation still
  uses the normal saved-state launch path, not a live session transfer.

## Measurements and validation boundary

`SlotKonkr` logs backend choice, surface epoch, owner thread and swap interval.
The `EGL owner swap` report includes the lifetime submitted ID and swap wall time
mean/max/over-16ms. These are submissions and CPU waits, **not physical display
timestamps or unique emulated frames**. The existing EGL presentation probe and
SurfaceFlinger capture remain the evidence for display timing. One pending app
request does not bound Android BufferQueue depth or prove input-latency parity.

Host tests cover request coalescing, pause/detach/close gating, and native recovery,
resume and shutdown with a real Mesa context and Run-Ahead 0/1/2 fake cores.
ARM64 compilation, JVM tests and an Android API32 emulator lifecycle harness are
CI gates. The harness uses the real presenter and a simple GL test renderer;
it does not exercise JNI or a game. Results must be checked on the exact commit.
The KONKR driver, native integration and visible output still need device validation.

First device check: pause/resume, sleep/wake, background/foreground, then same-APK
A/B in the same Wario Land 4 scene, mGBA, Run-Ahead OFF, 120 seconds per path. Keep
core/options/shader/audio and cadence fixed. Compare physical intervals, latency,
emulated speed and audio queue depth; do not accept a smoother result bought with
extra input lag. Only expand to Run-Ahead 1/2 and gpSP after lifecycle and baseline
presentation pass. No repeat of the old SYNC LAB clock-only experiment is needed.

Android lifecycle references:
- https://developer.android.com/reference/android/view/SurfaceHolder.Callback
- https://source.android.com/docs/core/graphics/arch-sv-glsv
- https://developer.android.com/reference/android/opengl/EGL14
