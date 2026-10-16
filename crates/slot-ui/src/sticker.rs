use slot_gfx::{Draw, TexId};

use crate::art::render_svg;
use crate::barcode::{code39, CODE39_NARROW, CODE39_WIDE};
use crate::plate::UndoFace;
use crate::text;

const STICKER_SVG: &str = include_str!("../assets/sticker.svg");
const STICKER_KONKR_SVG: &str = include_str!("../assets/sticker-konkr.svg");

const WORDMARK_SVG: &str = include_str!("../assets/wordmark.svg");
// Scalable artwork traced from the KONKR and Pocket Advance branding
// supplied specifically for this Android device, not substitute type.
const KONKR_SVG: &str = include_str!("../assets/konkr-logo.svg");
const POCKET_ADVANCE_SVG: &str = include_str!("../assets/pocket-advance-logo.svg");

const WORDMARK_W: u32 = 250;

pub const STICKER_W: u32 = 660;
pub const STICKER_H: u32 = 228;

const BLACK: [u8; 3] = [0x23, 0x1f, 0x20];
const WHITE: [u8; 3] = [0xff, 0xff, 0xff];

const PANEL_FX: f32 = 62.133 / 205.762;
const PANEL_FY: f32 = 3.81 / 71.116;
const PANEL_FW: f32 = 140.315 / 205.762;
const PANEL_FH: f32 = 35.433 / 71.116;

pub const DC: char = '\u{2393}';

const MARGIN: f32 = 11.0;
const HEAD_PX: f32 = 11.0;
const BODY_PX: f32 = 8.5;
const SERIAL_PX: f32 = 26.0;
const SMALL_PX: f32 = 9.0;

pub struct StickerFields<'a> {
    pub battery: Option<u8>,
    pub serial: &'a str,
    pub dirty_digit: char,
}

pub const CREDITS: [&str; 10] = [
    "EMULATION POWERED BY MGBA",
    "AND GPSP. UNDERLYING OS IS",
    "BASEOS BY PVAIBHAV. TYPE IS",
    "OPEN SANS AND NERD FONTS",
    "SYMBOLS BY RYAN L MCINTYRE.",
    "THE PANEL MASK IS DERIVED",
    "FROM GIGAHERZ'S LCD3X. THE",
    "CART SOUNDS ARE MY CHILDHOOD",
    "GAMEBOY. I WASTED WATER",
    "BUILDING THIS WITH CLAUDE.",
];

pub const ORIGIN: [&str; 2] = ["S/LOT-USA", "MADE IN ITHACA"];

pub const COPYRIGHT: &str = "\u{a9} 2026 BRANDON T. KOWALSKI";

pub const HOME: &str = "SEE README.";

pub fn head_rows(f: &StickerFields) -> [String; 3] {
    [
        "MODEL NO. AGS-102".into(),
        format!("INPUT : 5V{DC}1.5A"),
        match f.battery {
            Some(p) => format!("BATTERY : LI-ION ({p}%)"),
            None => "BATTERY : LI-ION".to_string(),
        },
    ]
}

pub fn sticker_lines(f: &StickerFields) -> Vec<String> {
    let mut out: Vec<String> = head_rows(f).into();
    out.extend(CREDITS.iter().map(|s| s.to_string()));
    out.extend(ORIGIN.iter().map(|s| s.to_string()));
    out.push(format!("{} {}", f.serial, f.dirty_digit));
    out.push(COPYRIGHT.to_string());
    out.push(HOME.to_string());
    out
}

struct Canvas {
    px: Vec<u8>,
    w: u32,
    h: u32,
}

impl Canvas {
    fn shape(source: &str, w: u32, h: u32) -> Canvas {
        let px = render_svg(source, w, h).unwrap_or_else(|| vec![0; (w * h * 4) as usize]);
        Canvas { px, w, h }
    }

    fn set(&mut self, x: u32, y: u32, c: [u8; 3], a: u8) {
        if x >= self.w || y >= self.h {
            return;
        }
        let at = ((y * self.w + x) * 4) as usize;
        self.px[at..at + 3].copy_from_slice(&c);
        self.px[at + 3] = a;
    }

