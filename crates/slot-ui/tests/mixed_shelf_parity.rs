use slot_gfx::{Draw, OUT_W};
use slot_store::{Cart, Platform};
use slot_ui::Shelf;

fn shelf() -> Shelf {
    let platforms = [
        Platform::Gba, Platform::Gb, Platform::Gbc, Platform::Gba,
        Platform::Gb, Platform::Gbc, Platform::Gba,
    ];
    Shelf::new(platforms.iter().enumerate().map(|(i, &platform)| Cart {
        platform,
        stem: format!("Game {i}"),
        rom: format!("Games/Game {i}").into(),
        label: None,
        code: String::new(),
        shell: None,
        title: format!("GAME {i}"),
    }).collect())
}

fn bounds(out: &[Draw]) -> Vec<(f32, f32)> {
    out.iter().filter_map(|draw| match *draw {
        Draw::Rect { x, w, .. } | Draw::Tex { x, w, .. } => Some((x, w)),
        _ => None,
    }).collect()
}

fn selected_drawn_location(shelf: &Shelf) -> (f32, f32) {
    let mut full = Vec::new();
    shelf.draw_row(None, 0.0, 0.0, 1.0, &mut full);
    let selected = &shelf.carts[shelf.index].stem;
    let mut hidden = Vec::new();
    shelf.draw_row(Some(selected.as_str()), 0.0, 0.0, 1.0, &mut hidden);
    let before = bounds(&full);
    let after = bounds(&hidden);
    let removed: Vec<_> = before.iter().copied().filter(|v| !after.contains(v)).collect();
    assert_eq!(removed.len(), 1, "exactly one selected cartridge should be removed");
    removed[0]
}

#[test]
fn selected_cartridge_and_slot_chrome_share_position_during_mixed_scroll() {
    let mut s = shelf();
    for _ in 0..3 {
        s.right();
        for _ in 0..14 {
            let expected = s.selected_at();
            let drawn = selected_drawn_location(&s);
            assert!((drawn.0 - expected.0).abs() < 0.02, "shelf/chrome x mismatch");
            s.update(1.0 / 60.0);
        }
    }
}

#[test]
fn mixed_neighbours_have_positive_non_overlapping_pitch() {
    let mut s = shelf();
    for _ in 0..s.carts.len() {
        for _ in 0..200 { s.update(1.0 / 60.0); }
        let mut draws = Vec::new();
        s.draw_row(None, 0.0, 0.0, 1.0, &mut draws);
        let mut visible = bounds(&draws);
        visible.sort_by(|a,b| a.0.total_cmp(&b.0));
        for pair in visible.windows(2) {
            assert!(pair[1].0 > pair[0].0, "nonmonotonic mixed carousel");
            assert!(pair[1].0 >= pair[0].0 + pair[0].1 - 0.1,
                "overlapping mixed cartridges: {pair:?}");
        }
        let selected = s.selected_at();
        assert!(selected.0.is_finite());
        assert!(selected.0 < OUT_W as f32);
        s.right();
    }
}

#[test]
fn side_cartridges_keep_full_original_slot_opacity_until_insertion() {
    let shelf = shelf();
    let mut idle = Vec::new();
    shelf.draw_row(Some("Game 0"), 0.0, 0.0, 1.0, &mut idle);
    // Real Slot 1.7 SIDE_ALPHA = 1, not Android's prior 0.55.
    for draw in idle {
        if let Draw::Rect { colour, .. } = draw {
            assert!((colour[3] - 1.0).abs() < 0.001);
        }
    }
}
