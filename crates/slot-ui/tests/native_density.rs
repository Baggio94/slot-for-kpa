//! Faces drawn for the KONKR's 960x640 panel keep Slot's logical sizes and
//! carry 4/3 the texels, re-rendered rather than stretched.
use slot_store::{Cart, Core, Platform};
use slot_ui::density::{self, Face};
use slot_ui::*;

const KONKR: f32 = 960.0 / 720.0;

fn cart(platform: Platform, stem: &str) -> Cart {
    Cart {
        platform,
        stem: stem.into(),
        rom: format!("Games/{stem}.gba").into(),
        label: None,
        code: "AGSE".into(),
        shell: None,
        title: stem.to_uppercase(),
    }
}

/// Logical size unchanged, texels at panel density, and something drawn.
fn dense<F: Face>(one: (u32, u32), make: impl Fn() -> (F, (u32, u32))) {
    let (face, logical) = density::with_scale(KONKR, &make);
    assert_eq!(logical, one, "layout must not move");
    let (tw, th) = face.texels();
    let px = |n: u32| (n as f32 * KONKR).round() as u32;
    assert_eq!((tw, th), (px(one.0), px(one.1)));
    assert!(face.rgba().chunks_exact(4).any(|p| p[3] > 200), "face is inked");
}

macro_rules! check {
    ($make:expr) => {{
        let one = $make;
        dense((one.w, one.h), || {
            let f = $make;
            let size = (f.w, f.h);
            (f, size)
        });
    }};
}

#[test]
fn legends_menus_and_hud_are_drawn_for_the_panel() {
    check!(hint_face("A", "Select"));
    check!(hint_face("SEL+R1", "Save"));
    check!(hint_face("Change", "Classic keycap"));
    check!(arrows_hint_face("Change"));
    check!(word_face("12:34"));
    check!(title_line_face("Golden Sun"));
    check!(quick_value_face("LCD Grid V2", true));
    check!(quick_caret_face(true));
    check!(menu_face("Power Off"));
    check!(onboarding_instruction_face("PRESS", "START", "TO SELECT YOUR ROM FOLDER"));
    check!(progress_face(12, 40, [255, 255, 255]));
    check!(icon_face(Icon::Volume, HUD_ICON_PX, HUD_INK));
    check!(badge_face(Badge::Link, HUD_ICON_PX, LINK_HOST_INK));
    check!(toast_face(Toast::all()[0]));
    check!(star_face(2, [0xf6, 0xc3, 0x2c]));
    check!(ClockPicker::from_secs(1_761_000_000).face());
    check!(sticker_face_konkr(&StickerFields { battery: Some(87), serial: "0000130", dirty_digit: '0' }));
}

#[test]
fn board_link_art_and_carts_are_drawn_for_the_panel() {
    let gba = cart(Platform::Gba, "Golden Sun");
    check!(board_face(&gba));
    check!(socket_face(Core::Mgba));
    check!(chip_face(Some(Core::Gpsp)));
    check!(chip_shadow_face());
    check!(link_art().adapter);
    check!({ let [_, _, arc] = link_art().arcs_left; arc });
    check!(cart_face(&gba));
    check!(cart_face_with_material(&gba, None));
    check!(cart_shadow());
    check!(gb_cart_shadow(GbShell::Rounded));
    check!(cart_face(&cart(Platform::Gb, "Tetris")));
}

#[test]
fn trimmed_titles_stay_within_a_pixel_of_slot() {
    // The rebuild title is cropped to its ink, which can round either way.
    let one = rebuild_title_face("Rebuilding Library");
    let dense = density::with_scale(KONKR, || rebuild_title_face("Rebuilding Library"));
    assert!(one.w.abs_diff(dense.w) <= 1 && one.h.abs_diff(dense.h) <= 1);
    assert!(dense.texels().0 > dense.w);
}

#[test]
fn one_x_faces_are_untouched() {
    let face = hint_face("A", "Select");
    assert_eq!(face.texels(), (face.w, face.h));
    assert_eq!(face.rgba.len(), (face.w * face.h * 4) as usize);
}
