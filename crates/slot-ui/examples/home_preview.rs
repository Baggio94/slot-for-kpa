//! Renders the NES / Famicom / Super Nintendo / Mega Drive carts the way the
//! shelf and the core picker draw them, one PNG per face, for previews.
//! usage: cargo run -p slot-ui --example home_preview -- OUT_DIR [SCALE]
use std::path::PathBuf;

use slot_store::{Cart, Platform, ShellChoice, ShellFinish};

fn save(dir: &std::path::Path, name: &str, face: &slot_ui::CartFace) {
    use slot_ui::Face as _;
    let (w, h) = face.texels();
    let file = std::io::BufWriter::new(std::fs::File::create(dir.join(name)).unwrap());
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(&face.rgba).unwrap();
}

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("OUT_DIR"));
    let scale: f32 = args.next().map_or(1.0, |s| s.parse().unwrap());
    slot_ui::density::set_scale(scale);
    std::fs::create_dir_all(&dir).unwrap();
    let games: &[(&str, Platform, bool, &str, Option<&str>)] = &[
        ("Super Mario Bros. (World)", Platform::Nes, false, "", None),
        ("Legend of Zelda, The (Europe)", Platform::Nes, true, "", None),
        ("Super Mario Bros. 3 (Japan)", Platform::Nes, false, "", None),
        ("Dig Dug (Japan)", Platform::Nes, false, "", None),
        ("Ganbare Goemon Gaiden - Kieta Ougon Kiseru (Japan)", Platform::Nes, true, "", None),
        ("Super Mario World (USA)", Platform::Snes, true, "", None),
        ("Super Mario Kart (Europe)", Platform::Snes, true, "", None),
        ("Killer Instinct (Europe)", Platform::Snes, false, "", None),
        ("Super Metroid (Europe)", Platform::Snes, true, "", Some("snes 7c4fb8 clear")),
        ("Sonic The Hedgehog (USA, Europe)", Platform::Md, false, "(C)SEGA 1991.APR", None),
        ("Sonic The Hedgehog 2 (Japan)", Platform::Md, false, "(C)SEGA 1992.NOV", None),
        ("Road Rash (USA, Europe)", Platform::Md, true, "(C)T-50 1991.SEP", None),
    ];
    for (i, (stem, platform, battery, maker, chosen)) in games.iter().enumerate() {
        let auto = slot_ui::home_shell(*platform, stem, "", maker);
        let shell = chosen.and_then(ShellChoice::parse).or(auto);
        let cart = Cart {
            platform: *platform,
            stem: stem.to_string(),
            rom: PathBuf::from(format!("{stem}.rom")),
            label: None,
            title: stem.to_string(),
            code: String::new(),
            shell,
            battery: *battery,
        };
        let closed = slot_ui::cart_face_with_material(&cart, None);
        save(&dir, &format!("{i:02}_closed.png"), &closed);
        save(&dir, &format!("{i:02}_open.png"), &slot_ui::board_face(&cart));
        let finish = shell.map_or(ShellFinish::Solid, |s| s.finish);
        println!("{i:02}\t{stem}\t{:?}\t{:?}\tbattery={battery}", slot_ui::body_of(&cart), finish);
    }
    for (p, name) in [(Platform::Nes, "nes"), (Platform::Snes, "snes"), (Platform::Md, "md")] {
        let core = slot_store::Core::primary_for(p);
        save(&dir, &format!("chip_{name}.png"), &slot_ui::home_chip_face(p, Some(core)));
    }
}
