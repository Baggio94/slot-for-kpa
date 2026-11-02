//! NES / Famicom, Super Nintendo and Mega Drive carts: every mould at the
//! Game Boy pak's width (312), its height following the real shell. The art
//! comes from `assets/home/`, made by the nes-megadrive tools from Siham's
//! support-2D captures and PCB photos: a neutral grey shell the game's colour
//! is blended onto (as the GB/GBA shells), its back shell, the board with or
//! without its battery, and the big chip the core names sit on.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use slot_store::{Cart, Outline, Platform, ShellChoice, ShellFinish};
use unicode_normalization::UnicodeNormalization;

use crate::art::decode_png_bytes;
use crate::density::{self, px};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum HomeMould {
    Nes,
    Famicom,
    FamicomNamco,
    FamicomKonami,
    Snes,
    SnesUs,
    MegaDrive,
    MegaDriveJp,
    MegaDriveEa,
}

/// A mould's art and where things sit on it, in its logical pixels.
pub(crate) struct MouldArt {
    pub h: u32,
    /// The label well: x, y, w, h.
    pub label: (u32, u32, u32, u32),
    /// The two big-chip footprints (left, top, w, h): the cores' sockets.
    pub footprints: [(f32, f32, f32, f32); 2],
    /// The small chip the game's title is printed on.
    pub title: (f32, f32, f32, f32),
    /// The plain shell's colour when nothing names another.
    pub colour: [u8; 3],
    neutral: &'static [u8],
    decal: Option<&'static [u8]>,
    pub shell_svg: &'static str,
    board: &'static [u8],
    board_battery: &'static [u8],
    pub chip_svg: &'static str,
}

pub const HOME_CART_W: u32 = 312;
/// Boards are drawn at twice the logical size.
const BOARD_K: u32 = 2;

macro_rules! art {
    ($name:literal, $h:expr, $label:expr, $fp:expr, $title:expr, $colour:expr, $decal:expr) => {
        MouldArt {
            h: $h,
            label: $label,
            footprints: $fp,
            title: $title,
            colour: $colour,
            neutral: include_bytes!(concat!("../../../assets/home/", $name, "_neutral.png")),
            decal: $decal,
            shell_svg: include_str!(concat!("../../../assets/home/", $name, "_shell.svg")),
            board: include_bytes!(concat!("../../../assets/home/", $name, "_board.png")),
            board_battery: include_bytes!(concat!("../../../assets/home/", $name, "_board_battery.png")),
            chip_svg: include_str!(concat!("../../../assets/home/", $name, "_chip.svg")),
        }
    };
}

// Printed by nes-megadrive/export_app.py.
static ARTS: [MouldArt; 9] = [
    art!("nes", 352, (126, 0, 145, 239),
        [(49.5, 243.5, 93.2, 43.8), (169.3, 243.5, 93.2, 43.8)],
        (165.5, 120.3, 112.2, 37.1), [126, 126, 128], None),
    art!("fc", 207, (22, 22, 269, 133),
        [(42.2, 124.6, 100.3, 44.3), (170.0, 124.6, 100.3, 44.3)],
        (79.2, 86.1, 124.6, 26.4), [196, 125, 8], None),
    art!("fc_namco", 204, (14, 33, 280, 138),
        [(42.2, 121.4, 100.3, 44.3), (170.0, 121.4, 100.3, 44.3)],
        (79.2, 82.9, 124.6, 26.4), [18, 18, 18], None),
    art!("fc_konami", 294, (27, 25, 214, 159),
        [(42.2, 211.2, 100.3, 44.3), (170.0, 211.2, 100.3, 44.3)],
        (79.2, 172.6, 124.6, 26.4), [12, 21, 34], None),
    art!("snes", 215, (22, 11, 269, 92),
        [(141.2, 82.9, 59.2, 59.2), (217.3, 82.9, 59.2, 59.2)],
        (44.0, 75.7, 76.9, 33.0), [162, 157, 163], None),
    art!("snes_us", 200, (63, 0, 185, 84),
        [(140.1, 82.2, 58.7, 58.7), (215.5, 82.2, 58.7, 58.7)],
        (43.6, 75.1, 76.3, 32.7), [163, 156, 163], None),
    art!("md", 210, (48, 0, 216, 179),
        [(73.9, 105.6, 86.6, 44.3), (167.9, 105.6, 86.6, 44.3)],
        (158.4, 57.5, 90.8, 26.4), [18, 18, 19], None),
    art!("md_jp", 209, (39, 0, 232, 175),
        [(55.2, 93.8, 100.5, 51.5), (164.3, 93.8, 100.5, 51.5)],
        (153.2, 38.0, 105.4, 30.6), [12, 12, 12], None),
    art!("md_ea", 290, (62, 0, 218, 240),
        [(69.2, 186.4, 86.6, 44.3), (163.1, 186.4, 86.6, 44.3)],
        (153.6, 138.3, 90.8, 26.4), [14, 14, 15],
        Some(include_bytes!("../../../assets/home/md_ea_decal.png"))),
];

