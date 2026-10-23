//! Linux-only real EGL fixture for lifecycle regression tests. No window server.
use crate::{GfxError, Surface};
use libloading::Library;
use std::ffi::{c_char, c_void, CString};
type Ptr = *mut c_void;

pub struct TestEgl {
    lib: Library,
    display: Ptr,
    surface: Ptr,
    context: Ptr,
}
impl TestEgl {
    pub fn new() -> Self {
        unsafe {
            let lib = Library::new("libEGL.so.1").expect("Mesa EGL must be installed");
            let platform = lib
                .get::<unsafe extern "C" fn(u32, Ptr, *const isize) -> Ptr>(
                    b"eglGetPlatformDisplay\0",
                )
                .unwrap();
            let display = platform(0x31dd, std::ptr::null_mut(), std::ptr::null());
            let initialize = lib
                .get::<unsafe extern "C" fn(Ptr, *mut i32, *mut i32) -> u32>(b"eglInitialize\0")
                .unwrap();
            assert_ne!(
                initialize(display, std::ptr::null_mut(), std::ptr::null_mut()),
                0
            );
            let bind = lib
                .get::<unsafe extern "C" fn(u32) -> u32>(b"eglBindAPI\0")
                .unwrap();
            assert_ne!(bind(0x30a2), 0); // Desktop GL matches Linux compositor upload format.
            let choose = lib
                .get::<unsafe extern "C" fn(Ptr, *const i32, *mut Ptr, i32, *mut i32) -> u32>(
                    b"eglChooseConfig\0",
                )
                .unwrap();
            let attrs = [
                0x3033, 1, 0x3040, 8, 0x3024, 8, 0x3023, 8, 0x3022, 8, 0x3021, 8, 0x3038,
            ];
            let mut config = std::ptr::null_mut();
            let mut count = 0;
            assert_ne!(
                choose(display, attrs.as_ptr(), &mut config, 1, &mut count),
                0
            );
            assert!(count > 0);
            let create = lib
                .get::<unsafe extern "C" fn(Ptr, Ptr, Ptr, *const i32) -> Ptr>(
                    b"eglCreateContext\0",
                )
                .unwrap();
            let context = create(
                display,
                config,
                std::ptr::null_mut(),
                [0x3098, 3, 0x30fb, 3, 0x3038].as_ptr(),
            );
            assert!(!context.is_null());
            let pbuffer = lib
                .get::<unsafe extern "C" fn(Ptr, Ptr, *const i32) -> Ptr>(
                    b"eglCreatePbufferSurface\0",
                )
                .unwrap();
            let surface = pbuffer(display, config, [0x3057, 960, 0x3056, 640, 0x3038].as_ptr());
            assert!(!surface.is_null());
            let mut result = Self {
                lib,
                display,
                surface,
                context,
            };
            result.make_current().unwrap();
            result
        }
    }
}
impl Surface for TestEgl {
    fn make_current(&mut self) -> Result<(), GfxError> {
        let ok = unsafe {
            self.lib
                .get::<unsafe extern "C" fn(Ptr, Ptr, Ptr, Ptr) -> u32>(b"eglMakeCurrent\0")
                .unwrap()(self.display, self.surface, self.surface, self.context)
        };
        if ok != 0 {
            Ok(())
        } else {
            Err(GfxError::Context("test make-current failed".into()))
        }
    }
    fn window_size(&self) -> (u32, u32) {
        (960, 640)
    }
    fn swap(&mut self) -> Result<(), GfxError> {
        Ok(())
    }
    fn proc_address(&self, name: &str) -> *const c_void {
        let name = CString::new(name).unwrap();
        unsafe {
            self.lib
                .get::<unsafe extern "C" fn(*const c_char) -> *const c_void>(b"eglGetProcAddress\0")
                .unwrap()(name.as_ptr())
        }
    }
}
impl Drop for TestEgl {
    fn drop(&mut self) {
        unsafe {
            self.lib
                .get::<unsafe extern "C" fn(Ptr, Ptr, Ptr, Ptr) -> u32>(b"eglMakeCurrent\0")
                .unwrap()(
                self.display,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
            self.lib
                .get::<unsafe extern "C" fn(Ptr, Ptr) -> u32>(b"eglDestroyContext\0")
                .unwrap()(self.display, self.context);
            self.lib
                .get::<unsafe extern "C" fn(Ptr, Ptr) -> u32>(b"eglDestroySurface\0")
                .unwrap()(self.display, self.surface);
            self.lib
                .get::<unsafe extern "C" fn(Ptr) -> u32>(b"eglTerminate\0")
                .unwrap()(self.display);
        }
    }
}
