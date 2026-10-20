# slot. 1.2.2-rc1 × SleepManager 0.7.3 — physical KONKR QA

This is a **read-only** cross-app integration. Never clear either app's data,
uninstall an app, force stop SleepManager during a sleep test, or automatically
change SleepManager's enabled toggle. The existing slot. status artwork remains
running/stopped, **not** a text assertion about the user setting.

## Prerequisites

- Physical KONKR Pocket Advance (Android 12; serial `BW0308N250009576`).
- Signed SleepManager **0.7.3-touch beta with Status API v1** installed
  in-place over any existing stable SleepManager.
- Signed slot. for KPA **1.2.2-rc1** (versionCode 137), installed using
  `adb install -r`; leave app data, ROM grants and game saves intact.
- In slot. Settings enable integration indicators and return to the game shelf.

## Read the provider (repeat after toggling enabled on-device)

```sh
adb -s BW0308N250009576 shell content call \
  --uri content://com.med.sleepmanager.status --method status
```

Expect one `Bundle` field `status` containing JSON:
`{"apiVersion":1,"enabled":true|false,"serviceRunning":true|false}`.
The `serviceRunning` field can be false while `enabled` remains true.
This is normal; do not equate enabled with running.

| Actual API | Expected slot shelf sprite |
| --- | --- |
| enabled=true, serviceRunning=true | SleepManager running |
| enabled=true, serviceRunning=false | SleepManager stopped |
| enabled=false, serviceRunning=false | SleepManager stopped |
| app missing | SleepManager indicator hidden |
| provider unavailable/unsupported (0.7.2) | Best-effort legacy fallback; unknown must never be presented as confirmed disabled |

## Cross-app scenarios

1. Observe **enabled + running** in SleepManager and provider; check running
   icon appears on slot.'s shelf without reopening slot.
2. Disable via SleepManager UI. Confirm provider `enabled=false`; return
   to slot. and verify stopped icon after observer/onResume.
3. Re-enable. Verify provider `enabled=true`; if service starts, verify
   running icon; if not, stopped icon is correct.
4. Leave slot. visible while toggling via multiwindow/return-to-app to
   check `ContentObserver` refresh, not only launch-time status.
5. With both apps installed, play Wario Land 4 for 3 min on **mGBA + SYNC LAB
   + Run-Ahead OFF**. Verify controls, sound, RetroAchievements, save/load,
   and that the status query produces no UI/GL jank.
6. Sleep/wake with the lid closed and verify that an observer status read
   did not start the service or interfere with SleepManager's actual
   pause/resume/sync orchestrations.
7. With existing custom slot. settings, upgrade in-place and confirm they
   are preserved. On a **separate clean test profile only**, verify fresh
   defaults: SYNC LAB, Run-Ahead OFF. Do not clear the primary device's data.

## Release gates

- Green native/JVM/Android CI, JNI export and signed APK upgrade checks.
- Real KONKR provider results and matching shelf icons in enabled/disabled
  states; cross-app smoke test and at least one real sleep/wake cycle.
- Controls/fluidity at least as good as beta15, **zero multi-step callbacks**;
  no claim that RA1 audio underruns are solved in 1.2.2.
- Validate SleepManager 0.7.3 separately against its own release gates.
- Do not merge/publish either release without explicit owner sign-off.