    fn rect(&mut self, x: u32, y: u32, w: u32, h: u32, c: [u8; 3]) {
        for yy in y..(y + h).min(self.h) {
            for xx in x..(x + w).min(self.w) {
                self.set(xx, yy, c, 255);
            }
        }
    }

    fn print_measure(&self, s: &str, px: f32) -> f32 {
        let Some(font) = text::label_font() else {
            return 0.0;
        };
        let layout = text::fit(font, s, f32::MAX, 1, px, px);
        layout
            .lines
            .first()
            .map(|l| text::line_width(font, l, px, layout.tracking))
            .unwrap_or(0.0)
    }

    fn dc(&mut self, x: f32, y: f32, px: f32, c: [u8; 3]) -> f32 {
        let bar_w = px * 0.78;
        let t = (px / 8.0).round().max(1.0);
        let gap = (t * 2.0).max(3.0);
        let bar_y = (y + px * 0.685 - (t * 2.0 + gap) / 2.0).round();
        self.rect(x as u32, bar_y as u32, bar_w as u32, t as u32, c);
        let dash = bar_w / 5.0;
        for n in 0..3 {
            let dx = x + n as f32 * dash * 2.0;
            self.rect(
                dx as u32,
                (bar_y + t + gap) as u32,
                dash.ceil() as u32,
                t as u32,
                c,
            );
        }
        bar_w
    }

    fn blit(&mut self, x: u32, y: u32, src: &[u8], sw: u32, sh: u32) {
        for row in 0..sh {
            for col in 0..sw {
                let s = ((row * sw + col) * 4) as usize;
                let a = src[s + 3] as u32;
                if a == 0 {
                    continue;
                }
                let (dx, dy) = (x + col, y + row);
                if dx >= self.w || dy >= self.h {
                    continue;
                }
                let d = ((dy * self.w + dx) * 4) as usize;
                for k in 0..3 {
                    let under = self.px[d + k] as u32;
                    self.px[d + k] = ((src[s + k] as u32 * a + under * (255 - a)) / 255) as u8;
                }
                self.px[d + 3] = 255;
            }
        }
    }

    fn outline(&mut self, x: f32, y: f32, w: f32, h: f32, t: f32, c: [u8; 3]) {
        let (x, y, w, h, t) = (x as u32, y as u32, w as u32, h as u32, t as u32);
        self.rect(x, y, w, t, c);
        self.rect(x, y + h - t, w, t, c);
        self.rect(x, y, t, h, c);
        self.rect(x + w - t, y, t, h, c);
    }

    fn print(&mut self, x: f32, y: f32, s: &str, px: f32, c: [u8; 3]) -> f32 {
        let Some(font) = text::label_font() else {
            return y;
        };
        let layout = text::fit(font, s, f32::MAX, 1, px, px);
        let measured = layout
            .lines
            .first()
            .map(|l| text::line_width(font, l, px, layout.tracking))
            .unwrap_or(0.0);
        let (bw, bh) = ((measured.ceil() as u32).max(1), (px * 1.6).ceil() as u32);
        let cov = text::coverage(bw, bh, &layout);
        for row in 0..bh {
            for col in 0..bw {
                let a = cov[(row * bw + col) as usize];
                if a == 0 {
                    continue;
                }
                let (dx, dy) = (x as u32 + col, y as u32 + row);
                if dx < self.w && dy < self.h {
                    let at = ((dy * self.w + dx) * 4) as usize;
                    let inv = 255 - a as u32;
                    for (k, ink) in c.iter().enumerate() {
                        let under = self.px[at + k] as u32;
                        self.px[at + k] = ((*ink as u32 * a as u32 + under * inv) / 255) as u8;
                    }
                    self.px[at + 3] = 255;
                }
            }
        }
        y + px * 1.35
    }
}

pub fn sticker_face(f: &StickerFields) -> UndoFace {
    sticker_face_custom(f, &CREDITS, &ORIGIN, COPYRIGHT, HOME, "MODEL NO. AGS-102")
}

