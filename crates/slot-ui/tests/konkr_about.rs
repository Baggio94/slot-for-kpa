use slot_ui::{sticker_face, sticker_face_konkr, StickerFields, STICKER_W, STICKER_H};

#[test]
fn about_reuses_the_real_slot_barcode_while_crediting_the_android_port() {
    let fields = StickerFields {
        battery: Some(85), serial: "0000130", dirty_digit: '0',
    };
    let android = sticker_face_konkr(&fields);
    let original = sticker_face(&fields);
    assert_eq!((android.w, android.h), (STICKER_W, STICKER_H));
    assert_eq!(android.rgba.len(), (STICKER_W * STICKER_H * 4) as usize);
    assert_eq!(original.rgba.len(), android.rgba.len());
    assert_ne!(android.rgba, original.rgba, "KONKR credits must not misrepresent the original's OS");
    assert!(android.rgba.chunks_exact(4).any(|p| p[3] > 0));
}
