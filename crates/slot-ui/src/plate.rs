use crate::density::Face as _;
use crate::density::{self, Canvas};
use crate::draw::{Draw, TexId, OUT_W};
use crate::text;

pub const HINT_H: u32 = 24;
pub const CAP: u32 = 20;
const CAP_PAD: u32 = 4;
const CAP_MAX_W: f32 = 64.0;
pub const HINT_GAP: f32 = 14.0;

pub const CAP_GAP: u32 = 5;
const EDGE: u32 = 2;
pub const HINT_EDGE: u32 = EDGE;
const LABEL_MAX_W: f32 = 140.0;

pub const TITLE_W: u32 = 360;
pub const TITLE_H: u32 = 24;

const INK: [u8; 3] = [0xf6, 0xf4, 0xef];
const CAP_INK: [u8; 3] = [0x1a, 0x19, 0x17];
const KEY_PX: f32 = 14.0;
const LABEL_PX: f32 = 16.0;
const LABEL_MIN_PX: f32 = 10.0;
const TITLE_PX: f32 = 20.0;
const TITLE_MIN_PX: f32 = 12.0;

// Exact upstream Slot v1.7 shelf Game Titles face, not Polaroid title_face.
pub const TITLE_LINE_H: u32 = 32;
const TITLE_LINE_PX: f32 = 24.0;
const TITLE_LINE_MIN_PX: f32 = 14.0;
const TITLE_LINE_MAX_W: f32 = 560.0;

