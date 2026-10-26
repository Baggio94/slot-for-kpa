use crate::surface::{OUT_H, out_w};

/// Upstream Slot's 4:3 handhelds show the 3:2 canvas under a bezel. A
/// canvas already fitted to the panel's shape is never framed.
pub fn framed(panel: (u32, u32)) -> bool {
    panel.0 > 0 && panel.0 * 3 == panel.1 * 4 && out_w() == crate::surface::BASE_W
}

pub fn screen_rect(panel: (u32, u32)) -> (i32, i32, i32, i32) {
    let w = panel.0;
    let h = (w * OUT_H + out_w() / 2) / out_w();
    ((panel.0 - w) as i32 / 2, 0, w as i32, h as i32)
}

pub fn lifted_rect(panel: (u32, u32), lift: f32) -> (i32, i32, i32, i32) {
    let (x, _, w, h) = screen_rect(panel);
    let t = lift.clamp(0.0, 1.0);
    let eased = t * t * (3.0 - 2.0 * t);
    let drop = ((panel.1 as i32 - h) as f32 * (1.0 - eased)).round() as i32;
    (x, drop, w, h)
}
