use slot_gfx::{blit_rect, canvas_size, OUT_H, OUT_W};

#[test]
fn konkr_960x640_fills_the_entire_panel() {
    assert_eq!(OUT_W, 720);
    assert_eq!(OUT_H, 480);
    assert_eq!(blit_rect((960, 640), 0.0), (0, 0, 960, 640));
    assert_eq!(canvas_size((960, 640)), (960, 640));
}

#[test]
fn other_aspect_ratios_keep_the_original_integer_placement() {
    assert_eq!(blit_rect((1500, 980), 0.0), (30, 10, 1440, 960));
}
