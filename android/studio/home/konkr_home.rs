//! KONKR: NES / Famicom, Super Nintendo and Mega Drive carts in Cart Studio.
//!
//! The moulds, their wells and the automatic shell choice are slot-ui's own
//! `home_art.rs`, compiled here by `#[path]`, so Studio draws the shells the
//! device draws. The face follows slot-ui's `cart_face_with` for a home cart
//! at 1x: the neutral shell under an Overlay of the game's colour, the label
//! covering its well, the well's shade, the publisher's decal and the
//! outline. A clear shell shows its board through it.
//!
//! A home platform crosses the wasm boundary as `NES`, `SNES` or `MD`, or with
//! the mould after a colon (`NES:famicom`) where the label's shape needs it.
//! Every face is drawn on the system's tallest box, standing on its bottom
//! edge, so the page sizes one canvas per platform as it does for GB and GBA.

use std::sync::OnceLock;

use crate::home_art::{home_pick, HomeMould, HomeSystem, BOARD_K, FRONT_K, HOME_CART_W};
use crate::{art, cart, label, text};

/// The longer side of a home label, in pixels: the well's shape at about the
/// cost of a Game Boy label.
const LABEL_LONG: f32 = 1024.0;
/// How much of a clear shell is opaque, as slot-ui's `CLEAR_ALPHA`.
const CLEAR_ALPHA: u8 = 204;
const NEUTRAL: u8 = 128;
const PAD: u32 = 10;
const MAX_LINES: usize = 3;
const MIN_PX: f32 = 10.0;

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
enum Finish {
    Solid,
    Clear,
    Glitter,
}

/// The system a platform names, and the mould after its colon, if any.
pub fn system_of(platform: &str) -> Option<(HomeSystem, Option<HomeMould>)> {
    let (name, mould) = match platform.split_once(':') {
        Some((name, mould)) => (name, Some(mould)),
        None => (platform, None),
    };
    let system = match name {
        "NES" => HomeSystem::Nes,
        "SNES" => HomeSystem::Snes,
        "MD" => HomeSystem::Md,
        _ => return None,
    };
    let mould = mould.and_then(HomeMould::of_name).filter(|m| m.system() == system);
    Some((system, mould))
}

/// The Mega Drive header's copyright field, which names Electronic Arts.
fn maker(system: HomeSystem, head: &[u8]) -> String {
    if system != HomeSystem::Md || head.len() < 0x120 {
        return String::new();
    }
    head[0x110..0x120].iter().map(|b| *b as char).collect()
}

/// A shell value as `cart_shell.txt` holds it: outline, RRGGBB, finish. An
/// outline of another system (or `auto`) leaves the mould to the cart, as the
/// device does.
fn parse_value(system: HomeSystem, value: &str) -> Option<(Option<HomeMould>, [u8; 3], Finish)> {
    let mut words = value.split_whitespace();
    let outline = words.next()?;
    let hex = words.next()?;
    let finish = match words.next()? {
        "solid" => Finish::Solid,
        "clear" => Finish::Clear,
        "glitter" => Finish::Glitter,
        _ => return None,
    };
    if words.next().is_some() || hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let colour = [0, 2, 4].map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0));
    let mould = HomeMould::of_name(outline).filter(|m| m.system() == system);
    Some((mould, colour, finish))
}

fn resolve(
    system: HomeSystem,
    named: Option<HomeMould>,
    stem: &str,
    head: &[u8],
    shell: &str,
) -> (HomeMould, [u8; 3], Finish) {
    let (auto, colour, glitter) = home_pick(system, stem, "", &maker(system, head));
    match parse_value(system, shell) {
        Some((mould, colour, finish)) => (mould.or(named).unwrap_or(auto), colour, finish),
        None => (
            named.unwrap_or(auto),
            colour,
            if glitter { Finish::Glitter } else { Finish::Solid },
        ),
    }
}

