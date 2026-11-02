// The core picker keeps the side carts out of its way on any screen, and
// changes nothing on the KONKR, where they were already clear of it.
use slot_gfx::{set_canvas_for_panel, Draw};
use slot_store::{Cart, Platform};
use slot_ui::{ease, picker_span, shelf_cart_for, Shelf};

fn shelf(platforms: &[Platform]) -> Shelf {
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

fn columns(out: &[Draw]) -> Vec<(f32, f32)> {
    out.iter().filter_map(|draw| match *draw {
        Draw::Rect { x, w, .. } | Draw::Tex { x, w, .. } => Some((x, w)),
        _ => None,
    }).collect()
}

/// The row as the runtime draws it under the picker, and the picker's span.
fn under_picker(shelf: &Shelf, openness: f32, clear: bool) -> (Vec<(f32, f32)>, [f32; 2]) {
    let (x, scale) = shelf.selected_at();
    let platform = shelf.carts[shelf.index].platform;
    let span = picker_span(platform, shelf_cart_for(platform, x, scale), openness);
    let t = ease(openness);
    let mut out = Vec::new();
    let hidden = Some(shelf.carts[shelf.index].stem.as_str());
    let dim = 1.0 + (0.614 - 1.0) * t;
    shelf.draw_row_clear_of(hidden, 0.0, 0.26 * t, dim, clear.then_some(span), &mut out);
    (columns(&out), span)
}

fn shelves() -> Vec<Shelf> {
    use Platform::*;
    let mut all = Vec::new();
    for row in [[Gb, Gb, Gb], [Gbc, Gbc, Gbc], [Gba, Gba, Gba], [Gba, Gb, Gba], [Gb, Gba, Gbc]] {
        let mut s = shelf(&row);
        s.select(1);
        all.push(s);
    }
    all
}

#[test]
fn side_carts_stay_clear_of_the_picker() {
    set_canvas_for_panel((960, 640));
    for s in shelves() {
        for step in 0..=40 {
            let openness = step as f32 / 40.0;
            assert_eq!(under_picker(&s, openness, true).0, under_picker(&s, openness, false).0,
                "the KONKR's shelf moves as before");
        }
    }

    for panel in [(1240, 1080), (1440, 1080), (1080, 1080), (960, 640)] {
        set_canvas_for_panel(panel);
        for s in shelves() {
            for step in 0..=40 {
                let openness = step as f32 / 40.0;
                let (carts, [left, right]) = under_picker(&s, openness, true);
                for (x, w) in carts {
                    assert!(x + w <= left - 5.9 || x >= right + 5.9,
                        "{panel:?} at {openness}: cart {x}..{} under picker {left}..{right}", x + w);
                }
            }
        }
    }
}
