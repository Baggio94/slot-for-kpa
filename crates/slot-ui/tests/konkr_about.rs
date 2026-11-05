use slot_ui::{sticker_face, sticker_face_handheld, sticker_face_konkr, StickerFields, STICKER_W, STICKER_H};

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

#[test]
fn other_handhelds_get_their_own_name_on_the_same_label() {
    let fields = StickerFields {
        battery: Some(58), serial: "0000130", dirty_digit: '0',
    };
    let konkr = sticker_face_konkr(&fields);
    let rpc = sticker_face_handheld(&fields, "Retroid Pocket Classic", "Retroid Pocket Classic");
    let other = sticker_face_handheld(&fields, "ANBERNIC RG405M", "RG405M");
    assert_eq!((rpc.w, rpc.h), (STICKER_W, STICKER_H));
    assert_eq!(rpc.rgba.len(), konkr.rgba.len());
    assert_ne!(rpc.rgba, konkr.rgba, "no KONKR logo on another handheld");
    assert_ne!(rpc.rgba, other.rgba, "each handheld shows its own name");
    // No name at all still makes a label.
    let unnamed = sticker_face_handheld(&fields, "", "");
    assert!(unnamed.rgba.chunks_exact(4).any(|p| p[3] > 0));
}