/// The mould this cart is drawn with, by name; empty for a GB or GBA cart.
pub fn outline(platform: &str, stem: &str, head: &[u8], shell: &str) -> String {
    match system_of(platform) {
        Some((system, named)) => resolve(system, named, stem, head, shell).0.name().to_string(),
        None => String::new(),
    }
}

/// The shell the device picks with no choice made, as a value; empty for a GB
/// or GBA cart.
pub fn auto_value(platform: &str, stem: &str, head: &[u8]) -> String {
    let Some((system, _)) = system_of(platform) else {
        return String::new();
    };
    let (mould, c, glitter) = home_pick(system, stem, "", &maker(system, head));
    let finish = if glitter { "glitter" } else { "solid" };
    format!("{} {:02x}{:02x}{:02x} {finish}", mould.name(), c[0], c[1], c[2])
}

/// The canvas a platform's carts are drawn on: the width of every home cart,
/// the height of the system's tallest.
pub fn cart_size(platform: &str) -> Option<(u32, u32)> {
    let (system, _) = system_of(platform)?;
    Some(box_of(system))
}

fn box_of(system: HomeSystem) -> (u32, u32) {
    let h = HomeMould::ALL
        .into_iter()
        .filter(|m| m.system() == system)
        .map(|m| m.size().1)
        .max()
        .unwrap_or(HOME_CART_W);
    (HOME_CART_W, h)
}

/// The label a mould's well wants, at its own shape.
pub fn label_size(platform: &str) -> Option<(u32, u32)> {
    let (system, named) = system_of(platform)?;
    Some(well_label(named.unwrap_or(HomeMould::default_of(system))))
}

fn well_label(m: HomeMould) -> (u32, u32) {
    let (_, _, w, h) = m.art().label;
    let k = LABEL_LONG / w.max(h) as f32;
    (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1))
}

/// Whether a label of this size was made for a home cart's well, which slot
/// shows whole.
pub fn is_home_label(lw: u32, lh: u32) -> bool {
    HomeMould::ALL.into_iter().any(|m| well_label(m) == (lw, lh))
}

/// A card's label on its cart. `None` for a GB or GBA cart.
pub fn existing_face(png: &[u8], platform: &str, head: &[u8], shell: &str, stem: &str) -> Option<Vec<u8>> {
    let (system, named) = system_of(platform)?;
    let art = art::decode(png);
    let label = art.as_ref().map(|(px, w, h)| (px.as_slice(), *w, *h));
    Some(draw(system, resolve(system, named, stem, head, shell), label, stem))
}

/// A composed label on its cart. `None` for a GB or GBA cart.
pub fn label_face(l: &label::Label, platform: &str, head: &[u8], shell: &str, stem: &str) -> Option<Vec<u8>> {
    let (system, named) = system_of(platform)?;
    Some(draw(system, resolve(system, named, stem, head, shell), Some(l.konkr_parts()), stem))
}

fn decoded(bytes: &'static [u8]) -> (Vec<u8>, u32, u32) {
    art::decode(bytes).expect("a home cart png decodes")
}

/// A master drawn at `k` times the logical size, averaged down to 1x.
fn at_1x(bytes: &'static [u8], m: HomeMould, k: u32) -> Vec<u8> {
    let (big, bw, _) = decoded(bytes);
    let (w, h) = m.size();
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0u32; 4];
            for dy in 0..k {
                for dx in 0..k {
                    let i = (((y * k + dy) * bw + x * k + dx) * 4) as usize;
                    let a = big[i + 3] as u32;
                    for c in 0..3 {
                        acc[c] += big[i + c] as u32 * a;
                    }
                    acc[3] += a;
                }
            }
            let o = ((y * w + x) * 4) as usize;
            if acc[3] > 0 {
                for c in 0..3 {
                    out[o + c] = ((acc[c] + acc[3] / 2) / acc[3]) as u8;
                }
            }
            out[o + 3] = ((acc[3] + (k * k) / 2) / (k * k)) as u8;
        }
    }
    out
}

