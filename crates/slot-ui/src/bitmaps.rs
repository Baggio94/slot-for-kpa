use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};


use crate::art::decode_png_bytes;

#[derive(Copy, Clone, PartialEq, Eq, Debug, Hash)]
pub enum Mould {
    Gba,
    Notched,
    Rounded,
    Home(crate::home::HomeMould),
}

pub const GBA_CART_W: u32 = 360;
pub const GBA_CART_H: u32 = 212;
pub const GB_CART_W: u32 = 312;
pub const GB_CART_H: u32 = 337;
pub const BOARD_W: u32 = GBA_CART_W;
pub const BOARD_H: u32 = GBA_CART_H;
pub const SOCKET_W: u32 = 64;
pub const SOCKET_H: u32 = 46;
pub const CHIP_W: u32 = 59;
pub const CHIP_H: u32 = 41;
pub const SLOT_W: u32 = slot_gfx::BASE_W;
pub const SLOT_H: u32 = 58;
pub const PORT_W: u32 = slot_gfx::BASE_W;
pub const PORT_H: u32 = SLOT_H;
pub const PLUG_W: u32 = 92;
pub const PLUG_H: u32 = 244;
pub const ADAPTER_W: u32 = 290;
pub const ADAPTER_H: u32 = 180;
pub const STICKER_W: u32 = 660;
pub const STICKER_H: u32 = 228;

const GBA_CART_PNG: &[u8] = include_bytes!("../../../assets/gba_cart.png");
const GB_CART_PNG: &[u8] = include_bytes!("../../../assets/gb_cart.png");
const GBC_CART_PNG: &[u8] = include_bytes!("../../../assets/gbc_cart.png");
const GBA_INTERIOR_PNG: &[u8] = include_bytes!("../../../assets/gba_interior.png");
const GB_INTERIOR_PNG: &[u8] = include_bytes!("../../../assets/gb_interior.png");
const GBC_INTERIOR_PNG: &[u8] = include_bytes!("../../../assets/gbc_interior.png");
const GB_BOARD_PNG: &[u8] = include_bytes!("../../../assets/gb_board.png");
const BOARD_PNG: &[u8] = include_bytes!("../../../assets/board.png");
const SOCKET_PNG: &[u8] = include_bytes!("../../../assets/socket.png");
const CHIP_PNG: &[u8] = include_bytes!("../../../assets/chip.png");
const BEZEL_GB_PNG: &[u8] = include_bytes!("../../../assets/bezel_gb.png");
const BEZEL_GBC_PNG: &[u8] = include_bytes!("../../../assets/bezel_gbc.png");
// Watomsk's overlays, cut for 3:2 by tools/watomsk-overlays/make.py.
const OVERLAY_GB_PNG: &[u8] = include_bytes!("../../../assets/overlay_watomsk_gb.png");
const OVERLAY_POCKET_PNG: &[u8] = include_bytes!("../../../assets/overlay_watomsk_pocket.png");
const OVERLAY_COLOR_PNG: &[u8] = include_bytes!("../../../assets/overlay_watomsk_color.png");
pub const OVERLAY_W: u32 = 960;
pub const OVERLAY_H: u32 = 640;
const SLOT_BACK_DARK_PNG: &[u8] = include_bytes!("../../../assets/slot_back_dark.png");
const SLOT_BACK_LIGHT_PNG: &[u8] = include_bytes!("../../../assets/slot_back_light.png");
const SLOT_FRONT_DARK_PNG: &[u8] = include_bytes!("../../../assets/slot_front_dark.png");
const SLOT_FRONT_LIGHT_PNG: &[u8] = include_bytes!("../../../assets/slot_front_light.png");
const PORT_DARK_PNG: &[u8] = include_bytes!("../../../assets/link_port_dark.png");
const PORT_LIGHT_PNG: &[u8] = include_bytes!("../../../assets/link_port_light.png");
const PLUG_HOST_PNG: &[u8] = include_bytes!("../../../assets/link_plug_host.png");
const PLUG_JOIN_PNG: &[u8] = include_bytes!("../../../assets/link_plug_join.png");
const ADAPTER_PNG: &[u8] = include_bytes!("../../../assets/link_adapter.png");
const STICKER_PNG: &[u8] = include_bytes!("../../../assets/sticker.png");