impl HomeMould {
    pub const ALL: [HomeMould; 9] = [
        HomeMould::Nes,
        HomeMould::Famicom,
        HomeMould::FamicomNamco,
        HomeMould::FamicomKonami,
        HomeMould::Snes,
        HomeMould::SnesUs,
        HomeMould::MegaDrive,
        HomeMould::MegaDriveJp,
        HomeMould::MegaDriveEa,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub(crate) fn art(self) -> &'static MouldArt {
        &ARTS[self.index()]
    }

    pub fn size(self) -> (u32, u32) {
        (HOME_CART_W, self.art().h)
    }

    pub fn platform(self) -> Platform {
        match self {
            HomeMould::Nes | HomeMould::Famicom | HomeMould::FamicomNamco | HomeMould::FamicomKonami => {
                Platform::Nes
            }
            HomeMould::Snes | HomeMould::SnesUs => Platform::Snes,
            HomeMould::MegaDrive | HomeMould::MegaDriveJp | HomeMould::MegaDriveEa => Platform::Md,
        }
    }

    /// The shell a system's carts get when nothing picks another.
    pub fn default_for(platform: Platform) -> Option<HomeMould> {
        match platform {
            Platform::Nes => Some(HomeMould::Nes),
            Platform::Snes => Some(HomeMould::Snes),
            Platform::Md => Some(HomeMould::MegaDrive),
            Platform::Gba | Platform::Gb | Platform::Gbc => None,
        }
    }

    pub fn outline(self) -> Outline {
        match self {
            HomeMould::Nes => Outline::Nes,
            HomeMould::Famicom => Outline::Famicom,
            HomeMould::FamicomNamco => Outline::FamicomNamco,
            HomeMould::FamicomKonami => Outline::FamicomKonami,
            HomeMould::Snes => Outline::Snes,
            HomeMould::SnesUs => Outline::SnesUs,
            HomeMould::MegaDrive => Outline::MegaDrive,
            HomeMould::MegaDriveJp => Outline::MegaDriveJp,
            HomeMould::MegaDriveEa => Outline::MegaDriveEa,
        }
    }

    pub fn of_outline(outline: Outline) -> Option<HomeMould> {
        HomeMould::ALL.into_iter().find(|m| m.outline() == outline)
    }
}

/// The mould a home console cart is drawn with: the one its shell names
/// when it belongs to that system, else the system's own.
pub fn home_mould_of(cart: &Cart) -> Option<HomeMould> {
    let default = HomeMould::default_for(cart.platform)?;
    Some(
        cart.shell
            .and_then(|choice| HomeMould::of_outline(choice.outline))
            .filter(|m| m.platform() == cart.platform)
            .unwrap_or(default),
    )
}

