//! GB/GBC carts run on mGBA or Gambatte. Both keep RetroArch's per-core
//! folders (Saves/mGBA, Saves/Gambatte), so switching the chip on the board
//! must bring the newer cartridge battery along instead of starting over.
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

use slot_store::{Core, Platform};

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

fn other_core(which: Core, platform: Platform) -> Option<Core> {
    if platform == Platform::Gba { return None; }
    match which {
        Core::Mgba => Some(Core::Gambatte),
        Core::Gambatte => Some(Core::Mgba),
        Core::Gpsp => None,
    }
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
    let Some(other) = other_core(which, platform) else { return Ok(false) };
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
        let common = own.len().min(theirs.len());
        if !newer || own[..common] == theirs[..common] { return Ok(false); }
        carried.extend_from_slice(&own[common..]);
        atomic_write(&backup_path(&own_path, other), own)?;
    }
    std::fs::create_dir_all(own_path.parent().unwrap_or(saves))?;
    atomic_write(&own_path, &carried)?;
    Ok(true)
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

    #[test]
    fn gba_carts_never_share_through_this_path() {
        let d = tempfile::tempdir().unwrap();
        put(d.path(), Core::Gpsp, &[5; 8], 1);
        assert!(!carry_battery(d.path(), "Pokemon Crystal", Core::Mgba, Platform::Gba).unwrap());
    }
}
