use std::ffi::c_void;
use std::fmt;
use std::sync::atomic::{AtomicU32, Ordering};

/// Slot's 3:2 canvas width, kept wherever the panel is 3:2 (the KONKR).
pub const BASE_W: u32 = 720;
pub const OUT_H: u32 = 480;

static CANVAS_W: AtomicU32 = AtomicU32::new(BASE_W);

/// Logical canvas width. The canvas is always 480 high; its width follows
/// the panel's shape so the interface fills any screen without bands.
pub fn out_w() -> u32 {
    CANVAS_W.load(Ordering::Relaxed)
}

/// Canvas width for a landscape panel: 480 high, same shape as the panel.
/// Even, and never narrower than a square or wider than 21:9.
pub fn canvas_w_for(panel: (u32, u32)) -> u32 {
    let (w, h) = (panel.0.max(panel.1), panel.0.min(panel.1));
    if w == 0 || h == 0 {
        return BASE_W;
    }
    let exact = OUT_H as f64 * w as f64 / h as f64;
    let even = ((exact / 2.0).round() * 2.0) as u32;
    even.clamp(OUT_H, OUT_H * 21 / 9)
}

/// Fit the canvas to `panel`. Call once, before the first face or texture.
pub fn set_canvas_for_panel(panel: (u32, u32)) -> u32 {
    let w = canvas_w_for(panel);
    CANVAS_W.store(w, Ordering::Relaxed);
    w
}

#[derive(Debug)]
pub enum GfxError {
    Context(String),
    Shader(String),
    Framebuffer(u32),
}

impl fmt::Display for GfxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GfxError::Context(m) => write!(f, "gl context: {m}"),
            GfxError::Shader(m) => write!(f, "shader: {m}"),
            GfxError::Framebuffer(s) => write!(f, "framebuffer incomplete: 0x{s:x}"),
        }
    }
}

impl std::error::Error for GfxError {}

pub trait Surface {
    fn make_current(&mut self) -> Result<(), GfxError>;
    fn window_size(&self) -> (u32, u32);
    fn swap(&mut self) -> Result<(), GfxError>;
    fn proc_address(&self, name: &str) -> *const c_void;
}

pub fn fit_scale(win_w: u32, win_h: u32) -> u32 {
    (win_w / out_w()).min(win_h / OUT_H).max(1)
}

pub fn fit_rect(win_w: u32, win_h: u32) -> (i32, i32, i32, i32) {
    let s = fit_scale(win_w, win_h) as i32;
    let (w, h) = (out_w() as i32 * s, OUT_H as i32 * s);
    ((win_w as i32 - w) / 2, (win_h as i32 - h) / 2, w, h)
}

pub fn blit_rect_fit(panel: (u32, u32), shake: f32) -> (i32, i32, i32, i32) {
    let scale = (panel.0 as f32 / out_w() as f32).min(panel.1 as f32 / OUT_H as f32);
    let w = (out_w() as f32 * scale).round() as i32;
    let h = (OUT_H as f32 * scale).round() as i32;
    let dx = (shake * scale).round() as i32;
    (
        (panel.0 as i32 - w) / 2 + dx,
        (panel.1 as i32 - h) / 2,
        w,
        h,
    )
}

pub fn blit_rect(window: (u32, u32), shake: f32) -> (i32, i32, i32, i32) {
    if crate::bezel::framed(window) {
        let (x, y, w, h) = crate::bezel::screen_rect(window);
        let dx = shake * w as f32 / out_w() as f32;
        return (x + dx.round() as i32, window.1 as i32 - y - h, w, h);
    }
    // A canvas fitted to the panel (set_canvas_for_panel) fills it edge to
    // edge; the even-width rounding is far below a pixel's stretch.
    let canvas = out_w() as f32 / OUT_H as f32;
    let panel = window.0 as f32 / window.1.max(1) as f32;
    if out_w() != BASE_W && (panel / canvas - 1.0).abs() < 0.01 {
        let dx = shake * window.1 as f32 / OUT_H as f32;
        return (dx.round() as i32, 0, window.0 as i32, window.1 as i32);
    }
    // Fractional 3:2 scaling fills KONKR's 960x640 panel instead of
    // leaving a 720x480 1x viewport. Preserve behavior on other ratios.
    if window.0.saturating_mul(OUT_H) == window.1.saturating_mul(out_w())
        || window.0 < out_w()
        || window.1 < OUT_H
    {
        return blit_rect_fit(window, shake);
    }
    let (x, y, w, h) = fit_rect(window.0, window.1);
    let dx = shake * fit_scale(window.0, window.1) as f32;
    (x + dx.round() as i32, y, w, h)
}

#[cfg(test)]
mod canvas_tests {
    use super::canvas_w_for;

    #[test]
    fn the_canvas_takes_each_panel_shape_and_stays_720_on_the_konkr() {
        assert_eq!(canvas_w_for((960, 640)), 720);
        assert_eq!(canvas_w_for((640, 960)), 720, "portrait boot still landscape");
        assert_eq!(canvas_w_for((1920, 1080)), 854);
        assert_eq!(canvas_w_for((1280, 960)), 640);
        assert_eq!(canvas_w_for((1240, 1080)), 552);
        assert_eq!(canvas_w_for((720, 720)), 480);
        assert_eq!(canvas_w_for((1920, 1200)), 768);
        assert_eq!(canvas_w_for((3200, 1080)), 1120, "capped at 21:9");
        assert_eq!(canvas_w_for((0, 0)), 720);
    }
}
