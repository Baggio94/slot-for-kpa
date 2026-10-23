# RetroArch parity development

Starting point: beta16, `b3e653a78d26165750fc024a40eebea5b2322f78`.
Working branch: `dev/retroarch-parity-foundation`. Target: comparable experience
on KONKR with Run-Ahead OFF, 1 and 2, preserving Slot's UI and data guarantees.
The 21–22 October captures remain the hardware baseline; repeating SYNC LAB
before changing the presentation architecture does not answer the next question.

## Completed foundation (P0)

- The RetroAchievements `Tracked` core forwards `save_ram_verified()` including
  its error. It must not fall back to the potentially stale libretro SRAM mirror
  after state restoration. Tests cover current data after rollback, no battery,
  and a rejected active-SRAM clone through the real trait-object wrapper.
- The capture script includes the case-sensitive Kotlin `SlotKonkr` tag alongside
  `SlotKONKR` and `SlotEmuDiag`. Audio hardware/queue and EGL diagnostics now reach
  the capture. The test compares the filter to the actual producer tags.
- EGL presentation accounting uses a lifetime sequence per surface and a separate
  report callback count. Reporting no longer changes the age of pending frame IDs.
  The same production accounting runs with a fake timestamp provider in host tests:
  three 300-callback windows, delayed/invalid/error responses, a missed next ID,
  surface reset, and unavailable/incomplete timestamps. A missed next ID breaks
  interval continuity in queue order instead of inventing a slow presented frame.
  Changes of EGL display as well as surface invalidate the probe.
- `runtime-tests` compiles the actual Android session/runtime modules for Linux
  tests. Previously their `cfg(target_os = "android")` excluded their test bodies.
  EGL is linked, but these tests do not create a context or execute JNI callbacks.
- Five new tests call the production `GameSession`: real audio/state/SRAM/RTC with
  0/1/2 prediction frames, future turbo phases, failure before speculation, rollback
  quarantine with disk checkpoints intact, and FLASH1M/SMA4/Link exclusion gates.
  The fake core deliberately keeps audio mixer state outside its ordinary state,
  so the test detects a missing audio-sidecar restoration.

Local verification: 248 Android session/runtime/probe tests, 40 achievements and
63 libretro tests passed (one pre-existing ignored achievements test), plus 28
Emulation Lab and 19 parity-tool Python tests. Dedicated CI additionally compiles
and links the Android ARM64 library and compiles/tests both Kotlin build variants.
See the exact commit's Actions result for CI status; local Linux tests do not
prove device presentation, JNI lifecycle or determinism of the real mGBA/gpSP cores.

No version bump or installable APK is produced by this development workflow.
It uses read-only repository permissions, no signing secrets and no release job.

## P1: context recovery on the existing owner thread

`nativeSurfaceCreated` now retains an existing Engine and GameSession and rebuilds
only the compositor. UI texture IDs remain stable. Opt-in CPU texture snapshots
retain the latest dimensions, pixels and filtering; shared immutable snapshots
avoid doubling the entire UI recovery reserve while uploading a new context.
Game pixels are re-uploaded from the session, without a new per-frame CPU copy.
Shader configuration, paper, bezel, grading and canvas size are restored.

All GL resource owners abandon old names before reconstruction: their destructors
must never delete those names against the replacement context. New allocations
are released normally on a failed attempt, while the old CPU snapshots and session
remain available for retry. Failed recovery gates both the JNI frame entry and the
engine draw method; no frame/input/save work is processed there. The explicit
Android suspend/save path retains its existing safety policy. A successful retry
resets elapsed emulation debt and the cadence clock, and clears old queued PCM.

Validation added: a real surfaceless Mesa EGL context is destroyed twice; rendered
pixels are compared exactly, including updated texture IDs, filtering and game
shader settings. An injected GL error exercises failure and retry. A second test
calls the production surface-created JNI entry with real GameSessions and fake
cores at Run-Ahead 0/1/2, checking identity, state, pixels, battery-call counts,
FLASH1M/SMA4 flags and rollback quarantine through failure/retry. These tests are
mandatory in the development CI, alongside ARM64 compilation.

