//! GB/GBC carts run on mGBA or Gambatte. Both keep RetroArch's per-core
//! folders (Saves/mGBA, Saves/Gambatte), so switching the chip on the board
//! must bring the newer cartridge battery along instead of starting over.
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use slot_store::{Core, Platform};

/// Library > Share Saves: off keeps each core's own battery, like mGBA/gpSP.
static SHARING: AtomicBool = AtomicBool::new(false);

pub fn set_sharing(enabled: bool) { SHARING.store(enabled, Ordering::Release); }

pub fn sharing() -> bool { SHARING.load(Ordering::Acquire) }

/// Slot's palette list uses mGBA's names; Gambatte ships the same GBC boot
/// and SGB palettes under its own names.
pub fn gambatte_palette(mgba: &str) -> Option<&'static str> {
    Some(match mgba {
        "Grayscale" | "GBC Gray ←B" => "GBC - Grayscale",
        "DMG Green" => "GB - DMG",
        "GB Pocket" => "GB - Pocket",
        "GB Light" => "GB - Light",
        "GBC Brown ↑" => "GBC - Brown",
        "GBC Red ↑A" => "GBC - Red",
        "GBC Dark Brown ↑B" => "GBC - Dark Brown",
        "GBC Pale Yellow ↓" => "GBC - Pastel Mix",
        "GBC Orange ↓A" => "GBC - Orange",
        "GBC Yellow ↓B" => "GBC - Yellow",
        "GBC Blue ←" => "GBC - Blue",
        "GBC Dark Blue ←A" => "GBC - Dark Blue",
        "GBC Green →" => "GBC - Green",
        "GBC Dark Green →A" => "GBC - Dark Green",
        "GBC Reverse →B" => "GBC - Inverted",
        // Its colours are picked by gambatte_twb64() below.
        slot_store::TWB64_GAME_COM => "TWB64 - Pack 1",
        sgb => {
            const SGB: [&str; 32] = [
                "SGB - 1A", "SGB - 1B", "SGB - 1C", "SGB - 1D",
                "SGB - 1E", "SGB - 1F", "SGB - 1G", "SGB - 1H",
                "SGB - 2A", "SGB - 2B", "SGB - 2C", "SGB - 2D",
                "SGB - 2E", "SGB - 2F", "SGB - 2G", "SGB - 2H",
                "SGB - 3A", "SGB - 3B", "SGB - 3C", "SGB - 3D",
                "SGB - 3E", "SGB - 3F", "SGB - 3G", "SGB - 3H",
                "SGB - 4A", "SGB - 4B", "SGB - 4C", "SGB - 4D",
                "SGB - 4E", "SGB - 4F", "SGB - 4G", "SGB - 4H",
            ];
            let wanted = sgb.strip_prefix("SGB ")?.replace('-', "");
            SGB.into_iter().find(|name| name[6..] == wanted)?
        }
    })
}

/// Gambatte's TWB64 Pack 1 entry for a slot. palette from that pack.
pub fn gambatte_twb64(name: &str) -> Option<&'static str> {
    (name == slot_store::TWB64_GAME_COM).then_some("TWB64 028 - Game.com Ver.")
}

/// The other core whose battery file holds the same SRAM: mGBA and
/// Gambatte for GB/GBC, FCEUmm and Nestopia for NES, Snes9x and bsnes for
/// SNES, Genesis Plus GX and PicoDrive for a Mega Drive cart's battery SRAM
/// (see md_shares_sram and carry_md).
fn other_core(which: Core, platform: Platform) -> Option<Core> {
    if platform == Platform::Gba || !which.runs(platform) {
        return None;
    }
    let first = Core::primary_for(platform);
    Some(if which == first { Core::alternate_for(platform) } else { first })
}

/// Before `which` boots a GB/GBC cart, copy the other core's battery over
/// when it is newer and different. Only the SRAM bytes both cores share are
/// replaced: mGBA's trailing RTC block survives a Gambatte save. The file
/// being replaced is kept as `<stem>.srm.before-<other>`.
///
/// Returns true when a battery came across: the caller must not resume an
/// older save state of `which`, or the progress would roll back.
pub fn carry_battery(saves: &Path, stem: &str, which: Core, platform: Platform)
    -> std::io::Result<bool>
{
    carry_battery_for(saves, stem, which, platform, None)
}

