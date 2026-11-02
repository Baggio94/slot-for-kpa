//! The NES / Famicom, Super Nintendo and Mega Drive moulds: their art, where
//! things sit on them, and which one (and which colour) a game's cart came in.
//!
//! Nothing here depends on slot-store, so Cart Studio compiles this very file
//! (by `#[path]`) and draws the same shells, wells and automatic choices the
//! device does. It needs only `crate::cart::label_tags`, which both have.

use std::collections::HashMap;
use std::sync::OnceLock;

use unicode_normalization::UnicodeNormalization;

/// The three home consoles.
#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug)]
pub enum HomeSystem {
    Nes,
    Snes,
    Md,
}

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
    pub neutral: &'static [u8],
    pub decal: Option<&'static [u8]>,
    pub shell_svg: &'static str,
    pub board: &'static [u8],
    pub board_battery: &'static [u8],
    pub chip_svg: &'static str,
}

pub const HOME_CART_W: u32 = 312;
/// Boards are drawn at twice the logical size.
pub const BOARD_K: u32 = 2;

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
        [(49.5, 231.1, 93.2, 43.8), (169.3, 231.1, 93.2, 43.8)],
        (165.5, 108.0, 112.2, 37.1), [126, 126, 128], None),
    art!("fc", 207, (22, 22, 269, 133),
        [(42.0, 89.0, 100.3, 44.3), (169.7, 89.0, 100.3, 44.3)],
        (78.9, 50.4, 124.6, 26.4), [196, 125, 8], None),
    art!("fc_namco", 204, (14, 33, 280, 138),
        [(42.0, 87.1, 100.3, 44.3), (169.7, 87.1, 100.3, 44.3)],
        (78.9, 48.6, 124.6, 26.4), [18, 18, 18], None),
    art!("fc_konami", 294, (27, 25, 214, 159),
        [(42.2, 132.2, 100.3, 44.3), (170.0, 132.2, 100.3, 44.3)],
        (79.2, 93.7, 124.6, 26.4), [12, 21, 34], None),
    art!("snes", 215, (22, 11, 269, 92),
        [(142.9, 83.0, 52.5, 52.5), (210.4, 83.0, 52.5, 52.5)],
        (56.5, 76.6, 68.3, 29.3), [162, 157, 163], None),
    art!("snes_us", 200, (63, 0, 185, 84),
        [(143.1, 82.0, 51.5, 51.5), (209.3, 82.0, 51.5, 51.5)],
        (58.5, 75.8, 67.0, 28.7), [163, 156, 163], None),
    art!("md", 210, (48, 0, 216, 179),
        [(69.2, 86.1, 86.6, 44.3), (163.1, 86.1, 86.6, 44.3)],
        (153.6, 38.0, 90.8, 26.4), [18, 18, 19], None),
    art!("md_jp", 209, (39, 0, 232, 175),
        [(55.6, 82.9, 100.1, 51.2), (164.2, 82.9, 100.1, 51.2)],
        (153.3, 27.3, 104.9, 30.5), [12, 12, 12], None),
    art!("md_ea", 290, (62, 0, 218, 240),
        [(69.2, 126.4, 86.6, 44.3), (163.1, 126.4, 86.6, 44.3)],
        (153.6, 78.4, 90.8, 26.4), [14, 14, 15],
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

    pub fn system(self) -> HomeSystem {
        match self {
            HomeMould::Nes | HomeMould::Famicom | HomeMould::FamicomNamco | HomeMould::FamicomKonami => {
                HomeSystem::Nes
            }
            HomeMould::Snes | HomeMould::SnesUs => HomeSystem::Snes,
            HomeMould::MegaDrive | HomeMould::MegaDriveJp | HomeMould::MegaDriveEa => HomeSystem::Md,
        }
    }

    /// The shell a system's carts get when nothing picks another.
    pub fn default_of(system: HomeSystem) -> HomeMould {
        match system {
            HomeSystem::Nes => HomeMould::Nes,
            HomeSystem::Snes => HomeMould::Snes,
            HomeSystem::Md => HomeMould::MegaDrive,
        }
    }

    /// The outline's name in `cart_shell.txt`.
    pub fn name(self) -> &'static str {
        match self {
            HomeMould::Nes => "nes",
            HomeMould::Famicom => "famicom",
            HomeMould::FamicomNamco => "famicom-namco",
            HomeMould::FamicomKonami => "famicom-konami",
            HomeMould::Snes => "snes",
            HomeMould::SnesUs => "snes-us",
            HomeMould::MegaDrive => "megadrive",
            HomeMould::MegaDriveJp => "megadrive-jp",
            HomeMould::MegaDriveEa => "megadrive-ea",
        }
    }

    pub fn of_name(name: &str) -> Option<HomeMould> {
        HomeMould::ALL.into_iter().find(|m| m.name().eq_ignore_ascii_case(name))
    }
}

// ---------------------------------------------------------------- shells ---

/// The Legend of Zelda (and Zelda II) came in gold outside Japan.
pub(crate) const ZELDA_GOLD: [u8; 3] = [158, 133, 54];
const ZELDA_GOLD_KEYS: &[&str] = &["legendofzeldathe", "thelegendofzelda", "zeldaiitheadventureoflink"];
pub(crate) const KILLER_INSTINCT: [u8; 3] = [17, 16, 17];
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
/// US) and the game's colour where it is known. The bool is a glitter finish.
///
/// `region` is the header's (J, U, E or empty) and only counts when the
/// file name lists no region; `maker` is the Mega Drive copyright field.
pub fn home_pick(system: HomeSystem, stem: &str, region: &str, maker: &str) -> (HomeMould, [u8; 3], bool) {
    let (japan_only, usa) = match name_regions(stem) {
        Some((j, u, e)) => (j && !u && !e, u),
        None => (region == "J", region == "U"),
    };
    let key = title_key(stem);
    match system {
        HomeSystem::Nes if japan_only => {
            let (colour, mould) = famicom_table().get(key.as_str()).copied().unwrap_or(
                if key == "supermariobros" {
                    (FAMICOM_SMB, HomeMould::Famicom)
                } else {
                    (FAMICOM_BLACK, HomeMould::Famicom)
                },
            );
            (mould, colour, false)
        }
        HomeSystem::Nes if ZELDA_GOLD_KEYS.contains(&key.as_str()) => (HomeMould::Nes, ZELDA_GOLD, true),
        HomeSystem::Nes => (HomeMould::Nes, HomeMould::Nes.art().colour, false),
        HomeSystem::Snes => {
            let mould = if usa { HomeMould::SnesUs } else { HomeMould::Snes };
            let colour = if key == "killerinstinct" { KILLER_INSTINCT } else { mould.art().colour };
            (mould, colour, false)
        }
        HomeSystem::Md => {
            let maker = maker.to_ascii_uppercase();
            let ea = maker.contains("T-50") || maker.contains("ELECTRONIC ARTS");
            let mould = if japan_only {
                HomeMould::MegaDriveJp
            } else if ea && usa {
                HomeMould::MegaDriveEa
            } else {
                HomeMould::MegaDrive
            };
            (mould, mould.art().colour, false)
        }
    }
}
