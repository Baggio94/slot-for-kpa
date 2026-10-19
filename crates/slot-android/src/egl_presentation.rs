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
use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CStr, CString};
use std::time::Instant;
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
const EGL_DISPLAY_PRESENT_TIME_ANDROID: i32 = 0x343A;
const EGL_TIMESTAMP_PENDING_ANDROID: i64 = -2;
const EGL_TIMESTAMP_INVALID_ANDROID: i64 = -1;
const MAX_PENDING: usize = 12;
// Nominal KONKR 60Hz gap is ~16,667us; 25ms indicates missed cadence.
const MAX_VALID_GAP_NS: i64 = 1_000_000_000;

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
struct Probe {
    extension: Option<Ext>,
    tried: bool,
    status: &'static str,
    pending: VecDeque<(u64, u64)>, // EGL frame ID, tick at capture
    ticks: u64,
    present_count: u64,
    last_present_ns: i64,
    gap_count: u64,
    gap_sum_us: u64,
    gap_max_us: u64,
    gap_over25: u64,
    gap_over35: u64,
    // Bounded histogram of actual display-present intervals: 1ms buckets.
    gap_bins: [u32; 51],
    pending_lost: u64,
    query_errors: u64,
    query_time_us: u64,
    last_report: Instant,
}

impl Probe {
    fn new() -> Self {
        Self {
            extension: None, tried: false, status: "pending",
            pending: VecDeque::with_capacity(MAX_PENDING),
            ticks: 0, present_count: 0, last_present_ns: 0,
            gap_count: 0, gap_sum_us: 0, gap_max_us: 0,
            gap_over25: 0, gap_over35: 0, gap_bins: [0; 51],
            pending_lost: 0, query_errors: 0, query_time_us: 0,
            last_report: Instant::now(),
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
        self.extension = Some(ext);
        self.status = "supported";
    }
    fn tick(&mut self) {
        self.init();
        let Some(ext) = self.extension else { return };
        // GLSurfaceView can recreate its EGL window surface independently
        // of our renderer. Never query IDs against a stale EGLSurface.
        if unsafe { eglGetCurrentSurface(EGL_DRAW) } != ext.surface {
            *self = Self::new();
            self.init();
            return;
        }
        self.ticks += 1;
        let started = Instant::now();
        // Read at most 2 pending timestamps per callback. A frame must have
        // passed through >=2 swaps before we query it, avoiding GPU waits.
        for _ in 0..2 {
            let Some(&(id, tick)) = self.pending.front() else { break };
            if self.ticks.saturating_sub(tick) < 3 { break; }
            let names = [EGL_DISPLAY_PRESENT_TIME_ANDROID];
            let mut result = [EGL_TIMESTAMP_PENDING_ANDROID];
            let ok = unsafe {
                (ext.timestamps)(ext.display, ext.surface, id, 1,
                    names.as_ptr(), result.as_mut_ptr())
            };
            if ok == 0 {
                self.query_errors += 1;
                self.pending.pop_front();
                continue;
            }
            if result[0] == EGL_TIMESTAMP_PENDING_ANDROID {
                if self.ticks.saturating_sub(tick) <= 9 { break; }
                self.pending_lost += 1;
                self.pending.pop_front();
                continue;
            }
            self.pending.pop_front();
            if result[0] == EGL_TIMESTAMP_INVALID_ANDROID || result[0] <= 0 {
                self.pending_lost += 1;
                continue;
            }
            self.present_count += 1;
            let time = result[0];
            if self.last_present_ns > 0 {
                let delta = time - self.last_present_ns;
                if delta > 0 && delta <= MAX_VALID_GAP_NS {
                    let us = (delta / 1000) as u64;
                    self.gap_count += 1;
                    self.gap_sum_us += us;
                    self.gap_max_us = self.gap_max_us.max(us);
                    if us > 25_000 { self.gap_over25 += 1; }
                    if us > 35_000 { self.gap_over35 += 1; }
                    let bucket = (us / 1000).min(50) as usize;
                    self.gap_bins[bucket] += 1;
                }
            }
            self.last_present_ns = time;
        }
        while self.pending.len() >= MAX_PENDING {
            self.pending.pop_front();
            self.pending_lost += 1;
        }
        let mut next_frame: u64 = 0;
        if unsafe { (ext.next_id)(ext.display, ext.surface, &mut next_frame) } != 0 {
            self.pending.push_back((next_frame, self.ticks));
        } else {
            self.query_errors += 1;
        }
        self.query_time_us += started.elapsed().as_micros() as u64;
    }
    fn report(&mut self) -> String {
        let p95 = if self.gap_count == 0 { 0 } else {
            let goal = self.gap_count.saturating_mul(95).div_ceil(100);
            let mut sum = 0u64;
            self.gap_bins.iter().enumerate().find_map(|(i, &n)| {
                sum += u64::from(n);
                (sum >= goal).then_some(i as u64 * 1000)
            }).unwrap_or(0)
        };
        let span = self.last_report.elapsed().as_secs_f64();
        let result = format!(
            "EGL presentation: status={} callbacks={} present_samples={} present_fps={:.2} \
             present_gap_mean_us={} present_gap_p95_us={} present_gap_max_us={} \
             present_over25={} present_over35={} lost={} errors={} \
             probe_us_per_callback={:.2}",
            self.status, self.ticks, self.present_count,
            if span > 0.0 { self.present_count as f64 / span } else { 0.0 },
            if self.gap_count > 0 { self.gap_sum_us / self.gap_count } else { 0 },
            p95, self.gap_max_us, self.gap_over25, self.gap_over35,
            self.pending_lost, self.query_errors,
            if self.ticks > 0 { self.query_time_us as f64 / self.ticks as f64 } else { 0.0 },
        );
        self.ticks = 0;
        self.present_count = 0;
        self.gap_count = 0;
        self.gap_sum_us = 0;
        self.gap_max_us = 0;
        self.gap_over25 = 0;
        self.gap_over35 = 0;
        self.gap_bins = [0; 51];
        self.pending_lost = 0;
        self.query_errors = 0;
        self.query_time_us = 0;
        self.last_report = Instant::now();
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
    _env: *mut c_void, _this: *mut c_void,
) {
    EGL_PROBE.with(|p| p.borrow_mut().tick());
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