/// As carry_battery, with the cart's ROM: a Mega Drive cart shares its
/// battery only when md_shares_sram says its save is plain battery SRAM.
pub fn carry_battery_for(saves: &Path, stem: &str, which: Core, platform: Platform,
                         rom: Option<&Path>) -> std::io::Result<bool>
{
    let Some(other) = other_core(which, platform) else { return Ok(false) };
    if platform == Platform::Md {
        let header = match rom {
            Some(rom) => read_head(rom, 0x200)?,
            None => return Ok(false),
        };
        if !md_shares_sram(&header) { return Ok(false); }
    }
    let own_path = saves.join(which.text()).join(format!("{stem}.srm"));
    let their_path = saves.join(other.text()).join(format!("{stem}.srm"));
    let theirs = match std::fs::read(&their_path) {
        Ok(bytes) if !bytes.is_empty() => bytes,
        Ok(_) => return Ok(false),
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e),
    };
    let own = match std::fs::read(&own_path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == ErrorKind::NotFound => None,
        Err(e) => return Err(e),
    };
    let mut carried = theirs.clone();
    if let Some(own) = &own {
        let newer = std::fs::metadata(&their_path)?.modified()?
            > std::fs::metadata(&own_path)?.modified()?;
        if !newer { return Ok(false); }
        if platform == Platform::Md {
            carried = carry_md(&theirs, Some(own.len()), which);
            if &carried == own { return Ok(false); }
        } else {
            let common = own.len().min(theirs.len());
            if own[..common] == theirs[..common] { return Ok(false); }
            carried.extend_from_slice(&own[common..]);
        }
        atomic_write(&backup_path(&own_path, other), own)?;
    } else if platform == Platform::Md {
        carried = carry_md(&theirs, None, which);
    }
    std::fs::create_dir_all(own_path.parent().unwrap_or(saves))?;
    atomic_write(&own_path, &carried)?;
    Ok(true)
}

/// Mega Drive battery SRAM sits at the same offsets in both cores' files:
/// Genesis Plus GX indexes it by address in a 64 KiB block it cuts after
/// the last byte that is not 0xFF; PicoDrive by offset from the SRAM start,
/// at the cart's real size (both from the ROM header's 0x1B4, word-aligned).
/// So the bytes carry over as they are: into PicoDrive's own file at its
/// size, the rest 0xFF as Genesis Plus GX left it; into Genesis Plus GX
/// as they are, its unwritten bytes staying 0xFF. Never the old file's tail,
/// which would bring back bytes the newer save had erased.
fn carry_md(theirs: &[u8], own_len: Option<usize>, which: Core) -> Vec<u8> {
    let mut out = theirs.to_vec();
    if which == Core::Picodrive {
        if let Some(len) = own_len { out.resize(len, 0xFF); }
    }
    out
}

/// Mega Drive saves kept in an EEPROM (Genesis Plus GX's eeprom_i2c
/// database, pinned f27655b), whose files the two cores need not lay out
/// alike: their product codes, as at 0x183 in the header.
const MD_EEPROM_CODES: [&str; 28] = [
    "T-50176", "T-50396", "T-50446", "T-50516", "T-50606", "T-12046", "T-12053",
    "MK-1215", "MK-1228", "G-5538", "PR-1993", "G-4060", "00001211", "00004076",
    "G-4524", "00054503", "T-81033", "T-081326", "T-081276", "T-81406", "T-081586",
    "T-81476", "T-81576", "T-120106", "T-120096", "T-120146", "T-120066", "T-123456",
];

