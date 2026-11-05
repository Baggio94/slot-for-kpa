//! Composite the real runtime draw list to catch masking and material regressions.
use super::*;
use slot_ui::{density, CartFace, Face, TexId};

fn composite(draws: &[Draw], faces: &[(TexId, CartFace)]) -> CartFace {
    let (w, h) = (out_w(), OUT_H);
    let (tw, th) = (density::px(w), density::px(h));
    let mut rgba = [5, 5, 7, 255].repeat((tw * th) as usize);
    for draw in draws {
        let Draw::Tex {
            tex, x, y, alpha, ..
        } = *draw
        else {
            continue;
        };
        let face = &faces.iter().find(|(id, _)| *id == tex).unwrap().1;
        let (sw, sh) = face.texels();
        let x = (x * density::scale()).round() as i32;
        let y = (y * density::scale()).round() as i32;
        for sy in 0..sh as i32 {
            let dy = y + sy;
            if dy < 0 || dy >= th as i32 {
                continue;
            }
            for sx in 0..sw as i32 {
                let dx = x + sx;
                if dx < 0 || dx >= tw as i32 {
                    continue;
                }
                let src = ((sy * sw as i32 + sx) * 4) as usize;
                let dst = ((dy * tw as i32 + dx) * 4) as usize;
                let a = alpha * face.rgba[src + 3] as f32 / 255.;
                for c in 0..3 {
                    rgba[dst + c] = (face.rgba[src + c] as f32 * a
                        + rgba[dst + c] as f32 * (1. - a))
                        .round() as u8;
                }
            }
        }
    }
    CartFace { w, h, rgba }
}

fn pixel(face: &CartFace, x: f32, y: f32) -> &[u8] {
    let (w, _) = face.texels();
    let (x, y) = (
        (x * density::scale()).round() as u32,
        (y * density::scale()).round() as u32,
    );
    let at = ((y * w + x) * 4) as usize;
    &face.rgba[at..at + 3]
}

#[test]
fn seated_draws_show_the_same_shank_inside_a_fixed_receiver() {
    density::with_scale(2., || {
        for (family, name, wireless) in [
            (0, "fc", false),
            (1, "nes", false),
            (2, "snes", false),
            (3, "md", false),
            (4, "gb", false),
            (5, "gbc", false),
            (6, "gba", false),
            (6, "rfu", true),
        ] {
            let key = Hardware::new(family, 0, wireless);
            let art = link_hardware::hardware_art(key);
            let m = slot_ui::link_marks();
            let mut faces = Vec::new();
            let mut upload = |face: CartFace| {
                let sprite = Sprite {
                    tex: TexId::from_raw(faces.len()),
                    w: face.w,
                    h: face.h,
                };
                faces.push((sprite.tex, face));
                sprite
            };
            let [host, join] = art.grips;
            let s = LinkSprites {
                key,
                wall: upload(art.wall),
                front: upload(art.front),
                socket: upload(art.socket),
                rim: upload(art.rim),
                contact: upload(art.contact),
                nose: upload(art.nose),
                grips: [upload(host), upload(join)],
                buttons: art
                    .buttons
                    .map(|roles| roles.map(|sides| sides.map(&mut upload))),
            };
            let marks = Marks {
                clicks: upload(m.clicks),
                arrow_left: upload(m.arrow_left),
                arrow_right: upload(m.arrow_right),
                arcs_left: m.arcs_left.map(&mut upload),
                arcs_right: m.arcs_right.map(&mut upload),
            };
            for role in LinkRow::ALL {
                let draw = |menu, now| {
                    let mut list = Vec::new();
                    link_screen::draw_link_art(menu, now, &s, &marks, &mut list);
                    composite(&list, &faces)
                };
                let ready = draw(LinkMenu::Pick(role), 0);
                let linked = draw(
                    LinkMenu::Linked {
                        role,
                        worked: 0,
                        since: 200,
                        opened: false,
                    },
                    1000,
                );
                let waiting = draw(
                    LinkMenu::Working {
                        role,
                        step: LinkStep::Waiting,
                        since: 0,
                    },
                    1000,
                );
                let x = out_w() as f32 / 2. + key.offset(role == LinkRow::Join);
                let shank = &faces.iter().find(|(t, _)| *t == s.nose.tex).unwrap().1;
                let y = link_hardware::BAR_Y;
                for dx in [-5., 0.] {
                    assert_eq!(
                        pixel(&linked, x + dx, y + 4.),
                        pixel(shank, shank.w as f32 / 2. + dx, key.shoulder() + 4.),
                        "the inserted shank changed colour: {name}/{role:?}"
                    );
                    assert_eq!(
                        pixel(&waiting, x + dx, y + 4.),
                        pixel(&linked, x + dx, y + 4.)
                    );
                }
                // A second controller socket is fixed and stays unoccupied.
                if key.dual() {
                    let other = out_w() as f32 - x;
                    for dx in [-20., 0., 20.] {
                        assert_eq!(
                            pixel(&ready, other + dx, y + 5.),
                            pixel(&linked, other + dx, y + 5.)
                        );
                    }
                }
                // Optional native frame export for visual review; no mock UI.
                if let Ok(dir) = std::env::var("LINK_HARDWARE_FRAMES") {
                    std::fs::create_dir_all(&dir).unwrap();
                    for (state, face) in
                        [("ready", ready), ("linked", linked), ("waiting", waiting)]
                    {
                        let path = std::path::Path::new(&dir)
                            .join(format!("{name}-{}-{state}.png", role.index()));
                        let file = std::fs::File::create(path).unwrap();
                        let (tw, th) = face.texels();
                        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), tw, th);
                        encoder.set_color(png::ColorType::Rgba);
                        encoder.set_depth(png::BitDepth::Eight);
                        encoder
                            .write_header()
                            .unwrap()
                            .write_image_data(&face.rgba)
                            .unwrap();
                    }
                }
            }
        }
    });
}