pub struct UndoFace {
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hint {
    pub key: &'static str,
    pub label: String,
}

pub fn hint_row(legend: &[(&'static str, &'static str)]) -> Vec<Hint> {
    legend
        .iter()
        .map(|(key, label)| Hint {
            key,
            label: label.to_string(),
        })
        .collect()
}

pub fn cap_width(key: &str) -> u32 {
    if let Some(art) = crate::button_art::key_art(key) {
        return art.w;
    }
    let Some(font) = text::label_font() else {
        return CAP;
    };
    let layout = text::fit(font, key, CAP_MAX_W, 1, KEY_PX, KEY_PX);
    let ink = layout
        .lines
        .iter()
        .map(|l| text::line_width(font, l, layout.px, layout.tracking))
        .fold(0.0, f32::max);
    CAP.max(ink.ceil() as u32 + 2 * CAP_PAD)
}

pub fn hint_width(key: &str, label: &str) -> u32 {
    cap_width(key) + CAP_GAP + band_width(label) + EDGE
}

pub fn hint_face(key: &str, label: &str) -> UndoFace {
    let mut face = Canvas::new(hint_width(key, label), HINT_H);
    let cap_w = cap_width(key);
    stamp_key(&mut face, key, cap_w, 0);
    let text_w = band_width(label);
    if let Some(font) = text::label_font() {
        let layout = text::fit(font, label, text_w as f32, 1, LABEL_PX, LABEL_MIN_PX);
        face.text_in(cap_w + CAP_GAP, 0, text_w, HINT_H, &layout, INK);
    }
    face.into_undo()
}

/// The button art for `key`, or the classic keycap, vertically centred in
/// the hint band at logical `x`.
fn stamp_key(face: &mut Canvas, key: &str, cap_w: u32, x: u32) {
    if let Some(art) = crate::button_art::key_art(key) {
        let (aw, ah) = art.texels();
        let y = (face.th as i32 - ah as i32) / 2;
        face.blit_texels(&art.rgba, aw, ah, density::px(x) as i32, y);
        return;
    }
    let mut cap = Canvas::new(cap_w, CAP);
    for p in cap.rgba.chunks_exact_mut(4) {
        p.copy_from_slice(&[INK[0], INK[1], INK[2], 255]);
    }
    if let Some(font) = text::label_font() {
        let layout = text::fit(font, key, cap_w as f32, 1, KEY_PX, KEY_PX);
        cap.text(&layout, CAP_INK);
    }
    let y = (face.th as i32 - cap.th as i32) / 2;
    face.blit_texels(&cap.rgba, cap.tw, cap.th, density::px(x) as i32, y);
}

/// Complete first-run instructions must never inherit the 140px footer
/// hint limit, which dropped the tail of "OPEN MENU TO ADD ROMS".
pub const ONBOARDING_MAX_W: u32 = OUT_W - 48;
fn onboarding_text_width(text: &str, px: f32) -> u32 {
    let Some(font)=text::label_font() else {return 8};
    let spacing=(px*0.10).round();
    text::line_width(font,&text.to_uppercase(),px,spacing).ceil() as u32+8
}
fn onboarding_band(text: &str, px: f32) -> UndoFace {
    let mut face=Canvas::new(onboarding_text_width(text,px),HINT_H);
    if let Some(font)=text::label_font() {
        let layout=text::fit(font,text,face.w as f32,1,px,px);
        face.text(&layout,INK);
    }
    face.into_undo()
}
/// One wide single-line instruction with the ORIGINAL boxed keycap between
/// the word "PRESS" and the rest of the instruction.
pub fn onboarding_instruction_face(before: &str, key: &str, after: &str) -> UndoFace {
    const GAP:u32=7;
    let cap_w=cap_width(key);
    let mut px=LABEL_PX;
    while px>LABEL_MIN_PX &&
        onboarding_text_width(before,px)+onboarding_text_width(after,px)
            +cap_w+2*GAP>ONBOARDING_MAX_W {
        px-=1.0;
    }
    let before=onboarding_band(before,px);
    let after=onboarding_band(after,px);
    let w=before.w+cap_w+after.w+2*GAP;
    let mut face=Canvas::new(w,HINT_H);
    face.blit_face(&before,0,0);
    let x=before.w+GAP;
    // The exact Slot cap renderer, as in hint_face().
    stamp_key(&mut face,key,cap_w,x);
    face.blit_face(&after,x+cap_w+GAP,0);
    face.into_undo()
}
/// Smaller but fully readable second line on BOTH empty-library screens.
pub fn onboarding_note_face(text: &str) -> UndoFace {
    onboarding_band(text,13.0)
}
#[cfg(test)]
mod onboarding_tests {
    use super::*;
    #[test]
    fn first_run_prompts_render_without_losing_words_or_exceeding_screen() {
        for (before,key,after) in [
            ("PRESS","START",", THEN SELECT LIBRARY TO ADD GAMES"),
            ("PRESS","START","TO SELECT YOUR ROM FOLDER"),
        ] {
            let face=onboarding_instruction_face(before,key,after);
            assert!(face.w<=ONBOARDING_MAX_W,"hint clips: {after}");
            assert_eq!(face.h,HINT_H);
            assert!(face.rgba.chunks_exact(4).any(|p|p[3]!=0));
            assert!(onboarding_text_width(after,LABEL_PX)<ONBOARDING_MAX_W);
        }
        let note=onboarding_note_face("(ZIP ROMS SUPPORTED)");
        assert!(note.w<=ONBOARDING_MAX_W);
        assert!(note.rgba.chunks_exact(4).any(|p|p[3]!=0));
        assert!(hint_width("START","Open Library")<=ONBOARDING_MAX_W);
    }
}

pub fn word_width(text: &str) -> u32 {
    band_width(text)
}

pub fn word_face(text: &str) -> UndoFace {
    word_ink(text, INK)
}

/// Render footer words in Slot's exact Dark/Light ink without touching menu
/// typography or the game HUD. Called only on clock/battery/theme changes.
pub fn word_ink(text: &str, ink: [u8; 3]) -> UndoFace {
    let mut face = Canvas::new(word_width(text), HINT_H);
    if let Some(font) = text::label_font() {
        let layout = text::fit(font, text, face.w as f32, 1, LABEL_PX, LABEL_MIN_PX);
        face.text(&layout, ink);
    }
    face.into_undo()
}

/// High-resolution shelf label for the KONKR's 960×640 panel.
/// Render at 3× Slot's original 16px label size, then draw at 1/3.
///
/// IMPORTANT: word_width() is capped at 140 native pixels for the footer.
/// Using it here silently wraps and drops the last word of "Game Boy Advance"
/// and "Game Boy Color" because the platform face permits only one line.
/// Measure the WHOLE label at the actual 3× font size instead.
pub const PLATFORM_NAME_SCALE: u32 = 3;

pub fn platform_name_face(text: &str) -> UndoFace {
    let scale = PLATFORM_NAME_SCALE;
    let px = LABEL_PX * scale as f32;
    let w = if let Some(font) = text::label_font() {
        let tracking = (px * 0.10).round(); // text::fit() letter spacing
        let full_width = text::line_width(font, &text.to_uppercase(), px, tracking);
        full_width.ceil() as u32 + 6 * scale // protect both glyph edges
    } else {
        (word_width(text) + 6) * scale
    };
    let h = HINT_H * scale;
    let mut rgba = vec![0u8; (w * h * 4) as usize];
    if let Some(font) = text::label_font() {
        let layout = text::fit(font, text, w as f32, 1, px, px);
        text::draw_centred(&mut rgba, w, h, &layout, [255,255,255]);
    }
    UndoFace { rgba, w, h }
}

pub fn hint_quad(x: f32, y: f32, w: f32, face: Option<TexId>) -> Draw {
    let h = HINT_H as f32;
    match face {
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
            colour: [1.0, 1.0, 1.0, 0.12],
        },
    }
}

pub const LEGEND_GAP: f32 = 36.0;