/// The KONKR port uses the *same* SVG backing, Code39 barcode, typography,
/// dimensions and logo as original Slot, with truthful port credits.
pub fn sticker_face_konkr(f: &StickerFields) -> UndoFace {
    const PORT_CREDITS: [&str; 10] = [
        "SLOT. FOR KONKR POCKET",
        "ADVANCE. ORIGINAL SLOT.",
        "BY BRANDON T. KOWALSKI.",
        "ANDROID PORT BY BAGGIO94.",
        "EMULATION POWERED BY MGBA",
        "AND GPSP LIBRETRO CORES.",
        "ORIGINAL UI & CART ART BY",
        "BRANDON T. KOWALSKI.",
        "TYPE: OPEN SANS / NERD",
        "FONTS. OPEN SOURCE SOFTWARE.",
    ];
    sticker_face_custom(
        f, &PORT_CREDITS, &["S/LOT-KONKR", "ANDROID EDITION"],
        "2026 SLOT. COMMUNITY",
        option_env!("SLOT_KONKR_VERSION").unwrap_or("DEV BUILD"),
        "MODEL NO. KONKR ADV",
    )
}

fn sticker_face_custom(f: &StickerFields, credits: &[&str; 10],
                       origin: &[&str; 2], copyright: &str,
                       home: &str, model: &str) -> UndoFace {
    let mut c = Canvas::shape(
        if model == "MODEL NO. KONKR ADV" { STICKER_KONKR_SVG } else { STICKER_SVG },
        STICKER_W, STICKER_H
    );
    let panel_x = (PANEL_FX * STICKER_W as f32).round() as u32;
    let panel_y = (PANEL_FY * STICKER_H as f32).round() as u32;
    let panel_w = (PANEL_FW * STICKER_W as f32).round() as u32;
    let panel_h = (PANEL_FH * STICKER_H as f32).round() as u32;

    let left = MARGIN;
    let mut y = MARGIN;
    let mut headers = head_rows(f);
    headers[0] = model.into();
    for (n, line) in headers.iter().enumerate() {
        if n == 1 {
            if let Some((before, after)) = line.split_once(DC) {
                let bw = c.print_measure(before, HEAD_PX);
                c.print(left, y, before, HEAD_PX, WHITE);
                let dw = c.dc(left + bw + 4.0, y, HEAD_PX, WHITE);
                y = c.print(left + bw + dw + 8.0, y, after, HEAD_PX, WHITE);
                continue;
            }
        }
        y = c.print(left, y, line, HEAD_PX, WHITE);
    }
    y += 3.0;
    for line in credits {
        y = c.print(left, y, line, BODY_PX, WHITE);
    }
    y += 3.0;
    let col_right = panel_x as f32 - MARGIN;
    c.print(left, y, origin[0], BODY_PX, WHITE);
    let maker_w = c.print_measure(origin[1], BODY_PX);
    c.print(col_right - maker_w, y, origin[1], BODY_PX, WHITE);

    let bars_y = panel_y + 14;
    let bars_h = 62;
    let payload = format!("SLOT-{}-{}", f.serial, f.dirty_digit);
    if let Some(run) = code39(&format!("*{payload}*")) {
        let syms = run.len() as f32 / 9.0;
        let per_sym = 3.0 * CODE39_WIDE + 7.0 * CODE39_NARROW;
        let scale = ((panel_w as f32 - 12.0) / (syms * per_sym + 20.0 * CODE39_NARROW)).min(2.0);
        let narrow = ((CODE39_NARROW * scale).round() as u32).max(1);
        let wide = ((CODE39_WIDE * scale).round() as u32).max(narrow * 2);
        let total: u32 = run
            .iter()
            .map(|w| if *w { wide } else { narrow })
            .sum::<u32>()
            + (run.len() as u32 / 9) * narrow;
        let mut x = panel_x + panel_w.saturating_sub(total) / 2;
        for chunk in run.chunks_exact(9) {
            for (n, is_wide) in chunk.iter().enumerate() {
                let ew = if *is_wide { wide } else { narrow };
                if n.is_multiple_of(2) {
                    c.rect(x, bars_y, ew, bars_h, BLACK);
                }
                x += ew;
            }
            x += narrow;
        }
    }

    let sy = (bars_y + bars_h + 4) as f32;
    let hash_w = c.print_measure(f.serial, SERIAL_PX);
    let box_w = SERIAL_PX * 0.9;
    let total = hash_w + 10.0 + box_w;
    let sx = panel_x as f32 + (panel_w as f32 - total) / 2.0;
    c.print(sx, sy, f.serial, SERIAL_PX, BLACK);
    let bx = sx + hash_w + 10.0;
    c.outline(bx, sy + 1.0, box_w, SERIAL_PX * 1.25, 2.0, BLACK);
    c.print(
        bx + box_w * 0.28,
        sy + 2.0,
        &f.dirty_digit.to_string(),
        SERIAL_PX * 0.85,
        BLACK,
    );

    let mut ry = panel_y as f32 + panel_h as f32 + 10.0;
    let right_edge = (panel_x + panel_w) as f32;
    if model == "MODEL NO. KONKR ADV" {
        // The original 217 px KONKR mark used 56 px of the 92 px lower
        // zone, leaving the edition and version printed beyond the sticker.
        // Reserve explicit space for all four rows. Keep the source artwork
        // vector-based, and give the very wide POCKET ADVANCE wordmark more
        // horizontal pixels so its thin strokes remain clean.
        ry = panel_y as f32 + panel_h as f32 + 3.0;
        if let Some((logo, (w, h))) = vector_mark(KONKR_SVG, 155) {
            c.blit((right_edge - w as f32 - 14.0) as u32,
                   ry as u32, &logo, w, h);
            ry += h as f32 + 2.0;
        }
        if let Some((edition, (w, h))) = vector_mark(POCKET_ADVANCE_SVG, 305) {
            c.blit((right_edge - w as f32 - 14.0) as u32,
                   ry as u32, &edition, w, h);
            ry += h as f32 + 2.0;
        }
        for line in [copyright, home] {
            let px = SMALL_PX * 0.88;
            let lw = c.print_measure(line, px);
            ry = c.print(right_edge - lw - 14.0, ry, line, px, WHITE);
        }
    } else {
        // Upstream ANBERNIC original is unaffected by KONKR layout fixes.
        if let Some(mark) = wordmark(WORDMARK_W) {
            let (mw, mh) = mark.1;
            c.blit(
                (right_edge - mw as f32 - 10.0) as u32,
                ry as u32,
                &mark.0,
                mw,
                mh,
            );
            ry += mh as f32 + 4.0;
        }
        for line in [copyright, home] {
            let lw = c.print_measure(line, SMALL_PX);
            ry = c.print(right_edge - lw - 10.0, ry, line, SMALL_PX, WHITE);
        }
    }

    UndoFace {
        rgba: c.px,
        w: STICKER_W,
        h: STICKER_H,
    }
}

fn wordmark(w: u32) -> Option<(Vec<u8>, (u32, u32))> {
    vector_mark(WORDMARK_SVG, w)
}

fn vector_mark(source: &str, w: u32) -> Option<(Vec<u8>, (u32, u32))> {
    let tree = usvg::Tree::from_str(source, &usvg::Options::default()).ok()?;
    let size = tree.size();
    let h = (w as f32 * size.height() / size.width()).round() as u32;
    Some((render_svg(source, w, h)?, (w, h)))
}

pub fn draw_sticker(face: Option<TexId>, out: &mut Vec<Draw>) {
    let Some(tex) = face else {
        return;
    };
    out.push(Draw::Tex {
        x: (slot_gfx::OUT_W as f32 - STICKER_W as f32) / 2.0,
        y: (slot_gfx::OUT_H as f32 - STICKER_H as f32) / 2.0,
        w: STICKER_W as f32,
        h: STICKER_H as f32,
        tex,
        alpha: 1.0,
    });
}
