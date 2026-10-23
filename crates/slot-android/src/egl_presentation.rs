//! Beta9 read-only EGL frame presentation probe.
//!
//! GLSurfaceView owns EGL and calls eglSwapBuffers AFTER Renderer.onDrawFrame.
//! We capture the next EGL frame ID just before that swap and query its
//! actual display-present timestamp on later render callbacks. The optional
//! EGL_ANDROID_get_frame_timestamps extension is device/driver dependent.
//! Unsupported or invalid timestamp responses are reported, never fatal.
//! No threads, emulator steps, audio buffers, video presentation hints,
//! renderer locks or EGL swap calls are added by this module.
#![cfg(target_os = "android")]

use std::cell::RefCell;
use crate::presentation_probe::{PresentationProbe, Source, Timestamp};
use std::ffi::{c_char, c_void, CStr, CString};
use jni::{JNIEnv, objects::JObject, sys::jstring};

type EGLDisplay = *mut c_void;
type EGLSurface = *mut c_void;
type EGLBoolean = u32;
type NextFrameId = unsafe extern "C" fn(EGLDisplay, EGLSurface, *mut u64) -> EGLBoolean;
type IsTimestampSupported = unsafe extern "C" fn(EGLDisplay, EGLSurface, i32) -> EGLBoolean;
type FrameTimestamps = unsafe extern "C" fn(
    EGLDisplay, EGLSurface, u64, i32, *const i32, *mut i64,
) -> EGLBoolean;

const EGL_DRAW: i32 = 0x3059;
const EGL_EXTENSIONS: i32 = 0x3055;
const EGL_TIMESTAMPS_ANDROID: i32 = 0x3430;
const EGL_COMPOSITION_LATCH_TIME_ANDROID: i32 = 0x3436;
const EGL_DISPLAY_PRESENT_TIME_ANDROID: i32 = 0x343A;
const EGL_TIMESTAMP_PENDING_ANDROID: i64 = -2;
const EGL_TIMESTAMP_INVALID_ANDROID: i64 = -1;

#[link(name = "EGL")]
unsafe extern "C" {
    fn eglGetCurrentDisplay() -> EGLDisplay;
    fn eglGetCurrentSurface(kind: i32) -> EGLSurface;
    fn eglQueryString(display: EGLDisplay, name: i32) -> *const c_char;
    fn eglSurfaceAttrib(display: EGLDisplay, surface: EGLSurface, attr: i32, value: i32) -> EGLBoolean;
    fn eglGetProcAddress(name: *const c_char) -> *const c_void;
}

#[derive(Copy, Clone)]
struct Ext {
    display: EGLDisplay,
    surface: EGLSurface,
    next_id: NextFrameId,
    supported: IsTimestampSupported,
    timestamps: FrameTimestamps,
}
struct EglSource(Ext);
impl EglSource {
    fn query(&self, id: u64, name: i32) -> Result<Timestamp, ()> {
        let ext = self.0;
        let mut time = EGL_TIMESTAMP_PENDING_ANDROID;
        if unsafe { (ext.timestamps)(ext.display, ext.surface, id, 1, &name, &mut time) } == 0 {
            return Err(());
        }
        Ok(match time {
            EGL_TIMESTAMP_PENDING_ANDROID => Timestamp::Pending,
            EGL_TIMESTAMP_INVALID_ANDROID => Timestamp::Invalid,
            time => Timestamp::Presented(time),
        })
    }
}
impl Source for EglSource {
    fn timestamp(&mut self, id: u64) -> Result<Timestamp, ()> {
        self.query(id, EGL_DISPLAY_PRESENT_TIME_ANDROID)
    }
    fn next_id(&mut self) -> Result<u64, ()> {
        let ext = self.0;
        let mut id = 0;
        if unsafe { (ext.next_id)(ext.display, ext.surface, &mut id) } == 0 {
            Err(())
        } else { Ok(id) }
    }
}
struct Probe {
    extension: Option<Ext>,
    tried: bool,
    status: &'static str,
    samples: PresentationProbe,
    latch: LatchWait,
}

