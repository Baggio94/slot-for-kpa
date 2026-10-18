// Regression: slot. v1.7 moves adjacent carts outward on insertion
// and back inward on ejection. The Android renderer must feed its current
// seat progress into Shelf::draw_row instead of hard-coding zero.
use slot_gfx::Draw;
use slot_store::{Cart, Platform};
use slot_ui::Shelf;

const PART: f32 = 130.0; // upstream shelf's per-neighbour recede distance

fn shelf() -> Shelf {
    Shelf::new(
        (0..3)
            .map(|i| Cart {
                platform: Platform::Gba,
                stem: format!("Game {i}"),
                rom: format!("Games/GBA/Game {i}.gba").into(),
                label: None,
                code: String::new(),
                shell: None,
                title: format!("GAME {i}"),
            })
            .collect(),
    )
}

fn neighbours_at(shelf: &Shelf, seat: f32) -> Vec<(f32, f32)> {
    let mut commands = Vec::new();
    shelf.draw_row(Some("Game 0"), 0.0, seat, 1.0, &mut commands);
    let mut carts: Vec<_> = commands
        .iter()
        .filter_map(|d| match d {
            // No faces have been uploaded, so the fallback is a rectangle.
            Draw::Rect { x, colour, .. } => Some((*x, colour[3])),
            _ => None,
        })
        .collect();
    carts.sort_by(|a, b| a.0.total_cmp(&b.0));
    carts
}

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.02, "expected {b}, found {a}");
}

#[test]
fn adjacent_carts_separate_and_fade_as_selected_cart_inserts() {
    let shelf = shelf();
    let idle = neighbours_at(&shelf, 0.0);
    let quarter = neighbours_at(&shelf, 0.25);
    let halfway = neighbours_at(&shelf, 0.5);
    assert_eq!(idle.len(), 2);
    assert_eq!(quarter.len(), 2);
    assert_eq!(halfway.len(), 2);

    // Offset +/-1 uses upstream away = sign(offset) * (1 + abs(offset)).
    close(quarter[0].0, idle[0].0 - 2.0 * PART * 0.25);
    close(quarter[1].0, idle[1].0 + 2.0 * PART * 0.25);
    close(halfway[0].0, idle[0].0 - PART);
    close(halfway[1].0, idle[1].0 + PART);
    close(quarter[0].1, idle[0].1 * 0.75);
    close(quarter[1].1, idle[1].1 * 0.75);

    // At full insertion the neighbours have receded out of sight.
    assert!(neighbours_at(&shelf, 1.0).is_empty());
}

#[test]
fn adjacent_carts_rejoin_when_ejection_reverses_seat_progress() {
    let shelf = shelf();
    let half = neighbours_at(&shelf, 0.5);
    let quarter = neighbours_at(&shelf, 0.25);
    let idle = neighbours_at(&shelf, 0.0);

    // During ejection (1 -> 0), both neighbours move *toward* the slot.
    assert!(quarter[0].0 > half[0].0);
    assert!(quarter[1].0 < half[1].0);
    assert!(quarter[0].1 > half[0].1);
    assert!(quarter[1].1 > half[1].1);
    close(idle[0].0 - quarter[0].0, 2.0 * PART * 0.25);
    close(quarter[1].0 - idle[1].0, 2.0 * PART * 0.25);
}
