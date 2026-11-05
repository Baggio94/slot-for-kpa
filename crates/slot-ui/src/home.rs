//! NES / Famicom, Super Nintendo and Mega Drive carts: every mould at the
//! Game Boy pak's width (312), its height following the real shell. The art
//! comes from `assets/home/`, made by the nes-megadrive tools from Siham's
//! support-2D captures and PCB photos: a neutral grey shell the game's colour
//! is blended onto (as the GB/GBA shells), its back shell, the board with or
//! without its battery, and the big chip the core names sit on.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use slot_store::{Cart, Outline, Platform, ShellChoice, ShellFinish};

pub use crate::home_art::*;
#[cfg(test)]
use crate::home_art::{KILLER_INSTINCT, ZELDA_GOLD};

use crate::art::decode_png_bytes;
use crate::density::{self, px};

impl HomeMould {
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

/// The publisher's decal at panel density, from its master.
pub(crate) fn dense_decal(m: HomeMould) -> Option<Arc<Vec<u8>>> {
    let (w, h) = m.size();
    Some(crate::bitmaps::dense_png(m.art().decal?, w, h))
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
            let big = crate::bitmaps::dense_shell(crate::bitmaps::Mould::Home(m), false);
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

/// The automatic shell of a home console cart (see `home_art::home_pick`).
pub fn home_shell(platform: Platform, stem: &str, region: &str, maker: &str) -> Option<ShellChoice> {
    let system = match platform {
        Platform::Nes => HomeSystem::Nes,
        Platform::Snes => HomeSystem::Snes,
        Platform::Md => HomeSystem::Md,
        Platform::Gba | Platform::Gb | Platform::Gbc => return None,
    };
    let (mould, colour, glitter) = home_pick(system, stem, region, maker);
    let finish = if glitter { ShellFinish::Glitter } else { ShellFinish::Solid };
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
    fn mould_names_are_the_shell_file_outlines() {
        for m in HomeMould::ALL {
            assert_eq!(m.name(), m.outline().name(), "{m:?}");
            assert_eq!(HomeMould::of_name(m.name()), Some(m));
            assert_eq!(HomeMould::default_for(m.platform()), Some(HomeMould::default_of(m.system())));
        }
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