/// Keeps the queue in front of the display short. Without it the blocking
/// swap lets several finished frames queue, each one adding a refresh
/// between the buttons being read and the picture being shown (~110 ms
/// measured on the KONKR). Android reports a latch ~10 ms after it happens,
/// so waiting on the frame just swapped left too little time and halved the
/// frame rate (beta12); waiting on the one before keeps 60 fps.
#[derive(Default)]
struct LatchWait {
    supported: bool,
    disabled: bool,
    window_waits: u32,
    window_timeouts: u32,
    waits: u64,
    wait_sum_us: u64,
    wait_max_us: u64,
    timeouts: u64,
}

impl LatchWait {
    fn end_of_wait(&mut self) {
        self.window_waits += 1;
        if self.window_waits >= LATCH_WINDOW {
            self.window_waits = 0;
            self.window_timeouts = 0;
        }
    }
}

/// Wait for the frame swapped this many callbacks before the latest one.
const LATCH_BACK: u64 = 1;
/// Longer than one 60 Hz refresh: past it the compositor has skipped us.
const LATCH_TIMEOUT_NS: i64 = 20_000_000;
/// More than this many timeouts in a window of waits means the wait costs
/// frames: give up for this surface and keep the plain swap-paced loop.
const LATCH_WINDOW: u32 = 120;
const LATCH_GIVE_UP: u32 = 4;