fn load(bytes: &'static [u8], w: u32, h: u32, slot: &'static OnceLock<Vec<u8>>) -> &'static [u8] {
    slot.get_or_init(|| {
        let (mut rgba, sw, sh) = decode_png_bytes(bytes).expect("a template png decodes");
        if sh == h && sw > w {
            rgba = crop_centre(&rgba, sw, sh, w);
        } else if (sw, sh) != (w, h) {
            rgba = crate::native_assets::resample(&rgba, sw, sh, w, h);
        }
        // Transparent pixels are often stored as white. The cart is sampled with a linear
        // filter, which would pull that white in along every edge.
        bleed(&mut rgba, w, h);
        rgba
    })
}

/// Copy an opaque neighbour's colour into each transparent pixel, leaving its alpha at 0.
fn bleed(rgba: &mut [u8], w: u32, h: u32) {
    let src = rgba.to_vec();
    let (w, h) = (w as i32, h as i32);
    for y in 0..h {
        for x in 0..w {
            let i = ((y * w + x) * 4) as usize;
            if src[i + 3] != 0 {
                continue;
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx < 0 || ny < 0 || nx >= w || ny >= h {
                    continue;
                }
                let n = ((ny * w + nx) * 4) as usize;
                if src[n + 3] == 0 {
                    continue;
                }
                rgba[i..i + 3].copy_from_slice(&src[n..n + 3]);
                break;
            }
        }
    }
}

fn crop_centre(src: &[u8], sw: u32, sh: u32, w: u32) -> Vec<u8> {
    let left = ((sw - w) / 2) as usize;
    let mut out = Vec::with_capacity((w * sh * 4) as usize);
    for row in src.chunks_exact(sw as usize * 4) {
        out.extend_from_slice(&row[left * 4..(left + w as usize) * 4]);
    }
    out
}

pub fn gba_cart() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(GBA_CART_PNG, GBA_CART_W, GBA_CART_H, &P)
}

pub fn gb_cart() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(GB_CART_PNG, GB_CART_W, GB_CART_H, &P)
}

pub fn gbc_cart() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(GBC_CART_PNG, GB_CART_W, GB_CART_H, &P)
}

/// The inside of a GBA cart, from `assets/gba_interior.png`. Same size as the cart and
/// the board, so the carousel can stack the three 1:1. Its colour is set by the game, with the
/// same overlay blend the shells use.
pub fn gba_interior() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(GBA_INTERIOR_PNG, BOARD_W, BOARD_H, &P)
}

/// The back of a Game Boy pak, from `assets/gb_interior.png`.
pub fn gb_interior() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(GB_INTERIOR_PNG, GB_CART_W, GB_CART_H, &P)
}

/// The back of a Game Boy Color pak, from `assets/gbc_interior.png`.
pub fn gbc_interior() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(GBC_INTERIOR_PNG, GB_CART_W, GB_CART_H, &P)
}

/// The Game Boy PCB, from `assets/gb_board.png`, drawn between the pak's back and front plastic.
pub fn gb_board() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(GB_BOARD_PNG, GB_CART_W, GB_CART_H, &P)
}

pub fn board() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(BOARD_PNG, BOARD_W, BOARD_H, &P)
}

pub fn socket() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(SOCKET_PNG, SOCKET_W, SOCKET_H, &P)
}

pub fn chip() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(CHIP_PNG, CHIP_W, CHIP_H, &P)
}

/// The slot. bezels' 3x masters (2160x1440), for drawing at panel density.
pub fn bezel_master(gbc: bool) -> &'static [u8] {
    if gbc { BEZEL_GBC_PNG } else { BEZEL_GB_PNG }
}

/// Official Slot v1.7 full-panel transparent bezels, at their 720x480.
pub fn bezel_gb() -> &'static [u8] {
    static P: OnceLock<Vec<u8>>=OnceLock::new();
    load(BEZEL_GB_PNG,720,480,&P)
}
pub fn bezel_gbc() -> &'static [u8] {
    static P: OnceLock<Vec<u8>>=OnceLock::new();
    load(BEZEL_GBC_PNG,720,480,&P)
}

