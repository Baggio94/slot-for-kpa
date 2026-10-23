//! slot. 1.8.1 button legend art (upstream #102): the controller's own
//! buttons drawn beside each hint word instead of a flat white keycap.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::art::decode_png_bytes;
use crate::plate::UndoFace;

/// Drawn height inside the 24px hint band. The 58px art is box-filtered.
pub const BUTTON_H: u32 = 22;
const COMBO_GAP: u32 = 2;

fn png(key: &str) -> Option<&'static [u8]> {
    Some(match key {
        "A" => include_bytes!("../../../assets/buttonlegend_round_a.png"),
        "B" => include_bytes!("../../../assets/buttonlegend_round_b.png"),
        "X" => include_bytes!("../../../assets/buttonlegend_round_x.png"),
        "Y" => include_bytes!("../../../assets/buttonlegend_round_y.png"),
        "LEFT" => include_bytes!("../../../assets/buttonlegend_dpad_left.png"),
        "RIGHT" => include_bytes!("../../../assets/buttonlegend_dpad_right.png"),
        "UP" => include_bytes!("../../../assets/buttonlegend_dpad_up.png"),
        "DOWN" => include_bytes!("../../../assets/buttonlegend_dpad_down.png"),
        "START" => include_bytes!("../../../assets/buttonlegend_long_start.png"),
        "SELECT" | "SEL" => include_bytes!("../../../assets/buttonlegend_long_select.png"),
        "L1" | "L" => include_bytes!("../../../assets/buttonlegend_square_l1.png"),
        "R1" | "R" => include_bytes!("../../../assets/buttonlegend_square_r1.png"),
        "L2" => include_bytes!("../../../assets/buttonlegend_square_l2.png"),
        "R2" => include_bytes!("../../../assets/buttonlegend_square_r2.png"),
        _ => return None,
    })
}

/// Premultiplied box filter: every source pixel lands in exactly one
/// destination pixel, weighted by overlap, so edges stay soft and clean.
fn shrink(rgba: &[u8], sw: u32, sh: u32, dh: u32) -> UndoFace {
    let dw = ((sw as f32 * dh as f32 / sh as f32).round() as u32).max(1);
    let (fx, fy) = (sw as f32 / dw as f32, sh as f32 / dh as f32);
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    for y in 0..dh {
        let (y0, y1) = (y as f32 * fy, (y + 1) as f32 * fy);
        for x in 0..dw {
            let (x0, x1) = (x as f32 * fx, (x + 1) as f32 * fx);
            let mut acc = [0f32; 4];
            let mut area = 0f32;
            for sy in y0.floor() as u32..(y1.ceil() as u32).min(sh) {
                let wy = (y1.min(sy as f32 + 1.0) - y0.max(sy as f32)).max(0.0);
                for sx in x0.floor() as u32..(x1.ceil() as u32).min(sw) {
                    let wx = (x1.min(sx as f32 + 1.0) - x0.max(sx as f32)).max(0.0);
                    let w = wx * wy;
                    let p = &rgba[((sy * sw + sx) * 4) as usize..][..4];
                    let a = p[3] as f32 / 255.0;
                    for c in 0..3 {
                        acc[c] += p[c] as f32 * a * w;
                    }
                    acc[3] += a * w;
                    area += w;
                }
            }
            let o = ((y * dw + x) * 4) as usize;
            if acc[3] > 0.0 && area > 0.0 {
                for c in 0..3 {
                    out[o + c] = (acc[c] / acc[3]).round().clamp(0.0, 255.0) as u8;
                }
                out[o + 3] = (acc[3] / area * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    UndoFace { rgba: out, w: dw, h: dh }
}

fn single(key: &str) -> Option<UndoFace> {
    // Keyed by the embedded PNG, so "SEL" and "SELECT" share one decode.
    static CACHE: OnceLock<Mutex<HashMap<usize, Option<(Vec<u8>, u32, u32)>>>> = OnceLock::new();
    let bytes = png(key)?;
    let mut cache = CACHE.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner());
    let entry = cache.entry(bytes.as_ptr() as usize).or_insert_with(|| {
        let (rgba, w, h) = decode_png_bytes(bytes)?;
        let face = shrink(&rgba, w, h, BUTTON_H);
        Some((face.rgba, face.w, face.h))
    });
    entry.clone().map(|(rgba, w, h)| UndoFace { rgba, w, h })
}

/// The art for a hint key such as "A", "START" or "SEL+R1"; None keeps the
/// classic keycap for anything that is not a physical button.
pub fn key_art(key: &str) -> Option<UndoFace> {
    let parts: Vec<UndoFace> = key.split('+').map(single).collect::<Option<_>>()?;
    let w = parts.iter().map(|p| p.w).sum::<u32>() + COMBO_GAP * (parts.len() as u32 - 1);
    let mut rgba = vec![0u8; (w * BUTTON_H * 4) as usize];
    let mut x = 0;
    for part in &parts {
        crate::plate::blit(&mut rgba, w, &part.rgba, part.w, part.h, x, 0);
        x += part.w + COMBO_GAP;
    }
    Some(UndoFace { rgba, w, h: BUTTON_H })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_button_decodes_at_hint_height_and_combos_join() {
        for key in ["A", "B", "X", "Y", "LEFT", "RIGHT", "UP", "DOWN", "START", "SELECT", "L1", "R1", "L2", "R2"] {
            let art = key_art(key).unwrap_or_else(|| panic!("{key} art"));
            assert_eq!(art.h, BUTTON_H);
            assert!(art.rgba.chunks_exact(4).any(|p| p[3] > 200), "{key} is visible");
        }
        assert_eq!(key_art("A").unwrap().w, BUTTON_H, "round buttons stay round");
        let combo = key_art("SEL+R1").unwrap();
        assert_eq!(combo.w, key_art("SELECT").unwrap().w + COMBO_GAP + key_art("R1").unwrap().w);
        assert!(key_art("Change").is_none() && key_art("A+?").is_none());
    }
}
