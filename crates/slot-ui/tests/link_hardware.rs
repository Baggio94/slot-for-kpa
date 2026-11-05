use slot_ui::{
    density,
    link_hardware::{hardware_art, Hardware},
    CartFace, Face,
};

fn pixel(face: &CartFace, x: f32, y: f32) -> [u8; 4] {
    let (w, h) = face.texels();
    let x = (x * density::scale()).floor() as u32;
    let y = (y * density::scale()).floor() as u32;
    assert!(x < w && y < h);
    let at = ((y * w + x) * 4) as usize;
    face.rgba[at..at + 4].try_into().unwrap()
}

#[test]
fn receivers_are_permanent_openings_in_the_top_of_every_native_bar() {
    for scale in [1., 4. / 3., 2., 3.] {
        density::with_scale(scale, || {
            for family in 0..7 {
                for theme in 0..3 {
                    for &wireless in if family == 6 {
                        &[false, true][..]
                    } else {
                        &[false][..]
                    } {
                        let key = Hardware::new(family, theme, wireless);
                        let art = hardware_art(key);
                        assert_eq!(art.front.texels(), art.wall.texels());
                        let centre = art.wall.w as f32 / 2.;
                        for join in [false, true] {
                            let x = centre + key.offset(join);
                            for y in [0., 3., 5.] {
                                assert_eq!(
                                    pixel(&art.front, x, y)[3],
                                    0,
                                    "blocked receiver: {family}/{theme}/{wireless}/{scale}"
                                );
                            }
                            if key.wireless {
                                for dx in [-74.25, 74.25] {
                                    assert_eq!(pixel(&art.front, x + dx, 1.)[3], 0);
                                }
                            }
                        }
                        // The original surface, texture and highlights survive
                        // outside the holes and below their shallow recesses.
                        for (x, y) in [(10., 0.), (10., 20.), (centre, 20.), (centre, 50.)] {
                            assert_eq!(pixel(&art.front, x, y), pixel(&art.wall, x, y));
                        }
                        assert_eq!(art.socket.texels(), art.rim.texels());
                        assert_eq!(art.socket.h, 18);
                    }
                }
            }
        });
    }
}

#[test]
fn seated_metal_keeps_its_original_colour_beyond_the_small_contact_shadow() {
    density::with_scale(3., || {
        for family in 4..7 {
            for wireless in [false, true] {
                for theme in 0..3 {
                    let key = Hardware::new(family, theme, wireless);
                    let art = hardware_art(key);
                    for dx in [-5., 0., 5.] {
                        let x = art.nose.w as f32 / 2. + dx;
                        let y = key.shoulder() + 4.;
                        let exposed = pixel(&art.nose, x, y);
                        assert!(exposed[3] > 245, "missing metal: {key:?}");
                        // The exact same nose texture is visible through the
                        // fascia; neither the fixed rim nor grip tints it.
                        assert_eq!(pixel(&art.front, art.front.w as f32 / 2. + dx, 4.)[3], 0);
                        assert_eq!(pixel(&art.rim, art.rim.w as f32 / 2. + dx, 4.)[3], 0);
                        assert_eq!(pixel(&art.grips[0], x, y)[3], 0);
                        assert!(art.contact.h <= 2);
                        assert!(exposed[0] > 70, "metal has been blackened: {exposed:?}");
                    }
                }
            }
        }
    });
}

#[test]
fn both_casings_stop_at_the_shoulder_and_keep_distinct_role_finishes() {
    density::with_scale(4. / 3., || {
        for family in 0..7 {
            for wireless in [false, true] {
                let key = Hardware::new(family, 0, wireless);
                let art = hardware_art(key);
                assert_ne!(art.grips[0].rgba, art.grips[1].rgba);
                for grip in &art.grips {
                    let (w, h) = grip.texels();
                    let first = (key.shoulder() * density::scale()).ceil() as u32;
                    for y in first..h {
                        assert!(grip.rgba[(y * w * 4) as usize..((y + 1) * w * 4) as usize]
                            .chunks_exact(4)
                            .all(|px| px[3] == 0));
                    }
                }
            }
        }
    });
}