/// Watomsk full-panel overlays, already at the native 960x640.
pub fn overlay_gb() -> &'static [u8] {
    static P: OnceLock<Vec<u8>>=OnceLock::new();
    load(OVERLAY_GB_PNG,OVERLAY_W,OVERLAY_H,&P)
}
pub fn overlay_pocket() -> &'static [u8] {
    static P: OnceLock<Vec<u8>>=OnceLock::new();
    load(OVERLAY_POCKET_PNG,OVERLAY_W,OVERLAY_H,&P)
}
pub fn overlay_color() -> &'static [u8] {
    static P: OnceLock<Vec<u8>>=OnceLock::new();
    load(OVERLAY_COLOR_PNG,OVERLAY_W,OVERLAY_H,&P)
}

pub fn slot_back() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(SLOT_BACK_DARK_PNG, SLOT_W, SLOT_H, &P)
}

pub fn slot_back_light() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(SLOT_BACK_LIGHT_PNG, SLOT_W, SLOT_H, &P)
}

pub fn slot_front() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(SLOT_FRONT_DARK_PNG, SLOT_W, SLOT_H, &P)
}

pub fn slot_front_light() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(SLOT_FRONT_LIGHT_PNG, SLOT_W, SLOT_H, &P)
}

/// A console's slot bar (`family`: 0 Famicom, 1 NES, 2 Super Nintendo,
/// 3 Mega Drive, 4 Game Boy, 5 Game Boy Color, 6 GBA) in a bar theme: 0
/// the console's own colours, 1 dark (black and grey), 2 light (light
/// versions of its colours) (Siham 2026-11-03).
/// A console's slot bar masters, PNG bytes at 3x (`family`, `theme` as `home_slot`).
pub fn home_slot_png(family: usize, theme: usize) -> (&'static [u8], &'static [u8]) {
    macro_rules! bars {
        ($part:literal, $suffix:literal) => {[
            include_bytes!(concat!("../../../assets/home/slot_", $part, "_fc", $suffix, ".png")).as_slice(),
            include_bytes!(concat!("../../../assets/home/slot_", $part, "_nes", $suffix, ".png")).as_slice(),
            include_bytes!(concat!("../../../assets/home/slot_", $part, "_snes", $suffix, ".png")).as_slice(),
            include_bytes!(concat!("../../../assets/home/slot_", $part, "_md", $suffix, ".png")).as_slice(),
            include_bytes!(concat!("../../../assets/slot_", $part, "_gb", $suffix, ".png")).as_slice(),
            include_bytes!(concat!("../../../assets/slot_", $part, "_gbc", $suffix, ".png")).as_slice(),
            include_bytes!(concat!("../../../assets/slot_", $part, "_gba", $suffix, ".png")).as_slice(),
        ]};
    }
    const BACK: [[&[u8]; CONSOLE_SLOTS]; BAR_THEMES] = [bars!("back", ""), bars!("back", "_dark"), bars!("back", "_light")];
    const FRONT: [[&[u8]; CONSOLE_SLOTS]; BAR_THEMES] = [bars!("front", ""), bars!("front", "_dark"), bars!("front", "_light")];
    let (t, i) = (theme.min(BAR_THEMES - 1), family.min(CONSOLE_SLOTS - 1));
    (BACK[t][i], FRONT[t][i])
}

/// A console's slot bar at its logical 720x58 (see `home_slot_png`).
pub fn home_slot(family: usize, theme: usize) -> (&'static [u8], &'static [u8]) {
    static B: [[OnceLock<Vec<u8>>; CONSOLE_SLOTS]; BAR_THEMES] = [const { [const { OnceLock::new() }; CONSOLE_SLOTS] }; BAR_THEMES];
    static F: [[OnceLock<Vec<u8>>; CONSOLE_SLOTS]; BAR_THEMES] = [const { [const { OnceLock::new() }; CONSOLE_SLOTS] }; BAR_THEMES];
    let (t, i) = (theme.min(BAR_THEMES - 1), family.min(CONSOLE_SLOTS - 1));
    let (back, front) = home_slot_png(family, theme);
    (load(back, SLOT_W, SLOT_H, &B[t][i]), load(front, SLOT_W, SLOT_H, &F[t][i]))
}

/// slot.'s own bar masters (back, front), dark or light, at 3x.
pub fn slot_bar_png(light: bool) -> (&'static [u8], &'static [u8]) {
    if light { (SLOT_BACK_LIGHT_PNG, SLOT_FRONT_LIGHT_PNG) } else { (SLOT_BACK_DARK_PNG, SLOT_FRONT_DARK_PNG) }
}

