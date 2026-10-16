//! SAF ROM library metadata. SAF content URIs are *not* POSIX file paths.
//! This module only converts metadata and never attempts filesystem I/O on URIs.
use serde::Deserialize;
use slot_store::{sort_key, Cart, Outline, Platform, ShellChoice, ShellFinish};
use std::path::PathBuf;
use std::sync::Mutex;

const MAX_ROMS: usize = 5000;

#[derive(Clone, Debug, Deserialize)]
pub struct RomEntry {
    pub title: String,
    pub platform: String,
    pub uri: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub color_only: bool,
    #[serde(default)]
    pub shell_override: Option<String>,
}

pub struct LibraryState {
    pub version: u64,
    /// None: no folder selected yet, show empty shelf and onboarding.
    /// Some([]): scanned folder has no supported games; show empty shelf.
    pub entries: Option<Vec<RomEntry>>,
}

pub static LIBRARY: Mutex<LibraryState> = Mutex::new(LibraryState {
    version: 0,
    entries: None,
});

/// Select the next shelf with games in the given direction, skipping empty
/// GB/GBC/GBA systems. Returns the current shelf if no alternative exists.
pub fn next_populated(current: usize, direction: i32, count: &[usize]) -> usize {
    let n = count.len();
    if n == 0 || current >= n { return current; }
    for step in 1..=n {
        let i = (current as i32 + direction * step as i32).rem_euclid(n as i32) as usize;
        if count[i] != 0 { return i; }
    }
    current
}

pub fn carts_by_platform(entries: &[RomEntry]) -> [Vec<Cart>; 3] {
    let mut groups: [Vec<Cart>; 3] = std::array::from_fn(|_| Vec::new());
    for entry in entries.iter().take(MAX_ROMS) {
        if entry.title.trim().is_empty() || !entry.uri.starts_with("content://") {
            continue;
        }
        let (platform, group) = match entry.platform.as_str() {
            "GBA" => (Platform::Gba, 0),
            "GB" => (Platform::Gb, 1),
            "GBC" => (Platform::Gbc, 2),
            _ => continue,
        };
        let shell = match platform {
            Platform::Gba => None,
            Platform::Gb | Platform::Gbc => {
                let color_only = entry.color_only;
                Some(ShellChoice {
                    outline: if color_only { Outline::Rounded } else { Outline::Notched },
                    colour: match (platform, color_only) {
                        (_, true) => [0x7c, 0x7a, 0x8a],
                        (Platform::Gbc, false) => [0x33, 0x30, 0x31],
                        _ => [0x9a, 0x97, 0x8f],
                    },
                    finish: if color_only { ShellFinish::Clear } else { ShellFinish::Solid },
                })
            }
        };
        let shell = entry.shell_override.as_deref()
            .and_then(ShellChoice::parse)
            .or(shell);
        groups[group].push(Cart {
            platform,
            stem: entry.title.trim().to_owned(),
            title: entry.title.trim().to_owned(),
            code: entry.code.clone(),
            // Kept as opaque URI for the planned Android ContentResolver bridge.
            // Never pass this through std::fs::File or libc open().
            rom: PathBuf::from(&entry.uri),
            label: None,
            shell,
        });
    }
    for shelf in &mut groups {
        // Unicode casefold keys are allocated once per cart instead of on
        // every comparator call (thousands of extra allocations at startup).
        shelf.sort_by_cached_key(|cart| sort_key(&cart.stem));
    }
    groups
}

#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
pub extern "system" fn Java_fyi_slot_konkr_MainActivity_nativeSetLibrary(
    mut env: jni::JNIEnv<'_>,
    _this: jni::objects::JObject<'_>,
    json: jni::objects::JString<'_>,
) -> jni::sys::jint {
    let started = std::time::Instant::now();
    let Ok(input) = env.get_string(&json) else { return -1 };
    let Ok(entries) = serde_json::from_str::<Vec<RomEntry>>(&input.to_string_lossy()) else { return -1 };
    if entries.len() > MAX_ROMS { return -1; }
    let count = entries.len() as i32;
    let mut state = LIBRARY.lock().unwrap_or_else(|err| err.into_inner());
    state.entries = Some(entries);
    state.version = state.version.wrapping_add(1);
    crate::game::log_launch_timing(&format!(
        "ROM shelf JSON import: {count} carts in {}ms", started.elapsed().as_millis()));
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shelf_navigation_skips_empty_platforms() {
        assert_eq!(next_populated(0, 1, &[]), 0);
        assert_eq!(next_populated(0, 1, &[2, 0, 0]), 0);
        assert_eq!(next_populated(0, -1, &[2, 0, 0]), 0);
        assert_eq!(next_populated(0, 1, &[2, 0, 3]), 2);
        assert_eq!(next_populated(0, -1, &[2, 0, 3]), 2);
        assert_eq!(next_populated(2, 1, &[2, 0, 3]), 0);
        assert_eq!(next_populated(0, 1, &[0, 0, 0]), 0);
    }

    #[test]
    fn sorts_and_rejects_non_saf_entries() {
        let example = |platform: &str, title: &str, uri: &str| RomEntry {
            platform: platform.into(), title: title.into(), uri: uri.into(),
            code: String::new(), color_only: false, shell_override: None
        };
        let grouped = carts_by_platform(&[
            example("GBA", "Zelda", "content://roms/z"),
            example("GBA", "Advance Wars", "content://roms/a"),
            example("GBA", "Not a ROM", "/storage/emulated/0/ROMs/foo.gba"),
            example("GBC", "Color Game", "content://roms/c"),
            example("SNES", "Ignored", "content://roms/s"),
        ]);
        assert_eq!(grouped[0].len(), 2);
        assert_eq!(grouped[0][0].stem, "Advance Wars");
        assert_eq!(grouped[1].len(), 0);
        assert_eq!(grouped[2].len(), 1);
    }
}