fn decoded(bytes: &'static [u8], w: u32, h: u32) -> Vec<u8> {
    let (rgba, sw, sh) = decode_png_bytes(bytes).expect("a home cart png decodes");
    if (sw, sh) == (w, h) {
        rgba
    } else {
        crate::native_assets::resample(&rgba, sw, sh, w, h)
    }
}

/// The neutral front shell at its logical size.
pub(crate) fn neutral(m: HomeMould) -> &'static [u8] {
    static CACHE: [OnceLock<Vec<u8>>; 9] = [const { OnceLock::new() }; 9];
    CACHE[m.index()].get_or_init(|| {
        let (w, h) = m.size();
        decoded(m.art().neutral, w, h)
    })
}

/// A publisher's printed part of the shell (EA's yellow tab), over the colour.
pub(crate) fn decal(m: HomeMould) -> Option<&'static [u8]> {
    static CACHE: [OnceLock<Vec<u8>>; 9] = [const { OnceLock::new() }; 9];
    let bytes = m.art().decal?;
    Some(CACHE[m.index()].get_or_init(|| {
        let (w, h) = m.size();
        decoded(bytes, w, h)
    }))
}

/// The outline at 1x: the neutral shell's own alpha.
pub(crate) fn mask(m: HomeMould) -> &'static [u8] {
    static CACHE: [OnceLock<Vec<u8>>; 9] = [const { OnceLock::new() }; 9];
    CACHE[m.index()].get_or_init(|| neutral(m).iter().skip(3).step_by(4).copied().collect())
}

/// The outline at panel density.
pub(crate) fn dense_mask(m: HomeMould) -> Arc<Vec<u8>> {
    type Cache = Mutex<HashMap<(HomeMould, u32), Arc<Vec<u8>>>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let key = (m, density::scale().to_bits());
    let mut cache = CACHE.get_or_init(Mutex::default).lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entry(key)
        .or_insert_with(|| {
            let (w, h) = m.size();
            let big = crate::native_assets::resample(neutral(m), w, h, px(w), px(h));
            Arc::new(big.iter().skip(3).step_by(4).copied().collect())
        })
        .clone()
}

/// The board at panel density, with its battery or only the bare pads.
pub(crate) fn board_rgba(m: HomeMould, battery: bool) -> Vec<u8> {
    let art = m.art();
    let bytes = if battery { art.board_battery } else { art.board };
    let (w, h) = m.size();
    let (rgba, sw, sh) = decode_png_bytes(bytes).expect("a home board png decodes");
    debug_assert_eq!((sw, sh), (w * BOARD_K, h * BOARD_K));
    let (tw, th) = (px(w), px(h));
    if (sw, sh) == (tw, th) {
        rgba
    } else {
        crate::native_assets::resample(&rgba, sw, sh, tw, th)
    }
}

// ---------------------------------------------------------------- shells ---

/// The Legend of Zelda (and Zelda II) came in gold outside Japan.
const ZELDA_GOLD: [u8; 3] = [158, 133, 54];
const ZELDA_GOLD_KEYS: &[&str] = &["legendofzeldathe", "thelegendofzelda", "zeldaiitheadventureoflink"];
const KILLER_INSTINCT: [u8; 3] = [17, 16, 17];
/// Unknown Famicom carts: the most common shell, black.
const FAMICOM_BLACK: [u8; 3] = [16, 16, 16];
/// Super Mario Bros.' own Famicom shell, measured from its support-2D.
const FAMICOM_SMB: [u8; 3] = [196, 125, 8];

const FAMICOM_TSV: &str = include_str!("../../../assets/home/famicom.tsv");

