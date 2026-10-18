use slot_gfx::Draw;
use slot_store::{Cart, Platform};
use slot_ui::SlotChrome;

fn game_cart() -> Cart {
    Cart {
        platform: Platform::Gba,
        stem: "test".into(),
        rom: "test.gba".into(),
        label: None,
        code: String::new(),
        shell: None,
        title: "TEST".into(),
    }
}

fn frame<'a>(cart: &'a Cart, over: bool, screen: f32, game: bool) -> Vec<Draw> {
    let mut out = Vec::new();
    SlotChrome {
        cart,
        face: None,
        rest: 0.0,
        scale: 1.0,
        seat: 1.0,
        alert: None,
        dim: 0.0,
        screen,
        game,
        over,
        slot_faces: None,
    }.draw(&mut out);
    out
}

#[test]
fn entering_game_lcd_expands_above_slot_and_cart() {
    let cart = game_cart();
    let out = frame(&cart, false, 0.5, true);
    // Slot 1.7: with an already seated cartridge, the LCD picture is
    // composed last, in front of both halves of the hardware slot.
    let game_at = out.iter().position(|cmd| matches!(cmd, Draw::Game)).unwrap();
    assert_eq!(game_at, out.len() - 1);
    assert!(out[..game_at].iter().any(|cmd| matches!(cmd, Draw::Rect { .. })));
}

#[test]
fn insert_mode_uses_game_below_slot_when_screen_is_opening() {
    let cart = game_cart();
    let out = frame(&cart, true, 0.5, true);
    let game_at = out.iter().position(|cmd| matches!(cmd, Draw::Game)).unwrap();
    assert_eq!(game_at, 0);
    assert!(out.iter().skip(1).any(|cmd| matches!(cmd, Draw::Rect { .. })));
}

#[test]
fn mechanical_insert_without_lcd_never_draws_game() {
    let cart = game_cart();
    let out = frame(&cart, true, 0.0, false);
    assert!(!out.iter().any(|cmd| matches!(cmd, Draw::Game)));
}

#[test]
fn refused_action_has_upstream_short_fading_shake() {
    let refusal = slot_ui::Refusal::started(100);
    assert!(refusal.active(100));
    assert!(refusal.offset(100).abs() <= 6.01);
    assert!(refusal.offset(120).abs() < 6.0);
    assert!(!refusal.active(400));
    assert_eq!(refusal.offset(400), 0.0);
}
