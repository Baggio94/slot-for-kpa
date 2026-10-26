//! JNI entry points called by MainActivity, plus the EGL recovery test fixture.
use super::*;

// The primitive JNI ABI is sufficient for this stage; no Java objects are touched.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSurfaceCreated(
    _env: *mut c_void,
    _activity: *mut c_void,
) -> u8 {
    ENGINE.with(|holder| {
        let started = Instant::now();
        LIBRARY_UPDATE_SAFE.store(false, Ordering::Release);
        if let Some(engine) = holder.borrow_mut().as_mut() {
            // GLSurfaceView calls this on its existing owner thread after EGL
            // context recreation. Do not reload a core, auto-state or settings.
            AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
            let result = engine.gpu.restore_after_context_loss(&AndroidSurface { size: engine.size });
            if let Err(error) = result {
                eprintln!("slot-konkr: graphics recovery failed; session retained: {error}");
                return 0;
            }
            if let Some(session) = engine.game.as_ref() { engine.gpu.upload_game(session.frame()); }
            engine.last = Instant::now();
            engine.game_accum = 0.0;
            engine.ra_clock.reset();
            engine.ra_sync_steps.reset();
            crate::game::log_launch_timing(&format!(
                "Graphics recovered in {}ms; retained UI pixels={} bytes",
                started.elapsed().as_millis(), engine.gpu.recovery_bytes()));
            return 1;
        }
        RA_STATUS_KEY.store(u64::MAX, Ordering::Release);
    *RA_STATUS.lock().unwrap_or_else(|e| e.into_inner()) = None;
        RA_NOTICES.lock().unwrap_or_else(|e| e.into_inner()).clear();
        PLAYING.store(false, Ordering::Release);
        CAROUSEL_QUIET.store(false, Ordering::Release);
        *WARM_ROM_URI.lock().unwrap_or_else(|e|e.into_inner()) = None;
        SAMPLE_RATE.store(0, Ordering::Release);
        let (version, snapshot) = {
            let state = LIBRARY.lock().unwrap_or_else(|err| err.into_inner());
            (state.version, state.entries.clone())
        };
        match Engine::new(version, snapshot.as_deref()) {
            Ok(engine) => {
                *holder.borrow_mut() = Some(engine);
                LIBRARY_UPDATE_SAFE.store(true, Ordering::Release);
                crate::game::log_launch_timing(&format!(
                    "Cold renderer ready in {}ms", started.elapsed().as_millis()));
                1
            }
            Err(error) => {
                eprintln!("slot-konkr: OpenGL renderer initialization failed: {error}");
                0
            }
        }
    })
}

