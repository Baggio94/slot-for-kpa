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
impl Source for EglSource {
    fn timestamp(&mut self, id: u64) -> Result<Timestamp, ()> {
        let ext = self.0;
        let name = EGL_DISPLAY_PRESENT_TIME_ANDROID;
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
}

impl Probe {
    fn new() -> Self {
        Self {
            extension: None, tried: false, status: "pending",
            samples: PresentationProbe::new(),
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
    fn report(&mut self) -> String {
        self.samples.report(self.status)
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
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeEglPresentationReport(
    env: JNIEnv<'_>, _this: JObject<'_>,
) -> jstring {
    EGL_PROBE.with(|p| {
        let report = p.borrow_mut().report();
        env.new_string(report).ok().map_or(std::ptr::null_mut(), |s| s.into_raw())
    })
}
