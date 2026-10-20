# SleepManager status indicator interoperability

The next SleepManager 0.7.3+ exposes a read-only status API at
`content://com.med.sleepmanager.status` using
`ContentResolver.call(uri, "status", null, null)`.

Response key `status` is a JSON object string, for example:

```json
{"apiVersion":1,"enabled":true,"serviceRunning":true}
```

Slot renders the existing *SleepManager running/stopped* status sprite as
**running** only when both `enabled` and `serviceRunning` are true.
A configured-on but non-running service displays stopped (it is not operating).
Apps installed without the v1 provider, including stable SleepManager 0.7.2,
retain the previous **best-effort** Android 12 service enumeration fallback.
Unknown API responses never silently disable SleepManager.

The status provider is read-only and cannot toggle the manager. Slot listens
for `ContentObserver` notifications while the shelf is visible/active,
resumes with a fresh status query and executes provider calls on its existing
low-priority worker. No synchronous IPC is done on the UI/GL render thread.
No new sprite artwork, changing Slot's controller UX or settings, or
foreground service is required.

**Release sequencing:** ship SleepManager 0.7.3 with the provider first, then
build a new slot. APK with this client; the slot. version remains compatible
with SleepManager 0.7.2 as an older installation. This integration can be
cherry-picked into the next active slot. beta without overwriting its emulator
performance work or changing signing identity/versionCode.
