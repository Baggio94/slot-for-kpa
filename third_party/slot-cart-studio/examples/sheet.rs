//! A contact sheet: every labelled cart on one card drawn by slot's own `cart_face`, beside the
//! same cart on a second card, at 2x. `task sheet` sets the studio's generated labels next to
//! slot's hand-made ones, so the house style is judged at cart size.
//!
//! cargo run --example sheet -- <left card> <right card> <out.png>

use std::collections::BTreeMap;
use std::path::PathBuf;

use slot_ui::{cart_face, CartFace, CART_H, CART_W};

const SCALE: u32 = 2;
const GAP: u32 = 16;
/// slot's site page colour, so a cart sits on the ground it has on the studio's page.
const PAGE: [u8; 3] = [0x1b, 0x1b, 0x21];

fn main() {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    let [left, right, out] = args.as_slice() else {
        eprintln!("usage: sheet <left card> <right card> <out.png>");
        std::process::exit(2);
    };

    let mut rows: BTreeMap<String, [Option<CartFace>; 2]> = BTreeMap::new();
    for (side, root) in [left, right].into_iter().enumerate() {
        for cart in slot_store::scan(root).expect("scan the card") {
            if cart.label.is_some() {
                rows.entry(cart.stem.clone()).or_default()[side] = Some(cart_face(&cart));
            }
        }
    }

    let (cw, ch) = (CART_W * SCALE, CART_H * SCALE);
    let (w, h) = (GAP + 2 * (cw + GAP), GAP + rows.len() as u32 * (ch + GAP));
    let mut rgb = PAGE.repeat((w * h) as usize);
    for (row, (stem, faces)) in rows.iter().enumerate() {
        println!("row {row}: {stem}");
        for (side, face) in faces.iter().enumerate() {
            let Some(face) = face else { continue };
            let (ox, oy) = (
                GAP + side as u32 * (cw + GAP),
                GAP + row as u32 * (ch + GAP),
            );
            for y in 0..ch {
                for x in 0..cw {
                    let s = (((y / SCALE) * face.w + x / SCALE) * 4) as usize;
                    let a = face.rgba[s + 3] as u32;
                    let d = (((oy + y) * w + ox + x) * 3) as usize;
                    for c in 0..3 {
                        let over = face.rgba[s + c] as u32 * a + rgb[d + c] as u32 * (255 - a);
                        rgb[d + c] = ((over + 127) / 255) as u8;
                    }
                }
            }
        }
    }

    let file = std::fs::File::create(out).expect("create the sheet");
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .expect("png header")
        .write_image_data(&rgb)
        .expect("png data");
    println!(
        "wrote {} ({w}x{h}): left {}, right {}",
        out.display(),
        left.display(),
        right.display()
    );
}