fn neutral(m: HomeMould) -> &'static [u8] {
    static CACHE: [OnceLock<Vec<u8>>; 9] = [const { OnceLock::new() }; 9];
    CACHE[m.index()].get_or_init(|| at_1x(m.art().neutral, m, FRONT_K))
}

fn decal(m: HomeMould) -> Option<&'static [u8]> {
    static CACHE: [OnceLock<Vec<u8>>; 9] = [const { OnceLock::new() }; 9];
    let bytes = m.art().decal?;
    Some(CACHE[m.index()].get_or_init(|| at_1x(bytes, m, FRONT_K)))
}

/// The bare board at 1x.
fn board(m: HomeMould) -> &'static [u8] {
    static CACHE: [OnceLock<Vec<u8>>; 9] = [const { OnceLock::new() }; 9];
    CACHE[m.index()].get_or_init(|| at_1x(m.art().board, m, BOARD_K))
}

/// Photoshop's Overlay of one channel, as slot-ui's.
fn overlay(base: u8, colour: u8) -> u8 {
    let (base, colour) = (i32::from(base), i32::from(colour));
    let out = if base >= i32::from(NEUTRAL) {
        colour + ((255 - colour) * (base - i32::from(NEUTRAL)) + 63) / 127
    } else {
        colour - (colour * (i32::from(NEUTRAL) - base) + 64) / i32::from(NEUTRAL)
    };
    out.clamp(0, 255) as u8
}

fn fleck(x: u32, y: u32) -> bool {
    let mut h = x.wrapping_mul(0x9e37_79b9) ^ y.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2c1b_3c6d);
    h ^= h >> 12;
    h % 100 == 0
}

fn blend_over(dst: &mut [u8], src: &[u8]) {
    for (back, top) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let a = u32::from(top[3]);
        if a == 0 {
            continue;
        }
        let da = u32::from(back[3]);
        let out_a = a * 255 + da * (255 - a);
        if out_a == 0 {
            continue;
        }
        for c in 0..3 {
            let n = u32::from(top[c]) * a * 255 + u32::from(back[c]) * da * (255 - a);
            back[c] = ((n + out_a / 2) / out_a).min(255) as u8;
        }
        back[3] = ((out_a + 127) / 255) as u8;
    }
}

fn ink(bg: [u8; 3]) -> [u8; 3] {
    let luma = 0.2126 * bg[0] as f32 + 0.7152 * bg[1] as f32 + 0.0722 * bg[2] as f32;
    if luma > 140.0 {
        [0x1a, 0x18, 0x16]
    } else {
        [0xf4, 0xf1, 0xea]
    }
}

/// slot's label for a cart with no art: its title on a colour of its own.
fn generated(lw: u32, lh: u32, stem: &str) -> Vec<u8> {
    let title = cart::clean_label(stem);
    let bg = cart::label_colour(&title);
    let mut rgba = Vec::with_capacity((lw * lh * 4) as usize);
    for _ in 0..lw * lh {
        rgba.extend_from_slice(&[bg[0], bg[1], bg[2], 255]);
    }
    if let Some(font) = text::label_font() {
        let room = lh.saturating_sub(2 * PAD) as f32;
        let layout = text::fit_box(
            font,
            &title,
            lw.saturating_sub(2 * PAD) as f32,
            room,
            MAX_LINES,
            room / MAX_LINES as f32,
            MIN_PX,
        );
        text::draw_centred(&mut rgba, lw, lh, &layout, ink(bg));
    }
    rgba
}

