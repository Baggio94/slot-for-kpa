use crate::surface::{OUT_H, out_w};

const LINE_PX: f32 = 2.0;

const DOT_T: f32 = 0.12;

const OVERSHOOT: f32 = 0.6;

pub fn screen_scale(t: f32) -> f32 {
    let left = 1.0 - t.clamp(0.0, 1.0);
    1.0 - (1.0 - LINE_PX / OUT_H as f32) * left * left
}

pub fn screen_width(t: f32) -> f32 {
    let left = 1.0 - (t.clamp(0.0, 1.0) / DOT_T).min(1.0);
    1.0 - (1.0 - LINE_PX / out_w() as f32) * left
}

pub fn screen_brightness(t: f32) -> f32 {
    let left = 1.0 - t.clamp(0.0, 1.0);
    1.0 + OVERSHOOT * left * left
}

pub fn screen_rect(t: f32) -> (f32, f32, f32, f32) {
    screen_rect_in([0.0, 0.0, out_w() as f32, OUT_H as f32], t)
}

/// The CRT power curve applied to the game picture's own `area`.
pub fn screen_rect_in(area: [f32; 4], t: f32) -> (f32, f32, f32, f32) {
    let w = area[2] * screen_width(t);
    let h = area[3] * screen_scale(t);
    (area[0] + (area[2] - w) / 2.0, area[1] + (area[3] - h) / 2.0, w, h)
}