/// How many colour themes the console slot bars come in (see `home_slot`).
pub const BAR_THEMES: usize = 3;

/// How many console slot bars `home_slot` draws.
pub const CONSOLE_SLOTS: usize = 7;

/// Which console bar a cart goes into (see `home_slot`).
pub fn console_slot_family(cart: &slot_store::Cart) -> usize {
    home_slot_family(cart).unwrap_or(match cart.platform {
        slot_store::Platform::Gb => 4,
        slot_store::Platform::Gbc => 5,
        _ => 6,
    })
}

/// Whether the clock and battery printed on a console's bar are black or
/// white: white on the dark bars, black on the light ones, and in the
/// consoles' own colours black on light plastic, white on the GBA's indigo
/// and the Mega Drive's black (Siham 2026-11-02).
pub fn console_slot_takes_dark_ink(family: usize, theme: usize) -> bool {
    match theme {
        1 => false,
        2 => true,
        _ => !matches!(family, 3 | 6),
    }
}

/// What closes a home slot's hole, at its logical size: the NES flap
/// (family 1), the Super Nintendo's two flaps (2), the Mega Drive's two
/// doors (3). The Famicom's slot (0) is open.
pub fn home_closer(family: usize) -> Vec<(&'static [u8], u32, u32)> {
    const NES: &[u8] = include_bytes!("../../../assets/home/closer_nes.png");
    const SNES_TOP: &[u8] = include_bytes!("../../../assets/home/closer_snes_top.png");
    const SNES_BOTTOM: &[u8] = include_bytes!("../../../assets/home/closer_snes_bottom.png");
    const MD_LEFT: &[u8] = include_bytes!("../../../assets/home/closer_md_left.png");
    const MD_RIGHT: &[u8] = include_bytes!("../../../assets/home/closer_md_right.png");
    static C: [OnceLock<Vec<u8>>; 5] = [const { OnceLock::new() }; 5];
    match family {
        1 => vec![(load(NES, 248, 26, &C[0]), 248, 26)],
        2 => vec![(load(SNES_TOP, 244, 13, &C[1]), 244, 13), (load(SNES_BOTTOM, 244, 13, &C[2]), 244, 13)],
        3 => vec![(load(MD_LEFT, 124, 26, &C[3]), 124, 26), (load(MD_RIGHT, 124, 26, &C[4]), 124, 26)],
        _ => Vec::new(),
    }
}

/// Which home slot bar a cart goes into, None for the GBA / Game Boy bar.
pub fn home_slot_family(cart: &slot_store::Cart) -> Option<usize> {
    use crate::home::HomeMould as M;
    Some(match crate::home::home_mould_of(cart)? {
        M::Famicom | M::FamicomNamco | M::FamicomKonami => 0,
        M::Nes => 1,
        M::Snes | M::SnesUs => 2,
        M::MegaDrive | M::MegaDriveJp | M::MegaDriveEa => 3,
    })
}

pub fn link_port() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(PORT_DARK_PNG, PORT_W, PORT_H, &P)
}

pub fn link_port_light() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(PORT_LIGHT_PNG, PORT_W, PORT_H, &P)
}

pub fn link_plug_host() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(PLUG_HOST_PNG, PLUG_W, PLUG_H, &P)
}

pub fn link_plug_join() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(PLUG_JOIN_PNG, PLUG_W, PLUG_H, &P)
}

pub fn link_adapter() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(ADAPTER_PNG, ADAPTER_W, ADAPTER_H, &P)
}

pub fn sticker() -> &'static [u8] {
    static P: OnceLock<Vec<u8>> = OnceLock::new();
    load(STICKER_PNG, STICKER_W, STICKER_H, &P)
}

// Shells are hand drawn. There is no Game Boy clear file yet, so a clear Game Boy pak uses
// the neutral shell at 80%.
const GBA_SHELL_PNG: &[u8] = include_bytes!("../../../assets/gba_neutral.png");
const GB_SHELL_PNG: &[u8] = include_bytes!("../../../assets/gb_neutral.png");
const GBC_SHELL_PNG: &[u8] = include_bytes!("../../../assets/gbc_neutral.png");
const GBA_CLEAR_PNG: &[u8] = include_bytes!("../../../assets/gba_clear.png");
const GB_CLEAR_PNG: &[u8] = include_bytes!("../../../assets/gb_neutral.png");
const GBC_CLEAR_PNG: &[u8] = include_bytes!("../../../assets/gbc_clear.png");