**Boundary:** this covers EGL context replacement on the same GLSurfaceView owner
thread. It does not preserve a thread-local Engine when that thread or the Activity
is destroyed, and it does not yet replace GLSurfaceView or own buffer swaps. The
next presenter must keep a persistent owner thread across surface detach/attach.
No physical-device pacing, driver behavior or memory budget is validated by Mesa.
The runtime logs retained CPU pixel bytes after successful recovery so the actual
KONKR reserve can be measured with its populated library and artwork.

## P2 prototype (removed in 1.3.0)

A controlled-EGL presenter (EGL owned by a persistent HandlerThread, owner-thread
Choreographer) was prototyped as an opt-in developer path. KONKR captures measured
it worse than GLSurfaceView, so 1.3.0 removed it; GLSurfaceView is the only
presenter. Native session recovery remains tested with real Mesa EGL.

## Remaining presentation work

Context recreation no longer destroys the Engine. `Engine` still intermixes UI state, texture IDs, the achievements service and link
state. Replacing GLSurfaceView before this ownership boundary is separated risks
losing the only live core or deleting stale GL names in a new context.

Separate persistent session/UI state from context-owned graphics first, on one
persistent owner thread. Preserve the same core object, verified battery writer,
rollback quarantine, rewind, Undo, pending saves, achievements service and Link
rules across surface detach/reattach. Recreate graphics without loading another
core, rereading an auto-state or resetting user settings. A failed recreation must
retain the session and its error state. GPU resources must be released while their
own context is current, or explicitly abandoned after context loss; never delete
old names against a new context. Test these transitions before backend selection.

The prototype introduces a `SurfaceView` presenter that owns EGL on that same thread, with
an in-APK reference path for GLSurfaceView. Keep core options, shaders, emulation
cadence, Run-Ahead and audio policy constant for the first comparison. Record swap
wall time, surface epochs and frame IDs; do not call swap duration or a GPU fence
"time displayed". Use one presentation scheduler and at most one pending application
frame request, while retaining ordered input/lifecycle commands. Explicitly cover
pause, surface replacement, context loss and shutdown. The application bound alone
does not prove Android's BufferQueue depth is bounded.

After the prototype passes device lifecycle validation, evaluate the presentation scheduler (including
Swappy's presentation timestamps/fences), with the old Choreographer controller
removed from that path. Do not silently change 59.728 Hz emulation to 60 Hz or hide
misses by increasing audio buffering. At native cadence on a 60 Hz panel, a repeated
image about every 3.68 seconds is expected; callbacks and new surfaces are not unique
emulated images. Display-locked speed/audio resampling is a separate experiment.

## Evidence gates after a real presenter change

Use a focused same-device comparison on the same new APK: Wario Land 4, mGBA,
Run-Ahead OFF, same scene/options and 120 seconds. Existing RetroArch references
showed 1–3 intervals over 25 ms and p99 near 18 ms. Those are comparison targets,
not guaranteed thresholds for every game or proof that Vulkan alone explains the
current difference. Reject a gain bought with another frame of input latency,
queue growth or slower emulation. Repeat only if the comparison is inconclusive.

If controlled EGL cannot meet the measured target, implement a real Vulkan backend
against the same persistent host, preserving Slot's UI/rendering features. Porting
only a final blit is not a complete Vulkan renderer. No monolithic RetroArch frontend
embedding, removal of safety gates, or unisolated second `dlopen` is justified.

After presentation, test audio delivery and input timing separately, then real-core
Run-Ahead deterministic replay. Slot's current real-frame audio policy differs from
RetroArch single-instance predicted-frame audio; it is not safe to change it without
controlling installed core versions and recording the reference mode. A second core
requires explicit identity, isolated globals/callbacks/SRAM/RTC and zero speculative
external effects before it can replace rollback on the primary instance.

Publishing, merging into main and creating releases remain separate decisions.
Any future installable test APK must use the official certificate and versionCode
above 154, and preserve data with `adb install -r`.
