use std::path::Path;
use std::sync::OnceLock;

use slot_gfx::{Draw, TexId, OUT_H, OUT_W};

use crate::art;

const SCRIM: f32 = 0.62;

// Directly use the bundled v1.7.0 wallpaper, not a recreated colour gradient.
// A user-selected PNG still takes priority via wallpaper_face().
const DEFAULT_PNG: &[u8] = include_bytes!("../../../assets/background_default.png");
const BLUR_PNG: &[u8] = include_bytes!("../../../assets/background_default_blur.png");

pub fn default_background() -> Option<&'static [u8]> {
    static CACHED: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    CACHED.get_or_init(|| art::cover_bytes(DEFAULT_PNG, OUT_W, OUT_H)).as_deref()
}

pub fn blur_background() -> Option<&'static [u8]> {
    static CACHED: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    CACHED.get_or_init(|| art::cover_bytes(BLUR_PNG, OUT_W, OUT_H)).as_deref()
}

/// Preserve native pixels from custom HD wallpapers: decode directly to
/// 960x640 instead of downsampling to 720x480 and scaling a second time.
pub fn wallpaper_face_native(path: &Path) -> Option<Vec<u8>> {
    art::cover(path, crate::NATIVE_W, crate::NATIVE_H)
}
/// The upstream default is embedded verbatim and resampled once for GLES.
pub fn default_background_native() -> Option<&'static [u8]> {
    static CACHE: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    CACHE.get_or_init(|| default_background().map(|bytes| {
        crate::native_rgba(bytes, OUT_W, OUT_H).0
    })).as_deref()
}
pub fn blur_background_native() -> Option<&'static [u8]> {
    static CACHE: OnceLock<Option<Vec<u8>>> = OnceLock::new();
    CACHE.get_or_init(|| blur_background().map(|bytes| {
        crate::native_rgba(bytes, OUT_W, OUT_H).0
    })).as_deref()
}

pub fn wallpaper_face(path: &Path) -> Option<Vec<u8>> {
    art::cover(path, OUT_W, OUT_H)
}

pub fn bezel_face(path: &Path, w: u32, h: u32) -> Option<Vec<u8>> {
    art::cover(path, w, h)
}

/// Faithful Slot v1.7 shelf ground: a plain wallpaper has no fixed
/// scrim. Wallpaper Tint only darkens a selected personal image, using the
/// exact alpha values from the official app's tint_alpha().
pub fn draw_ground_tinted(face: Option<TexId>, tint: f32, out: &mut Vec<Draw>) {
    let Some(tex) = face else { return };
    out.push(Draw::Tex {
        x: 0.0, y: 0.0,
        w: OUT_W as f32, h: OUT_H as f32,
        tex, alpha: 1.0,
    });
    if tint > 0.0 {
        out.push(Draw::Rect {
            x: 0.0, y: 0.0,
            w: OUT_W as f32, h: OUT_H as f32,
            colour: [0.0, 0.0, 0.0, tint.clamp(0.0, 1.0)],
        });
    }
}

pub fn draw_backdrop(face: Option<TexId>, out: &mut Vec<Draw>) {
    let Some(tex) = face else {
        return;
    };
    out.push(Draw::Tex {
        x: 0.0,
        y: 0.0,
        w: OUT_W as f32,
        h: OUT_H as f32,
        tex,
        alpha: 1.0,
    });
    out.push(Draw::Rect {
        x: 0.0,
        y: 0.0,
        w: OUT_W as f32,
        h: OUT_H as f32,
        colour: [0.0, 0.0, 0.0, SCRIM],
    });
}
