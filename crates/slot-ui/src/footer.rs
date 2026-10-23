use slot_gfx::{Draw, TexId, OUT_H, OUT_W};
use slot_power::Battery;

use crate::battery::{draw_gauge_with_ink, GAUGE_H};
use crate::density::{self, Canvas};
use crate::hud::HUD_INK;
use crate::plate::{word_ink, UndoFace, HINT_H};
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

/// RetroAchievements gold, kept for a mastered game's trophy in both themes.
pub const MASTERED_INK: [u8; 3] = [0xe8, 0xa8, 0x18];
// Material Design "trophy" (Pictogrammers, Apache-2.0), a 24-unit square.
const TROPHY_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="white" d="M18 2C17.1 2 16 3 16 4H8C8 3 6.9 2 6 2H2V11C2 12 3 13 4 13H6.2C6.6 15 7.9 16.7 11 17V19.08C8 19.54 8 22 8 22H16C16 22 16 19.54 13 19.08V17C16.1 16.7 17.4 15 17.8 13H20C21 13 22 12 22 11V2H18M6 11H4V4H6V11M20 11H18V4H20V11Z"/></svg>"#;
const TROPHY_PX: u32 = 16;
const TROPHY_GAP: u32 = 4;

/// Shelf footer progress, "<trophy> 12/40", in the footer ink. A mastered
/// game shows its trophy in gold. Built only when the numbers or theme change.
pub fn progress_face(unlocked: u16, total: u16, ink: [u8; 3]) -> UndoFace {
    let word = word_ink(&format!("{unlocked}/{total}"), ink);
    let trophy_ink = if total > 0 && unlocked >= total { MASTERED_INK } else { ink };
    let side = density::px(TROPHY_PX);
    let Some(cov) = crate::silhouette::rasterise_svg(TROPHY_SVG, side, side) else {
        return word;
    };
    let mut face = Canvas::new(TROPHY_PX + TROPHY_GAP + word.w, HINT_H);
    let top = (face.th - side) / 2;
    for gy in 0..side {
        for gx in 0..side {
            let a = cov[(gy * side + gx) as usize];
            if a == 0 {
                continue;
            }
            let o = (((top + gy) * face.tw + gx) * 4) as usize;
            face.rgba[o..o + 4].copy_from_slice(&[trophy_ink[0], trophy_ink[1], trophy_ink[2], a]);
        }
    }
    face.blit_face(&word, TROPHY_PX + TROPHY_GAP, 0);
    face.into_undo()
}

/// Centre slot of the shelf footer, between the battery and the clock.
pub fn draw_footer_centre(p: Printed, out: &mut Vec<Draw>) {
    if p.face.is_some() {
        printed(((OUT_W as f32 - p.w as f32) / 2.0).round(), p, out);
    }
}

#[cfg(test)]
mod progress_tests {
    use super::*;

    #[test]
    fn progress_face_has_a_trophy_and_turns_gold_only_when_mastered() {
        let white = [255, 255, 255];
        let word = word_ink("12/40", white);
        let face = progress_face(12, 40, white);
        assert!(face.w > word.w, "the trophy glyph sits before the numbers");
        assert_eq!(face.h, HINT_H);
        let gold = |f: &UndoFace| f.rgba.chunks_exact(4)
            .any(|p| p[3] > 0 && p[..3] == MASTERED_INK);
        assert!(!gold(&face));
        assert!(gold(&progress_face(40, 40, white)));
        assert!(gold(&progress_face(40, 40, [0, 0, 0])), "gold in the light theme too");
    }
}