fn draw(
    system: HomeSystem,
    (m, colour, finish): (HomeMould, [u8; 3], Finish),
    label: Option<(&[u8], u32, u32)>,
    stem: &str,
) -> Vec<u8> {
    let (w, h) = m.size();
    let (lx, ly, lw, lh) = m.art().label;
    let clear = finish == Finish::Clear;
    let mask = neutral(m);
    let mut rgba = mask.to_vec();
    for y in 0..h {
        for x in 0..w {
            let on_label = (lx..lx + lw).contains(&x) && (ly..ly + lh).contains(&y);
            let i = ((y * w + x) * 4) as usize;
            if !on_label {
                for c in 0..3 {
                    rgba[i + c] = overlay(rgba[i + c], colour[c]);
                }
            }
            let own = u32::from(rgba[i + 3]);
            rgba[i + 3] = if on_label {
                255
            } else if clear {
                ((own * u32::from(CLEAR_ALPHA) + 127) / 255) as u8
            } else {
                own as u8
            };
        }
    }
    if let Some(decal) = decal(m) {
        blend_over(&mut rgba, decal);
    }
    if finish == Finish::Glitter {
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                if !fleck(x, y) || rgba[i + 3] == 0 {
                    continue;
                }
                for c in 0..3 {
                    rgba[i + c] = ((rgba[i + c] as u32 * 155 + 255 * 100) / 255) as u8;
                }
                rgba[i + 3] = 255;
            }
        }
    }
    let art = label
        .and_then(|(px, sw, sh)| art::cover_rgba(px, sw, sh, lw, lh))
        .unwrap_or_else(|| generated(lw, lh, stem));
    for y in 0..lh.min(h.saturating_sub(ly)) {
        for x in 0..lw.min(w.saturating_sub(lx)) {
            let s = ((y * lw + x) * 4) as usize;
            let a = art[s + 3] as u32;
            if a == 0 {
                continue;
            }
            let d = (((y + ly) * w + x + lx) * 4) as usize;
            for c in 0..3 {
                rgba[d + c] = ((art[s + c] as u32 * a + rgba[d + c] as u32 * (255 - a) + 127) / 255) as u8;
            }
        }
    }
    // The well's top and left edges shade the label.
    for y in ly..(ly + lh).min(h) {
        for x in lx..(lx + lw).min(w) {
            let d = (x - lx).min(y - ly) as f32;
            let shade = 0.28 * (1.0 - d / 6.0).max(0.0);
            if shade <= 0.0 {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                rgba[i + c] = (rgba[i + c] as f32 * (1.0 - shade)).round() as u8;
            }
        }
    }
    for (px, cover) in rgba.chunks_exact_mut(4).zip(mask.iter().skip(3).step_by(4)) {
        px[3] = ((px[3] as u32 * *cover as u32 + 127) / 255) as u8;
    }
    if clear {
        let mut under = board(m).to_vec();
        blend_over(&mut under, &rgba);
        rgba = under;
    }
    // Stand the cart on the bottom of its system's box.
    let (bw, bh) = box_of(system);
    let mut out = vec![0u8; (bw * bh * 4) as usize];
    let top = bh.saturating_sub(h);
    for y in 0..h.min(bh) {
        let s = (y * w * 4) as usize;
        let d = (((y + top) * bw) * 4) as usize;
        let n = (w.min(bw) * 4) as usize;
        out[d..d + n].copy_from_slice(&rgba[s..s + n]);
    }
    out
}

/// Whether a `cart_shell.txt` value names a home mould, which the upstream
/// parser Studio reads the files with does not know.
fn home_value(value: &str) -> bool {
    let Some(m) = value.split_whitespace().next().and_then(HomeMould::of_name) else {
        return false;
    };
    parse_value(m.system(), value).is_some()
}

