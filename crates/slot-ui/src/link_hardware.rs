//! Multiplayer artwork uses the same family and material as the inserted cartridge.
use crate::{bitmaps, density, native_assets, CartFace, Face};

const CABLES: [&[u8]; bitmaps::CONSOLE_SLOTS] = [
    include_bytes!("../assets/link/fc.png"), include_bytes!("../assets/link/nes.png"),
    include_bytes!("../assets/link/snes.png"), include_bytes!("../assets/link/md.png"),
    include_bytes!("../assets/link/gb.png"), include_bytes!("../assets/link/gbc.png"),
    include_bytes!("../assets/link/gba.png"),
];
const RFU: &[u8] = include_bytes!("../assets/link/rfu.png");
pub const BAR_Y: f32 = slot_gfx::OUT_H as f32 - bitmaps::SLOT_H as f32;
pub const SOCKET_CENTRE: f32 = 17.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hardware {
    family: usize,
    theme: usize,
    pub wireless: bool,
}

impl Hardware {
    pub fn new(family: usize, theme: usize, wireless: bool) -> Self {
        assert!(family < CABLES.len() && theme < bitmaps::BAR_THEMES);
        Self { family, theme, wireless: wireless && family == 6 }
    }

    pub fn dual(self) -> bool { matches!(self.family, 1..=3) }
    pub fn side_exit(self) -> bool { self.family == 0 }
    pub fn dark_ink(self) -> bool { bitmaps::console_slot_takes_dark_ink(self.family, self.theme) }
    pub fn lip(self) -> f32 {
        BAR_Y + SOCKET_CENTRE + [10.3, 10.0, 7.4, 10.0, 7.3, 6.0, 7.7][self.family]
    }
    pub fn shoulder(self) -> f32 {
        if self.wireless { 127.0 } else { [218., 218., 218., 218., 220., 220., 219.][self.family] }
    }
    pub fn offset(self, join: bool) -> f32 {
        if !self.dual() { 0.0 } else if join { 62.0 } else { -62.0 }
    }
}

// All faces stay in Slot's logical units. Each 3x master has three columns:
// Host grip, Join grip, metal shank. The next row holds the three socket themes.
// The RFU master also has the two finishes of the left release button.
pub struct HardwareArt {
    pub wall: CartFace,
    pub front: CartFace,
    pub socket: CartFace,
    pub rim: CartFace,
    pub grips: [CartFace; 2],
    pub nose: CartFace,
    pub buttons: Option<[[CartFace; 2]; 2]>,
}

fn front_of(face: &CartFace, cut: f32) -> CartFace {
    let (tw, th) = face.texels();
    let mut rgba = face.rgba.clone();
    let rows = ((cut * density::scale()).round() as u32).min(th);
    rgba[..(rows * tw * 4) as usize].fill(0);
    CartFace { w: face.w, h: face.h, rgba }
}

fn bar(key: Hardware) -> CartFace {
    let (_, master) = bitmaps::home_slot_png(key.family, key.theme);
    let (h, w) = (bitmaps::SLOT_H, crate::out_w());
    let (tw, th, sw) = (density::px(w), density::px(h), density::px(bitmaps::SLOT_W));
    let source = native_assets::png_at(master, sw, th);
    let guard = density::px(158);
    let span = density::px(112);
    let wing = |d: u32| {
        if d <= guard { return d; }
        let n = (d - guard) % (2 * span);
        guard - n.min(2 * span - n)
    };
    let columns: Vec<_> = (0..tw).map(|x| {
        let t = ((x as f32 / tw as f32 - 0.44) / 0.12).clamp(0.0, 1.0);
        (wing(x), sw - 1 - wing(tw - 1 - x), t * t * (3.0 - 2.0 * t))
    }).collect();
    let mut rgba = Vec::with_capacity((tw * th * 4) as usize);
    for y in 0..th {
        for &(l, r, t) in &columns {
            let a = ((y * sw + l) * 4) as usize;
            let b = ((y * sw + r) * 4) as usize;
            for c in 0..3 {
                rgba.push((source[a + c] as f32 + (source[b + c] as f32 - source[a + c] as f32) * t).round() as u8);
            }
            rgba.push(255);
        }
    }
    let mut face = CartFace { w, h, rgba };
    if key.dual() {
        let ink = if key.dark_ink() { [55, 52, 56] } else { [225, 221, 217] };
        for (dx, label) in [(-115.0,"1"), (115.0,"2")] {
            crate::link_art::port_number(&mut face, w as f32 / 2.0 + dx, label, ink);
        }
    }
    face
}

pub fn hardware_art(key: Hardware) -> HardwareArt {
    let (bytes, w, h, cw) = if key.wireless { (RFU, 240, 151, 168) }
        else if key.side_exit() { (CABLES[key.family], 244, 244, 110) }
        else { (CABLES[key.family], 120, 244, 100) };
    let (pixels, aw, _) = crate::decode_png(bytes).expect("bundled link artwork");
    let cell = |x: u32, y: u32, w: u32, h: u32| {
        let (sw, sh) = (3 * w, 3 * h);
        let mut src = Vec::with_capacity((sw * sh * 4) as usize);
        for row in 3 * y..3 * y + sh {
            let off = ((row * aw + x * 3) * 4) as usize;
            src.extend_from_slice(&pixels[off..off + (sw * 4) as usize]);
        }
        CartFace { w, h, rgba: native_assets::resample(&src, sw, sh, density::px(w), density::px(h)) }
    };
    let wall = bar(key);
    let socket = cell(key.theme as u32 * w, h, cw, 32);
    let front = front_of(&wall, key.lip() - BAR_Y);
    let rim = front_of(&socket, key.lip() - (BAR_Y + SOCKET_CENTRE - 16.0));
    let buttons = key.wireless.then(|| [0, 1].map(|side| {
        let left = cell(side * 40, h + 32, 40, 40);
        let right = crate::link_art::mirror(&left);
        [left, right]
    }));
    HardwareArt { wall, front, socket, rim, grips: [cell(0, 0, w, h), cell(w, 0, w, h)],
        nose: cell(2 * w, 0, w, h), buttons }
}
