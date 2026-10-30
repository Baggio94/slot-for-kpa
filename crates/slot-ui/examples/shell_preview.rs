//! Renders cartridge faces with given shells, for reviewing the colour table.
//! Input lines: `GB|GBC|GBA outline RRGGBB finish name`; writes `<out>/<n>.png`.

use std::io::BufRead;
use std::path::PathBuf;

use slot_store::{Cart, Platform, ShellChoice};

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("out dir"));
    std::fs::create_dir_all(&out).unwrap();
    for (n, line) in std::io::stdin().lock().lines().enumerate() {
        let line = line.unwrap();
        let mut words = line.splitn(5, ' ');
        let platform = match words.next().unwrap_or("") {
            "GB" => Platform::Gb,
            "GBC" => Platform::Gbc,
            _ => Platform::Gba,
        };
        let shell_text = [words.next(), words.next(), words.next()]
            .map(|w| w.unwrap_or(""))
            .join(" ");
        let title = words.next().unwrap_or("").to_string();
        let cart = Cart {
            platform,
            stem: title.clone(),
            rom: PathBuf::from("/nonexistent"),
            label: None,
            title,
            code: String::new(),
            shell: ShellChoice::parse(&shell_text),
        };
        let face = slot_ui::cart_face_with_material(&cart, None);
        let file = std::fs::File::create(out.join(format!("{n}.png"))).unwrap();
        let mut enc = png::Encoder::new(std::io::BufWriter::new(file), face.w, face.h);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&face.rgba).unwrap();
    }
}