/// Whether a Mega Drive cart's save is plain battery SRAM: its header
/// declares backup RAM ("RA" at 0x1B0, not the 0xE8 EEPROM mark) and its
/// product code is not one of the EEPROM games.
pub fn md_shares_sram(header: &[u8]) -> bool {
    let Some(ra) = header.get(0x1B0..0x1B3) else { return false };
    if &ra[..2] != b"RA" || ra[2] == 0xE8 { return false; }
    let product = header.get(0x180..0x18E).map(|b| String::from_utf8_lossy(b).into_owned())
        .unwrap_or_default();
    !MD_EEPROM_CODES.iter().any(|code| product.contains(code))
}

fn read_head(path: &Path, len: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut head = Vec::with_capacity(len);
    std::fs::File::open(path)?.take(len as u64).read_to_end(&mut head)?;
    Ok(head)
}

fn backup_path(own: &Path, other: Core) -> PathBuf {
    let mut name = own.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".before-{}", other.as_str()));
    own.with_file_name(name)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    let temp = path.with_file_name(name);
    let mut file = std::fs::File::create(&temp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    std::fs::rename(&temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn put(root: &Path, core: Core, bytes: &[u8], age_s: u64) {
        let dir = root.join(core.text());
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Pokemon Crystal.srm");
        std::fs::write(&path, bytes).unwrap();
        let at = SystemTime::now() - Duration::from_secs(age_s);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(at).unwrap();
    }

    fn get(root: &Path, core: Core) -> Vec<u8> {
        std::fs::read(root.join(core.text()).join("Pokemon Crystal.srm")).unwrap()
    }

    #[test]
    fn every_slot_palette_has_a_gambatte_twin() {
        for palette in slot_store::GbPalette::all() {
            assert!(gambatte_palette(palette.core_name()).is_some(), "{}", palette.core_name());
        }
        assert_eq!(gambatte_palette("SGB 3-F"), Some("SGB - 3F"));
        assert_eq!(gambatte_palette("SGB 9-Z"), None);
    }

    #[test]
    fn first_gambatte_boot_takes_the_mgba_battery() {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), Core::Mgba, &[1, 2, 3, 4, 0xEE, 0xEE], 60);
        assert!(carry_battery(d.path(), "Pokemon Crystal", Core::Gambatte, Platform::Gbc).unwrap());
        assert_eq!(get(d.path(), Core::Gambatte), [1, 2, 3, 4, 0xEE, 0xEE]);
    }

    #[test]
    fn newer_gambatte_progress_returns_to_mgba_and_keeps_its_rtc_tail() {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), Core::Mgba, &[1, 2, 3, 4, 0xEE, 0xEE], 60);
        put(d.path(), Core::Gambatte, &[9, 9, 9, 9], 10);
        assert!(carry_battery(d.path(), "Pokemon Crystal", Core::Mgba, Platform::Gbc).unwrap());
        assert_eq!(get(d.path(), Core::Mgba), [9, 9, 9, 9, 0xEE, 0xEE]);
        let backup = d.path().join("mGBA/Pokemon Crystal.srm.before-gambatte");
        assert_eq!(std::fs::read(backup).unwrap(), [1, 2, 3, 4, 0xEE, 0xEE]);
    }

    #[test]
    fn same_battery_or_older_one_changes_nothing() {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), Core::Mgba, &[1, 2, 3, 4, 0xEE], 10);
        put(d.path(), Core::Gambatte, &[1, 2, 3, 4], 60);
        assert!(!carry_battery(d.path(), "Pokemon Crystal", Core::Mgba, Platform::Gbc).unwrap());
        put(d.path(), Core::Gambatte, &[1, 2, 3, 4], 5);
        assert!(!carry_battery(d.path(), "Pokemon Crystal", Core::Mgba, Platform::Gbc).unwrap());
        put(d.path(), Core::Gambatte, &[7, 7, 7, 7], 60);
        assert!(!carry_battery(d.path(), "Pokemon Crystal", Core::Mgba, Platform::Gbc).unwrap());
        assert_eq!(get(d.path(), Core::Mgba), [1, 2, 3, 4, 0xEE]);
    }

    fn md_header(product: &str, ra: &[u8; 3]) -> Vec<u8> {
        let mut h = vec![0x20u8; 0x200];
        h[0x100..0x104].copy_from_slice(b"SEGA");
        h[0x180..0x180 + product.len()].copy_from_slice(product.as_bytes());
        h[0x1B0..0x1B3].copy_from_slice(ra);
        h
    }

    fn md_put(root: &Path, core: Core, bytes: &[u8], age_s: u64) {
        let dir = root.join(core.text());
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("Phantasy Star IV.srm");
        std::fs::write(&path, bytes).unwrap();
        let when = SystemTime::now() - Duration::from_secs(age_s);
        std::fs::File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
    }

    #[test]
    fn mega_drive_battery_sram_carries_between_genesis_plus_gx_and_picodrive() {
        let d = tempfile::tempdir().unwrap();
        let rom = d.path().join("psiv.md");
        std::fs::write(&rom, md_header("GM MK-1307 -00", b"RA\xF8")).unwrap();
        // PicoDrive: the cart's 16 KiB; odd bytes saved, even ones 0.
        let mut pico = vec![0u8; 0x4000];
        for (i, b) in pico.iter_mut().enumerate() { if i % 2 == 1 { *b = (i / 2) as u8; } }
        md_put(d.path(), Core::Picodrive, &pico, 10);
        // Genesis Plus GX: an older save, cut after its last written byte.
        md_put(d.path(), Core::GenesisPlusGx, &[0xFF, 7, 0xFF, 7], 60);
        assert!(carry_battery_for(d.path(), "Phantasy Star IV", Core::GenesisPlusGx,
            Platform::Md, Some(&rom)).unwrap());
        assert_eq!(std::fs::read(d.path().join("Genesis Plus GX/Phantasy Star IV.srm")).unwrap(), pico,
            "the newer bytes, at the same offsets");
        // Back to PicoDrive after a shorter Genesis Plus GX save: its size,
        // the rest 0xFF, none of PicoDrive's older bytes.
        md_put(d.path(), Core::GenesisPlusGx, &[0xFF, 9, 0xFF, 9], 1);
        assert!(carry_battery_for(d.path(), "Phantasy Star IV", Core::Picodrive,
            Platform::Md, Some(&rom)).unwrap());
        let back = std::fs::read(d.path().join("PicoDrive/Phantasy Star IV.srm")).unwrap();
        assert_eq!(back.len(), 0x4000);
        assert_eq!(&back[..4], &[0xFF, 9, 0xFF, 9]);
        assert!(back[4..].iter().all(|b| *b == 0xFF));
    }

    #[test]
    fn mega_drive_eeprom_games_and_carts_without_backup_ram_keep_their_own_saves() {
        assert!(md_shares_sram(&md_header("GM MK-1307 -00", b"RA\xF8")));
        assert!(!md_shares_sram(&md_header("GM T-081326 00", b"RA\xF8")), "NBA Jam: EEPROM");
        assert!(!md_shares_sram(&md_header("GM G-4060  -00", b"RA\xF8")), "Monster World: EEPROM");
        assert!(!md_shares_sram(&md_header("GM MK-1307 -00", b"RA\xE8")), "EEPROM mark");
        assert!(!md_shares_sram(&md_header("GM MK-1307 -00", b"  \x20")), "no backup RAM");
        let d = tempfile::tempdir().unwrap();
        let rom = d.path().join("nbajam.md");
        std::fs::write(&rom, md_header("GM T-081326 00", b"RA\xF8")).unwrap();
        md_put(d.path(), Core::Picodrive, &[1, 2, 3], 1);
        assert!(!carry_battery_for(d.path(), "Phantasy Star IV", Core::GenesisPlusGx,
            Platform::Md, Some(&rom)).unwrap());
        assert!(!carry_battery(d.path(), "Phantasy Star IV", Core::GenesisPlusGx, Platform::Md).unwrap(),
            "without its ROM a Mega Drive cart never shares");
    }

    #[test]
    fn gba_carts_never_share_through_this_path() {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), Core::Gpsp, &[5; 8], 1);
        assert!(!carry_battery(d.path(), "Pokemon Crystal", Core::Mgba, Platform::Gba).unwrap());
    }
}
