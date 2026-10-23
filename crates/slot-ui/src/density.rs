//! Panel density for Slot's faces.
//!
//! Slot lays every face out on its 720x480 canvas and a face's `w`/`h` stay in
//! those logical pixels, so placement code never changes. On a denser panel
//! (the KONKR's 960x640) the runtime raises the density once and faces carry
//! their pixels at panel resolution instead: `rgba` then holds `px(w)` x
//! `px(h)` texels and the GPU draws them 1:1. Text, glyphs and SVG art are
//! re-rendered at that size, never upscaled. At the default 1x every face is
//! byte-for-byte what upstream Slot draws.
use std::cell::Cell;
use std::sync::atomic::{AtomicU32, Ordering};

use crate::plate::UndoFace;
use crate::text::{self, Layout};
use crate::CartFace;

static SCALE: AtomicU32 = AtomicU32::new(0x3f80_0000); // 1.0f32

thread_local! {
    static LOCAL: Cell<Option<f32>> = const { Cell::new(None) };
}

/// Set the panel density for every thread, e.g. 960/720 on the KONKR.
/// Call once before the first face is built: faces are cached.
pub fn set_scale(scale: f32) {
    let scale = if scale.is_finite() { scale.clamp(1.0, 4.0) } else { 1.0 };
    SCALE.store(scale.to_bits(), Ordering::Release);
}

/// Run `f` at `scale` on this thread only. For tests and offline exports.
pub fn with_scale<T>(scale: f32, f: impl FnOnce() -> T) -> T {
    let before = LOCAL.with(|l| l.replace(Some(scale)));
    let out = f();
    LOCAL.with(|l| l.set(before));
    out
}

pub fn scale() -> f32 {
    LOCAL
        .with(Cell::get)
        .unwrap_or_else(|| f32::from_bits(SCALE.load(Ordering::Acquire)))
}

pub fn is_dense() -> bool {
    scale() > 1.0
}

/// Panel texels for a logical length.
pub fn px(n: u32) -> u32 {
    (n as f32 * scale()).round() as u32
}

/// Texel size of `rgba` for a face whose logical size is `w` x `h`.
/// A dense face holds `px(w)` x `px(h)` texels at the density it was built
/// at, which is recovered from its size; anything else is 1:1.
pub fn texels(w: u32, h: u32, rgba_len: usize) -> (u32, u32) {
    let area = (w as u64) * (h as u64) * 4;
    if area == 0 || rgba_len as u64 == area {
        return (w, h);
    }
    let texels = rgba_len as u64 / 4;
    let guess = (w as f64 * (rgba_len as f64 / area as f64).sqrt()).round() as i64;
    // Each side was rounded on its own: search the few widths near the
    // estimate for the pair that one density gives.
    (guess - 3..=guess + 3)
        .filter(|&tw| tw > 0 && texels % tw as u64 == 0)
        .map(|tw| (tw as u32, (texels / tw as u64) as u32))
        .find(|&(tw, th)| {
            // Some one density must round `w` to `tw` and `h` to `th`.
            let (w, h) = (w as f64, h as f64);
            let lo = ((tw as f64 - 0.5) / w).max((th as f64 - 0.5) / h);
            let hi = ((tw as f64 + 0.5) / w).min((th as f64 + 0.5) / h);
            lo <= hi
        })
        .unwrap_or((w, h))
}

/// Pixels ready for a texture: `texels()` is the upload size, which equals
/// the logical `w`/`h` unless the face is dense.
pub trait Face {
    fn rgba(&self) -> &[u8];
    fn texels(&self) -> (u32, u32);
}

impl Face for UndoFace {
    fn rgba(&self) -> &[u8] {
        &self.rgba
    }
    fn texels(&self) -> (u32, u32) {
        texels(self.w, self.h, self.rgba.len())
    }
}

impl Face for crate::PhotoFace {
    fn rgba(&self) -> &[u8] {
        &self.rgba
    }
    fn texels(&self) -> (u32, u32) {
        texels(self.w, self.h, self.rgba.len())
    }
}

impl Face for CartFace {
    fn rgba(&self) -> &[u8] {
        &self.rgba
    }
    fn texels(&self) -> (u32, u32) {
        texels(self.w, self.h, self.rgba.len())
    }
}

/// An RGBA face under construction: logical size `w` x `h`, pixels at the
/// current density.
pub struct Canvas {
    pub w: u32,
    pub h: u32,
    pub tw: u32,
    pub th: u32,
    pub rgba: Vec<u8>,
}

impl Canvas {
    pub fn new(w: u32, h: u32) -> Self {
        let (tw, th) = (px(w), px(h));
        Canvas { w, h, tw, th, rgba: vec![0; (tw * th * 4) as usize] }
    }