/// All four presenter lifecycle calls run on the same persistent owner as Engine.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeGraphicsReady(
    _env: *mut c_void, _activity: *mut c_void,
) -> u8 {
    ENGINE.with(|holder| holder.borrow().as_ref().is_some_and(|e| e.gpu.graphics_ready()) as u8)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeGraphicsContextLost(
    _env: *mut c_void, _activity: *mut c_void,
) {
    ENGINE.with(|holder| {
        if let Some(engine) = holder.borrow_mut().as_mut() { engine.gpu.abandon_context(); }
    });
    LIBRARY_UPDATE_SAFE.store(false, Ordering::Release);
    AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePresentationResume(
    _env: *mut c_void, _activity: *mut c_void,
) {
    ENGINE.with(|holder| {
        if let Some(engine) = holder.borrow_mut().as_mut() {
            engine.last = Instant::now();
            engine.game_accum = 0.0;
            engine.ra_clock.reset();
            engine.ra_sync_steps.reset();
            engine.perf_last = Instant::now();
            engine.perf_rendered = 0;
            engine.perf_emulated = 0;
            engine.perf_catchup = 0;
            engine.perf_step0 = 0;
            engine.perf_step1 = 0;
            engine.perf_step_multi = 0;
        }
    });
    AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRendererShutdown(
    _env: *mut c_void, _activity: *mut c_void, context_current: u8,
) {
    ENGINE.with(|holder| {
        let mut engine = holder.borrow_mut().take();
        if context_current == 0 {
            if let Some(engine) = engine.as_mut() { engine.gpu.abandon_context(); }
        }
        // The normal suspend checkpoint precedes shutdown. Dropping a session
        // does not serialize or save a speculative/poisoned core.
        drop(engine);
    });
    PLAYING.store(false, Ordering::Release);
    SAMPLE_RATE.store(0, Ordering::Release);
    LIBRARY_UPDATE_SAFE.store(false, Ordering::Release);
    AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSurfaceChanged(
    _env: *mut c_void,
    _activity: *mut c_void,
    width: i32,
    height: i32,
) {
    ENGINE.with(|holder| {
        if let Some(engine) = holder.borrow_mut().as_mut() {
            engine.size = (width.max(1) as u32, height.max(1) as u32);
        }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollCartLabelUri(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    let uri = LABEL_PICK_REQUEST.lock().unwrap_or_else(|e| e.into_inner()).take();
    uri.and_then(|s| env.new_string(s).ok())
        .map_or(std::ptr::null_mut(), |s| s.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeReloadCartLabel(
    mut env: JNIEnv<'_>, _this: JObject<'_>, uri: JString<'_>,
) {
    if let Ok(uri) = env.get_string(&uri) {
        INPUT.lock().unwrap_or_else(|e| e.into_inner())
            .push_back(Input::ReloadCartLabel {
                uri: uri.to_string_lossy().into_owned()
            });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeReloadVisualAssets(
    _env: *mut c_void,
    _activity: *mut c_void,
) {
    INPUT.lock().unwrap_or_else(|e| e.into_inner()).push_back(Input::ReloadVisuals);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeDrawFrame(
    mut env: JNIEnv<'_>,
    activity: JObject<'_>,
    vsync_frame_ns: jni::sys::jlong,
) {
    ENGINE.with(|holder| {
        // Even a direct JNI draw must not advance or save while recovery failed.
        if holder.borrow().as_ref().is_some_and(|e| !e.gpu.graphics_ready()) { return; }
        let safe = holder.borrow().as_ref().is_some_and(|e|
            may_apply_library_update(e.game.is_some(), e.inserted, e.awaiting_game));
        LIBRARY_UPDATE_SAFE.store(safe, Ordering::Release);
        let current = holder.borrow().as_ref().map(|engine| engine.library_version);
        let updated = {
            let state = LIBRARY.lock().unwrap_or_else(|err| err.into_inner());
            if safe && current.is_some_and(|version| version != state.version) {
                Some((state.version, state.entries.clone()))
            } else {
                None
            }
        };
        if let Some((version, snapshot)) = updated {
            // Saving an in-progress game on library replacement is mandatory.
            if let Some(engine) = holder.borrow_mut().as_mut() {
                if let Some(mut session) = engine.game.take() { session.save(true); }
            }
            PLAYING.store(false, Ordering::Release);
            SAMPLE_RATE.store(0, Ordering::Release);
            // Preserve each system's selected URI before the Library update
            // destroys the old shelf. Cart Studio changes artwork, not focus.
            // On cold Android launch, the engine may be created before the
            // cached SAF library arrives. Do not overwrite the restored
            // on-disk U2 shelf position with that empty placeholder engine.
            let selected_uris = holder.borrow().as_ref()
                .filter(|engine|engine.shelves.iter().any(|s|!s.carts.is_empty()))
                .map(|engine| (
                    engine.active,
                    engine.shelves.iter().map(|shelf|
                        shelf.carts.get(shelf.index).map(|cart| cart.rom.clone())
                    ).collect::<Vec<_>>(),
                ));
            // Destruct old GL resources on the GL thread before recreating them.
            let size = holder.borrow().as_ref().map(|engine| engine.size).unwrap_or((960, 640));
            *holder.borrow_mut() = None;
            match Engine::new(version, snapshot.as_deref()) {
                Ok(mut engine) => {
                    engine.size = size;
                    if let Some((active, uris)) = selected_uris {
                        engine.active = restore_shelf_focus(&mut engine.shelves, active, &uris);
                    }
                    *holder.borrow_mut() = Some(engine);
                }
                Err(error) => eprintln!("slot-konkr: could not reload library: {error}"),
            }
        }
        if let Some(engine) = holder.borrow_mut().as_mut() {
            engine.draw(vsync_frame_ns, || {
                // One callback per committed core frame maximum. Never
                // invoke Java while holding the native PCM queue mutex.
                if env.call_method(&activity, "onNativePcmReady", "()V", &[]).is_err() {
                    let _ = env.exception_clear();
                }
            });
            engine.observe_frame_delay(vsync_frame_ns);
        }
        let now_safe = holder.borrow().as_ref().is_some_and(|e|
            may_apply_library_update(e.game.is_some(), e.inserted, e.awaiting_game));
        LIBRARY_UPDATE_SAFE.store(now_safe, Ordering::Release);
    });
}

/// The UI only stages library updates while the renderer is idle; the
/// renderer itself independently enforces that invariant before rebuilding.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeCanRefreshLibrary(
    _env: *mut c_void, _activity: *mut c_void,
) -> jboolean {
    if LIBRARY_UPDATE_SAFE.load(Ordering::Acquire) { 1 } else { 0 }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeKey(
    _env: *mut c_void,
    _activity: *mut c_void,
    key: i32,
    pressed: u8,
) {
    INPUT
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .push_back(Input::Key(key, pressed != 0));
}

/// Kotlin reports a compact, predefined library status. The renderer alone
/// owns the typography and START keycap; no Android TextView is overlaid.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSetShelfStatus(
    _env: *mut c_void, _activity: *mut c_void, status: i32,
) {
    let safe = if (0..SHELF_STATUS_TEXT.len() as i32).contains(&status) {
        status as u8
    } else { 0 };
    INPUT.lock().unwrap_or_else(|poison| poison.into_inner())
        .push_back(Input::ShelfStatus(safe));
}

/// Kotlin resolves the connected Wi-Fi interface and takes a MulticastLock
/// before handing its IPv4 address to the independent Rust discovery worker.
/// Android's user-visible confirmation for sharing a complete ROM with
/// a trusted KMLB/4 mixed-cartridge peer. AGL never shows dialogs or blocks.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeLinkPeerRomDecision(
    _env: *mut c_void, _activity: *mut c_void, choice: i32,
) {
    INPUT.lock().unwrap_or_else(|e| e.into_inner())
        .push_back(Input::LinkPeerRomConsent(choice));
}

/// Safe short fingerprint derived from an authenticated Ed25519 public key;
/// no peer strings or ROM data reach the Android dialog.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeLinkPeerLabel(
    env: JNIEnv<'_>, _activity: JObject<'_>,
) -> jstring {
    LINK_PEER_LABEL.lock().unwrap_or_else(|e| e.into_inner())
        .as_ref().and_then(|value| env.new_string(value).ok())
        .map_or(std::ptr::null_mut(), |value| value.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeLinkNetworkReady(
    mut env: JNIEnv<'_>, _this: JObject<'_>, ipv4: JString<'_>,
) {
    if let Ok(value) = env.get_string(&ipv4) {
        INPUT.lock().unwrap_or_else(|e| e.into_inner())
            .push_back(Input::LinkNetworkReady(value.to_string_lossy().into_owned()));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeLinkNetworkError(
    mut env: JNIEnv<'_>, _this: JObject<'_>, reason: JString<'_>,
) {
    if let Ok(value) = env.get_string(&reason) {
        INPUT.lock().unwrap_or_else(|e| e.into_inner())
            .push_back(Input::LinkNetworkError(value.to_string_lossy().into_owned()));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeLinkNeedsMulticast(
    _env: *mut c_void, _this: *mut c_void,
) -> jboolean {
    LINK_NEEDS_MULTICAST.load(Ordering::Acquire) as jboolean
}

/// Called on the GL thread through GLSurfaceView.queueEvent before changing
/// account. The evaluator must drain its final queued frames, atomically store
/// all earned awards and confirm none remain only in RAM.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaFlushReady(
    _env: *mut c_void, _activity: *mut c_void,
) -> jboolean {
    ENGINE.with(|holder| {
        let engine = holder.borrow();
        let Some(engine) = engine.as_ref() else { return 1 };
        if engine.game.is_some() || engine.pending_game.is_some() { return 0; }
        match engine.ra_service.as_ref() {
            Some(service) if service.flush_ready() && !service.has_unsaved_awards() => 1,
            Some(_) => 0,
            None => 1,
        }
    })
}

/// Called only through GLSurfaceView.queueEvent AFTER the local flush barrier.
/// The service cannot be dropped until BOTH its evaluator and HTTP worker
/// acknowledge shutdown. Pending/badge/cache files remain untouched.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaStopReady(
    _env: *mut c_void, _activity: *mut c_void,
) -> jboolean {
    ENGINE.with(|holder| {
        let mut guard = holder.borrow_mut();
        let Some(engine) = guard.as_mut() else { return 1 };
        if engine.game.is_some() || engine.pending_game.is_some() { return 0 }
        engine.ra_signing_out = true;
        let ready = engine.ra_service.as_ref()
            .is_none_or(slot_achievements::Service::shutdown_ready);
        if ready {
            engine.ra_service.take();
            1
        } else { 0 }
    })
}

/// Account login or app startup: begin RA cache preparation and pending-award
/// retries even when no GBA cartridge has been inserted.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaAccountConfigured(
    _env: *mut c_void, _activity: *mut c_void,
) {
    INPUT.lock().unwrap_or_else(|e| e.into_inner())
        .push_back(Input::RaAccountConfigured);
}

/// Full uncompressed GBA/GB/GBC ROM MD5 comes from Android SAF off the GL thread.
/// No ROM bytes or credentials are passed via this JNI boundary.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaPrefetchHash(
    mut env: JNIEnv<'_>, _this: JObject<'_>, hash: JString<'_>, console: jint,
) {
    if !matches!(console, 4..=6) { return; }
    let Ok(hash) = env.get_string(&hash) else { return };
    let hash = hash.to_string_lossy().into_owned();
    if hash.len() != 32 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return;
    }
    let mut input = INPUT.lock().unwrap_or_else(|e| e.into_inner());
    // Bound the GL input queue if Android is indexing thousands of files.
    if input.len() < 128 { input.push_back(Input::RaPrefetchHash(hash, console as u32)); }
}

/// Wake the RA HTTP worker on Android network reconnection. Do not perform
/// any HTTP, hashing or blocking operation on this callback thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaNetworkAvailable(
    _env: *mut c_void, _activity: *mut c_void,
) {
    INPUT.lock().unwrap_or_else(|e| e.into_inner())
        .push_back(Input::RaNetworkAvailable);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeExternalReturn(
    _env: *mut c_void, _activity: *mut c_void,
) {
    push(Input::ExternalReturn);
}

/// Choreographer pacing experiment: this is only a render-request policy.
/// The underlying libretro emulation budget remains RESPONSIVE (one frame).
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeWarmRomCandidate(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    WARM_ROM_URI.lock().unwrap_or_else(|e|e.into_inner())
        .clone()
        .and_then(|path|env.new_string(path).ok())
        .map_or(std::ptr::null_mut(),|value|value.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeCarouselQuiet(
    _env: *mut c_void, _activity: *mut c_void,
) -> u8 {
    u8::from(CAROUSEL_QUIET.load(Ordering::Acquire))
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeDisplaySyncEnabled(
    _env: JNIEnv<'_>, _this: JObject<'_>,
) -> jboolean {
    if DISPLAY_SYNC.load(Ordering::Acquire) { 1 } else { 0 }
}

/// Bitmask: bit0 carousel idle, bit1 Wi-Fi, bit2 Bluetooth,
/// bit3 integrations. Never query the GL-only Engine from the UI thread.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeShelfIndicatorFlags(
    _env: *mut c_void, _this: *mut c_void,
) -> jint { SHELF_INDICATOR_FLAGS.load(Ordering::Acquire) }

/// Read-only libretro FPS and speculative timing, irrespective of on-screen FPS settings.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeEmulationDiagnostics(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    EMULATION_DIAGNOSTICS.lock().unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|line| env.new_string(line).ok())
        .map_or(std::ptr::null_mut(), |text| text.into_raw())
}

/// Null when show-FPS is OFF, the game is not running or measurement isn't ready.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePerformanceStats(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    PERF_STATUS.lock().unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|text| env.new_string(text).ok())
        .map_or(std::ptr::null_mut(), |text| text.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeResetInput(
    _env: *mut c_void,
    _activity: *mut c_void,
) {
    let mut input = INPUT.lock().unwrap_or_else(|poison| poison.into_inner());
    input.clear();
    input.push_back(Input::Reset);
}


#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeCoreForUri(
    mut env: JNIEnv<'_>, _this: JObject<'_>, uri: JString<'_>,
) -> jni::sys::jstring {
    let name = env.get_string(&uri).ok()
        .map(|uri| {
            let uri = uri.to_string_lossy().into_owned();
            let launch = LAUNCH_CORE.lock().unwrap_or_else(|e| e.into_inner()).clone();
            if let Some((_, core)) = launch.filter(|(launched, _)| *launched == uri) {
                return core.as_str();
            }
            let paths=PATHS.lock().unwrap_or_else(|e|e.into_inner()).clone();
            paths.map(|(storage,_)| core_selection::selected(&storage, &uri))
                .unwrap_or(Core::Mgba).as_str()
        }).unwrap_or("mgba");
    env.new_string(name).map_or(std::ptr::null_mut(), |s| s.into_raw())
}

/// Readable from the GL renderer without consulting Android preferences.
/// Android persists the opt-in first and then updates this display flag.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSetShareSaves(
    _env: *mut c_void, _this: *mut c_void, enabled: jboolean,
) {
    SHARE_SAVES_ENABLED.store(enabled != 0, Ordering::Release);
}

/// Set only after Android's destination-core Save State dialogue has been
/// answered, and consume exactly once at the matching ROM load.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSetNextResumeMode(
    _env: *mut c_void, _this: *mut c_void, mode: jint,
) {
    NEXT_RESUME_MODE.store(if matches!(mode, -1..=1) {mode} else {-1},
        Ordering::Release);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollSaveFlush(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jni::sys::jstring {
    let item=SAVE_SYNC.lock().unwrap_or_else(|e|e.into_inner()).pop_front();
    match item.and_then(|s|env.new_string(s).ok()) {
        Some(s)=>s.into_raw(),
        None=>std::ptr::null_mut(),
    }
}

// The Activity reads RA messages on its own main thread; it must not
// access the GL-thread-only ENGINE RefCell from Kotlin.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollRaNotice(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    let notice = RA_NOTICES.lock().unwrap_or_else(|e| e.into_inner()).pop_front();
    notice.and_then(|s| env.new_string(s).ok())
        .map_or(std::ptr::null_mut(), |s| s.into_raw())
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaStatus(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    let state = RA_STATUS.lock().unwrap_or_else(|e| e.into_inner()).clone();
    state.and_then(|s| env.new_string(s).ok())
        .map_or(std::ptr::null_mut(), |s| s.into_raw())
}

/// Metadata is delivered from Android through the existing render-thread queue.
/// No JNI thread may create a texture or mutate the GL-owned Engine directly.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeUpdateStatus(
    mut env: JNIEnv<'_>, _this: JObject<'_>,
    status: jint, installed: JString<'_>, available: JString<'_>, automatic: jboolean,
) {
    let Ok(current) = env.get_string(&installed) else {return;};
    let Ok(candidate) = env.get_string(&available) else {return;};
    let current = current.to_string_lossy().into_owned();
    let candidate = candidate.to_string_lossy().into_owned();
    if current.is_empty() || current.len() > 48 || !current.is_ascii() ||
        candidate.len() > 48 || !candidate.is_ascii() {return;}
    push(Input::UpdateStatus(status.clamp(0,5) as u8,current,candidate,automatic != 0));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollUiAction(
    _env: *mut c_void, _this: *mut c_void,
) -> jint {
    UI_ACTION.lock().unwrap_or_else(|e| e.into_inner()).pop_front().unwrap_or(0)
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollRumbleStrength(
    _env: *mut c_void, _this: *mut c_void,
) -> jint {
    RUMBLE_STRENGTH.load(Ordering::Acquire)
}

/// Device sensors as a mask: 1 accelerometer, 2 gyroscope, 4 light.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSensorsAvailable(
    _env: *mut c_void, _this: *mut c_void, mask: jint,
) {
    slot_retro::sensors::set_available(mask as u8);
}

/// Sensors the running game switched on, in the same mask.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSensorsWanted(
    _env: *mut c_void, _this: *mut c_void,
) -> jint {
    jint::from(slot_retro::sensors::wanted())
}

/// One Android SensorEvent, raw: m/s², rad/s or lux in `x`.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSensorEvent(
    _env: *mut c_void, _this: *mut c_void, sensor: jint, x: f32, y: f32, z: f32,
) {
    use slot_retro::sensors;
    match sensor as u8 {
        sensors::ACCELEROMETER => sensors::push_accelerometer(x, y, z),
        sensors::GYROSCOPE => sensors::push_gyroscope(x, y, z),
        sensors::ILLUMINANCE => sensors::push_illuminance(x),
        _ => {}
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollCartSfx(
    _env: *mut c_void, _this: *mut c_void,
) -> jint {
    CART_SFX.lock().unwrap_or_else(|e| e.into_inner()).pop_front().unwrap_or(0)
}

fn push(input: Input) {
    INPUT.lock().unwrap_or_else(|e| e.into_inner()).push_back(input);
}

/// Android provides the local clock and device battery to the original Slot footer.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSystemStatus(
    mut env: JNIEnv<'_>, _this: JObject<'_>, clock: JString<'_>,
    percent: jint, charging: jboolean,
) {
    let Ok(clock) = env.get_string(&clock) else { return; };
    let clock = clock.to_string_lossy().into_owned();
    if clock.len() > 16 || !clock.chars().all(|c| c.is_ascii_digit() || c == ':') { return; }
    let battery = if (0..=100).contains(&percent) {
        Some(Battery { percent: percent as u8,
            charge: if charging != 0 { Charge::Charging } else { Charge::Discharging } })
    } else { None };
    {
        *SYSTEM_STATUS.lock().unwrap_or_else(|p| p.into_inner()) = SystemStatus { clock, battery };
    }
    SYSTEM_STATUS_REV.fetch_add(1, Ordering::Release);
}

/// App-private files directory and nativeLibraryDir (where mGBA is packaged).
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeConfigure(
    mut env: JNIEnv<'_>, _this: JObject<'_>, root: JString<'_>, libraries: JString<'_>,
) {
    if let (Ok(root), Ok(libraries)) = (env.get_string(&root), env.get_string(&libraries)) {
        *PATHS.lock().unwrap_or_else(|e| e.into_inner()) =
            Some((PathBuf::from(root.to_string_lossy().as_ref()), PathBuf::from(libraries.to_string_lossy().as_ref())));
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollLaunchUri(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    let next = REQUEST.lock().unwrap_or_else(|e| e.into_inner()).take();
    match next.and_then(|v| env.new_string(v).ok()) {
        Some(v) => v.into_raw(), None => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeGameReady(
    mut env: JNIEnv<'_>, _this: JObject<'_>, uri: JString<'_>, path: JString<'_>,
) {
    if let (Ok(uri), Ok(path)) = (env.get_string(&uri), env.get_string(&path)) {
        push(Input::GameReady {
            uri: uri.to_string_lossy().into_owned(),
            path: path.to_string_lossy().into_owned(),
        });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeGameError(
    mut env: JNIEnv<'_>, _this: JObject<'_>, uri: JString<'_>, message: JString<'_>,
) {
    if let (Ok(uri), Ok(message)) = (env.get_string(&uri), env.get_string(&message)) {
        push(Input::GameError {
            uri: uri.to_string_lossy().into_owned(),
            message: message.to_string_lossy().into_owned(),
        });
    }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeExitGame(
    _env: *mut c_void, _this: *mut c_void,
) { push(Input::Exit); }

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSuspend(
    _env: *mut c_void, _this: *mut c_void,
) {
    ENGINE.with(|holder| {
        if let Some(engine) = holder.borrow_mut().as_mut() { engine.handle(Input::Suspend); }
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeIsPlaying(
    _env: *mut c_void, _this: *mut c_void,
) -> jboolean { PLAYING.load(Ordering::Acquire) as jboolean }

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeAudioSpeedPermille(
    _env: *mut c_void, _this: *mut c_void,
) -> jint { AUDIO_SPEED_PERMILLE.load(Ordering::Acquire) }

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeFastAudioSupported(
    _env: *mut c_void, _this: *mut c_void, supported: jboolean,
) {
    FF_AUDIO_SUPPORTED.store(supported != 0, Ordering::Release);
    if supported == 0 { AUDIO.lock().unwrap_or_else(|e| e.into_inner()).clear(); }
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSetMgbaLinkRecoveryEnabled(
    _env: *mut c_void, _this: *mut c_void, enabled: jboolean,
) {
    MGBA_CABLE_RECOVERY.store(enabled != 0, Ordering::Release);
    crate::game::log_launch_timing(if enabled != 0 {
        "A12 Link recovery setting: a11"
    } else {
        "A12 Link recovery setting: legacy"
    });
}

/// Diagnostic-only cumulative counters, in this stable order: PCM FIFO
/// overflow events and discarded-old PCM shorts. The GL thread owns
/// libretro; the Android audio thread only reads atomics.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeAudioClockCounters(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jlongArray {
    let values: [i64; 2] = [
        AUDIO_FIFO_OVERFLOW_EVENTS.load(Ordering::Relaxed) as i64,
        AUDIO_FIFO_DROPPED_SHORTS.load(Ordering::Relaxed) as i64,
    ];
    let Ok(result) = env.new_long_array(values.len() as i32) else {
        return std::ptr::null_mut();
    };
    let _ = env.set_long_array_region(&result, 0, &values);
    result.into_raw()
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeAudioQueuedShorts(
    _env: *mut c_void, _this: *mut c_void,
) -> jint {
    AUDIO.lock().unwrap_or_else(|e| e.into_inner())
        .len().min(i32::MAX as usize) as jint
}

/// RETROARCH pacing: the panel refresh rate, in mHz (0 = unknown).
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSetDisplayRefresh(
    _env: *mut c_void, _this: *mut c_void, millihz: jint,
) { DISPLAY_REFRESH_MILLIHZ.store(millihz.max(0), Ordering::Release); }

/// Frame Pacing NATIVE: the running solo game wants the swap-paced loop.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeRaSwapLoop(
    _env: *mut c_void, _this: *mut c_void,
) -> jboolean { if RA_SWAP_LOOP.load(Ordering::Acquire) { 1 } else { 0 } }

/// RETROARCH pacing: AudioTrack frames waiting to play and the size of its
/// buffer, sampled by the audio worker before each write. Read-only for
/// every other mode.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSetAudioOutputDepth(
    _env: *mut c_void, _this: *mut c_void, queued_frames: jint, capacity_frames: jint,
) {
    AUDIO_OUT_QUEUED_FRAMES.store(queued_frames.max(0), Ordering::Release);
    AUDIO_OUT_CAPACITY_FRAMES.store(capacity_frames.max(0), Ordering::Release);
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeAudioSampleRate(
    _env: *mut c_void, _this: *mut c_void,
) -> jint { SAMPLE_RATE.load(Ordering::Acquire) }

/// F9: Kotlin owns a single reused PCM16 buffer for the lifetime of each
/// AudioTrack worker. Copy <=4096 interleaved shorts from the FIFO into a
/// fixed stack array, release AUDIO's mutex, then fill that same Java array.
/// Returning the valid count avoids replaying the unused tail of a short read.
/// No Vec/ShortArray allocation occurs on the normal per-chunk JNI path.
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeReadAudioInto(
    env: JNIEnv<'_>, _this: JObject<'_>, out: JShortArray<'_>,
) -> jint {
    let capacity = match env.get_array_length(&out) {
        Ok(length) if length >= 0 => length as usize,
        _ => return -1,
    };
    let mut chunk = [0i16; crate::audio_pcm::PCM_READ_MAX_SHORTS];
    let count = {
        let mut audio = AUDIO.lock().unwrap_or_else(|e| e.into_inner());
        let available = capacity.min(chunk.len());
        crate::audio_pcm::drain_pcm_into(&mut audio, &mut chunk[..available])
    };
    if count > 0 && env.set_short_array_region(&out, 0, &chunk[..count]).is_err() {
        return -1;
    }
    count as jint
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativePollMessage(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    match MESSAGE.lock().unwrap_or_else(|e| e.into_inner()).take().and_then(|v| env.new_string(v).ok()) {
        Some(v) => v.into_raw(), None => std::ptr::null_mut(),
    }
}

/// Real-context fixture used by the session tests. Runs the production JNI
/// surface-created callback, including a failed reconstruction and retry.
#[cfg(all(test, target_os = "linux"))]
pub(crate) fn exercise_session_graphics_recovery(game: GameSession) -> GameSession {
    let surface = slot_gfx::TestEgl::new();
    let mut engine = Engine::new(0, None).unwrap();
    engine.game = Some(game);
    engine.inserted = true;
    engine.game_accum = 42.0;
    let session_address = engine.game.as_ref().unwrap().frame().as_ptr();
    let retained = engine.gpu.recovery_bytes();
    assert!(retained > 0);
    eprintln!("bundled UI recovery pixels: {retained} bytes");
    ENGINE.with(|holder| *holder.borrow_mut() = Some(engine));
    Java_fyi_slot_konkr_MainActivity_nativeGraphicsContextLost(std::ptr::null_mut(), std::ptr::null_mut());
    assert_eq!(Java_fyi_slot_konkr_MainActivity_nativeGraphicsReady(std::ptr::null_mut(), std::ptr::null_mut()), 0);
    drop(surface);
    let replacement = slot_gfx::TestEgl::new();
    let enable: unsafe extern "system" fn(u32) = unsafe {
        std::mem::transmute(replacement.proc_address("glEnable"))
    };
    unsafe { enable(0xffff_ffff) };
    let create = || Java_fyi_slot_konkr_MainActivity_nativeSurfaceCreated(
        std::ptr::null_mut(), std::ptr::null_mut());
    assert_eq!(create(), 0);
    ENGINE.with(|holder| {
        let mut guard = holder.borrow_mut();
        let e = guard.as_mut().unwrap();
        assert!(!e.gpu.graphics_ready());
        e.draw(0, || panic!("audio must not advance during failed recovery"));
        assert_eq!(e.game_accum, 42.0);
        assert_eq!(e.game.as_ref().unwrap().frame().as_ptr(), session_address);
    });
    assert_eq!(create(), 1);
    Java_fyi_slot_konkr_MainActivity_nativePresentationResume(std::ptr::null_mut(), std::ptr::null_mut());
    let game = ENGINE.with(|holder| {
        let mut guard = holder.borrow_mut();
        let engine = guard.as_mut().unwrap();
        assert!(engine.gpu.graphics_ready());
        assert_eq!(engine.game_accum, 0.0);
        assert_eq!(engine.gpu.recovery_bytes(), retained);
        assert_eq!(engine.game.as_ref().unwrap().frame().as_ptr(), session_address);
        engine.game.take().unwrap()
    });
    Java_fyi_slot_konkr_MainActivity_nativeRendererShutdown(std::ptr::null_mut(), std::ptr::null_mut(), 1);
    assert_eq!(Java_fyi_slot_konkr_MainActivity_nativeGraphicsReady(std::ptr::null_mut(), std::ptr::null_mut()), 0);
    game
}
