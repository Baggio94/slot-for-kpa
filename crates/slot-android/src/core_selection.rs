//! Per-ROM core choice keyed by the opaque SAF URI; no filename collisions.
use std::path::Path;
use slot_store::{core_for, write_selected_core, Core};

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

pub fn set(storage: &Path, uri: &str, core: Core) -> std::io::Result<()> {
    write_selected_core(storage, &key(uri), core)
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
    }
}