    /// Wrap texels rendered at the current density. The logical size is the
    /// smallest that holds them; the texels are centred in its `px()` box.
    pub fn from_texels(rgba: &[u8], tw: u32, th: u32) -> Self {
        let s = scale();
        let w = (tw as f32 / s - 1e-3).ceil().max(0.0) as u32;
        let h = (th as f32 / s - 1e-3).ceil().max(0.0) as u32;
        let mut c = Canvas::new(w, h);
        let x = (c.tw as i32 - tw as i32) / 2;
        let y = (c.th as i32 - th as i32) / 2;
        c.blit_texels(rgba, tw, th, x, y);
        c
    }

    /// Centre `layout`, given in logical pixels, across the whole canvas.
    pub fn text(&mut self, layout: &Layout, colour: [u8; 3]) {
        let (tw, th) = (self.tw, self.th);
        text::draw_centred(&mut self.rgba, tw, th, &scaled(layout), colour);
    }

    /// Centre `layout` inside the logical box `w` x `h` at `x`, `y`.
    pub fn text_in(&mut self, x: u32, y: u32, w: u32, h: u32, layout: &Layout, colour: [u8; 3]) {
        let mut cell = Canvas::new(w, h);
        cell.text(layout, colour);
        self.blit(&cell.rgba, cell.tw, cell.th, x, y);
    }

    /// Copy texels to the logical position `x`, `y`, clipped to the canvas.
    pub fn blit(&mut self, src: &[u8], sw: u32, sh: u32, x: u32, y: u32) {
        self.blit_texels(src, sw, sh, px(x) as i32, px(y) as i32);
    }

    pub fn blit_face(&mut self, face: &UndoFace, x: u32, y: u32) {
        let (sw, sh) = face.texels();
        self.blit(&face.rgba, sw, sh, x, y);
    }

    /// Copy texels to the texel position `x`, `y`, clipped to the canvas.
    pub fn blit_texels(&mut self, src: &[u8], sw: u32, sh: u32, x: i32, y: i32) {
        if src.len() < (sw * sh * 4) as usize {
            return;
        }
        let x0 = x.max(0);
        let x1 = (x + sw as i32).min(self.tw as i32);
        if x1 <= x0 {
            return;
        }
        let len = ((x1 - x0) * 4) as usize;
        for row in 0..sh as i32 {
            let dy = y + row;
            if dy < 0 || dy >= self.th as i32 {
                continue;
            }
            let from = ((row * sw as i32 + (x0 - x)) * 4) as usize;
            let to = ((dy * self.tw as i32 + x0) * 4) as usize;
            self.rgba[to..to + len].copy_from_slice(&src[from..from + len]);
        }
    }

    pub fn into_undo(self) -> UndoFace {
        UndoFace { rgba: self.rgba, w: self.w, h: self.h }
    }

    pub fn into_cart(self) -> CartFace {
        CartFace { rgba: self.rgba, w: self.w, h: self.h }
    }
}

/// `layout` at the current density: same lines, larger type.
pub fn scaled(layout: &Layout) -> Layout {
    let s = scale();
    Layout {
        lines: layout.lines.clone(),
        px: layout.px * s,
        tracking: layout.tracking * s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_x_is_untouched_and_konkr_is_four_thirds() {
        assert_eq!(px(24), 24);
        assert_eq!(texels(3, 2, 24), (3, 2));
        with_scale(4.0 / 3.0, || {
            assert_eq!((px(24), px(720), px(480)), (32, 960, 640));
            assert_eq!(texels(3, 3, 16 * 4), (4, 4));
            assert_eq!(texels(3, 3, 9 * 4), (3, 3), "a 1x face stays 1x");
            let hint = Canvas::new(97, 24);
            assert_eq!(texels(hint.w, hint.h, hint.rgba.len()), (hint.tw, hint.th));
            let c = Canvas::from_texels(&[255; 50 * 4], 50, 1);
            assert_eq!((c.w, c.tw), (38, 51));
        });
    }

    #[test]
    fn a_face_knows_its_texels_whatever_the_current_density() {
        for (w, h) in [(97, 24), (360, 212), (312, 337), (22, 22), (37, 22), (720, 58), (1, 40)] {
            let c = with_scale(4.0 / 3.0, || Canvas::new(w, h));
            assert_eq!(texels(w, h, c.rgba.len()), (c.tw, c.th), "{w}x{h}");
            let c = with_scale(2.0, || Canvas::new(w, h));
            assert_eq!(texels(w, h, c.rgba.len()), (c.tw, c.th), "{w}x{h} at 2x");
        }
    }

    #[test]
    fn blits_clip_at_every_edge() {
        let mut c = Canvas::new(4, 4);
        c.blit_texels(&[9; 3 * 3 * 4], 3, 3, -1, 2);
        assert_eq!(c.rgba.iter().filter(|&&b| b == 9).count(), 2 * 2 * 4);
    }
}