/// `layered_cart_shells` with the home carts' lines the upstream parser
/// dropped: a line in the Labels layer over one in the system file.
pub fn layered(mut pairs: Vec<String>, system: &str, labels: &str) -> Vec<String> {
    let above = slot_store::ini::parse(labels);
    let mut home: Vec<(String, String)> = Vec::new();
    for (key, value) in slot_store::ini::parse(system) {
        if home_value(&value) && !above.contains_key(&key) {
            home.push((key, value));
        }
    }
    for (key, value) in above {
        if home_value(&value) {
            home.push((key, value));
        }
    }
    for (key, value) in home {
        let mut i = 0;
        while i + 1 < pairs.len() {
            if pairs[i] == key {
                pairs.drain(i..i + 2);
            } else {
                i += 2;
            }
        }
        pairs.push(key);
        pairs.push(value.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase());
    }
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platforms_name_their_systems_and_moulds() {
        assert_eq!(system_of("NES"), Some((HomeSystem::Nes, None)));
        assert_eq!(system_of("NES:famicom"), Some((HomeSystem::Nes, Some(HomeMould::Famicom))));
        assert_eq!(system_of("SNES:megadrive"), Some((HomeSystem::Snes, None)));
        assert_eq!(system_of("GBA"), None);
    }

    #[test]
    fn the_automatic_shell_is_the_devices() {
        assert_eq!(auto_value("NES", "Dig Dug (Japan)", &[]).split(' ').next(), Some("famicom-namco"));
        assert_eq!(auto_value("NES", "Legend of Zelda, The (Europe)", &[]), "nes 9e8536 glitter");
        assert_eq!(auto_value("SNES", "Super Mario World (USA)", &[]).split(' ').next(), Some("snes-us"));
        let mut head = vec![0u8; 0x150];
        head[0x110..0x120].copy_from_slice(b"(C)T-50 1991.SEP");
        assert_eq!(auto_value("MD", "Road Rash (USA, Europe)", &head).split(' ').next(), Some("megadrive-ea"));
        assert_eq!(outline("MD", "Road Rash (USA, Europe)", &head, "megadrive 112233 solid"), "megadrive");
        assert_eq!(outline("MD", "Road Rash (USA, Europe)", &head, "notched 112233 solid"), "megadrive-ea");
        assert_eq!(auto_value("GB", "Tetris", &[]), "");
    }

    #[test]
    fn faces_fill_their_systems_box() {
        for (platform, stem) in [("NES", "Dig Dug (Japan)"), ("NES", "Zelda (USA)"), ("SNES", "Mario (USA)"), ("MD", "Sonic (Japan)")] {
            let (w, h) = cart_size(platform).unwrap();
            let face = existing_face(&[], platform, &[], "", stem).unwrap();
            assert_eq!(face.len(), (w * h * 4) as usize, "{platform} {stem}");
            assert!(face.chunks_exact(4).any(|p| p[3] == 255));
        }
        let clear = existing_face(&[], "SNES", &[], "snes 7c4fb8 clear", "Super Metroid (Europe)").unwrap();
        assert!(clear.chunks_exact(4).any(|p| p[3] == 255));
    }

    #[test]
    fn home_lines_survive_the_upstream_parser() {
        let pairs = layered(
            vec!["Tetris".into(), "notched 112233 solid".into()],
            "Dig Dug (Japan) = famicom-namco 121212 solid\nZelda = nes 9e8536 glitter\n",
            "Zelda = snes-us 010203 clear\n",
        );
        assert!(pairs.windows(2).any(|p| p[0] == "Dig Dug (Japan)" && p[1] == "famicom-namco 121212 solid"));
        assert!(pairs.windows(2).any(|p| p[0] == "Zelda" && p[1] == "snes-us 010203 clear"));
        assert!(pairs.windows(2).any(|p| p[0] == "Tetris"));
    }

    #[test]
    fn home_labels_are_their_wells_shape() {
        for m in HomeMould::ALL {
            let (lw, lh) = well_label(m);
            let (_, _, w, h) = m.art().label;
            assert!(((lw as f32 / lh as f32) - (w as f32 / h as f32)).abs() < 0.01, "{m:?}");
            assert!(is_home_label(lw, lh));
            assert_ne!((lw, lh), (label::W, label::H));
            assert_ne!((lw, lh), (label::GB_W, label::GB_H));
        }
    }
}