/// A game's title as the shell tables key it: the No-Intro name without its
/// tags, lowercased, letters and digits only.
pub fn title_key(name: &str) -> String {
    let mut bare = String::with_capacity(name.len());
    let mut depth = 0u32;
    for ch in name.chars() {
        match ch {
            '(' | '[' => depth += 1,
            ')' | ']' => depth = depth.saturating_sub(1),
            _ if depth == 0 => bare.push(ch),
            _ => {}
        }
    }
    bare.nfkd()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn famicom_table() -> &'static HashMap<&'static str, ([u8; 3], HomeMould)> {
    static TABLE: OnceLock<HashMap<&'static str, ([u8; 3], HomeMould)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        FAMICOM_TSV
            .lines()
            .filter_map(|line| {
                let mut cols = line.split('\t');
                let key = cols.next()?;
                let hex = cols.next()?;
                let mould = match cols.next()? {
                    "namco" => HomeMould::FamicomNamco,
                    "konami" => HomeMould::FamicomKonami,
                    _ => HomeMould::Famicom,
                };
                if hex.len() != 6 {
                    return None;
                }
                let colour = [0, 2, 4].map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0));
                Some((key, (colour, mould)))
            })
            .collect()
    })
}

/// Which regions a No-Intro / GoodTools name lists: (Japan, USA, Europe),
/// None when it lists none.
fn name_regions(stem: &str) -> Option<(bool, bool, bool)> {
    let mut found = None::<(bool, bool, bool)>;
    for tag in crate::cart::label_tags(stem) {
        for word in tag.split(',') {
            let r = found.get_or_insert((false, false, false));
            match word.trim().to_ascii_lowercase().as_str() {
                "japan" | "j" | "jp" | "jpn" | "asia" => r.0 = true,
                "usa" | "u" | "us" | "canada" | "world" | "w" => r.1 = true,
                "europe" | "e" | "eu" | "pal" | "france" | "germany" | "spain" | "italy" | "uk"
                | "australia" | "sweden" | "netherlands" | "f" | "g" | "scandinavia" | "brazil" => {
                    r.2 = true
                }
                _ => {}
            }
        }
    }
    found.filter(|r| r.0 || r.1 || r.2)
}

