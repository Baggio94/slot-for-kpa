//! The studio's faces against slot's, pixel for pixel. Slot's `cart_face` reads a real card in a
//! temp dir; the studio's reads the same bytes from memory. Any difference is the studio
//! drifting from the device.

#![cfg(not(target_arch = "wasm32"))]

use std::path::Path;

use slot_cart_studio::face;
use tempfile::TempDir;

const EMERALD: &str = "BPEE";
/// Neither in slot's shell table nor a family letter, so the default grey shell.
const PLAIN: &str = "AMTE";
const STEM: &str = "Probe Cart (USA)";

/// A card in the shape slot reads now: every file under the folder its platform names. `dir` is
/// that folder, `GBA` or `GB`, which is also how the studio names a platform across the wasm
/// boundary.
/// A Game Boy header: title, the code at 0x13F, CGB flag, and whether it was sold in Japan.
type Gb = (&'static [u8], &'static [u8], u8, bool);
const NO_GB: Gb = (b"", b"", 0, false);

fn card(dir: &str, code: &str, (title, gb_code, cgb, japan): Gb, label: &[u8]) -> TempDir {
    let d = tempfile::tempdir().expect("tempdir");
    for sub in ["Games", "Labels"] {
        std::fs::create_dir_all(d.path().join(sub).join(dir)).expect("content dir");
    }
    // Long enough for either header: a GBA game code sits at 0xAC, a Game Boy title at 0x134.
    let mut rom = vec![0u8; 0x200];
    if !code.is_empty() {
        rom[0xac..0xac + code.len()].copy_from_slice(code.as_bytes());
    }
    rom[0x134..0x134 + title.len()].copy_from_slice(title);
    rom[0x13f..0x13f + gb_code.len()].copy_from_slice(gb_code);
    rom[0x143] = cgb;
    rom[0x14a] = u8::from(!japan);
    let ext = dir.to_lowercase();
    std::fs::write(d.path().join(format!("Games/{dir}/{STEM}.{ext}")), rom).expect("rom");
    std::fs::write(d.path().join(format!("Labels/{dir}/{STEM}.png")), label).expect("label");
    d
}

/// The header bytes the page hands the studio for the only cart on `root`.
fn head(root: &Path) -> Vec<u8> {
    let rom = &slot_store::scan(root).expect("scan")[0].rom;
    std::fs::read(rom).expect("rom")[..0x150].to_vec()
}

/// Slot's face for the only cart on `root`, with the code and stem slot read for it.
fn slot_face(root: &Path) -> (Vec<u8>, String, String) {
    let mut cart = slot_store::scan(root).expect("scan").remove(0);
    cart.read_header();
    let cart = &cart;
    (
        slot_ui::cart_face(cart).rgba,
        cart.code.clone(),
        cart.stem.clone(),
    )
}

fn encode(w: u32, h: u32, colour: png::ColorType, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(colour);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .expect("png header")
            .write_image_data(data)
            .expect("png data");
    }
    out
}