impl Probe {
    fn new() -> Self {
        Self {
            extension: None, tried: false, status: "pending",
            samples: PresentationProbe::new(),
            latch: LatchWait::default(),
        }
    }
    fn init(&mut self) {
        if self.tried { return; }
        self.tried = true;
        // Current only on the GLSurfaceView render thread.
        let display = unsafe { eglGetCurrentDisplay() };
        let surface = unsafe { eglGetCurrentSurface(EGL_DRAW) };
        if display.is_null() || surface.is_null() {
            self.status = "no_current_egl_surface";
            return;
        }
        let extensions = unsafe { eglQueryString(display, EGL_EXTENSIONS) };
        if extensions.is_null() {
            self.status = "no_egl_extensions";
            return;
        }
        let supported = unsafe { CStr::from_ptr(extensions) }
            .to_string_lossy()
            .split_ascii_whitespace()
            .any(|s| s == "EGL_ANDROID_get_frame_timestamps");
        if !supported {
            self.status = "unsupported_extension";
            return;
        }
        // Drivers advertising the extension may lack one entrypoint.
        unsafe fn proc(name: &str) -> *const c_void {
            CString::new(name).ok().map_or(std::ptr::null(), |str_name| unsafe {
                eglGetProcAddress(str_name.as_ptr())
            })
        }
        let next = unsafe { proc("eglGetNextFrameIdANDROID") };
        let yes = unsafe { proc("eglGetFrameTimestampSupportedANDROID") };
        let timestamps = unsafe { proc("eglGetFrameTimestampsANDROID") };
        if next.is_null() || yes.is_null() || timestamps.is_null() {
            self.status = "missing_entrypoints";
            return;
        }
        // SAFETY: entrypoint signatures come from EGL_ANDROID_get_frame_timestamps,
        // and the addresses are used only with a live, current EGLSurface.
        let ext = unsafe { Ext {
            display, surface,
            next_id: std::mem::transmute::<*const c_void, NextFrameId>(next),
            supported: std::mem::transmute::<*const c_void, IsTimestampSupported>(yes),
            timestamps: std::mem::transmute::<*const c_void, FrameTimestamps>(timestamps),
        }};
        if unsafe { (ext.supported)(display, surface, EGL_DISPLAY_PRESENT_TIME_ANDROID) } == 0 {
            self.status = "present_timestamp_unsupported";
            return;
        }
        if unsafe { eglSurfaceAttrib(display, surface, EGL_TIMESTAMPS_ANDROID, 1) } == 0 {
            self.status = "timestamps_enable_failed";
            return;
        }
        self.latch.supported = unsafe {
            (ext.supported)(display, surface, EGL_COMPOSITION_LATCH_TIME_ANDROID)
        } != 0;
        self.extension = Some(ext);
        self.status = "supported";
    }
    fn tick(&mut self, input_ns: i64) {
        self.init();
        let Some(ext) = self.extension else { return };
        // GLSurfaceView can recreate its EGL window surface independently
        // of our renderer. Never query IDs against a stale EGLSurface.
        if unsafe { eglGetCurrentDisplay() } != ext.display
            || unsafe { eglGetCurrentSurface(EGL_DRAW) } != ext.surface {
            *self = Self::new();
            self.init();
            return;
        }
        self.samples.tick_with_input(&mut EglSource(ext), input_ns);
    }
    /// Waits until the compositor has taken the frame swapped by the last
    /// callback, so the next one reads the buttons as late as possible.
    fn wait_for_latch(&mut self) {
        let Some(ext) = self.extension else { return };
        if !self.latch.supported || self.latch.disabled { return; }
        if unsafe { eglGetCurrentDisplay() } != ext.display
            || unsafe { eglGetCurrentSurface(EGL_DRAW) } != ext.surface { return; }
        let Some(id) = self.samples.frame_id_back(LATCH_BACK) else { return };
        let source = EglSource(ext);
        let started = crate::ra_sync::monotonic_ns();
        loop {
            match source.query(id, EGL_COMPOSITION_LATCH_TIME_ANDROID) {
                Ok(Timestamp::Pending) => {}
                Ok(Timestamp::Presented(_)) => break,
                // Dropped or unknown frame: nothing to wait for.
                Ok(Timestamp::Invalid) | Err(()) => return,
            }
            if crate::ra_sync::monotonic_ns() - started >= LATCH_TIMEOUT_NS {
                self.latch.timeouts += 1;
                self.latch.window_timeouts += 1;
                if self.latch.window_timeouts > LATCH_GIVE_UP {
                    self.latch.disabled = true;
                }
                self.latch.end_of_wait();
                return;
            }
            std::thread::sleep(std::time::Duration::from_micros(250));
        }
        let us = ((crate::ra_sync::monotonic_ns() - started) / 1000).max(0) as u64;
        self.latch.end_of_wait();
        self.latch.waits += 1;
        self.latch.wait_sum_us += us;
        self.latch.wait_max_us = self.latch.wait_max_us.max(us);
    }
    fn report(&mut self) -> String {
        let latch = &mut self.latch;
        let state = if !latch.supported { "unsupported" }
            else if latch.disabled { "gave_up" } else { "on" };
        let result = format!("{} latch_wait={} latch_waits={} latch_wait_mean_us={} \
             latch_wait_max_us={} latch_timeouts={}",
            self.samples.report(self.status), state, latch.waits,
            if latch.waits > 0 { (latch.wait_sum_us / latch.waits) as i64 } else { -1 },
            latch.wait_max_us, latch.timeouts);
        latch.waits = 0;
        latch.wait_sum_us = 0;
        latch.wait_max_us = 0;
        latch.timeouts = 0;
        result
    }
}

thread_local! {
    static EGL_PROBE: RefCell<Probe> = RefCell::new(Probe::new());
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeEglPresentationReset(
    _env: *mut c_void, _this: *mut c_void,
) {
    EGL_PROBE.with(|p| *p.borrow_mut() = Probe::new());
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeEglPresentationTick(
    _env: *mut c_void, _this: *mut c_void, input_ns: i64,
) {
    EGL_PROBE.with(|p| p.borrow_mut().tick(input_ns));
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeEglWaitForLatch(
    _env: *mut c_void, _this: *mut c_void,
) {
    EGL_PROBE.with(|p| p.borrow_mut().wait_for_latch());
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeEglPresentationReport(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    EGL_PROBE.with(|p| {
        let report = p.borrow_mut().report();
        env.new_string(report).ok().map_or(std::ptr::null_mut(), |s| s.into_raw())
    })
}