/// The automatic shell of a home console cart, as the real one: the
/// region's mould (Famicom, the US Super Nintendo, the Japanese Mega Drive),
/// a publisher's own (Namco and Konami Famicom carts, Electronic Arts in the
/// US) and the game's colour where it is known.
///
/// `region` is the header's (J, U, E or empty) and only counts when the
/// file name lists no region; `maker` is the Mega Drive copyright field.
pub fn home_shell(platform: Platform, stem: &str, region: &str, maker: &str) -> Option<ShellChoice> {
    let (japan_only, usa) = match name_regions(stem) {
        Some((j, u, e)) => (j && !u && !e, u),
        None => (region == "J", region == "U"),
    };
    let key = title_key(stem);
    let (mould, colour, finish) = match platform {
        Platform::Nes if japan_only => {
            let (colour, mould) = famicom_table().get(key.as_str()).copied().unwrap_or(
                if key == "supermariobros" {
                    (FAMICOM_SMB, HomeMould::Famicom)
                } else {
                    (FAMICOM_BLACK, HomeMould::Famicom)
                },
            );
            (mould, colour, ShellFinish::Solid)
        }
        Platform::Nes if ZELDA_GOLD_KEYS.contains(&key.as_str()) => {
            (HomeMould::Nes, ZELDA_GOLD, ShellFinish::Glitter)
        }
        Platform::Nes => (HomeMould::Nes, HomeMould::Nes.art().colour, ShellFinish::Solid),
        Platform::Snes => {
            let mould = if usa { HomeMould::SnesUs } else { HomeMould::Snes };
            let colour = if key == "killerinstinct" { KILLER_INSTINCT } else { mould.art().colour };
            (mould, colour, ShellFinish::Solid)
        }
        Platform::Md => {
            let maker = maker.to_ascii_uppercase();
            let ea = maker.contains("T-50") || maker.contains("ELECTRONIC ARTS");
            let mould = if japan_only {
                HomeMould::MegaDriveJp
            } else if ea && usa {
                HomeMould::MegaDriveEa
            } else {
                HomeMould::MegaDrive
            };
            (mould, mould.art().colour, ShellFinish::Solid)
        }
        Platform::Gba | Platform::Gb | Platform::Gbc => return None,
    };
    Some(ShellChoice { outline: mould.outline(), colour, finish })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mould(platform: Platform, stem: &str, region: &str, maker: &str) -> HomeMould {
        let choice = home_shell(platform, stem, region, maker).unwrap();
        HomeMould::of_outline(choice.outline).unwrap()
    }

    #[test]
    fn regions_pick_the_real_shells() {
        assert_eq!(mould(Platform::Nes, "Super Mario Bros. (World)", "", ""), HomeMould::Nes);
        assert_eq!(mould(Platform::Nes, "Super Mario Bros. (Japan, USA)", "", ""), HomeMould::Nes);
        assert_eq!(mould(Platform::Nes, "Dig Dug (Japan)", "", ""), HomeMould::FamicomNamco);
        assert_eq!(mould(Platform::Nes, "Ganbare Goemon Gaiden - Kieta Ougon Kiseru (Japan)", "", ""), HomeMould::FamicomKonami);
        assert_eq!(mould(Platform::Nes, "Unknown Game (Japan)", "", ""), HomeMould::Famicom);
        // Other publishers' tall carts do not carry Konami's logo.
        assert_eq!(mould(Platform::Nes, "Final Fantasy III (Japan)", "", ""), HomeMould::Famicom);
        assert_eq!(mould(Platform::Snes, "Super Mario World (USA)", "", ""), HomeMould::SnesUs);
        assert_eq!(mould(Platform::Snes, "Super Mario World (Europe)", "", ""), HomeMould::Snes);
        assert_eq!(mould(Platform::Snes, "Super Mario World (Japan)", "", ""), HomeMould::Snes);
        assert_eq!(mould(Platform::Snes, "smw", "U", ""), HomeMould::SnesUs);
        assert_eq!(mould(Platform::Md, "Sonic (Japan)", "", ""), HomeMould::MegaDriveJp);
        assert_eq!(mould(Platform::Md, "Road Rash (USA, Europe)", "", "(C)T-50 1991.SEP"), HomeMould::MegaDriveEa);
        assert_eq!(mould(Platform::Md, "Road Rash (Europe)", "", "(C)T-50 1991.SEP"), HomeMould::MegaDrive);
        assert_eq!(mould(Platform::Md, "Sonic (USA, Europe)", "", "(C)SEGA 1991.APR"), HomeMould::MegaDrive);
    }

    #[test]
    fn colour_exceptions() {
        let zelda = home_shell(Platform::Nes, "Legend of Zelda, The (Europe)", "", "").unwrap();
        assert_eq!((zelda.colour, zelda.finish), (ZELDA_GOLD, ShellFinish::Glitter));
        let fc = home_shell(Platform::Nes, "Zelda no Densetsu 1 - The Hyrule Fantasy (Japan)", "", "").unwrap();
        assert_eq!(fc.colour, [0x01, 0x9f, 0x56]);
        let ki = home_shell(Platform::Snes, "Killer Instinct (Europe)", "", "").unwrap();
        assert_eq!(ki.colour, KILLER_INSTINCT);
    }

    #[test]
    fn every_mould_decodes_at_its_size() {
        for m in HomeMould::ALL {
            let (w, h) = m.size();
            assert_eq!(neutral(m).len(), (w * h * 4) as usize, "{m:?}");
            assert_eq!(board_rgba(m, true).len(), (w * h * 4) as usize, "{m:?}");
            assert!(mask(m).iter().any(|a| *a == 255));
            let (lx, ly, lw, lh) = m.art().label;
            assert!(lx + lw <= w && ly + lh <= h, "{m:?} label");
        }
    }
}
