use slot_gfx::{Draw, TexId, OUT_H, OUT_W};
use slot_power::Battery;

use crate::battery::{draw_gauge_with_ink, GAUGE_H};
use crate::hud::HUD_INK;
use crate::plate::HINT_H;
use crate::slot_chrome::MOUTH_H;

const FOOTER_Y: f32 = OUT_H as f32 - MOUTH_H + (MOUTH_H - HINT_H as f32) / 2.0;
const FOOTER_MARGIN: f32 = 24.0;

#[derive(Copy, Clone, Default, PartialEq, Eq, Debug)]
pub struct Printed {
    pub face: Option<TexId>,
    pub w: u32,
}

impl Printed {
    pub fn new(face: TexId, w: u32) -> Self {
        Printed {
            face: Some(face),
            w,
        }
    }
}

pub fn draw_footer(
    battery: Option<Battery>,
    percent: Printed,
    bolt: Option<TexId>,
    clock: Printed,
    out: &mut Vec<Draw>,
) {
    draw_footer_with_ink(battery,percent,bolt,clock,HUD_INK,out)
}
/// Exact official v1.7 dark/light shelf ink, also used for clock/percent/bolt.
pub fn footer_ink(light: bool) -> [u8;3] {
    if light { [0,0,0] } else { [255,255,255] }
}

/// Slot 1.7's shelf footer uses black ink when light appearance is active.
/// Game HUD colours and menu chrome remain unmodified.
pub fn draw_footer_with_ink(
    battery: Option<Battery>,percent: Printed,bolt: Option<TexId>,
    clock: Printed,ink: [u8;3],out:&mut Vec<Draw>,
) {
    let y = FOOTER_Y + (HINT_H as f32 - GAUGE_H) / 2.0;
    let c=[ink[0] as f32/255.0,ink[1] as f32/255.0,
        ink[2] as f32/255.0,1.0];
    draw_gauge_with_ink(FOOTER_MARGIN,y,battery,percent,bolt,c,out);
    printed(OUT_W as f32 - FOOTER_MARGIN - clock.w as f32,clock,out);
}

pub(crate) fn draw_printed(x: f32, y: f32, p: Printed, out: &mut Vec<Draw>) {
    if p.w == 0 {
        return;
    }
    let (w, h) = (p.w as f32, HINT_H as f32);
    out.push(match p.face {
        Some(tex) => Draw::Tex {
            x,
            y,
            w,
            h,
            tex,
            alpha: 1.0,
        },
        None => Draw::Rect {
            x,
            y,
            w,
            h,
            colour: [1.0, 1.0, 1.0, 0.08],
        },
    });
}

fn printed(x: f32, p: Printed, out: &mut Vec<Draw>) {
    draw_printed(x, FOOTER_Y, p, out);
}

#[cfg(test)]
mod theme_tests {
    use super::*;
    #[test]
    fn upstream_footer_ink_switches_whole_palette() {
        assert_eq!(footer_ink(false), [255, 255, 255]);
        assert_eq!(footer_ink(true), [0, 0, 0]);
    }
}
