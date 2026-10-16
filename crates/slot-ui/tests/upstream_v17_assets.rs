use slot_ui::{bitmaps, blur_background, default_background};

#[test]
fn official_1_7_cart_moulds_are_exact_rgba_pixel_dimensions() {
    assert_eq!(bitmaps::gba_cart().len(), 360 * 212 * 4);
    assert_eq!(bitmaps::gb_cart().len(), 312 * 337 * 4);
    assert_eq!(bitmaps::gbc_cart().len(), 312 * 337 * 4);
    assert_eq!(bitmaps::gba_interior().len(), 360 * 212 * 4);
    assert_eq!(bitmaps::gb_interior().len(), 312 * 337 * 4);
    assert_eq!(bitmaps::gbc_interior().len(), 312 * 337 * 4);
    assert_eq!(bitmaps::board().len(), 360 * 212 * 4);
    assert_eq!(bitmaps::gb_board().len(), 312 * 337 * 4);
}

#[test]
fn official_1_7_slot_housing_has_real_rear_and_front_at_native_size() {
    assert_eq!(bitmaps::slot_back().len(), 720 * 58 * 4);
    assert_eq!(bitmaps::slot_front().len(), 720 * 58 * 4);
    assert_eq!(bitmaps::slot_back_light().len(), 720 * 58 * 4);
    assert_eq!(bitmaps::slot_front_light().len(), 720 * 58 * 4);
    assert_ne!(bitmaps::slot_back(), bitmaps::slot_front());
}

#[test]
fn official_1_7_default_and_blurred_wallpapers_cover_native_panel() {
    let regular = default_background().expect("upstream bundled default PNG decodes");
    let blurred = blur_background().expect("upstream bundled blur PNG decodes");
    assert_eq!(regular.len(), 720 * 480 * 4);
    assert_eq!(blurred.len(), regular.len());
    assert_ne!(regular, blurred);
}
