//! Per-ROM core choice keyed by the opaque SAF URI; no filename collisions.
use std::path::Path;
use slot_store::{choose_core, core_for, core_for_platform, default_core, write_default_core, Core,
    Platform};

pub fn key(uri: &str) -> String {
    // Stable FNV-1a 64-bit digest; independent of RandomState and process.
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in uri.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

pub fn selected(storage: &Path, uri: &str) -> Core {
    core_for(storage, &key(uri))
}

/// The core this cart actually runs on: a stale choice the platform
/// cannot run (gpSP on a GB cart, Gambatte on a GBA cart) falls back.
pub fn selected_for(storage: &Path, uri: &str, platform: Platform) -> Core {
    core_for_platform(storage, &key(uri), platform)
}

/// The board's pick for one cart; picking the system's default clears it.
pub fn choose(storage: &Path, uri: &str, platform: Platform, core: Core) -> std::io::Result<()> {
    choose_core(storage, &key(uri), platform, core)
}

pub fn default(storage: &Path, platform: Platform) -> Core {
    default_core(storage, platform)
}

pub fn set_default(storage: &Path, platform: Platform, core: Core) -> std::io::Result<()> {
    write_default_core(storage, platform, core)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_unique_rom_uri_keys() {
        assert_eq!(key("content://roms/1"), "cd3adfea5b691b9d");
        assert_ne!(key("content://roms/1"), key("content://roms/2"));
        assert_ne!(key("content://roms/a"), key("content://other/a"));
    }
    #[test]
    fn core_file_matches_available_binary_names() {
        assert_eq!(Core::Mgba.as_str(), "mgba");
        assert_eq!(Core::Gpsp.as_str(), "gpsp");
        assert_eq!(Core::Gambatte.as_str(), "gambatte");
    }
}