pub fn centred_hints(hints: &[(TexId, u32)], gap: f32) -> Vec<(TexId, u32, f32)> {
    let seen = |w: u32| w.saturating_sub(HINT_EDGE) as f32;
    let total = hints.iter().map(|&(_, w)| seen(w)).sum::<f32>()
        + gap * hints.len().saturating_sub(1) as f32;
    let mut x = ((OUT_W as f32 - total) / 2.0).round();
    hints
        .iter()
        .map(|&(tex, w)| {
            let at = x.round();
            x += seen(w) + gap;
            (tex, w, at)
        })
        .collect()
}

fn band_width(label: &str) -> u32 {
    let Some(font) = text::label_font() else {
        return LABEL_MAX_W as u32;
    };
    let layout = text::fit(font, label, LABEL_MAX_W, 1, LABEL_PX, LABEL_MIN_PX);
    let ink = layout
        .lines
        .iter()
        .map(|l| text::line_width(font, l, layout.px, layout.tracking))
        .fold(0.0, f32::max);
    (ink.ceil() as u32).clamp(1, LABEL_MAX_W as u32)
}

/// Pixel geometry and white ink copied from upstream v1.7.0 plate.rs.
pub fn title_line_face(text: &str) -> UndoFace {
    let Some(font) = text::label_font() else {
        return UndoFace { rgba: Vec::new(), w: 0, h: 0 };
    };
    let layout = text::fit(
        font, text, TITLE_LINE_MAX_W, 1, TITLE_LINE_PX, TITLE_LINE_MIN_PX,
    );
    let ink = layout.lines.iter()
        .map(|line| text::line_width(font, line, layout.px, layout.tracking))
        .fold(0.0, f32::max);
    let w = (ink.ceil() as u32).clamp(1, TITLE_LINE_MAX_W as u32);
    let mut face = Canvas::new(w, TITLE_LINE_H);
    face.text(&layout, [0xff, 0xff, 0xff]);
    face.into_undo()
}

pub fn title_face(text: &str) -> UndoFace {
    let mut face = Canvas::new(TITLE_W, TITLE_H);
    if let Some(font) = text::label_font() {
        let layout = text::fit(font, text, TITLE_W as f32, 1, TITLE_PX, TITLE_MIN_PX);
        face.text(&layout, INK);
    }
    face.into_undo()
}

pub const ARROW_GAP: u32 = 3;

const LEFT_CARET: char = '\u{f0d9}';
const RIGHT_CARET: char = '\u{f0da}';

fn arrows_art() -> Option<UndoFace> {
    crate::button_art::key_art("LEFT+RIGHT")
}

fn arrows_w() -> u32 {
    arrows_art().map_or(2 * CAP + ARROW_GAP, |art| art.w)
}

pub fn arrows_hint_width(label: &str) -> u32 {
    arrows_w() + CAP_GAP + band_width(label) + EDGE
}

pub fn arrows_hint_face(label: &str) -> UndoFace {
    let mut face = Canvas::new(arrows_hint_width(label), HINT_H);
    if let Some(art) = arrows_art() {
        let (aw, ah) = art.texels();
        let y = (face.th as i32 - ah as i32) / 2;
        face.blit_texels(&art.rgba, aw, ah, 0, y);
    } else {
        for (i, glyph) in [LEFT_CARET, RIGHT_CARET].into_iter().enumerate() {
            let cap = glyph_cap(glyph);
            let y = (face.th as i32 - cap.th as i32) / 2;
            let x = density::px(i as u32 * (CAP + ARROW_GAP)) as i32;
            face.blit_texels(&cap.rgba, cap.tw, cap.th, x, y);
        }
    }
    let text_w = band_width(label);
    if let Some(font) = text::label_font() {
        let layout = text::fit(font, label, text_w as f32, 1, LABEL_PX, LABEL_MIN_PX);
        face.text_in(arrows_w() + CAP_GAP, 0, text_w, HINT_H, &layout, INK);
    }
    face.into_undo()
}

fn glyph_cap(glyph: char) -> Canvas {
    let mut cap = Canvas::new(CAP, CAP);
    for p in cap.rgba.chunks_exact_mut(4) {
        p.copy_from_slice(&[INK[0], INK[1], INK[2], 255]);
    }
    let Some(font) = crate::icon::symbols_font() else {
        return cap;
    };
    let side = cap.tw as i32;
    let (m, cov) = font.rasterize(glyph, KEY_PX * density::scale());
    let x0 = (side - m.width as i32) / 2;
    let y0 = (side - m.height as i32) / 2;
    for gy in 0..m.height {
        for gx in 0..m.width {
            let (dx, dy) = (x0 + gx as i32, y0 + gy as i32);
            if dx < 0 || dy < 0 || dx >= side || dy >= side {
                continue;
            }
            let a = cov[gy * m.width + gx] as u32;
            let at = ((dy * side + dx) * 4) as usize;
            for k in 0..3 {
                cap.rgba[at + k] =
                    ((CAP_INK[k] as u32 * a + cap.rgba[at + k] as u32 * (255 - a)) / 255) as u8;
            }
        }
    }
    cap
}