fn pixels<const N: usize>(w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; N]) -> Vec<u8> {
    (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .flat_map(|(x, y)| px(x, y))
        .collect()
}

fn png_rgb(w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    encode(w, h, png::ColorType::Rgb, &pixels(w, h, px))
}

fn png_rgba(w: u32, h: u32, px: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    encode(w, h, png::ColorType::Rgba, &pixels(w, h, px))
}

fn assert_same_as_slot(label: &[u8]) {
    // A GBA cart slot's shell table knows, one that falls through to the default grey, and a Game
    // Boy pak for each CGB flag: grey, black and clear, the last in its own rounded mould. A pak
    // filed under the other Game Boy folder still takes its shell from the flag, not the folder.
    // Then paks slot's table colours by their header: by code, by title, and a Japanese copy
    // that shares a title but not the plastic.
    for (dir, code, gb) in [
        ("GBA", EMERALD, NO_GB),
        ("GBA", PLAIN, NO_GB),
        ("GB", "", (b"", b"", 0x00, false)),
        ("GB", "", (b"", b"", 0x80, false)),
        ("GB", "", (b"", b"", 0xc0, false)),
        ("GBC", "", (b"", b"", 0x00, false)),
        ("GBC", "", (b"", b"", 0xc0, false)),
        ("GBC", "", (b"POKEMON_GLD", b"AAUE", 0x80, false)),
        ("GBC", "", (b"PM_CRYSTAL", b"BYTE", 0xc0, false)),
        ("GB", "", (b"POKEMON RED", b"", 0x00, false)),
        ("GB", "", (b"POKEMON RED", b"", 0x00, true)),
    ] {
        let d = card(dir, code, gb, label);
        let (want, read_code, stem) = slot_face(d.path());
        assert_eq!(read_code, code, "slot did not read the code back");
        assert!(
            want == face::from_png(
                label,
                face::platform_of(dir),
                &read_code,
                &head(d.path()),
                "",
                &stem
            ),
            "face differs from slot's in {dir} with code {code:?} and header {gb:?}"
        );
    }
}

#[test]
fn a_flat_opaque_label_matches_slot() {
    assert_same_as_slot(&png_rgb(64, 64, |_, _| [0xd0, 0x20, 0xa0]));
}

#[test]
fn a_label_with_partial_alpha_matches_slot() {
    assert_same_as_slot(&png_rgba(196, 86, |x, y| {
        [x as u8, y as u8, 0x40, ((x * 3 + y) % 256) as u8]
    }));
}

#[test]
fn a_tall_label_is_cropped_the_way_slot_crops_it() {
    assert_same_as_slot(&png_rgb(160, 480, |_, y| match y / 160 {
        0 => [255, 0, 0],
        1 => [0, 255, 0],
        _ => [0, 0, 255],
    }));
}

#[test]
fn bytes_that_are_not_a_png_get_slots_generated_label() {
    assert_same_as_slot(b"not a png");
}

#[test]
fn raw_rgba_draws_the_same_face_as_its_png() {
    let (w, h) = (300, 150);
    let px = |x: u32, y: u32| [(x % 256) as u8, (y % 256) as u8, 0x80, 255];
    let png = png_rgba(w, h, px);
    let raw = pixels(w, h, px);
    for code in [EMERALD, PLAIN] {
        let d = card("GBA", code, NO_GB, &png);
        let (want, _, stem) = slot_face(d.path());
        assert!(
            want == face::from_rgba(&raw, w, h, face::platform_of("GBA"), code, &[], "", &stem),
            "raw RGBA face differs with code {code}"
        );
    }
}

#[test]
fn a_composed_label_matches_slot_reading_its_png() {
    let logo = png_rgba(120, 60, |x, y| {
        if (x / 10 + y / 10) % 2 == 0 {
            [250, 210, 20, 255]
        } else {
            [0, 0, 0, 0]
        }
    });
    let deep = slot_cart_studio::hue::hsl_to_rgb(210.0, 0.65, 0.24);
    let label =
        slot_cart_studio::label::Label::from_png(&logo, deep, face::platform_of("GBA"), None)
            .expect("logo decodes");
    let png = label.png();
    for code in [EMERALD, PLAIN] {
        let d = card("GBA", code, NO_GB, &png);
        let (want, _, stem) = slot_face(d.path());
        assert!(
            want == label.face(face::platform_of("GBA"), code, &[], "", &stem),
            "composed face differs with code {code}"
        );
    }
}

/// A card that chooses a shell draws the cart the way the studio draws it when
/// handed the same line.
#[test]
fn a_chosen_shell_matches_slot() {
    let label = png_rgb(64, 64, |_, _| [0x20, 0x90, 0xd0]);
    for (dir, code, gb, line) in [
        (
            "GB",
            "",
            (b"" as &[u8], b"" as &[u8], 0x00u8, false),
            "rounded 336699 clear",
        ),
        ("GBC", "", (b"", b"", 0xc0, false), "notched 123456 glitter"),
        ("GBA", EMERALD, NO_GB, "auto c0282c solid"),
        ("GBA", PLAIN, NO_GB, "rounded 2b3a88 clear"),
    ] {
        let d = card(dir, code, gb, &label);
        let shells = d.path().join(slot_store::CART_SHELL_FILE);
        std::fs::create_dir_all(shells.parent().unwrap()).unwrap();
        std::fs::write(shells, format!("{STEM} = {line}\n")).unwrap();
        let (want, read_code, stem) = slot_face(d.path());
        assert!(
            want == face::from_png(
                &label,
                face::platform_of(dir),
                &read_code,
                &head(d.path()),
                line,
                &stem
            ),
            "face differs from slot's in {dir} with {line:?}"
        );
    }
}

#[test]
fn the_game_code_is_read_the_way_slot_reads_it() {
    for code in [EMERALD, PLAIN, "AB", ""] {
        let d = card("GBA", code, NO_GB, b"");
        let (_, slot_code, _) = slot_face(d.path());
        let rom = std::fs::read(d.path().join(format!("Games/GBA/{STEM}.gba"))).expect("rom");
        assert_eq!(
            slot_cart_studio::rom::header_code(&rom[..0xb0]),
            slot_code,
            "{code:?}"
        );
    }
}