/// A bundled PNG master at panel density: `px(w)` x `px(h)` texels for a
/// `w` x `h` logical layer, filtered once from the master's own pixels
/// (reduced from a 3x master, enlarged from a 1x one) and kept.
pub(crate) fn dense_png(bytes: &'static [u8], w: u32, h: u32) -> Arc<Vec<u8>> {
    type Cache = Mutex<HashMap<(usize, u32, u32, u32), Arc<Vec<u8>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let key = (bytes.as_ptr() as usize, w, h, crate::density::scale().to_bits());
    if let Some(hit) = CACHE.get_or_init(Mutex::default).lock()
        .unwrap_or_else(|e| e.into_inner()).get(&key) {
        return hit.clone();
    }
    let (tw, th) = (crate::density::px(w), crate::density::px(h));
    let rgba = Arc::new(crate::native_assets::png_at(bytes, tw, th));
    CACHE.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner())
        .insert(key, rgba.clone());
    rgba
}

/// The inside of a GBA cart at panel density.
pub(crate) fn dense_gba_interior() -> Arc<Vec<u8>> {
    dense_png(GBA_INTERIOR_PNG, BOARD_W, BOARD_H)
}

/// The neutral shell for a mould at panel density, from its master.
pub(crate) fn dense_shell(mould: Mould, clear: bool) -> Arc<Vec<u8>> {
    let (bytes, w, h) = match (mould, clear) {
        (Mould::Gba, false) => (GBA_SHELL_PNG, GBA_CART_W, GBA_CART_H),
        (Mould::Notched, false) => (GB_SHELL_PNG, GB_CART_W, GB_CART_H),
        (Mould::Rounded, false) => (GBC_SHELL_PNG, GB_CART_W, GB_CART_H),
        (Mould::Gba, true) => (GBA_CLEAR_PNG, GBA_CART_W, GBA_CART_H),
        (Mould::Notched, true) => (GB_CLEAR_PNG, GB_CART_W, GB_CART_H),
        (Mould::Rounded, true) => (GBC_CLEAR_PNG, GB_CART_W, GB_CART_H),
        (Mould::Home(m), _) => {
            let (w, h) = m.size();
            (m.art().neutral, w, h)
        }
    };
    dense_png(bytes, w, h)
}

/// The neutral shell asset for a mould, opaque or the separate clear one. The game's colour is
/// blended onto it by the caller.
pub fn shell_asset(mould: Mould, clear: bool) -> &'static [u8] {
    static GBA: OnceLock<Vec<u8>> = OnceLock::new();
    static GB: OnceLock<Vec<u8>> = OnceLock::new();
    static GBC: OnceLock<Vec<u8>> = OnceLock::new();
    static GBA_CLEAR: OnceLock<Vec<u8>> = OnceLock::new();
    static GB_CLEAR: OnceLock<Vec<u8>> = OnceLock::new();
    static GBC_CLEAR: OnceLock<Vec<u8>> = OnceLock::new();
    let (bytes, w, h, slot) = match (mould, clear) {
        (Mould::Gba, false) => (GBA_SHELL_PNG, GBA_CART_W, GBA_CART_H, &GBA),
        (Mould::Notched, false) => (GB_SHELL_PNG, GB_CART_W, GB_CART_H, &GB),
        (Mould::Rounded, false) => (GBC_SHELL_PNG, GB_CART_W, GB_CART_H, &GBC),
        (Mould::Gba, true) => (GBA_CLEAR_PNG, GBA_CART_W, GBA_CART_H, &GBA_CLEAR),
        (Mould::Notched, true) => (GB_CLEAR_PNG, GB_CART_W, GB_CART_H, &GB_CLEAR),
        (Mould::Rounded, true) => (GBC_CLEAR_PNG, GB_CART_W, GB_CART_H, &GBC_CLEAR),
        // No clear home shells are drawn: a clear one is the neutral at 80%.
        (Mould::Home(m), _) => return crate::home::neutral(m),
    };
    load(bytes, w, h, slot)
}
