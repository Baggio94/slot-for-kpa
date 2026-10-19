//! SAF ROM library metadata. SAF content URIs are *not* POSIX file paths.
//! This module only converts metadata and never attempts filesystem I/O on URIs.
use serde::Deserialize;
use slot_store::{sort_key, Cart, Outline, Platform, ShellChoice, ShellFinish};
use slot_store::gb::{Class, Header};
use slot_ui::Finish;
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
    // Nintendo GB/GBC header fields are read once by the Android SAF scanner,
    // including from ZIPs, instead of trying to open a content URI as a file.
    #[serde(default)]
    pub gb_title: String,
    #[serde(default)]
    pub gb_code: String,
    #[serde(default)]
    pub gb_japan: bool,
    #[serde(default)]
    pub gb_cgb_flag: Option<i32>,
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
                // Cart Studio and the shelf must resolve the same automatic
                // per-game shell, even for SAF content URIs and zipped ROMs.
                // Explicit saved Studio overrides still take precedence below.
                let detected = entry.gb_cgb_flag
                    .and_then(|flag| u8::try_from(flag).ok())
                    .map(|cgb| {
                        let header = Header {
                            title: entry.gb_title.clone(),
                            code: entry.gb_code.clone(),
                            japan: entry.gb_japan,
                            cgb,
                        };
                        let material = slot_ui::gb_shell_from_header(&header);
                        ShellChoice {
                            outline: if header.class() == Class::ColourOnly {
                                Outline::Rounded
                            } else {
                                Outline::Notched
                            },
                            colour: material.colour,
                            finish: match material.finish {
                                Finish::Solid => ShellFinish::Solid,
                                Finish::Translucent => ShellFinish::Clear,
                                Finish::Glitter => ShellFinish::Glitter,
                            },
                        }
                    });
                Some(detected.unwrap_or_else(|| {
                    // Legacy indexes continue to render until a background
                    // scan supplies GB header identity, with no ROM writes.
                    let color_only = entry.color_only;
                    ShellChoice {
                        outline: if color_only { Outline::Rounded } else { Outline::Notched },
                        colour: match (platform, color_only) {
                            (_, true) => [0x7c, 0x7a, 0x8a],
                            (Platform::Gbc, false) => [0x33, 0x30, 0x31],
                            _ => [0x9a, 0x97, 0x8f],
                        },
                        finish: if color_only { ShellFinish::Clear } else { ShellFinish::Solid },
                    }
                }))
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
            code: String::new(), color_only: false, shell_override: None,
            gb_title: String::new(), gb_code: String::new(),
            gb_japan: false, gb_cgb_flag: None
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
    #[test]
    fn gb_carts_use_studio_header_colours_and_preserve_overrides() {
        let entry = |platform: &str, name: &str, title: &str, code: &str, flag: i32| {
            RomEntry {
                platform: platform.into(), title: name.into(),
                uri: format!("content://roms/{name}"), code: String::new(),
                color_only: flag == 0xc0, shell_override: None,
                gb_title: title.into(), gb_code: code.into(),
                gb_japan: false, gb_cgb_flag: Some(flag),
            }
        };
        let mut manual = entry("GB", "Manual", "POKEMON RED", "", 0);
        manual.shell_override = Some("rounded 123abc clear".into());
        let grouped = carts_by_platform(&[
            entry("GB", "Blue", "POKEMON BLU", "", 0),
            entry("GB", "Red", "POKEMON RED", "", 0),
            entry("GBC", "Crystal", "", "BYT", 0xc0),
            manual,
        ]);
        let pick = |group: usize, title: &str| grouped[group].iter()
            .find(|cart| cart.title == title).unwrap().shell.unwrap();
        assert_eq!(pick(1, "Blue").colour, [0x2b, 0x3a, 0x88]);
        assert_eq!(pick(1, "Red").colour, [0xc0, 0x28, 0x2c]);
        assert_eq!(pick(2, "Crystal").finish, ShellFinish::Glitter);
        assert_eq!(pick(2, "Crystal").outline, Outline::Rounded);
        assert_eq!(pick(1, "Manual").colour, [0x12, 0x3a, 0xbc]);
        assert_eq!(pick(1, "Manual").finish, ShellFinish::Clear);
        assert_eq!(pick(1, "Manual").outline, Outline::Rounded);
    }

}
